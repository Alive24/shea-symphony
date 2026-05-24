use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::event_log::{EventLog, EventRecord};

pub const DEFAULT_PROGRESS_HEARTBEAT_THRESHOLD: Duration = Duration::from_secs(30);
pub const DEFAULT_PROGRESS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
pub const PROGRESS_HEARTBEAT_MS_ENV: &str = "SHEA_SYMPHONY_PROGRESS_HEARTBEAT_MS";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressHeartbeatConfig {
    pub enabled: bool,
    pub threshold: Duration,
    pub interval: Duration,
}

impl Default for ProgressHeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: DEFAULT_PROGRESS_HEARTBEAT_THRESHOLD,
            interval: DEFAULT_PROGRESS_HEARTBEAT_INTERVAL,
        }
    }
}

impl ProgressHeartbeatConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(value) = std::env::var(PROGRESS_HEARTBEAT_MS_ENV) {
            if let Ok(ms) = value.trim().parse::<u64>() {
                if ms == 0 {
                    config.enabled = false;
                } else {
                    let duration = Duration::from_millis(ms);
                    config.threshold = duration;
                    config.interval = duration;
                }
            }
        }
        config
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressHeartbeatSpec {
    wait: String,
    issue: Option<String>,
    pr: Option<String>,
    backend: Option<String>,
    artifact: Option<String>,
    next: Option<String>,
    event_log_path: Option<PathBuf>,
    actor_role: Option<String>,
    actor_label: Option<String>,
}

impl ProgressHeartbeatSpec {
    pub fn new(wait: impl Into<String>) -> Self {
        Self {
            wait: wait.into(),
            issue: None,
            pr: None,
            backend: None,
            artifact: None,
            next: Some("still_waiting".into()),
            event_log_path: None,
            actor_role: None,
            actor_label: None,
        }
    }

    pub fn issue(mut self, issue: impl Into<String>) -> Self {
        self.issue = Some(issue.into());
        self
    }

    pub fn pr(mut self, pr: impl Into<String>) -> Self {
        self.pr = Some(pr.into());
        self
    }

    pub fn backend(mut self, backend: impl Into<String>) -> Self {
        self.backend = Some(backend.into());
        self
    }

    pub fn artifact(mut self, artifact: impl Into<String>) -> Self {
        self.artifact = Some(artifact.into());
        self
    }

    pub fn next(mut self, next: impl Into<String>) -> Self {
        self.next = Some(next.into());
        self
    }

    pub fn event_log_path(mut self, path: impl AsRef<Path>) -> Self {
        self.event_log_path = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn actor(mut self, role: impl Into<String>, label: impl Into<String>) -> Self {
        self.actor_role = Some(role.into());
        self.actor_label = Some(label.into());
        self
    }
}

pub trait ProgressSink: Send + Sync {
    fn emit(&self, line: &str);
}

#[derive(Debug, Clone, Copy)]
pub struct StderrProgressSink;

impl ProgressSink for StderrProgressSink {
    fn emit(&self, line: &str) {
        eprintln!("{line}");
    }
}

pub struct ProgressHeartbeat {
    stopped: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

impl ProgressHeartbeat {
    pub fn start(spec: ProgressHeartbeatSpec) -> Self {
        Self::start_with_config_and_sink(
            spec,
            ProgressHeartbeatConfig::from_env(),
            Arc::new(StderrProgressSink),
        )
    }

