use std::fs;
use std::path::{Path, PathBuf};

use super::{AuditSeverity, ProjectAuditReport, ProjectAuditViolation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInstallTarget {
    pub label: String,
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
struct SkillSuiteManifest {
    version: String,
    skills: Vec<SkillSuiteEntry>,
}

#[derive(Debug, Clone)]
struct SkillSuiteEntry {
    name: String,
    path: PathBuf,
}

/// Returns the standard project-local skill roots supported by setup-shea.
///
/// Codex and Antigravity intentionally share the public `.agents/skills`
/// contract, while Claude Code uses `.claude/skills`. Installation and updates
/// remain owned by the standard Skills CLI.
pub fn default_shea_symphony_skill_targets(repo_root: &Path) -> Vec<SkillInstallTarget> {
    vec![
        SkillInstallTarget {
            label: "Codex/Antigravity".into(),
            root: nonempty_env("SHEA_SYMPHONY_AGENTS_SKILLS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| repo_root.join(".agents").join("skills")),
        },
        SkillInstallTarget {
            label: "Claude Code".into(),
            root: nonempty_env("SHEA_SYMPHONY_CLAUDE_SKILLS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| repo_root.join(".claude").join("skills")),
        },
    ]
}

pub fn append_local_skill_install_doctor_violations(
    report: &mut ProjectAuditReport,
    repo_root: &Path,
    targets: &[SkillInstallTarget],
) {
    let suite_root = repo_root.join("skills").join("shea-symphony");
    let manifest = match read_skill_suite_manifest(&suite_root) {
        Ok(manifest) => manifest,
        Err(error) => {
            report.violations.push(local_skill_violation(
                "Repo",
                &suite_root,
                "local_skill_suite_manifest_unavailable",
                format!("Shea Symphony skill suite metadata could not be read: {error}."),
                "Run from a checkout that contains `skills/shea-symphony/manifest.toml`, then rerun `doctor`.",
            ));
            return;
        }
    };

    for target in targets {
        audit_skill_target(report, &suite_root, &manifest, target);
    }
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn read_skill_suite_manifest(suite_root: &Path) -> Result<SkillSuiteManifest, String> {
    let manifest_path = suite_root.join("manifest.toml");
    let text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    let version = manifest_string_value(&text, "version").unwrap_or_else(|| "unknown".into());
    let mut skills = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_path: Option<PathBuf> = None;
    let mut current_default_install = true;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "[[skills]]" {
            push_manifest_skill(
                &mut skills,
                &mut current_name,
                &mut current_path,
                &mut current_default_install,
            );
            continue;
        }
        if let Some(value) = manifest_line_value(trimmed, "name") {
            current_name = Some(value);
        } else if let Some(value) = manifest_line_value(trimmed, "path") {
            current_path = Some(PathBuf::from(value));
        } else if trimmed == "default_install = false" {
            current_default_install = false;
        }
    }
    push_manifest_skill(
        &mut skills,
        &mut current_name,
        &mut current_path,
        &mut current_default_install,
    );

    if skills.is_empty() {
        return Err(format!(
            "{} does not define any skills",
            manifest_path.display()
        ));
    }

    Ok(SkillSuiteManifest { version, skills })
}

fn push_manifest_skill(
    skills: &mut Vec<SkillSuiteEntry>,
    current_name: &mut Option<String>,
    current_path: &mut Option<PathBuf>,
    current_default_install: &mut bool,
) {
    if let (Some(name), Some(path)) = (current_name.take(), current_path.take()) {
        if *current_default_install {
            skills.push(SkillSuiteEntry { name, path });
        }
    } else {
        *current_name = None;
        *current_path = None;
    }
    *current_default_install = true;
}

fn manifest_string_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| manifest_line_value(line.trim(), key))
}

fn manifest_line_value(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = \"");
    line.strip_prefix(&prefix)
        .and_then(|rest| rest.split_once('"').map(|(value, _)| value.to_string()))
}

fn audit_skill_target(
    report: &mut ProjectAuditReport,
    suite_root: &Path,
    manifest: &SkillSuiteManifest,
    target: &SkillInstallTarget,
) {
    if !target.root.exists() {
        report.violations.push(local_skill_violation(
            &target.label,
            &target.root,
            "local_skill_root_missing",
            format!(
                "{} Shea Symphony skill root is missing or undiscovered: `{}`.",
                target.label,
                target.root.display()
            ),
            "Run `setup-shea` to preview the standard project-local Skills CLI targets, or configure SHEA_SYMPHONY_AGENTS_SKILLS_DIR / SHEA_SYMPHONY_CLAUDE_SKILLS_DIR before rerunning `doctor`.",
        ));
        return;
    }

    for skill in &manifest.skills {
        let source = suite_root.join(&skill.path);
        let source_skill = source.join("SKILL.md");
        let destination = target.root.join(&skill.name);
        audit_skill_install(
            report,
            &target.label,
            manifest,
            skill,
            &destination,
            &source_skill,
        );
    }
}

fn audit_skill_install(
    report: &mut ProjectAuditReport,
    label: &str,
    manifest: &SkillSuiteManifest,
    skill: &SkillSuiteEntry,
    destination: &Path,
    source_skill: &Path,
) {
    let metadata = match fs::symlink_metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            report.violations.push(local_skill_violation(
                label,
                destination,
                "local_skill_missing",
                format!(
                    "{label} skill `{}` is not installed at `{}`.",
                    skill.name,
                    destination.display()
                ),
                "Run `setup-shea` to preview and confirm the standard Skills CLI install/update path.",
            ));
            return;
        }
        Err(error) => {
            report.violations.push(local_skill_violation(
                label,
                destination,
                "local_skill_path_unreadable",
                format!(
                    "{label} skill `{}` could not be inspected at `{}`: {error}.",
                    skill.name,
                    destination.display()
                ),
                "Check local filesystem permissions, then rerun `doctor`; doctor will not repair local skill files.",
            ));
            return;
        }
    };

    if metadata.file_type().is_symlink() {
        audit_skill_symlink(report, label, manifest, skill, destination, source_skill);
        return;
    }

    if metadata.is_file() {
        report.violations.push(local_skill_violation(
            label,
            destination,
            "local_skill_expected_directory_file",
            format!(
                "{label} skill `{}` is a file where a skill directory is expected; this commonly happens when a macOS Finder alias points at `SKILL.md`.",
                skill.name
            ),
            "Replace it through setup-shea and the standard Skills CLI; doctor is diagnostic and will not overwrite aliases or files.",
        ));
        return;
    }

    if metadata.is_dir() {
        audit_skill_directory(report, label, manifest, skill, destination, source_skill);
    }
}

