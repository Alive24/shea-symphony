use std::path::Path;
use std::process::Command as ProcessCommand;

use jade_symphony::canonical_checkout::{
    canonical_checkout_refresh_status_line, canonical_checkout_status_line,
    canonical_checkout_warning_lines, inspect_canonical_checkout,
    refresh_canonical_checkout_before_write, CanonicalCheckoutRefreshMode,
};
use jade_symphony::config::RuntimeConfig;

use super::tracker_context::live_github_tracker;
use crate::{shell_quote_display, single_line};

pub(crate) fn report_canonical_checkout_readonly(config: &RuntimeConfig) -> Vec<String> {
    let root = match std::env::current_dir() {
        Ok(root) => root,
        Err(error) => {
            return vec![format!("canonical_checkout_error={error}")];
        }
    };
    match inspect_canonical_checkout(&root, config) {
        Ok(report) => vec![canonical_checkout_status_line(&report)],
        Err(error) => vec![format!("canonical_checkout_error={error}")],
    }
}

pub(crate) fn enforce_canonical_checkout_before_write(
    config: &RuntimeConfig,
    command: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    match refresh_canonical_checkout_before_write(
        &root,
        config,
        CanonicalCheckoutRefreshMode::Apply,
    ) {
        Ok(refresh) => {
            println!("{}", canonical_checkout_refresh_status_line(&refresh));
            println!("{}", canonical_checkout_status_line(&refresh.checkout));
            for line in canonical_checkout_warning_lines(&refresh.checkout) {
                println!("{command}_{line}");
            }
            Ok(())
        }
        Err(error) => {
            println!(
                "canonical_checkout_refresh=blocked command={} reason=\"{}\"",
                shell_quote_display(command),
                single_line(&error.to_string())
            );
            Err(error.into())
        }
    }
}

fn preview_canonical_checkout_before_dry_run(config: &RuntimeConfig, command: &str) {
    let Ok(root) = std::env::current_dir() else {
        println!(
            "canonical_checkout_refresh=blocked command={} reason=\"current directory unavailable\"",
            shell_quote_display(command)
        );
        return;
    };
    match refresh_canonical_checkout_before_write(
        &root,
        config,
        CanonicalCheckoutRefreshMode::DryRun,
    ) {
        Ok(refresh) => {
            println!("{}", canonical_checkout_refresh_status_line(&refresh));
            println!("{}", canonical_checkout_status_line(&refresh.checkout));
            for line in canonical_checkout_warning_lines(&refresh.checkout) {
                println!("{command}_{line}");
            }
        }
        Err(error) => {
            println!(
                "canonical_checkout_refresh=blocked command={} reason=\"{}\"",
                shell_quote_display(command),
                single_line(&error.to_string())
            );
        }
    }
}

pub(crate) fn preflight_canonical_checkout_for_write_mode(
    config: &RuntimeConfig,
    command: &str,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if write {
        enforce_canonical_checkout_before_write(config, command)
    } else {
        preview_canonical_checkout_before_dry_run(config, command);
        Ok(())
    }
}

pub(crate) fn append_canonical_checkout_gap(config: &RuntimeConfig, gaps: &mut Vec<String>) {
    if !live_github_tracker(config) {
        return;
    }
    let Ok(current_dir) = std::env::current_dir() else {
        gaps.push("canonical_checkout_blocked: current directory is unavailable".into());
        return;
    };
    if let Some(reason) = canonical_checkout_report(&current_dir).blocker() {
        gaps.push(format!("canonical_checkout_blocked: {reason}"));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CanonicalCheckoutReport {
    Ready,
    Blocked { reason: String },
}

impl CanonicalCheckoutReport {
    fn blocker(&self) -> Option<&str> {
        match self {
            Self::Ready => None,
            Self::Blocked { reason } => Some(reason.as_str()),
        }
    }
}

pub(crate) fn canonical_checkout_report(path: &Path) -> CanonicalCheckoutReport {
    let branch = match git_stdout(path, &["branch", "--show-current"]) {
        Ok(branch) if !branch.trim().is_empty() => branch.trim().to_string(),
        Ok(_) => {
            return CanonicalCheckoutReport::Blocked {
                reason: "HEAD is detached".into(),
            }
        }
        Err(error) => {
            return CanonicalCheckoutReport::Blocked {
                reason: format!("git branch check failed: {error}"),
            }
        }
    };
    if branch != "main" {
        return CanonicalCheckoutReport::Blocked {
            reason: format!("current branch is {branch:?}, expected \"main\""),
        };
    }

    if let Err(error) = git_status(path, &["fetch", "--quiet", "origin", "main"]) {
        return CanonicalCheckoutReport::Blocked {
            reason: format!("git fetch origin main failed: {error}"),
        };
    }

    let head = match git_stdout(path, &["rev-parse", "HEAD"]) {
        Ok(value) => value.trim().to_string(),
        Err(error) => {
            return CanonicalCheckoutReport::Blocked {
                reason: format!("cannot read HEAD: {error}"),
            }
        }
    };
    let origin_main = match git_stdout(path, &["rev-parse", "origin/main"]) {
        Ok(value) => value.trim().to_string(),
        Err(error) => {
            return CanonicalCheckoutReport::Blocked {
                reason: format!("cannot read origin/main: {error}"),
            }
        }
    };
    if head != origin_main {
        return CanonicalCheckoutReport::Blocked {
            reason: "local main does not exactly match origin/main".into(),
        };
    }

    CanonicalCheckoutReport::Ready
}

fn git_stdout(path: &Path, args: &[&str]) -> Result<String, String> {
    let output = ProcessCommand::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!(
                "git {:?} exited with status {:?}",
                args,
                output.status.code()
            )
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_status(path: &Path, args: &[&str]) -> Result<(), String> {
    git_stdout(path, args).map(|_| ())
}
