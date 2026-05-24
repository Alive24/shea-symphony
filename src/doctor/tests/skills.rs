use std::fs;
use std::path::Path;

use super::super::{
    append_local_skill_install_doctor_violations, AuditSeverity, ProjectAuditReport,
    SkillInstallTarget,
};

#[test]
fn reports_missing_local_skill_root_as_warning() {
    let temp = tempfile::tempdir().unwrap();
    write_skill_suite(
        temp.path(),
        &[("shea-symphony-doctor", "suite/shea-symphony-doctor")],
    );
    let target = SkillInstallTarget {
        label: "Codex".into(),
        root: temp.path().join("missing-codex-skills"),
    };
    let mut report = ProjectAuditReport {
        total_issues: 0,
        violations: Vec::new(),
        integration_gaps: Vec::new(),
        skill_readiness_summary: None,
    };

    append_local_skill_install_doctor_violations(&mut report, temp.path(), &[target]);

    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].severity, AuditSeverity::Warning);
    assert_eq!(report.violations[0].code, "local_skill_root_missing");
    assert!(report.violations[0]
        .suggestion
        .contains("install-shea-symphony-skills.js --dry-run"));
}

#[test]
fn reports_unhealthy_local_skill_shapes_and_stale_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let skills = [
        ("shea-symphony-alias-file", "suite/shea-symphony-alias-file"),
        ("shea-symphony-missing-md", "suite/shea-symphony-missing-md"),
        ("shea-symphony-stale", "suite/shea-symphony-stale"),
        ("shea-symphony-file-link", "suite/shea-symphony-file-link"),
        (
            "shea-symphony-broken-link",
            "suite/shea-symphony-broken-link",
        ),
    ];
    write_skill_suite(temp.path(), &skills);
    let target_root = temp.path().join("codex-skills");
    fs::create_dir_all(&target_root).unwrap();

    fs::write(target_root.join("shea-symphony-alias-file"), "alias").unwrap();
    fs::create_dir_all(target_root.join("shea-symphony-missing-md")).unwrap();
    fs::create_dir_all(target_root.join("shea-symphony-stale")).unwrap();
    fs::write(
        target_root.join("shea-symphony-stale").join("SKILL.md"),
        "---\nname: stale-shea-skill\nmetadata:\n  suite-version: 2026.01.01\n---\nUse Shea CLI here.\n",
    )
    .unwrap();
    let file_target = temp.path().join("target-SKILL.md");
    fs::write(
        &file_target,
        skill_text("shea-symphony-file-link", "2026.05.17"),
    )
    .unwrap();
    symlink_file(&file_target, &target_root.join("shea-symphony-file-link"));
    symlink_file(
        &temp.path().join("does-not-exist"),
        &target_root.join("shea-symphony-broken-link"),
    );

    let target = SkillInstallTarget {
        label: "Codex".into(),
        root: target_root,
    };
    let mut report = ProjectAuditReport {
        total_issues: 0,
        violations: Vec::new(),
        integration_gaps: Vec::new(),
        skill_readiness_summary: None,
    };

    append_local_skill_install_doctor_violations(&mut report, temp.path(), &[target]);

    let codes = report
        .violations
        .iter()
        .map(|violation| violation.code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"local_skill_expected_directory_file"));
    assert!(codes.contains(&"local_skill_missing_skill_md"));
    assert!(codes.contains(&"local_skill_stale_name"));
    assert!(codes.contains(&"local_skill_stale_suite_version"));
    assert!(codes.contains(&"local_skill_stale_cli_naming"));
    assert!(codes.contains(&"local_skill_symlink_targets_file"));
    assert!(codes.contains(&"local_skill_broken_symlink"));
}

#[test]
fn accepts_healthy_local_skill_directory() {
    let temp = tempfile::tempdir().unwrap();
    write_skill_suite(
        temp.path(),
        &[("shea-symphony-doctor", "suite/shea-symphony-doctor")],
    );
    let target_root = temp.path().join("gemini-skills");
    let destination = target_root.join("shea-symphony-doctor");
    fs::create_dir_all(&destination).unwrap();
    fs::write(
        destination.join("SKILL.md"),
        skill_text("shea-symphony-doctor", "2026.05.17"),
    )
    .unwrap();
    let target = SkillInstallTarget {
        label: "Gemini".into(),
        root: target_root,
    };
    let mut report = ProjectAuditReport {
        total_issues: 0,
        violations: Vec::new(),
        integration_gaps: Vec::new(),
        skill_readiness_summary: None,
    };

    append_local_skill_install_doctor_violations(&mut report, temp.path(), &[target]);

    assert!(report.violations.is_empty());
}

fn write_skill_suite(repo_root: &Path, skills: &[(&str, &str)]) {
    let suite_root = repo_root.join("skills").join("shea-symphony");
    fs::create_dir_all(&suite_root).unwrap();
    let mut manifest =
        "suite_name = \"Shea Symphony skill suite\"\nversion = \"2026.05.17\"\n".to_string();
    for (name, path) in skills {
        manifest.push_str(&format!(
            "\n[[skills]]\nname = \"{name}\"\npath = \"{path}\"\n"
        ));
        let skill_dir = suite_root.join(path);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), skill_text(name, "2026.05.17")).unwrap();
    }
    fs::write(suite_root.join("manifest.toml"), manifest).unwrap();
}

fn skill_text(name: &str, version: &str) -> String {
    format!(
        "---\nname: {name}\nmetadata:\n  suite-version: {version}\n---\nUse the Shea Symphony CLI.\n"
    )
}

#[cfg(unix)]
fn symlink_file(source: &Path, destination: &Path) {
    std::os::unix::fs::symlink(source, destination).unwrap();
}

#[cfg(windows)]
fn symlink_file(source: &Path, destination: &Path) {
    std::os::windows::fs::symlink_file(source, destination).unwrap();
}