    pub fn start_with_config_and_sink(
        spec: ProgressHeartbeatSpec,
        config: ProgressHeartbeatConfig,
        sink: Arc<dyn ProgressSink>,
    ) -> Self {
        if !config.enabled {
            return Self {
                stopped: Arc::new((Mutex::new(true), Condvar::new())),
                handle: None,
            };
        }

        let stopped = Arc::new((Mutex::new(false), Condvar::new()));
        let thread_stopped = Arc::clone(&stopped);
        let handle = thread::spawn(move || {
            run_progress_thread(spec, config, sink, thread_stopped);
        });

        Self {
            stopped,
            handle: Some(handle),
        }
    }
}

impl Drop for ProgressHeartbeat {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.stopped;
        if let Ok(mut stopped) = lock.lock() {
            *stopped = true;
            cvar.notify_all();
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn run_with_progress_heartbeat<T>(
    spec: ProgressHeartbeatSpec,
    operation: impl FnOnce() -> T,
) -> T {
    let _heartbeat = ProgressHeartbeat::start(spec);
    operation()
}

pub fn run_with_progress_heartbeat_config<T>(
    spec: ProgressHeartbeatSpec,
    config: ProgressHeartbeatConfig,
    sink: Arc<dyn ProgressSink>,
    operation: impl FnOnce() -> T,
) -> T {
    let _heartbeat = ProgressHeartbeat::start_with_config_and_sink(spec, config, sink);
    operation()
}

fn run_progress_thread(
    spec: ProgressHeartbeatSpec,
    config: ProgressHeartbeatConfig,
    sink: Arc<dyn ProgressSink>,
    stopped: Arc<(Mutex<bool>, Condvar)>,
) {
    let started = Instant::now();
    let mut delay = config.threshold;
    loop {
        let (lock, cvar) = &*stopped;
        let guard = match lock.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        let (guard, timeout) = match cvar.wait_timeout(guard, delay) {
            Ok(result) => result,
            Err(_) => return,
        };
        if *guard {
            return;
        }
        if !timeout.timed_out() {
            continue;
        }
        drop(guard);

        let line = format_progress_line(&spec, started.elapsed());
        sink.emit(&line);
        append_progress_event(&spec, &line);
        delay = config.interval;
    }
}

pub fn format_progress_line(spec: &ProgressHeartbeatSpec, elapsed: Duration) -> String {
    let mut fields = vec![
        "progress".to_string(),
        format!("wait={}", clean_progress_value(&spec.wait)),
        format!("elapsed={}", format_progress_elapsed(elapsed)),
    ];

    append_optional_field(&mut fields, "issue", spec.issue.as_deref());
    append_optional_field(&mut fields, "pr", spec.pr.as_deref());
    append_optional_field(&mut fields, "backend", spec.backend.as_deref());
    append_optional_field(&mut fields, "artifact", spec.artifact.as_deref());
    append_optional_field(&mut fields, "next", spec.next.as_deref());

    fields.join(" ")
}

fn append_optional_field(fields: &mut Vec<String>, key: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    fields.push(format!("{key}={}", clean_progress_value(value)));
}

fn clean_progress_value(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| {
            if ch.is_control() {
                None
            } else if ch.is_whitespace() {
                Some('_')
            } else {
                Some(ch)
            }
        })
        .collect()
}

fn format_progress_elapsed(elapsed: Duration) -> String {
    if elapsed.as_secs() > 0 {
        format!("{}s", elapsed.as_secs())
    } else {
        format!("{}ms", elapsed.as_millis())
    }
}

fn append_progress_event(spec: &ProgressHeartbeatSpec, line: &str) {
    let Some(path) = &spec.event_log_path else {
        return;
    };
    let log = EventLog::new(path);
    let _ = log.append(&EventRecord {
        event: "progress_heartbeat".into(),
        issue_id: None,
        issue_identifier: spec.issue.clone(),
        session_id: None,
        profile_id: None,
        instance_name: None,
        actor_role: spec.actor_role.clone(),
        actor_label: spec.actor_label.clone(),
        git_author: None,
        tracker_mutation: None,
        message: line.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct MemoryProgressSink {
        lines: Arc<Mutex<Vec<String>>>,
    }

    impl MemoryProgressSink {
        fn lines(&self) -> Vec<String> {
            self.lines.lock().unwrap().clone()
        }
    }

    impl ProgressSink for MemoryProgressSink {
        fn emit(&self, line: &str) {
            self.lines.lock().unwrap().push(line.to_string());
        }
    }

    fn test_config(threshold_ms: u64) -> ProgressHeartbeatConfig {
        ProgressHeartbeatConfig {
            enabled: true,
            threshold: Duration::from_millis(threshold_ms),
            interval: Duration::from_millis(threshold_ms),
        }
    }

    #[test]
    fn formats_compact_progress_line_with_context() {
        let spec = ProgressHeartbeatSpec::new("github project read")
            .issue("#318")
            .backend("gh cli")
            .artifact("/tmp/review job.json")
            .next("waiting for child");

        let line = format_progress_line(&spec, Duration::from_secs(30));

        assert_eq!(
            line,
            "progress wait=github_project_read elapsed=30s issue=#318 backend=gh_cli artifact=/tmp/review_job.json next=waiting_for_child"
        );
    }

    #[test]
    fn quick_operations_do_not_emit_heartbeat_before_threshold() {
        let sink = MemoryProgressSink::default();
        run_with_progress_heartbeat_config(
            ProgressHeartbeatSpec::new("github_project_read").issue("#318"),
            test_config(80),
            Arc::new(sink.clone()),
            || {
                thread::sleep(Duration::from_millis(10));
            },
        );

        assert!(sink.lines().is_empty());
    }

    #[test]
    fn mocked_long_tracker_read_emits_heartbeat_after_threshold() {
        let sink = MemoryProgressSink::default();
        run_with_progress_heartbeat_config(
            ProgressHeartbeatSpec::new("github_project_read")
                .issue("#318")
                .backend("gh")
                .next("still_waiting"),
            test_config(10),
            Arc::new(sink.clone()),
            || {
                thread::sleep(Duration::from_millis(35));
            },
        );

        let lines = sink.lines();
        assert!(!lines.is_empty());
        assert!(lines[0].contains("progress wait=github_project_read"));
        assert!(lines[0].contains("issue=#318"));
        assert!(lines[0].contains("next=still_waiting"));
    }

    #[test]
    fn mocked_lane_backend_wait_emits_heartbeat_and_event_log() {
        let sink = MemoryProgressSink::default();
        let temp = tempfile::tempdir().unwrap();
        let event_log = temp.path().join("shea-symphony.jsonl");
        run_with_progress_heartbeat_config(
            ProgressHeartbeatSpec::new("review_backend")
                .issue("#243")
                .backend("gemini-cli")
                .artifact("/tmp/job.json")
                .event_log_path(&event_log)
                .next("waiting_for_child"),
            test_config(10),
            Arc::new(sink.clone()),
            || {
                thread::sleep(Duration::from_millis(35));
            },
        );

        let lines = sink.lines();
        assert!(!lines.is_empty());
        assert!(lines[0].contains("wait=review_backend"));
        assert!(lines[0].contains("backend=gemini-cli"));
        assert!(lines[0].contains("next=waiting_for_child"));

        let records = EventLog::new(&event_log).read_records().unwrap();
        assert!(records
            .iter()
            .any(|record| record.event == "progress_heartbeat"
                && record.issue_identifier.as_deref() == Some("#243")));
    }

    #[test]
    fn heartbeat_sink_keeps_json_payload_separate_from_progress_lines() {
        let sink = MemoryProgressSink::default();
        let json = run_with_progress_heartbeat_config(
            ProgressHeartbeatSpec::new("github_project_read").issue("#318"),
            test_config(10),
            Arc::new(sink.clone()),
            || {
                thread::sleep(Duration::from_millis(35));
                serde_json::json!({"issue":"#318","state":"In Progress"}).to_string()
            },
        );

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["issue"], "#318");
        assert!(sink
            .lines()
            .iter()
            .all(|line| line.starts_with("progress ")));
        assert!(!json.contains("progress wait="));
    }
}