fn audit_skill_symlink(
    report: &mut ProjectAuditReport,
    label: &str,
    manifest: &SkillSuiteManifest,
    skill: &SkillSuiteEntry,
    destination: &Path,
    source_skill: &Path,
) {
    let target = match fs::read_link(destination) {
        Ok(target) => target,
        Err(error) => {
            report.violations.push(local_skill_violation(
                label,
                destination,
                "local_skill_symlink_unreadable",
                format!(
                    "{label} skill `{}` symlink target could not be read: {error}.",
                    skill.name
                ),
                "Inspect the symlink manually or rerun setup-shea; doctor will not rewrite it.",
            ));
            return;
        }
    };
    let resolved = if target.is_absolute() {
        target
    } else {
        destination
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(target)
    };
    let target_metadata = match fs::metadata(&resolved) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            report.violations.push(local_skill_violation(
                label,
                destination,
                "local_skill_broken_symlink",
                format!(
                    "{label} skill `{}` is a broken symlink: `{}`.",
                    skill.name,
                    destination.display()
                ),
                "Rerun setup-shea or point the symlink at the skill directory; doctor will not repair it.",
            ));
            return;
        }
        Err(error) => {
            report.violations.push(local_skill_violation(
                label,
                destination,
                "local_skill_symlink_target_unreadable",
                format!(
                    "{label} skill `{}` symlink target could not be inspected: {error}.",
                    skill.name
                ),
                "Inspect local permissions or rerun setup-shea; doctor will not repair it.",
            ));
            return;
        }
    };

    if target_metadata.is_file() {
        report.violations.push(local_skill_violation(
            label,
            destination,
            "local_skill_symlink_targets_file",
            format!(
                "{label} skill `{}` symlink points at a file instead of the skill directory.",
                skill.name
            ),
            "Point the symlink at the skill directory or reinstall through the standard Skills CLI; doctor will not rewrite it.",
        ));
        return;
    }

    audit_skill_directory(report, label, manifest, skill, &resolved, source_skill);
}

