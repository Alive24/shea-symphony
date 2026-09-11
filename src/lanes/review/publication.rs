//! Durable result publication and per-Issue exclusion for manual and loop Review.
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use shea_symphony::config::RuntimeConfig;
use shea_symphony::lane_claim::LaneClaim;
use shea_symphony::model::TrackerIssue;
use shea_symphony::review::{review_job_is_terminal, ReviewJob};
use shea_symphony::workspace::safe_identifier;

pub(super) type Failure = Box<dyn std::error::Error>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum PublicationState {
    Starting,
    Running,
    ResultReady,
    EvidencePublished,
    Complete,
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ReviewPublication {
    pub schema_version: u32,
    pub issue: TrackerIssue,
    pub claim: Option<String>,
    pub job: Option<ReviewJob>,
    pub state: PublicationState,
    pub runtime_revision: String,
    pub prompt_fingerprint: Option<String>,
    pub workspace: Option<PathBuf>,
    pub evidence: Option<String>,
    pub diagnostic: Option<String>,
}

impl ReviewPublication {
    pub fn new(issue: &TrackerIssue, claim: Option<&LaneClaim>) -> Self {
        Self {
            schema_version: 1,
            issue: issue.clone(),
            claim: claim.map(LaneClaim::render),
            job: None,
            state: PublicationState::Starting,
            runtime_revision: env!("SHEA_SOURCE_REVISION").into(),
            prompt_fingerprint: None,
            workspace: None,
            evidence: None,
            diagnostic: None,
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), Failure> {
        let parent = path.parent().ok_or("publication path has no parent")?;
        fs::create_dir_all(parent)?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(&serde_json::to_vec_pretty(self)?)?;
        staged.as_file().sync_all()?;
        staged.persist(path)?;
        Ok(())
    }

    pub fn read(path: &Path) -> Result<Self, Failure> {
        let receipt: Self = serde_json::from_slice(&fs::read(path)?)?;
        if receipt.schema_version != 1 {
            return Err("unsupported Review publication schema".into());
        }
        Ok(receipt)
    }
}

fn issue_root(config: &RuntimeConfig, issue_ref: &str) -> PathBuf {
    config
        .observability
        .logs_root
        .join("reviews")
        .join("publications")
        .join(safe_identifier(issue_ref))
}

pub(super) fn publication_path(
    config: &RuntimeConfig,
    issue_ref: &str,
    claim: Option<&LaneClaim>,
    job_id: &str,
) -> PathBuf {
    let run = claim.map(|c| c.run.as_str()).unwrap_or(job_id);
    issue_root(config, issue_ref).join(format!("{}.json", safe_identifier(run)))
}

// The OS releases the lock when the process exits. Never delete a live lock file.
// This excludes callers sharing this runtime root; tracker claim readback also
// detects a different owner. It is not a cross-host transactional GitHub lock.
pub(super) fn lock_issue(config: &RuntimeConfig, issue_ref: &str) -> Result<File, Failure> {
    let root = issue_root(config, issue_ref);
    fs::create_dir_all(&root)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("dispatch.lock"))?;
    file.try_lock_exclusive().map_err(|error| {
        format!("Review operation already owns {issue_ref}, or locking is unavailable: {error}")
    })?;
    Ok(file)
}

pub(super) fn pending_publication(
    config: &RuntimeConfig,
    issue_ref: &str,
) -> Result<Option<(PathBuf, ReviewPublication)>, Failure> {
    let root = issue_root(config, issue_ref);
    if !root.exists() {
        return Ok(None);
    }
    let mut pending = None;
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let receipt = ReviewPublication::read(&path)?;
        if receipt.issue.identifier != issue_ref {
            return Err("Review receipt Issue mismatch".into());
        }
        if matches!(
            receipt.state,
            PublicationState::Complete | PublicationState::Superseded
        ) {
            continue;
        }
        if pending.is_some() {
            return Err("multiple pending Review publications require Doctor triage".into());
        }
        pending = Some((path, receipt));
    }
    Ok(pending)
}

/// A completed job alone is not proof that its evidence was published.
pub(super) fn latest_publication_status(
    config: &RuntimeConfig,
    issue_ref: &str,
) -> Result<Option<serde_json::Value>, Failure> {
    let selected = if let Some(pending) = pending_publication(config, issue_ref)? {
        Some(pending)
    } else {
        let root = issue_root(config, issue_ref);
        if !root.exists() {
            return Ok(None);
        }
        let mut paths = fs::read_dir(root)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        paths.retain(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("json")
        });
        paths.sort_by_key(|path| {
            fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok()
        });
        paths
            .pop()
            .map(|path| ReviewPublication::read(&path).map(|receipt| (path, receipt)))
            .transpose()?
    };
    Ok(selected.map(|(path, receipt)| {
        serde_json::json!({
            "issue_ref": issue_ref, "state": receipt.state, "receipt_path": path,
            "run_id": receipt.job.as_ref().map(|job| &job.id),
            "runtime_revision": receipt.runtime_revision,
            "terminal_result_captured": require_terminal(&receipt).is_ok(),
            "diagnostic": receipt.diagnostic,
        })
    }))
}

pub(super) fn require_no_pending(config: &RuntimeConfig, issue_ref: &str) -> Result<(), Failure> {
    if let Some((path, receipt)) = pending_publication(config, issue_ref)? {
        return Err(format!(
            "Review has unfinished {:?} evidence at {}; inspect `review status` and use `review recover` for a captured terminal result before launching again",
            receipt.state, path.display()
        ).into());
    }
    Ok(())
}

pub(super) fn require_terminal(receipt: &ReviewPublication) -> Result<&ReviewJob, Failure> {
    receipt.job.as_ref().filter(|job| review_job_is_terminal(job)).ok_or_else(||
        "Review has no captured terminal result; preserve the claim and artifacts for Doctor triage; recovery does not launch a backend".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{test_config, tracker_issue};

    #[test]
    fn local_exclusion_and_pending_receipts_survive_caller_restart() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.observability.logs_root = temp.path().into();
        let issue = tracker_issue("Agent Review");
        let lock = lock_issue(&config, &issue.identifier).unwrap();
        assert!(lock_issue(&config, &issue.identifier).is_err());
        let path = publication_path(&config, &issue.identifier, None, "starting");
        let mut receipt = ReviewPublication::new(&issue, None);
        receipt.save(&path).unwrap();
        drop(lock);
        let _new_process_lock = lock_issue(&config, &issue.identifier).unwrap();
        assert!(require_no_pending(&config, &issue.identifier).is_err());
        assert!(require_terminal(&ReviewPublication::read(&path).unwrap()).is_err());
        receipt.state = PublicationState::Complete;
        receipt.save(&path).unwrap();
        require_no_pending(&config, &issue.identifier).unwrap();
        let status = latest_publication_status(&config, &issue.identifier)
            .unwrap()
            .unwrap();
        assert_eq!(status["state"], "Complete");
        assert_eq!(status["terminal_result_captured"], false);
        fs::write(&path, "invalid JSON").unwrap();
        assert!(latest_publication_status(&config, &issue.identifier).is_err());
    }
}
