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
        &[("jade-symphony-doctor", "suite/jade-symphony-doctor")],
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
        .contains("install-jade-symphony-skills.js --dry-run"));
}

#[test]
fn reports_unhealthy_local_skill_shapes_and_stale_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let skills = [
        ("jade-symphony-alias-file", "suite/jade-symphony-alias-file"),
        ("jade-symphony-missing-md", "suite/jade-symphony-missing-md"),
        ("jade-symphony-stale", "suite/jade-symphony-stale"),
        ("jade-symphony-file-link", "suite/jade-symphony-file-link"),
        (
            "jade-symphony-broken-link",
            "suite/jade-symphony-broken-link",
        ),
    ];
    write_skill_suite(temp.path(), &skills);
    let target_root = temp.path().join("codex-skills");
    fs::create_dir_all(&target_root).unwrap();

    fs::write(target_root.join("jade-symphony-alias-file"), "alias").unwrap();
    fs::create_dir_all(target_root.join("jade-symphony-missing-md")).unwrap();
    fs::create_dir_all(target_root.join("jade-symphony-stale")).unwrap();
    fs::write(
        target_root.join("jade-symphony-stale").join("SKILL.md"),
        "---\nname: stale-jade-skill\nmetadata:\n  suite-version: 2026.01.01\n---\nUse Jade CLI here.\n",
    )
    .unwrap();
    let file_target = temp.path().join("target-SKILL.md");
    fs::write(
        &file_target,
        skill_text("jade-symphony-file-link", "2026.05.17"),
    )
    .unwrap();
    symlink_file(&file_target, &target_root.join("jade-symphony-file-link"));
    symlink_file(
        &temp.path().join("does-not-exist"),
        &target_root.join("jade-symphony-broken-link"),
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
        &[("jade-symphony-doctor", "suite/jade-symphony-doctor")],
    );
    let target_root = temp.path().join("gemini-skills");
    let destination = target_root.join("jade-symphony-doctor");
    fs::create_dir_all(&destination).unwrap();
    fs::write(
        destination.join("SKILL.md"),
        skill_text("jade-symphony-doctor", "2026.05.17"),
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
    let suite_root = repo_root.join("skills").join("jade-symphony");
    fs::create_dir_all(&suite_root).unwrap();
    let mut manifest =
        "suite_name = \"Jade Symphony skill suite\"\nversion = \"2026.05.17\"\n".to_string();
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
        "---\nname: {name}\nmetadata:\n  suite-version: {version}\n---\nUse the Jade Symphony CLI.\n"
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
