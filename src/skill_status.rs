use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::RuntimeConfig;
use crate::profiles::selected_execution_profile;
use crate::workflow::WorkflowDefinition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillStatusInput {
    pub workflow_path: PathBuf,
    pub suite_path: Option<PathBuf>,
    pub codex_dir: Option<PathBuf>,
    pub gemini_dir: Option<PathBuf>,
    pub require_gemini: bool,
    pub session_skills: Vec<String>,
    pub session_skills_file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillReadinessReport {
    pub source_suite: SourceSuiteReport,
    pub targets: Vec<SkillTargetReport>,
    pub session: SessionSkillReport,
    pub summary: SkillReadinessSummary,
    pub rows: Vec<SkillReadinessRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSuiteReport {
    pub status: String,
    pub source: String,
    pub path: Option<PathBuf>,
    pub manifest_path: Option<PathBuf>,
    pub version: Option<String>,
    pub release_date: Option<String>,
    pub repository: Option<String>,
    pub expected_count: usize,
    pub message: String,
    #[serde(default)]
    pub attempts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillTargetReport {
    pub label: String,
    pub root: PathBuf,
    pub configured: bool,
    pub required: bool,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSkillReport {
    pub status: String,
    pub provided_count: usize,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillReadinessSummary {
    pub expected_skills: usize,
    pub row_count: usize,
    pub warnings: usize,
    pub blockers: usize,
    pub source_suite_status: String,
    pub codex_status: String,
    pub gemini_status: String,
    pub session_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillReadinessRow {
    pub skill_name: String,
    pub source_status: String,
    pub codex_install_status: String,
    pub gemini_install_status: String,
    pub current_session_status: String,
    pub rendered_metadata_status: String,
    pub path_target: String,
    pub link_status: String,
    pub recommended_action: String,
    pub severity: String,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone)]
struct SkillSuite {
    report: SourceSuiteReport,
    skills: Vec<SourceSkill>,
}

#[derive(Debug, Clone)]
struct SourceSkill {
    name: String,
    path: PathBuf,
    skill_file: Option<PathBuf>,
    skill_text: Option<String>,
}

#[derive(Debug, Clone)]
struct TargetSpec {
    label: String,
    root: PathBuf,
    configured: bool,
    required: bool,
}

#[derive(Debug, Clone)]
struct InstallInspection {
    status: String,
    path: PathBuf,
    link_status: String,
    metadata_status: String,
    severity: String,
    diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct SkillMetadata {
    name: Option<String>,
    suite_version: Option<String>,
    source_repo: Option<String>,
    workflow_path: Option<String>,
    source_suite_path: Option<String>,
    rendered_at: Option<String>,
    source_hash: Option<String>,
}

pub fn build_skill_readiness_report(input: SkillStatusInput) -> SkillReadinessReport {
    let suite = discover_source_suite(&input);
    let targets = target_specs(&input);
    let session_skills = read_session_skill_set(&input);
    let source_names = suite
        .skills
        .iter()
        .map(|skill| skill.name.clone())
        .collect::<BTreeSet<_>>();
    let installed_names = if source_names.is_empty() {
        discover_installed_skill_names(&targets)
    } else {
        BTreeSet::new()
    };
    let mut names = source_names
        .into_iter()
        .chain(installed_names)
        .collect::<BTreeSet<_>>();
    if names.is_empty() {
        names.extend(session_skills.iter().cloned());
    }

    let source_by_name = suite
        .skills
        .iter()
        .cloned()
        .map(|skill| (skill.name.clone(), skill))
        .collect::<BTreeMap<_, _>>();
    let mut rows = Vec::new();
    for name in names {
        let source = source_by_name.get(&name);
        rows.push(build_row(
            &name,
            source,
            &suite.report,
            &targets,
            &session_skills,
        ));
    }

    let target_reports = targets.iter().map(target_report).collect::<Vec<_>>();
    let summary = summarize(&suite.report, &target_reports, &session_skills, &rows);
    let session = SessionSkillReport {
        status: if session_skills.is_empty() {
            "unknown".into()
        } else {
            "provided".into()
        },
        provided_count: session_skills.len(),
        source: if input.session_skills_file.is_some() {
            "file_or_inline".into()
        } else if input.session_skills.is_empty() {
            "none".into()
        } else {
            "inline".into()
        },
    };

    SkillReadinessReport {
        source_suite: suite.report,
        targets: target_reports,
        session,
        summary,
        rows,
    }
}

pub fn render_skill_readiness_report(report: &SkillReadinessReport) -> String {
    let mut lines = vec![
        "skills_status=ok".to_string(),
        format!(
            "source_suite status={} source={} path={} expected={} version={} release_date={}",
            report.source_suite.status,
            report.source_suite.source,
            option_path(report.source_suite.path.as_deref()),
            report.source_suite.expected_count,
            option_text(report.source_suite.version.as_deref()),
            option_text(report.source_suite.release_date.as_deref())
        ),
        format!(
            "summary expected={} rows={} warnings={} blockers={} codex={} gemini={} session={}",
            report.summary.expected_skills,
            report.summary.row_count,
            report.summary.warnings,
            report.summary.blockers,
            report.summary.codex_status,
            report.summary.gemini_status,
            report.summary.session_status
        ),
    ];
    for attempt in &report.source_suite.attempts {
        lines.push(format!("source_suite_attempt={attempt}"));
    }
    for target in &report.targets {
        lines.push(format!(
            "target label={} status={} configured={} required={} root={}",
            target.label,
            target.status,
            target.configured,
            target.required,
            target.root.display()
        ));
    }
    for row in &report.rows {
        lines.push(format!(
            "skill={} source={} codex={} gemini={} session={} metadata={} link={} severity={} path_target=\"{}\" action=\"{}\"",
            row.skill_name,
            row.source_status,
            row.codex_install_status,
            row.gemini_install_status,
            row.current_session_status,
            row.rendered_metadata_status,
            row.link_status,
            row.severity,
            row.path_target,
            row.recommended_action
        ));
        for diagnostic in &row.diagnostics {
            lines.push(format!("  diagnostic={diagnostic}"));
        }
    }
    lines.join("\n")
}

pub fn render_skill_readiness_report_json(
    report: &SkillReadinessReport,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

pub fn doctor_skill_readiness_summary(input: SkillStatusInput) -> String {
    let report = build_skill_readiness_report(input);
    format!(
        "skill_readiness source_suite={} expected={} codex={} gemini={} session={} warnings={} blockers={}",
        report.summary.source_suite_status,
        report.summary.expected_skills,
        report.summary.codex_status,
        report.summary.gemini_status,
        report.summary.session_status,
        report.summary.warnings,
        report.summary.blockers
    )
}

fn discover_source_suite(input: &SkillStatusInput) -> SkillSuite {
    let mut attempts = Vec::new();
    let mut candidates = Vec::new();
    if let Some(path) = &input.suite_path {
        candidates.push(("explicit", path.clone()));
    }
    if let Some(path) = nonempty_env_path("SHEA_SYMPHONY_SKILL_SUITE") {
        candidates.push(("env", path));
    }
    if let Some(repo_suite) = repo_suite_path(&input.workflow_path) {
        candidates.push(("repo", repo_suite));
    }

    for (source, path) in candidates {
        match read_source_suite(&path, source) {
            Ok(mut suite) => {
                suite.report.attempts = attempts;
                return suite;
            }
            Err(error) => attempts.push(format!("{source}:{}:{error}", path.display())),
        }
    }

    SkillSuite {
        report: SourceSuiteReport {
            status: "installed-only".into(),
            source: "installed-only".into(),
            path: None,
            manifest_path: None,
            version: None,
            release_date: None,
            repository: None,
            expected_count: 0,
            message:
                "No source suite was discovered; reporting installed Shea Symphony skills only."
                    .into(),
            attempts,
        },
        skills: Vec::new(),
    }
}

fn repo_suite_path(workflow_path: &Path) -> Option<PathBuf> {
    let start = if workflow_path.is_absolute() {
        workflow_path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(workflow_path)
    };
    let mut cursor = start.parent()?.to_path_buf();
    loop {
        let suite_path = cursor.join("skills").join("shea-symphony").join("suite");
        if suite_path.exists() {
            return Some(suite_path);
        }
        if !cursor.pop() {
            return None;
        }
    }
}

fn read_source_suite(path: &Path, source: &str) -> Result<SkillSuite, String> {
    let (suite_dir, manifest_path) = normalize_suite_path(path);
    if !suite_dir.is_dir() {
        return Err("suite path is not a directory".into());
    }

    let manifest = manifest_path
        .as_ref()
        .and_then(|path| fs::read_to_string(path).ok());
    let manifest_entries = manifest
        .as_deref()
        .map(parse_manifest_entries)
        .unwrap_or_default();
    let manifest_by_name = manifest_entries
        .iter()
        .filter(|entry| entry.default_install)
        .map(|entry| (entry.name.clone(), entry.path.clone()))
        .collect::<BTreeMap<_, _>>();
    let excluded_manifest_names = manifest_entries
        .iter()
        .filter(|entry| !entry.default_install)
        .map(|entry| entry.name.clone())
        .collect::<BTreeSet<_>>();

    let mut names = BTreeSet::new();
    names.extend(
        manifest_entries
            .iter()
            .filter(|entry| entry.default_install)
            .map(|entry| entry.name.clone()),
    );
    for entry in fs::read_dir(&suite_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if !excluded_manifest_names.contains(&name) {
                names.insert(name);
            }
        }
    }

    if names.is_empty() {
        return Err("suite contains no skill directories or manifest entries".into());
    }

    let mut skills = Vec::new();
    for name in names {
        let path = manifest_by_name
            .get(&name)
            .map(|relative| manifest_root(&suite_dir).join(relative))
            .unwrap_or_else(|| suite_dir.join(&name));
        let skill_file = path.join("SKILL.md");
        let skill_text = fs::read_to_string(&skill_file).ok();
        skills.push(SourceSkill {
            name,
            path,
            skill_file: skill_file.exists().then_some(skill_file),
            skill_text,
        });
    }

    let (version, release_date, repository) = manifest
        .as_deref()
        .map(|text| {
            (
                manifest_string_value(text, "version"),
                manifest_string_value(text, "release_date"),
                manifest_string_value(text, "repository"),
            )
        })
        .unwrap_or((None, None, None));
    let expected_count = skills.len();
    Ok(SkillSuite {
        report: SourceSuiteReport {
            status: "present".into(),
            source: source.into(),
            path: Some(suite_dir),
            manifest_path,
            version,
            release_date,
            repository,
            expected_count,
            message: "Source suite discovered.".into(),
            attempts: Vec::new(),
        },
        skills,
    })
}

fn normalize_suite_path(path: &Path) -> (PathBuf, Option<PathBuf>) {
    if path.join("manifest.toml").is_file() && path.join("suite").is_dir() {
        (path.join("suite"), Some(path.join("manifest.toml")))
    } else if path.file_name().and_then(|name| name.to_str()) == Some("suite") {
        let manifest = path.parent().map(|parent| parent.join("manifest.toml"));
        let manifest = manifest.filter(|path| path.is_file());
        (path.to_path_buf(), manifest)
    } else {
        let manifest = path.join("manifest.toml");
        (path.to_path_buf(), manifest.is_file().then_some(manifest))
    }
}

fn manifest_root(suite_dir: &Path) -> PathBuf {
    suite_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| suite_dir.to_path_buf())
}

#[derive(Debug, Clone)]
struct ManifestSkillEntry {
    name: String,
    path: PathBuf,
    default_install: bool,
}

fn parse_manifest_entries(text: &str) -> Vec<ManifestSkillEntry> {
    let mut entries = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_path: Option<PathBuf> = None;
    let mut current_default_install = true;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "[[skills]]" {
            push_manifest_entry(
                &mut entries,
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
    push_manifest_entry(
        &mut entries,
        &mut current_name,
        &mut current_path,
        &mut current_default_install,
    );
    entries
}

fn push_manifest_entry(
    entries: &mut Vec<ManifestSkillEntry>,
    current_name: &mut Option<String>,
    current_path: &mut Option<PathBuf>,
    current_default_install: &mut bool,
) {
    if let (Some(name), Some(path)) = (current_name.take(), current_path.take()) {
        entries.push(ManifestSkillEntry {
            name,
            path,
            default_install: *current_default_install,
        });
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

fn target_specs(input: &SkillStatusInput) -> Vec<TargetSpec> {
    let codex_root = input
        .codex_dir
        .clone()
        .unwrap_or_else(|| default_codex_root(input));
    vec![
        TargetSpec {
            label: "codex".into(),
            root: codex_root,
            configured: input.codex_dir.is_some()
                || nonempty_env_path("SHEA_SYMPHONY_CODEX_SKILLS_DIR").is_some()
                || profile_working_dir_from_workflow(&input.workflow_path).is_some(),
            required: true,
        },
        TargetSpec {
            label: "gemini".into(),
            root: input.gemini_dir.clone().unwrap_or_else(default_gemini_root),
            configured: input.gemini_dir.is_some()
                || input.require_gemini
                || nonempty_env_path("SHEA_SYMPHONY_GEMINI_SKILLS_DIR").is_some()
                || nonempty_env_path("GEMINI_HOME").is_some()
                || default_gemini_root().exists(),
            required: input.require_gemini,
        },
    ]
}

fn default_codex_root(input: &SkillStatusInput) -> PathBuf {
    if let Some(path) = nonempty_env_path("SHEA_SYMPHONY_CODEX_SKILLS_DIR") {
        return path;
    }
    if let Some(path) = profile_working_dir_from_workflow(&input.workflow_path) {
        return path.join(".agents").join("skills");
    }
    workflow_repo_root(&input.workflow_path)
        .join(".agents")
        .join("skills")
}

fn default_gemini_root() -> PathBuf {
    if let Some(path) = nonempty_env_path("SHEA_SYMPHONY_GEMINI_SKILLS_DIR") {
        return path;
    }
    if let Some(home) = nonempty_env_path("GEMINI_HOME") {
        return home.join("local-skills");
    }
    home_dir().join(".gemini").join("local-skills")
}

fn profile_working_dir_from_workflow(workflow_path: &Path) -> Option<PathBuf> {
    let path = absolute_workflow_path(workflow_path)?;
    let text = fs::read_to_string(&path).ok()?;
    let workflow = WorkflowDefinition::parse(&path, &text).ok()?;
    let config = RuntimeConfig::from_workflow(&workflow, &path).ok()?;
    selected_execution_profile(&config.profiles)
        .ok()
        .flatten()
        .and_then(|profile| profile.working_dir)
}

fn workflow_repo_root(workflow_path: &Path) -> PathBuf {
    let Some(start) = absolute_workflow_path(workflow_path) else {
        return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    };
    let mut cursor = start
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let fallback = if cursor.file_name().and_then(|name| name.to_str()) == Some("workflows") {
        cursor
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| cursor.clone())
    } else {
        cursor.clone()
    };
    loop {
        if cursor.join(".git").exists()
            || cursor.join(".codex").exists()
            || cursor.join("skills").join("shea-symphony").exists()
        {
            return cursor;
        }
        if !cursor.pop() {
            return fallback;
        }
    }
}

fn absolute_workflow_path(workflow_path: &Path) -> Option<PathBuf> {
    if workflow_path.is_absolute() {
        Some(workflow_path.to_path_buf())
    } else {
        std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join(workflow_path))
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn nonempty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn target_report(target: &TargetSpec) -> SkillTargetReport {
    let status = if !target.configured && !target.required && !target.root.exists() {
        "not_configured"
    } else if target.root.is_dir() {
        "present"
    } else if target.root.exists() {
        "not_directory"
    } else if target.required {
        "missing_required"
    } else {
        "missing_optional"
    };
    SkillTargetReport {
        label: target.label.clone(),
        root: target.root.clone(),
        configured: target.configured,
        required: target.required,
        status: status.into(),
    }
}

fn discover_installed_skill_names(targets: &[TargetSpec]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for target in targets {
        if !target.root.is_dir() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&target.root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("shea-symphony-") {
                    names.insert(name);
                }
            }
        }
    }
    names
}

fn read_session_skill_set(input: &SkillStatusInput) -> BTreeSet<String> {
    let mut skills = BTreeSet::new();
    for value in &input.session_skills {
        skills.extend(extract_skill_names(value));
    }
    if let Some(path) = &input.session_skills_file {
        if let Ok(text) = fs::read_to_string(path) {
            skills.extend(extract_skill_names(&text));
        }
    }
    skills
}

fn extract_skill_names(text: &str) -> BTreeSet<String> {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .filter(|token| token.starts_with("shea-symphony-"))
        .map(str::to_string)
        .collect()
}

fn build_row(
    name: &str,
    source: Option<&SourceSkill>,
    suite: &SourceSuiteReport,
    targets: &[TargetSpec],
    session_skills: &BTreeSet<String>,
) -> SkillReadinessRow {
    let codex = targets
        .iter()
        .find(|target| target.label == "codex")
        .map(|target| inspect_install(target, name, source, suite))
        .unwrap_or_else(|| missing_target_inspection("codex", name));
    let gemini = targets
        .iter()
        .find(|target| target.label == "gemini")
        .map(|target| inspect_install(target, name, source, suite))
        .unwrap_or_else(|| missing_target_inspection("gemini", name));
    let source_status = match source {
        Some(skill) if skill.skill_file.is_some() => "present",
        Some(_) => "missing_skill_md",
        None if suite.status == "installed-only" => "unknown_installed_only",
        None => "missing_from_source",
    };
    let current_session_status = if session_skills.is_empty() {
        "unknown"
    } else if session_skills.contains(name) {
        "exposed"
    } else {
        "missing"
    };
    let (metadata_status, metadata_diagnostics) =
        aggregate_metadata_status(source, suite, &[&codex, &gemini]);
    let link_status = aggregate_link_status(&[&codex, &gemini]);
    let mut diagnostics = Vec::new();
    diagnostics.extend(codex.diagnostics.clone());
    diagnostics.extend(gemini.diagnostics.clone());
    diagnostics.extend(metadata_diagnostics);
    if current_session_status == "missing" && installed_somewhere(&codex, &gemini) {
        diagnostics.push(
            "Skill is expected and installed but not exposed in provided session input.".into(),
        );
    }
    let severity = aggregate_severity(&codex, &gemini, current_session_status, &metadata_status);
    let recommended_action =
        recommended_action(&codex, &gemini, current_session_status, &metadata_status);
    let path_target = path_target(source, &codex, &gemini);
    SkillReadinessRow {
        skill_name: name.into(),
        source_status: source_status.into(),
        codex_install_status: codex.status,
        gemini_install_status: gemini.status,
        current_session_status: current_session_status.into(),
        rendered_metadata_status: metadata_status,
        path_target,
        link_status,
        recommended_action,
        severity,
        diagnostics,
    }
}

fn inspect_install(
    target: &TargetSpec,
    name: &str,
    source: Option<&SourceSkill>,
    suite: &SourceSuiteReport,
) -> InstallInspection {
    let path = target.root.join(name);
    if !target.configured && !target.required && !target.root.exists() {
        return InstallInspection {
            status: "not_configured".into(),
            path,
            link_status: "not_configured".into(),
            metadata_status: "unknown".into(),
            severity: "ok".into(),
            diagnostics: Vec::new(),
        };
    }
    if !target.root.exists() {
        return InstallInspection {
            status: if target.required {
                "root_missing_required"
            } else {
                "root_missing"
            }
            .into(),
            path,
            link_status: "root_missing".into(),
            metadata_status: "unknown".into(),
            severity: if target.required { "blocker" } else { "ok" }.into(),
            diagnostics: vec![format!(
                "{} skill root is missing: {}",
                target.label,
                target.root.display()
            )],
        };
    }

    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return InstallInspection {
                status: "missing".into(),
                path,
                link_status: "missing".into(),
                metadata_status: "unknown".into(),
                severity: if target.required { "warning" } else { "ok" }.into(),
                diagnostics: Vec::new(),
            };
        }
        Err(error) => {
            return InstallInspection {
                status: "unreadable".into(),
                path,
                link_status: "unreadable".into(),
                metadata_status: "unknown".into(),
                severity: "warning".into(),
                diagnostics: vec![format!(
                    "{} skill path could not be inspected: {error}",
                    target.label
                )],
            };
        }
    };

    if metadata.file_type().is_symlink() {
        return inspect_symlink(target, name, &path, source, suite);
    }
    if metadata.is_file() {
        return InstallInspection {
            status: "path_is_file".into(),
            path,
            link_status: "points_to_file".into(),
            metadata_status: "unknown".into(),
            severity: "warning".into(),
            diagnostics: vec![format!(
                "{} skill path is a file where a skill directory is expected.",
                target.label
            )],
        };
    }
    if !metadata.is_dir() {
        return InstallInspection {
            status: "not_directory".into(),
            path,
            link_status: "not_directory".into(),
            metadata_status: "unknown".into(),
            severity: "warning".into(),
            diagnostics: vec![format!("{} skill path is not a directory.", target.label)],
        };
    }
    inspect_directory(target, name, &path, source, suite, "ok")
}

fn inspect_symlink(
    target: &TargetSpec,
    name: &str,
    path: &Path,
    source: Option<&SourceSkill>,
    suite: &SourceSuiteReport,
) -> InstallInspection {
    let link_target = match fs::read_link(path) {
        Ok(link_target) => link_target,
        Err(error) => {
            return InstallInspection {
                status: "symlink_unreadable".into(),
                path: path.to_path_buf(),
                link_status: "symlink_unreadable".into(),
                metadata_status: "unknown".into(),
                severity: "warning".into(),
                diagnostics: vec![format!(
                    "{} symlink target could not be read: {error}",
                    target.label
                )],
            };
        }
    };
    let resolved = if link_target.is_absolute() {
        link_target
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .join(link_target)
    };
    let metadata = match fs::metadata(&resolved) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return InstallInspection {
                status: "broken_symlink".into(),
                path: path.to_path_buf(),
                link_status: "broken_symlink".into(),
                metadata_status: "unknown".into(),
                severity: "warning".into(),
                diagnostics: vec![format!(
                    "{} skill symlink is broken: {}",
                    target.label,
                    path.display()
                )],
            };
        }
        Err(error) => {
            return InstallInspection {
                status: "symlink_target_unreadable".into(),
                path: path.to_path_buf(),
                link_status: "symlink_target_unreadable".into(),
                metadata_status: "unknown".into(),
                severity: "warning".into(),
                diagnostics: vec![format!(
                    "{} symlink target could not be inspected: {error}",
                    target.label
                )],
            };
        }
    };
    if metadata.is_file() {
        return InstallInspection {
            status: "symlink_targets_file".into(),
            path: path.to_path_buf(),
            link_status: "points_to_file".into(),
            metadata_status: "unknown".into(),
            severity: "warning".into(),
            diagnostics: vec![format!(
                "{} skill symlink points at a file instead of a skill directory.",
                target.label
            )],
        };
    }
    if metadata.is_dir() {
        return inspect_directory(target, name, &resolved, source, suite, "symlink_ok");
    }
    InstallInspection {
        status: "symlink_target_not_directory".into(),
        path: path.to_path_buf(),
        link_status: "symlink_target_not_directory".into(),
        metadata_status: "unknown".into(),
        severity: "warning".into(),
        diagnostics: vec![format!(
            "{} skill symlink target is not a directory.",
            target.label
        )],
    }
}

fn inspect_directory(
    target: &TargetSpec,
    name: &str,
    path: &Path,
    source: Option<&SourceSkill>,
    suite: &SourceSuiteReport,
    link_status: &str,
) -> InstallInspection {
    let skill_file = path.join("SKILL.md");
    let text = match fs::read_to_string(&skill_file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return InstallInspection {
                status: "missing_skill_md".into(),
                path: path.to_path_buf(),
                link_status: link_status.into(),
                metadata_status: "missing".into(),
                severity: "warning".into(),
                diagnostics: vec![format!("{} skill is missing SKILL.md.", target.label)],
            };
        }
        Err(error) => {
            return InstallInspection {
                status: "skill_md_unreadable".into(),
                path: path.to_path_buf(),
                link_status: link_status.into(),
                metadata_status: "unknown".into(),
                severity: "warning".into(),
                diagnostics: vec![format!(
                    "{} SKILL.md could not be read: {error}",
                    target.label
                )],
            };
        }
    };

    let metadata = parse_skill_metadata(&text);
    let mut diagnostics = Vec::new();
    let mut metadata_status = "ok";
    let mut status = "installed";
    let mut severity = "ok";

    if metadata.name.as_deref() != Some(name) {
        metadata_status = "mismatched";
        status = "metadata_mismatch";
        severity = "warning";
        diagnostics.push(format!(
            "{} skill name metadata does not match `{name}`.",
            target.label
        ));
    }
    if let Some(version) = suite.version.as_deref() {
        if metadata.suite_version.as_deref() != Some(version) {
            metadata_status = "stale";
            status = "stale_metadata";
            severity = "warning";
            diagnostics.push(format!(
                "{} suite-version metadata is {:?}, expected `{version}`.",
                target.label, metadata.suite_version
            ));
        }
    }
    if let Some(repository) = suite.repository.as_deref() {
        if let Some(source_repo) = metadata.source_repo.as_deref() {
            if source_repo != repository {
                metadata_status = "stale";
                status = "stale_metadata";
                severity = "warning";
                diagnostics.push(format!(
                    "{} source-repo metadata is `{source_repo}`, expected `{repository}`.",
                    target.label
                ));
            }
        }
    }
    if metadata.workflow_path.is_none()
        || metadata.source_suite_path.is_none()
        || (metadata.rendered_at.is_none() && metadata.source_hash.is_none())
    {
        if metadata_status == "ok" {
            metadata_status = "partial";
        }
        diagnostics.push(format!(
            "{} rendered metadata is partial or unavailable for workflow/source/timestamp/hash.",
            target.label
        ));
    }
    if let Some(source) = source {
        if let Some(source_text) = source.skill_text.as_deref() {
            if source_text != text {
                status = "drift";
                severity = "warning";
                diagnostics.push(format!(
                    "{} SKILL.md differs from source suite copy.",
                    target.label
                ));
            }
        }
    }
    InstallInspection {
        status: status.into(),
        path: path.to_path_buf(),
        link_status: link_status.into(),
        metadata_status: metadata_status.into(),
        severity: severity.into(),
        diagnostics,
    }
}

fn parse_skill_metadata(text: &str) -> SkillMetadata {
    let mut metadata = SkillMetadata::default();
    let front_matter = if let Some(rest) = text.strip_prefix("---") {
        rest.split_once("---")
            .map(|(front, _)| front)
            .unwrap_or(rest)
    } else {
        text
    };
    for line in front_matter.lines() {
        let trimmed = line.trim();
        if let Some(value) = yaml_string_value(trimmed, "name") {
            metadata.name = Some(value);
        } else if let Some(value) = yaml_string_value(trimmed, "suite-version") {
            metadata.suite_version = Some(value);
        } else if let Some(value) = yaml_string_value(trimmed, "source-repo") {
            metadata.source_repo = Some(value);
        } else if let Some(value) = yaml_string_value(trimmed, "workflow-path") {
            metadata.workflow_path = Some(value);
        } else if let Some(value) = yaml_string_value(trimmed, "source-suite-path") {
            metadata.source_suite_path = Some(value);
        } else if let Some(value) = yaml_string_value(trimmed, "rendered-at") {
            metadata.rendered_at = Some(value);
        } else if let Some(value) = yaml_string_value(trimmed, "source-hash") {
            metadata.source_hash = Some(value);
        }
    }
    metadata
}

fn yaml_string_value(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    line.strip_prefix(&prefix)
        .map(str::trim)
        .map(|value| value.trim_matches('"').trim_matches('\'').to_string())
        .filter(|value| !value.is_empty())
}

fn missing_target_inspection(label: &str, name: &str) -> InstallInspection {
    InstallInspection {
        status: "not_checked".into(),
        path: PathBuf::from(format!("{label}/{name}")),
        link_status: "not_checked".into(),
        metadata_status: "unknown".into(),
        severity: "ok".into(),
        diagnostics: Vec::new(),
    }
}

fn aggregate_metadata_status(
    source: Option<&SourceSkill>,
    suite: &SourceSuiteReport,
    installs: &[&InstallInspection],
) -> (String, Vec<String>) {
    let mut diagnostics = Vec::new();
    if let Some(source) = source {
        if source.skill_file.is_none() {
            diagnostics.push("Source suite entry is missing SKILL.md.".into());
            return ("missing".into(), diagnostics);
        }
        if let Some(text) = source.skill_text.as_deref() {
            let metadata = parse_skill_metadata(text);
            if metadata.name.as_deref() != Some(source.name.as_str()) {
                diagnostics
                    .push("Source skill name metadata does not match expected skill name.".into());
                return ("source_mismatch".into(), diagnostics);
            }
            if let Some(version) = suite.version.as_deref() {
                if metadata.suite_version.as_deref() != Some(version) {
                    diagnostics.push(format!(
                        "Source skill suite-version metadata is {:?}, expected `{version}`.",
                        metadata.suite_version
                    ));
                    return ("source_stale".into(), diagnostics);
                }
            }
        }
    }
    if installs
        .iter()
        .any(|install| install.metadata_status == "mismatched")
    {
        ("mismatched".into(), diagnostics)
    } else if installs
        .iter()
        .any(|install| install.metadata_status == "stale")
    {
        ("stale".into(), diagnostics)
    } else if installs
        .iter()
        .any(|install| install.metadata_status == "partial")
    {
        ("partial".into(), diagnostics)
    } else if installs
        .iter()
        .all(|install| install.metadata_status == "unknown")
    {
        ("unknown".into(), diagnostics)
    } else {
        ("ok".into(), diagnostics)
    }
}

fn aggregate_link_status(installs: &[&InstallInspection]) -> String {
    if installs
        .iter()
        .any(|install| install.link_status == "broken_symlink")
    {
        "broken_symlink".into()
    } else if installs
        .iter()
        .any(|install| install.link_status == "points_to_file")
    {
        "points_to_file".into()
    } else if installs
        .iter()
        .any(|install| install.link_status.contains("symlink"))
    {
        "symlink".into()
    } else {
        "ok".into()
    }
}

fn installed_somewhere(codex: &InstallInspection, gemini: &InstallInspection) -> bool {
    [&codex.status, &gemini.status]
        .iter()
        .any(|status| matches!(status.as_str(), "installed" | "drift" | "stale_metadata"))
}

fn aggregate_severity(
    codex: &InstallInspection,
    gemini: &InstallInspection,
    current_session_status: &str,
    metadata_status: &str,
) -> String {
    if codex.severity == "blocker" || gemini.severity == "blocker" {
        "blocker".into()
    } else if codex.severity == "warning"
        || gemini.severity == "warning"
        || current_session_status == "missing"
        || matches!(
            metadata_status,
            "stale" | "mismatched" | "source_stale" | "source_mismatch" | "missing" | "partial"
        )
    {
        "warning".into()
    } else {
        "ok".into()
    }
}

fn recommended_action(
    codex: &InstallInspection,
    gemini: &InstallInspection,
    current_session_status: &str,
    metadata_status: &str,
) -> String {
    if codex.status == "missing" {
        "Preview the per-repo install/update path for Codex; no automatic repair was performed."
            .into()
    } else if gemini.status == "root_missing_required" {
        "Configure Gemini local-skills root or rerun without --require-gemini.".into()
    } else if matches!(
        codex.status.as_str(),
        "broken_symlink" | "symlink_targets_file" | "path_is_file" | "missing_skill_md"
    ) || matches!(
        gemini.status.as_str(),
        "broken_symlink" | "symlink_targets_file" | "path_is_file" | "missing_skill_md"
    ) {
        "Inspect the reported path and intentionally reinstall or repair; skills status is read-only.".into()
    } else if current_session_status == "missing" {
        "Start a new agent session or provide a session export that exposes this skill.".into()
    } else if matches!(
        metadata_status,
        "stale" | "mismatched" | "source_stale" | "source_mismatch"
    ) {
        "Refresh from the per-repo rendered skill suite after reviewing the diff.".into()
    } else if metadata_status == "partial" {
        "Rendered metadata is partial; validate source suite and installer metadata before relying on freshness.".into()
    } else {
        "No action required.".into()
    }
}

fn path_target(
    source: Option<&SourceSkill>,
    codex: &InstallInspection,
    gemini: &InstallInspection,
) -> String {
    format!(
        "source={}; codex={}; gemini={}",
        source
            .map(|skill| skill.path.display().to_string())
            .unwrap_or_else(|| "unknown".into()),
        codex.path.display(),
        gemini.path.display()
    )
}

fn summarize(
    source_suite: &SourceSuiteReport,
    targets: &[SkillTargetReport],
    session_skills: &BTreeSet<String>,
    rows: &[SkillReadinessRow],
) -> SkillReadinessSummary {
    let warnings = rows.iter().filter(|row| row.severity == "warning").count();
    let blockers = rows.iter().filter(|row| row.severity == "blocker").count();
    let target_status = |label: &str| {
        targets
            .iter()
            .find(|target| target.label == label)
            .map(|target| target.status.clone())
            .unwrap_or_else(|| "not_checked".into())
    };
    SkillReadinessSummary {
        expected_skills: source_suite.expected_count,
        row_count: rows.len(),
        warnings,
        blockers,
        source_suite_status: source_suite.status.clone(),
        codex_status: target_status("codex"),
        gemini_status: target_status("gemini"),
        session_status: if session_skills.is_empty() {
            "unknown".into()
        } else {
            "provided".into()
        },
    }
}

fn option_path(path: Option<&Path>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn option_text(value: Option<&str>) -> String {
    value.unwrap_or("unknown").into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs as unix_fs;
    use tempfile::TempDir;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn suite(root: &Path) -> PathBuf {
        let suite_root = root.join("skills/shea-symphony");
        write(
            &suite_root.join("manifest.toml"),
            r#"version = "2026.05.18"
release_date = "2026-05-18"
repository = "Alive24/shea-symphony"

[[skills]]
name = "shea-symphony-doctor"
path = "suite/shea-symphony-doctor"

[[skills]]
name = "shea-symphony-issue-forge-dream"
path = "suite/shea-symphony-issue-forge-dream"
"#,
        );
        write(
            &suite_root.join("suite/shea-symphony-doctor/SKILL.md"),
            "---\nname: shea-symphony-doctor\nmetadata:\n  suite-version: 2026.05.18\n  workflow-path: workflows/shea-symphony.md\n  source-suite-path: skills/shea-symphony/suite\n  rendered-at: 2026-05-18\n---\n",
        );
        write(
            &suite_root.join("suite/shea-symphony-issue-forge-dream/SKILL.md"),
            "---\nname: shea-symphony-issue-forge-dream\nmetadata:\n  suite-version: 2026.05.18\n  workflow-path: workflows/shea-symphony.md\n  source-suite-path: skills/shea-symphony/suite\n  rendered-at: 2026-05-18\n---\n",
        );
        suite_root.join("suite")
    }

    fn input(
        repo: &Path,
        suite_path: Option<PathBuf>,
        codex: PathBuf,
        gemini: PathBuf,
    ) -> SkillStatusInput {
        SkillStatusInput {
            workflow_path: repo.join("workflows/shea-symphony.md"),
            suite_path,
            codex_dir: Some(codex),
            gemini_dir: Some(gemini),
            require_gemini: false,
            session_skills: Vec::new(),
            session_skills_file: None,
        }
    }

    #[test]
    fn discovers_explicit_suite_before_repo_suite() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let explicit = temp.path().join("explicit");
        fs::create_dir_all(repo.join("workflows")).unwrap();
        write(&repo.join("workflows/shea-symphony.md"), "");
        let repo_suite = suite(&repo);
        let explicit_suite = suite(&explicit);
        write(
            &explicit_suite.join("shea-symphony-extra/SKILL.md"),
            "---\nname: shea-symphony-extra\nmetadata:\n  suite-version: 2026.05.18\n---\n",
        );
        let report = build_skill_readiness_report(input(
            &repo,
            Some(explicit_suite),
            temp.path().join("codex"),
            temp.path().join("gemini"),
        ));
        assert_eq!(report.source_suite.source, "explicit");
        assert!(report
            .rows
            .iter()
            .any(|row| row.skill_name == "shea-symphony-extra"));
        assert!(!repo_suite.as_os_str().is_empty());
    }

    #[test]
    fn installed_only_mode_does_not_fail_without_source_suite() {
        let temp = TempDir::new().unwrap();
        let codex = temp.path().join("codex");
        write(
            &codex.join("shea-symphony-doctor/SKILL.md"),
            "---\nname: shea-symphony-doctor\nmetadata:\n  suite-version: 2026.05.18\n---\n",
        );
        let report = build_skill_readiness_report(input(
            temp.path(),
            Some(temp.path().join("missing-suite")),
            codex,
            temp.path().join("missing-gemini"),
        ));
        assert_eq!(report.source_suite.status, "installed-only");
        assert!(report
            .rows
            .iter()
            .any(|row| row.skill_name == "shea-symphony-doctor"));
    }

    #[test]
    fn provided_session_input_marks_missing_expected_skill() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let source_suite = suite(&repo);
        let codex = temp.path().join("codex");
        write(
            &codex.join("shea-symphony-doctor/SKILL.md"),
            &fs::read_to_string(source_suite.join("shea-symphony-doctor/SKILL.md")).unwrap(),
        );
        let mut status_input = input(&repo, Some(source_suite), codex, temp.path().join("gemini"));
        status_input.session_skills = vec!["shea-symphony-doctor".into()];
        let report = build_skill_readiness_report(status_input);
        let dream = report
            .rows
            .iter()
            .find(|row| row.skill_name == "shea-symphony-issue-forge-dream")
            .unwrap();
        assert_eq!(dream.current_session_status, "missing");
    }

    #[test]
    fn excludes_manifest_skills_marked_out_of_normal_install() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let source_suite = suite(&repo);
        let manifest_path = source_suite.parent().unwrap().join("manifest.toml");
        let manifest = fs::read_to_string(&manifest_path).unwrap().replace(
            "path = \"suite/shea-symphony-issue-forge-dream\"",
            "path = \"suite/shea-symphony-issue-forge-dream\"\ndefault_install = false",
        );
        fs::write(manifest_path, manifest).unwrap();

        let report = build_skill_readiness_report(input(
            &repo,
            Some(source_suite),
            temp.path().join("codex"),
            temp.path().join("gemini"),
        ));

        assert!(!report
            .rows
            .iter()
            .any(|row| row.skill_name == "shea-symphony-issue-forge-dream"));
    }

    #[test]
    fn reports_broken_symlink_file_alias_and_missing_skill_md() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let source_suite = suite(&repo);
        let codex = temp.path().join("codex");
        let gemini = temp.path().join("gemini");
        fs::create_dir_all(&codex).unwrap();
        fs::create_dir_all(&gemini).unwrap();
        unix_fs::symlink(
            temp.path().join("missing-target"),
            codex.join("shea-symphony-doctor"),
        )
        .unwrap();
        write(&gemini.join("shea-symphony-doctor"), "alias to a file");
        fs::create_dir_all(codex.join("shea-symphony-issue-forge-dream")).unwrap();
        let report = build_skill_readiness_report(input(&repo, Some(source_suite), codex, gemini));
        let doctor = report
            .rows
            .iter()
            .find(|row| row.skill_name == "shea-symphony-doctor")
            .unwrap();
        assert_eq!(doctor.link_status, "broken_symlink");
        assert_eq!(doctor.gemini_install_status, "path_is_file");
        let dream = report
            .rows
            .iter()
            .find(|row| row.skill_name == "shea-symphony-issue-forge-dream")
            .unwrap();
        assert_eq!(dream.codex_install_status, "missing_skill_md");
    }

    #[test]
    fn optional_gemini_root_absence_is_not_a_blocker() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let source_suite = suite(&repo);
        let codex = temp.path().join("codex");
        write(
            &codex.join("shea-symphony-doctor/SKILL.md"),
            &fs::read_to_string(source_suite.join("shea-symphony-doctor/SKILL.md")).unwrap(),
        );
        let report = build_skill_readiness_report(input(
            &repo,
            Some(source_suite),
            codex,
            temp.path().join("missing-gemini"),
        ));
        assert_eq!(report.summary.blockers, 0);
        assert_eq!(report.summary.gemini_status, "missing_optional");
    }

    #[test]
    fn default_codex_target_is_workflow_repo_local_skills() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        write(&repo.join("workflows/shea-symphony.md"), "---\n---\nPrompt");
        let source_suite = suite(&repo);
        let repo_codex = repo.join(".agents/skills");
        write(
            &repo_codex.join("shea-symphony-doctor/SKILL.md"),
            &fs::read_to_string(source_suite.join("shea-symphony-doctor/SKILL.md")).unwrap(),
        );

        let report = build_skill_readiness_report(SkillStatusInput {
            workflow_path: repo.join("workflows/shea-symphony.md"),
            suite_path: Some(source_suite),
            codex_dir: None,
            gemini_dir: Some(temp.path().join("missing-gemini")),
            require_gemini: false,
            session_skills: Vec::new(),
            session_skills_file: None,
        });

        let codex_target = report
            .targets
            .iter()
            .find(|target| target.label == "codex")
            .unwrap();
        assert_eq!(codex_target.root, repo_codex);
        assert_eq!(codex_target.status, "present");
    }

    #[test]
    fn selected_profile_working_dir_sets_target_codex_skills_root() {
        let temp = TempDir::new().unwrap();
        let engine = temp.path().join("engine");
        let target = temp.path().join("target-repo");
        let workflow_path = engine.join("workflows/shea-symphony.md");
        fs::create_dir_all(engine.join(".git")).unwrap();
        let source_suite = suite(&engine);
        write(
            &workflow_path,
            &format!(
                "---\nprofiles:\n  default: target\n  entries:\n    - id: target\n      working_dir: {:?}\n---\nPrompt",
                target.display().to_string()
            ),
        );
        let target_codex = target.join(".agents/skills");
        write(
            &target_codex.join("shea-symphony-doctor/SKILL.md"),
            &fs::read_to_string(source_suite.join("shea-symphony-doctor/SKILL.md")).unwrap(),
        );

        let report = build_skill_readiness_report(SkillStatusInput {
            workflow_path,
            suite_path: Some(source_suite),
            codex_dir: None,
            gemini_dir: Some(temp.path().join("missing-gemini")),
            require_gemini: false,
            session_skills: Vec::new(),
            session_skills_file: None,
        });

        let codex_target = report
            .targets
            .iter()
            .find(|target| target.label == "codex")
            .unwrap();
        assert_eq!(codex_target.root, target_codex);
        assert_eq!(codex_target.status, "present");
    }
}
