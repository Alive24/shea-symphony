use std::{
    env, fs,
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shea_symphony::runtime_identity::{
    RuntimeIdentity, RuntimeRole, RUNTIME_IDENTITY_SCHEMA_VERSION, RUNTIME_INFO_FLAG,
};

const DISCOVERY_SCHEMA_VERSION: u32 = 1;
const LEGACY_COMPATIBILITY: &str = "shea-legacy-cli-v1";
const DISCOVERY_FILE_NAME: &str = "runtime-discovery.json";
const SIDECAR_FILE_NAME: &str = "shea-symphony-legacy";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeDiscovery {
    pub schema_version: u32,
    pub binary_role: RuntimeRole,
    pub cli_version: String,
    pub app_version: String,
    pub source_revision: String,
    pub target: String,
    pub platform: String,
    pub architecture: String,
    pub compatibility: String,
    pub executable_path: String,
    pub sha256: String,
}

pub fn default_discovery_path() -> PathBuf {
    if let Some(path) = env::var_os("SHEA_SYMPHONY_RUNTIME_DISCOVERY_PATH") {
        return PathBuf::from(path);
    }
    discovery_home(env::var_os("HOME"), env::var_os("USERPROFILE"))
        .join(".shea-symphony")
        .join(DISCOVERY_FILE_NAME)
}

fn discovery_home(home: Option<std::ffi::OsString>, user_profile: Option<std::ffi::OsString>) -> PathBuf {
    home.or(user_profile)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn validate_explicit_runtime(path: &Path) -> Result<(), String> {
    let Some(identity) = probe_runtime(path)? else {
        // Compatibility exception: an operator may explicitly select an
        // unmarked 2606 binary, but it is never eligible for discovery.
        return Ok(());
    };
    validate_legacy_identity(&identity, false)
}

pub fn resolve_installed_runtime(discovery_path: &Path) -> Result<Option<PathBuf>, String> {
    if !discovery_path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(discovery_path).map_err(|error| {
        format!(
            "installed Legacy runtime discovery is unreadable at {}: {error}",
            discovery_path.display()
        )
    })?;
    let discovery: RuntimeDiscovery = serde_json::from_str(&text).map_err(|error| {
        format!(
            "installed Legacy runtime discovery is invalid at {}: {error}",
            discovery_path.display()
        )
    })?;
    validate_discovery(&discovery)?;
    let executable = PathBuf::from(&discovery.executable_path);
    let actual_digest = sha256_file(&executable)?;
    if actual_digest != discovery.sha256 {
        return Err(format!(
            "installed Legacy runtime digest mismatch for {}",
            executable.display()
        ));
    }
    let identity = probe_runtime(&executable)?.ok_or_else(|| {
        format!(
            "automatically discovered runtime is unmarked: {}",
            executable.display()
        )
    })?;
    validate_legacy_identity(&identity, true)?;
    validate_discovery_identity(&discovery, &identity)?;
    Ok(Some(executable))
}

pub fn publish_installed_discovery_if_available() -> Result<Option<PathBuf>, String> {
    let executable = installed_sidecar_path()?;
    if !executable.is_file() {
        if cfg!(debug_assertions) && env::var_os("SHEA_SYMPHONY_BUNDLED_CLI_PATH").is_none() {
            return Ok(None);
        }
        return Err(format!(
            "bundled Legacy runtime is missing: {}",
            executable.display()
        ));
    }
    let discovery_path = default_discovery_path();
    publish_discovery(&executable, &discovery_path)?;
    Ok(Some(discovery_path))
}

fn publish_discovery(executable: &Path, discovery_path: &Path) -> Result<(), String> {
    let executable = fs::canonicalize(executable).map_err(|error| {
        format!(
            "bundled Legacy runtime path is unavailable at {}: {error}",
            executable.display()
        )
    })?;
    let identity = probe_runtime(&executable)?.ok_or_else(|| {
        format!(
            "bundled Legacy runtime does not expose {RUNTIME_INFO_FLAG}: {}",
            executable.display()
        )
    })?;
    validate_legacy_identity(&identity, true)?;
    let discovery = RuntimeDiscovery {
        schema_version: DISCOVERY_SCHEMA_VERSION,
        binary_role: identity.binary_role,
        cli_version: identity.cli_version.clone(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        source_revision: identity.source_revision.clone(),
        target: identity.target.clone(),
        platform: identity.platform.clone(),
        architecture: identity.architecture.clone(),
        compatibility: identity.compatibility.clone(),
        executable_path: executable.display().to_string(),
        sha256: sha256_file(&executable)?,
    };
    validate_discovery(&discovery)?;

    let parent = discovery_path.parent().ok_or_else(|| {
        format!(
            "runtime discovery path has no parent: {}",
            discovery_path.display()
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "could not create runtime discovery directory {}: {error}",
            parent.display()
        )
    })?;
    let temporary_path = discovery_path.with_extension(format!("tmp-{}", std::process::id()));
    match fs::remove_file(&temporary_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary_path)
        .map_err(|error| error.to_string())?;
    file.write_all(
        serde_json::to_string_pretty(&discovery)
            .map_err(|error| error.to_string())?
            .as_bytes(),
    )
    .map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    fs::rename(&temporary_path, discovery_path).map_err(|error| {
        format!(
            "could not atomically publish runtime discovery {}: {error}",
            discovery_path.display()
        )
    })?;

    resolve_installed_runtime(discovery_path)?.ok_or_else(|| {
        format!(
            "runtime discovery readback failed at {}",
            discovery_path.display()
        )
    })?;
    Ok(())
}

fn installed_sidecar_path() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("SHEA_SYMPHONY_BUNDLED_CLI_PATH") {
        return Ok(PathBuf::from(path));
    }
    let executable = env::current_exe()
        .map_err(|error| format!("could not resolve App executable path: {error}"))?;
    let directory = executable
        .parent()
        .ok_or_else(|| "App executable path has no parent".to_string())?;
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    Ok(directory.join(format!("{SIDECAR_FILE_NAME}{suffix}")))
}

fn probe_runtime(path: &Path) -> Result<Option<RuntimeIdentity>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let output = Command::new(path)
        .arg(RUNTIME_INFO_FLAG)
        .output()
        .map_err(|error| format!("could not inspect runtime {}: {error}", path.display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim())
        .map(Some)
        .map_err(|error| {
            format!(
                "runtime identity from {} is invalid: {error}",
                path.display()
            )
        })
}

fn validate_legacy_identity(
    identity: &RuntimeIdentity,
    require_same_build: bool,
) -> Result<(), String> {
    if identity.schema_version != RUNTIME_IDENTITY_SCHEMA_VERSION {
        return Err(format!(
            "unsupported runtime identity schema {}",
            identity.schema_version
        ));
    }
    if identity.binary_role == RuntimeRole::TemporalWorker {
        return Err(
            "selected binary has role temporal_worker; the App requires legacy_cli semantics"
                .into(),
        );
    }
    if identity.binary_role != RuntimeRole::LegacyCli
        || identity.compatibility != LEGACY_COMPATIBILITY
    {
        return Err("selected binary does not implement shea-legacy-cli-v1".into());
    }
    if require_same_build {
        let expected = RuntimeIdentity::for_role(RuntimeRole::LegacyCli);
        if identity.cli_version != expected.cli_version
            || identity.source_revision != expected.source_revision
            || identity.target != expected.target
            || identity.platform != expected.platform
            || identity.architecture != expected.architecture
        {
            return Err("bundled Legacy runtime does not match the App build identity".into());
        }
    }
    Ok(())
}

fn validate_discovery(discovery: &RuntimeDiscovery) -> Result<(), String> {
    if discovery.schema_version != DISCOVERY_SCHEMA_VERSION {
        return Err(format!(
            "unsupported runtime discovery schema {}",
            discovery.schema_version
        ));
    }
    if discovery.app_version != env!("CARGO_PKG_VERSION") {
        return Err("runtime discovery belongs to a different App version".into());
    }
    if discovery.binary_role != RuntimeRole::LegacyCli
        || discovery.compatibility != LEGACY_COMPATIBILITY
    {
        return Err("runtime discovery does not describe a compatible Legacy CLI".into());
    }
    if !Path::new(&discovery.executable_path).is_absolute() {
        return Err("runtime discovery executable_path must be absolute".into());
    }
    if discovery.sha256.len() != 64
        || !discovery
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("runtime discovery sha256 is invalid".into());
    }
    Ok(())
}

fn validate_discovery_identity(
    discovery: &RuntimeDiscovery,
    identity: &RuntimeIdentity,
) -> Result<(), String> {
    if discovery.binary_role != identity.binary_role
        || discovery.cli_version != identity.cli_version
        || discovery.source_revision != identity.source_revision
        || discovery.target != identity.target
        || discovery.platform != identity.platform
        || discovery.architecture != identity.architecture
        || discovery.compatibility != identity.compatibility
    {
        return Err("runtime discovery metadata is stale or incompatible".into());
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("could not hash runtime {}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn discovery_home_uses_windows_user_profile_when_home_is_absent() {
        assert_eq!(
            discovery_home(None, Some(std::ffi::OsString::from(r"C:\Users\operator"))),
            PathBuf::from(r"C:\Users\operator")
        );
        assert_eq!(
            discovery_home(
                Some(std::ffi::OsString::from("/Users/operator")),
                Some(std::ffi::OsString::from(r"C:\Users\operator"))
            ),
            PathBuf::from("/Users/operator")
        );
    }

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    #[test]
    fn rejects_known_temporal_binary_before_launch() {
        let temp = TempDir::new().unwrap();
        let binary = fake_runtime(temp.path(), RuntimeRole::TemporalWorker);

        let error = validate_explicit_runtime(&binary).unwrap_err();

        assert!(error.contains("temporal_worker"));
    }

    #[cfg(unix)]
    #[test]
    fn publishes_and_validates_atomic_legacy_discovery() {
        let temp = TempDir::new().unwrap();
        let binary = fake_runtime(temp.path(), RuntimeRole::LegacyCli);
        let discovery = temp.path().join("state/runtime.json");

        publish_discovery(&binary, &discovery).unwrap();

        assert_eq!(
            resolve_installed_runtime(&discovery).unwrap(),
            Some(fs::canonicalize(binary).unwrap())
        );
        assert!(!discovery
            .with_extension(format!("tmp-{}", std::process::id()))
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_tampered_runtime_after_discovery() {
        let temp = TempDir::new().unwrap();
        let binary = fake_runtime(temp.path(), RuntimeRole::LegacyCli);
        let discovery = temp.path().join("runtime.json");
        publish_discovery(&binary, &discovery).unwrap();

        fs::write(&binary, "tampered").unwrap();
        let error = resolve_installed_runtime(&discovery).unwrap_err();

        assert!(error.contains("digest mismatch"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_stale_discovery_metadata() {
        let temp = TempDir::new().unwrap();
        let binary = fake_runtime(temp.path(), RuntimeRole::LegacyCli);
        let discovery_path = temp.path().join("runtime.json");
        publish_discovery(&binary, &discovery_path).unwrap();
        let mut discovery: RuntimeDiscovery =
            serde_json::from_str(&fs::read_to_string(&discovery_path).unwrap()).unwrap();
        discovery.source_revision = "stale-revision".into();
        fs::write(
            &discovery_path,
            serde_json::to_string_pretty(&discovery).unwrap(),
        )
        .unwrap();

        let error = resolve_installed_runtime(&discovery_path).unwrap_err();

        assert!(error.contains("stale or incompatible"));
    }

    #[cfg(unix)]
    #[test]
    fn unmarked_runtime_is_allowed_only_as_an_explicit_override() {
        let temp = TempDir::new().unwrap();
        let binary = temp.path().join("unmarked-2606");
        fs::write(&binary, "#!/bin/sh\nexit 2\n").unwrap();
        let mut permissions = fs::metadata(&binary).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions).unwrap();

        validate_explicit_runtime(&binary).unwrap();

        let identity = RuntimeIdentity::for_role(RuntimeRole::LegacyCli);
        let discovery = RuntimeDiscovery {
            schema_version: DISCOVERY_SCHEMA_VERSION,
            binary_role: RuntimeRole::LegacyCli,
            cli_version: identity.cli_version,
            app_version: env!("CARGO_PKG_VERSION").into(),
            source_revision: identity.source_revision,
            target: identity.target,
            platform: identity.platform,
            architecture: identity.architecture,
            compatibility: identity.compatibility,
            executable_path: fs::canonicalize(&binary).unwrap().display().to_string(),
            sha256: sha256_file(&binary).unwrap(),
        };
        let discovery_path = temp.path().join("runtime.json");
        fs::write(
            &discovery_path,
            serde_json::to_string_pretty(&discovery).unwrap(),
        )
        .unwrap();

        let error = resolve_installed_runtime(&discovery_path).unwrap_err();
        assert!(error.contains("automatically discovered runtime is unmarked"));
    }

    #[cfg(unix)]
    fn fake_runtime(root: &Path, role: RuntimeRole) -> PathBuf {
        let path = root.join(match role {
            RuntimeRole::TemporalWorker => "temporal-runtime",
            RuntimeRole::LegacyCli => "legacy-runtime",
        });
        let identity = RuntimeIdentity::for_role(role);
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' '{}'\n",
                serde_json::to_string(&identity).unwrap()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }
}