fn audit_skill_directory(
    report: &mut ProjectAuditReport,
    label: &str,
    manifest: &SkillSuiteManifest,
    skill: &SkillSuiteEntry,
    destination: &Path,
    source_skill: &Path,
) {
    let skill_file = destination.join("SKILL.md");
    let text = match fs::read_to_string(&skill_file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            report.violations.push(local_skill_violation(
                label,
                destination,
                "local_skill_missing_skill_md",
                format!(
                    "{label} skill `{}` is missing `SKILL.md` in `{}`.",
                    skill.name,
                    destination.display()
                ),
                "Reinstall through setup-shea and the standard Skills CLI; doctor does not create missing files.",
            ));
            return;
        }
        Err(error) => {
            report.violations.push(local_skill_violation(
                label,
                &skill_file,
                "local_skill_md_unreadable",
                format!(
                    "{label} skill `{}` `SKILL.md` could not be read: {error}.",
                    skill.name
                ),
                "Check local filesystem permissions, then rerun `doctor`; doctor will not modify the file.",
            ));
            return;
        }
    };

    if !text.contains(&format!("name: {}", skill.name)) {
        report.violations.push(local_skill_violation(
            label,
            &skill_file,
            "local_skill_stale_name",
            format!(
                "{label} skill `{}` has stale or mismatched `name:` metadata.",
                skill.name
            ),
            "Run `npx skills list` and rerun setup-shea against the pinned repo-owned suite.",
        ));
    }

    if !text.contains(&format!("suite-version: {}", manifest.version)) {
        report.violations.push(local_skill_violation(
            label,
            &skill_file,
            "local_skill_stale_suite_version",
            format!(
                "{label} skill `{}` does not advertise suite-version `{}`.",
                skill.name, manifest.version
            ),
            "Rerun setup-shea to review and confirm a standard Skills CLI update.",
        ));
    }

    if text.contains("Shea CLI") {
        report.violations.push(local_skill_violation(
            label,
            &skill_file,
            "local_skill_stale_cli_naming",
            format!(
                "{label} skill `{}` still says `Shea CLI` instead of `Shea Symphony CLI`.",
                skill.name
            ),
            "Refresh the local skill from the repo-owned suite; doctor will not rewrite skill prose.",
        ));
    }

    if let Ok(source_text) = fs::read_to_string(source_skill) {
        if source_text != text {
            report.violations.push(local_skill_violation(
                label,
                &skill_file,
                "local_skill_drift",
                format!(
                    "{label} skill `{}` differs from the repo-owned suite copy.",
                    skill.name
                ),
                "Use the standard Skills CLI update path through setup-shea after reviewing the source revision and diff.",
            ));
        }
    }
}

fn local_skill_violation(
    label: &str,
    path: &Path,
    code: &str,
    message: String,
    suggestion: &str,
) -> ProjectAuditViolation {
    ProjectAuditViolation {
        issue_ref: format!("local-skill:{label}"),
        title: format!(
            "Local Shea Symphony skill install health: {}",
            path.display()
        ),
        state: "local".into(),
        severity: AuditSeverity::Warning,
        code: code.into(),
        message,
        suggestion: suggestion.into(),
    }
}
