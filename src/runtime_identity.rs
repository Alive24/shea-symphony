//! Machine-readable identity for Shea Symphony executable composition roots.
//!
//! The desktop App uses this contract to distinguish the transitional Legacy
//! CLI from the canonical Temporal worker before it launches an operator
//! command. It describes a build; it is not a publisher-signing mechanism.

use serde::{Deserialize, Serialize};

/// The only command-line flag shared by both executable composition roots.
pub const RUNTIME_INFO_FLAG: &str = "--runtime-info";

/// Schema version for [`RuntimeIdentity`] JSON output.
pub const RUNTIME_IDENTITY_SCHEMA_VERSION: u32 = 1;

/// The mutually exclusive runtime role owned by one executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRole {
    /// The canonical 2607 Temporal worker entrypoint.
    TemporalWorker,
    /// The transitional Legacy operator CLI entrypoint.
    LegacyCli,
}

/// Credential-free build and compatibility metadata emitted as JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeIdentity {
    /// Version of this JSON contract.
    pub schema_version: u32,
    /// Executable composition role.
    pub binary_role: RuntimeRole,
    /// Cargo package version shared by the two binaries.
    pub cli_version: String,
    /// Git revision embedded when the crate was built.
    pub source_revision: String,
    /// Rust compilation target triple.
    pub target: String,
    /// Operating system reported by the Rust target.
    pub platform: String,
    /// Processor architecture reported by the Rust target.
    pub architecture: String,
    /// Versioned compatibility contract implemented by this role.
    pub compatibility: String,
}

impl RuntimeIdentity {
    /// Builds identity metadata for one explicit executable role.
    pub fn for_role(binary_role: RuntimeRole) -> Self {
        let compatibility = match binary_role {
            RuntimeRole::TemporalWorker => "shea-temporal-worker-v1",
            RuntimeRole::LegacyCli => "shea-legacy-cli-v1",
        };
        Self {
            schema_version: RUNTIME_IDENTITY_SCHEMA_VERSION,
            binary_role,
            cli_version: env!("CARGO_PKG_VERSION").into(),
            source_revision: env!("SHEA_SOURCE_REVISION").into(),
            target: env!("SHEA_TARGET_TRIPLE").into(),
            platform: std::env::consts::OS.into(),
            architecture: std::env::consts::ARCH.into(),
            compatibility: compatibility.into(),
        }
    }
}

/// Prints identity JSON when `args` is exactly [`RUNTIME_INFO_FLAG`].
pub fn print_if_requested(
    binary_role: RuntimeRole,
    args: &[String],
) -> Result<bool, serde_json::Error> {
    if args != [RUNTIME_INFO_FLAG] {
        return Ok(false);
    }
    println!(
        "{}",
        serde_json::to_string(&RuntimeIdentity::for_role(binary_role))?
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_have_distinct_versioned_compatibility_contracts() {
        let temporal = RuntimeIdentity::for_role(RuntimeRole::TemporalWorker);
        let legacy = RuntimeIdentity::for_role(RuntimeRole::LegacyCli);

        assert_eq!(temporal.schema_version, 1);
        assert_eq!(legacy.schema_version, 1);
        assert_ne!(temporal.binary_role, legacy.binary_role);
        assert_ne!(temporal.compatibility, legacy.compatibility);
        assert_eq!(temporal.source_revision, legacy.source_revision);
        assert_eq!(temporal.target, legacy.target);
    }

    #[test]
    fn identity_contract_round_trips_as_json() {
        let expected = RuntimeIdentity::for_role(RuntimeRole::LegacyCli);
        let json = serde_json::to_string(&expected).unwrap();
        let actual: RuntimeIdentity = serde_json::from_str(&json).unwrap();

        assert_eq!(actual, expected);
    }
}
