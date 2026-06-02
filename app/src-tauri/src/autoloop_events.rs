use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::autoloop_state::{default_lane, AutoloopLine, LaneSnapshot, LoopStateSnapshot};

pub fn push_recent_line(state: &mut LoopStateSnapshot, line: AutoloopLine) {
    state.recent_lines.push(line);
    if state.recent_lines.len() > 200 {
        let overflow = state.recent_lines.len() - 200;
        state.recent_lines.drain(0..overflow);
    }
}

pub fn parse_autoloop_event(line: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("source").and_then(Value::as_str) != Some("shea-symphony") {
        return None;
    }
    value.get("event").and_then(Value::as_str)?;
    Some(value)
}

pub fn parse_autoloop_text_event(stream: &str, line: &str) -> Option<Value> {
    if line.trim().is_empty() {
        return None;
    }
    Some(json!({
        "schema_version": 1,
        "source": "shea-symphony",
        "event": "autopilot_cli_line",
        "payload": {
            "stream": stream,
            "kind": cli_line_kind(line),
            "raw": line,
            "fields": parse_cli_line_fields(line),
        }
    }))
}

pub fn parse_autoloop_lane_event(event: Option<&Value>, at_ms: u128) -> Option<LaneSnapshot> {
    let event = event?;
    if event.get("event").and_then(Value::as_str)? != "autopilot_loop_lane" {
        return None;
    }
    let payload = event.get("payload")?;
    let lane = string_json_field(payload, "lane")?;
    Some(LaneSnapshot {
        lane,
        status: string_json_field(payload, "status").unwrap_or_else(|| "unknown".into()),
        action: optional_json_field(payload, "action"),
        selected: selected_issue_field(payload),
        target: optional_json_field(payload, "target_state")
            .or_else(|| optional_json_field(payload, "target")),
        work_unit_completed: payload
            .get("work_unit_completed")
            .or_else(|| payload.get("workUnitCompleted"))
            .and_then(Value::as_bool),
        completed_work_units: payload
            .get("completed_work_units")
            .or_else(|| payload.get("completedWorkUnits"))
            .and_then(Value::as_u64)
            .map(|value| value as usize),
        issue_ref: optional_json_field(payload, "issue_ref"),
        latest_result: latest_result_field(payload),
        max_concurrent: payload
            .get("max_concurrent")
            .or_else(|| payload.get("maxConcurrent"))
            .and_then(Value::as_u64)
            .map(|value| value as usize),
        recover: payload.get("recover").and_then(Value::as_bool),
        updated_at_ms: Some(at_ms),
        latest_line: Some(event.to_string()),
    })
}

pub fn parse_autoloop_lane(line: &str, at_ms: u128) -> Option<LaneSnapshot> {
    if !line.starts_with("autopilot_loop_lane ") {
        return None;
    }
    let fields = parse_key_values(line);
    let lane = fields.get("lane")?.to_string();
    Some(LaneSnapshot {
        lane: lane.clone(),
        status: fields
            .get("status")
            .cloned()
            .unwrap_or_else(|| "unknown".into()),
        action: optional_field(&fields, "action"),
        selected: optional_field(&fields, "selected"),
        target: optional_field(&fields, "target"),
        work_unit_completed: fields
            .get("work_unit_completed")
            .and_then(|value| value.parse::<bool>().ok()),
        completed_work_units: fields
            .get("completed_work_units")
            .and_then(|value| value.parse::<usize>().ok()),
        issue_ref: optional_field(&fields, "issue_ref"),
        latest_result: optional_field(&fields, "latest_result"),
        max_concurrent: fields
            .get("max_concurrent")
            .and_then(|value| value.parse::<usize>().ok()),
        recover: fields
            .get("recover")
            .and_then(|value| value.parse::<bool>().ok()),
        updated_at_ms: Some(at_ms),
        latest_line: Some(line.into()),
    })
}

pub fn apply_autoloop_result(state: &mut LoopStateSnapshot, line: &str) -> bool {
    if !line.starts_with("autopilot_loop_result ") {
        return false;
    }
    let fields = parse_key_values(line);
    if let Some(mode) = fields.get("mode") {
        state.mode = mode.clone();
    }
    let lane_concurrency = [
        ("main", "main_max_concurrent"),
        ("review", "review_max_concurrent"),
        ("merge", "merge_max_concurrent"),
    ];
    for (lane, field) in lane_concurrency {
        if let Some(value) = fields
            .get(field)
            .and_then(|value| value.parse::<usize>().ok())
        {
            state
                .lanes
                .entry(lane.into())
                .or_insert_with(|| default_lane(lane))
                .max_concurrent = Some(value);
        }
    }
    true
}

pub fn apply_autoloop_event(
    state: &mut LoopStateSnapshot,
    event: Option<&Value>,
    at_ms: u128,
) -> bool {
    let Some(event) = event else {
        return false;
    };
    let Some(event_name) = event.get("event").and_then(Value::as_str) else {
        return false;
    };
    let payload = event.get("payload").unwrap_or(&Value::Null);
    match event_name {
        "autopilot_loop_status" => {
            if let Some(mode) = string_json_field(payload, "mode") {
                state.mode = mode;
            }
            apply_json_settings(state, payload.get("settings"));
            apply_json_lane_work_units(state, payload.get("lane_work_units"));
            if let Some(lanes) = payload.get("lane_activity").and_then(Value::as_array) {
                for lane_payload in lanes {
                    if let Some(lane) = parse_status_lane_activity(lane_payload, at_ms) {
                        state.lanes.insert(lane.lane.clone(), lane);
                    }
                }
            }
            true
        }
        "autopilot_loop_iteration" => {
            if let Some(mode) = string_json_field(payload, "mode") {
                state.mode = mode;
            }
            apply_json_settings(state, payload.get("settings"));
            apply_json_lane_work_units(state, payload.get("lane_work_units"));
            true
        }
        "autopilot_loop_result" => {
            if let Some(mode) = string_json_field(payload, "mode") {
                state.mode = mode;
            }
            apply_json_settings(state, payload.get("settings"));
            apply_json_lane_work_units(state, payload.get("lane_work_units"));
            if let Some(lanes) = payload.get("lanes").and_then(Value::as_array) {
                for lane_payload in lanes {
                    if let Some(lane) = parse_autoloop_lane_event(
                        Some(&json!({
                            "source": "shea-symphony",
                            "event": "autopilot_loop_lane",
                            "payload": lane_payload,
                        })),
                        at_ms,
                    ) {
                        state.lanes.insert(lane.lane.clone(), lane);
                    }
                }
            }
            true
        }
        "autopilot_loop_stopped" => {
            state.running = false;
            state.stopping = false;
            state.pid = None;
            state.stopped_at_ms = Some(at_ms);
            true
        }
        _ => false,
    }
}

pub fn apply_autoloop_stopped(state: &mut LoopStateSnapshot, line: &str, at_ms: u128) -> bool {
    if !line.starts_with("autopilot_loop=stopped ") {
        return false;
    }
    state.running = false;
    state.stopping = false;
    state.pid = None;
    state.stopped_at_ms = Some(at_ms);
    true
}

fn cli_line_kind(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.starts_with("Latest:") {
        return "latest".into();
    }
    trimmed
        .split_once(['=', ' '])
        .map(|(kind, _)| kind.trim_end_matches(':').to_string())
        .filter(|kind| !kind.is_empty())
        .unwrap_or_else(|| "line".into())
}

fn parse_cli_line_fields(line: &str) -> Value {
    let trimmed = line.trim();
    if trimmed.starts_with("Latest:") {
        return parse_latest_line(trimmed);
    }
    let mut fields = serde_json::Map::new();
    for (key, value) in parse_shell_like_key_values(trimmed) {
        fields.insert(key, Value::String(value));
    }
    Value::Object(fields)
}

fn parse_latest_line(line: &str) -> Value {
    let parts = line
        .strip_prefix("Latest:")
        .unwrap_or(line)
        .split('|')
        .map(str::trim)
        .collect::<Vec<_>>();
    let mut fields = serde_json::Map::new();
    for (key, value) in ["lane", "issue", "status", "action", "title"]
        .into_iter()
        .zip(parts.iter().copied())
    {
        if !value.is_empty() {
            fields.insert(key.into(), Value::String(value.into()));
        }
    }
    for part in parts.into_iter().skip(5) {
        if let Some((key, value)) = part.split_once('=') {
            fields.insert(key.into(), Value::String(value.into()));
        }
    }
    Value::Object(fields)
}

fn parse_shell_like_key_values(line: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for token in shell_like_tokens(line) {
        if let Some((key, value)) = token.split_once('=') {
            fields.insert(key.to_string(), value.to_string());
        }
    }
    fields
}

fn shell_like_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some(active), current_ch) if current_ch == active => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, ch) if ch.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            (_, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            (_, ch) => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn parse_status_lane_activity(payload: &Value, at_ms: u128) -> Option<LaneSnapshot> {
    let lane = string_json_field(payload, "lane")?;
    Some(LaneSnapshot {
        lane,
        status: string_json_field(payload, "status").unwrap_or_else(|| "unknown".into()),
        action: optional_json_field(payload, "action"),
        selected: selected_issue_field(payload),
        target: optional_json_field(payload, "target_state")
            .or_else(|| optional_json_field(payload, "target")),
        work_unit_completed: None,
        completed_work_units: None,
        issue_ref: selected_issue_field(payload),
        latest_result: Some(
            [
                string_json_field(payload, "status").unwrap_or_else(|| "unknown".into()),
                optional_json_field(payload, "action").unwrap_or_else(|| "event".into()),
            ]
            .join(":"),
        ),
        max_concurrent: None,
        recover: None,
        updated_at_ms: Some(at_ms),
        latest_line: Some(payload.to_string()),
    })
}

fn apply_json_settings(state: &mut LoopStateSnapshot, settings: Option<&Value>) {
    let Some(settings) = settings else {
        return;
    };
    let lane_concurrency = [
        ("main", "main_max_concurrent"),
        ("review", "review_max_concurrent"),
        ("merge", "merge_max_concurrent"),
    ];
    for (lane, field) in lane_concurrency {
        if let Some(value) = settings
            .get(field)
            .or_else(|| settings.get(&snake_to_camel(field)))
            .and_then(Value::as_u64)
        {
            state
                .lanes
                .entry(lane.into())
                .or_insert_with(|| default_lane(lane))
                .max_concurrent = Some(value as usize);
        }
    }
}

fn apply_json_lane_work_units(state: &mut LoopStateSnapshot, lane_work_units: Option<&Value>) {
    let Some(lane_work_units) = lane_work_units.and_then(Value::as_object) else {
        return;
    };
    for (lane, value) in lane_work_units {
        if let Some(count) = value.as_u64() {
            state
                .lanes
                .entry(lane.clone())
                .or_insert_with(|| default_lane(lane))
                .completed_work_units = Some(count as usize);
        }
    }
}

fn optional_field(fields: &BTreeMap<String, String>, key: &str) -> Option<String> {
    fields
        .get(key)
        .filter(|value| !value.is_empty() && value.as_str() != "none")
        .cloned()
}

fn string_json_field(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            payload
                .get(&snake_to_camel(key))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn optional_json_field(payload: &Value, key: &str) -> Option<String> {
    string_json_field(payload, key).filter(|value| !value.is_empty() && value != "none")
}

fn selected_issue_field(payload: &Value) -> Option<String> {
    optional_json_field(payload, "selected").or_else(|| {
        payload
            .get("selected_issue")
            .or_else(|| payload.get("selectedIssue"))
            .and_then(|issue| {
                issue
                    .get("identifier")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| issue.as_str().map(str::to_string))
            })
            .filter(|value| !value.is_empty() && value != "none")
    })
}

fn latest_result_field(payload: &Value) -> Option<String> {
    if let Some(value) = optional_json_field(payload, "latest_result") {
        return Some(value);
    }
    let latest = payload
        .get("latest_result")
        .or_else(|| payload.get("latestResult"))?;
    if latest.is_object() {
        let status = string_json_field(latest, "status").unwrap_or_else(|| "unknown".into());
        let action = optional_json_field(latest, "action").unwrap_or_else(|| "event".into());
        let issue = optional_json_field(latest, "issue_ref")
            .map(|value| format!(":{value}"))
            .unwrap_or_default();
        return Some(format!("{status}:{action}{issue}"));
    }
    latest.as_str().map(str::to_string)
}

fn snake_to_camel(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut uppercase_next = false;
    for ch in value.chars() {
        if ch == '_' {
            uppercase_next = true;
        } else if uppercase_next {
            output.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            output.push(ch);
        }
    }
    output
}

fn parse_key_values(line: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for token in line.split_whitespace().skip(1) {
        if let Some((key, value)) = token.split_once('=') {
            fields.insert(key.to_string(), unquote(value));
        }
    }
    fields
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_autopilot_lane_line() {
        let lane = parse_autoloop_lane(
            "autopilot_loop_lane lane=review status=completed action=lane_tick_completed selected=#421 target=HumanReview max_concurrent=2 recover=false",
            42,
        )
        .unwrap();

        assert_eq!(lane.lane, "review");
        assert_eq!(lane.status, "completed");
        assert_eq!(lane.action.as_deref(), Some("lane_tick_completed"));
        assert_eq!(lane.selected.as_deref(), Some("#421"));
        assert_eq!(lane.work_unit_completed, None);
        assert_eq!(lane.completed_work_units, None);
        assert_eq!(lane.max_concurrent, Some(2));
        assert_eq!(lane.recover, Some(false));
        assert_eq!(lane.updated_at_ms, Some(42));
    }

    #[test]
    fn parses_autopilot_lane_work_unit_line() {
        let lane = parse_autoloop_lane(
            "autopilot_loop_lane lane=merge status=completed action=lane_tick_completed selected=#412 target=Done work_unit_completed=true completed_work_units=2 issue_ref=#412 max_concurrent=1 recover=true",
            45,
        )
        .unwrap();

        assert_eq!(lane.lane, "merge");
        assert_eq!(lane.status, "completed");
        assert_eq!(lane.issue_ref.as_deref(), Some("#412"));
        assert_eq!(lane.work_unit_completed, Some(true));
        assert_eq!(lane.completed_work_units, Some(2));
    }

    #[test]
    fn parses_autopilot_running_lane_line() {
        let lane = parse_autoloop_lane(
            "autopilot_loop_lane lane=main status=running action=tick_started selected=#364 target=AgentReview max_concurrent=3 recover=true",
            84,
        )
        .unwrap();

        assert_eq!(lane.lane, "main");
        assert_eq!(lane.status, "running");
        assert_eq!(lane.action.as_deref(), Some("tick_started"));
        assert_eq!(lane.selected.as_deref(), Some("#364"));
        assert_eq!(lane.target.as_deref(), Some("AgentReview"));
    }

    #[test]
    fn parses_autopilot_json_lane_event() {
        let event = parse_autoloop_event(
            r##"{"schema_version":1,"source":"shea-symphony","event":"autopilot_loop_lane","payload":{"lane":"review","status":"completed","action":"lane_tick_completed","work_unit_completed":true,"completed_work_units":3,"issue_ref":"#364","latest_result":{"status":"completed","action":"lane_tick_completed","issue_ref":"#364"},"selected_issue":{"identifier":"#364","title":"Issue title","state":"Agent Review","url":null,"priority":null,"pull_request":null},"target_state":"Human Review | Rework","max_concurrent":2,"recover":false}}"##,
        )
        .unwrap();
        let lane = parse_autoloop_lane_event(Some(&event), 84).unwrap();

        assert_eq!(lane.lane, "review");
        assert_eq!(lane.status, "completed");
        assert_eq!(lane.action.as_deref(), Some("lane_tick_completed"));
        assert_eq!(lane.selected.as_deref(), Some("#364"));
        assert_eq!(lane.issue_ref.as_deref(), Some("#364"));
        assert_eq!(lane.work_unit_completed, Some(true));
        assert_eq!(lane.completed_work_units, Some(3));
        assert_eq!(
            lane.latest_result.as_deref(),
            Some("completed:lane_tick_completed:#364")
        );
        assert_eq!(lane.target.as_deref(), Some("Human Review | Rework"));
        assert_eq!(lane.max_concurrent, Some(2));
    }

    #[test]
    fn wraps_plain_autopilot_stdout_as_json_event() {
        let event = parse_autoloop_text_event(
            "stdout",
            "run_loop_action=backend issue=#364 backend=codex command='codex app-server'",
        )
        .unwrap();

        assert_eq!(event["event"], "autopilot_cli_line");
        assert_eq!(event["payload"]["kind"], "run_loop_action");
        assert_eq!(event["payload"]["fields"]["issue"], "#364");
        assert_eq!(event["payload"]["fields"]["command"], "codex app-server");
    }

    #[test]
    fn wraps_latest_status_line_as_json_event() {
        let event = parse_autoloop_text_event(
            "stdout",
            "Latest: main | #364 | running | backend | Issue title | actor=Shea Symphony Agent | next=save result",
        )
        .unwrap();

        assert_eq!(event["event"], "autopilot_cli_line");
        assert_eq!(event["payload"]["kind"], "latest");
        assert_eq!(event["payload"]["fields"]["issue"], "#364");
        assert_eq!(event["payload"]["fields"]["action"], "backend");
        assert_eq!(event["payload"]["fields"]["next"], "save result");
    }

    #[test]
    fn ignores_non_lane_lines() {
        assert!(parse_autoloop_lane("autopilot_loop=stopped reason=max_iterations", 1).is_none());
    }

    #[test]
    fn omits_none_selected_target() {
        let lane = parse_autoloop_lane(
            "autopilot_loop_lane lane=main status=completed action=lane_tick_completed selected=none target=none max_concurrent=1 recover=true",
            7,
        )
        .unwrap();

        assert_eq!(lane.selected, None);
        assert_eq!(lane.target, None);
        assert_eq!(lane.recover, Some(true));
    }

    #[test]
    fn applies_autopilot_result_line_to_snapshot() {
        let mut state = LoopStateSnapshot::default();

        assert!(apply_autoloop_result(
            &mut state,
            "autopilot_loop_result iteration=1 mode=dry-run order=main,review,merge recover=true main_max_concurrent=1 review_max_concurrent=2 merge_max_concurrent=3",
        ));

        assert_eq!(state.mode, "dry-run");
        assert_eq!(state.lanes["main"].max_concurrent, Some(1));
        assert_eq!(state.lanes["review"].max_concurrent, Some(2));
        assert_eq!(state.lanes["merge"].max_concurrent, Some(3));
    }

    #[test]
    fn applies_autopilot_json_result_to_snapshot() {
        let mut state = LoopStateSnapshot::default();
        let event = parse_autoloop_event(
            r##"{"schema_version":1,"source":"shea-symphony","event":"autopilot_loop_result","payload":{"mode":"write","completed_work_units":2,"lane_work_units":{"review":2},"settings":{"main_max_concurrent":3,"review_max_concurrent":2,"merge_max_concurrent":1},"lanes":[{"lane":"review","status":"completed","action":"lane_tick_completed","work_unit_completed":true,"completed_work_units":2,"issue_ref":"#412","selected_issue":{"identifier":"#412"},"target_state":"Merging","max_concurrent":2,"recover":false}]}}"##,
        )
        .unwrap();

        assert!(apply_autoloop_event(&mut state, Some(&event), 101));

        assert_eq!(state.mode, "write");
        assert_eq!(state.lanes["main"].max_concurrent, Some(3));
        assert_eq!(state.lanes["review"].max_concurrent, Some(2));
        assert_eq!(state.lanes["merge"].max_concurrent, Some(1));
        assert_eq!(state.lanes["review"].status, "completed");
        assert_eq!(state.lanes["review"].selected.as_deref(), Some("#412"));
        assert_eq!(state.lanes["review"].completed_work_units, Some(2));
        assert_eq!(state.lanes["review"].work_unit_completed, Some(true));
    }

    #[test]
    fn applies_autopilot_stopped_line_to_snapshot() {
        let mut state = LoopStateSnapshot {
            running: true,
            stopping: true,
            pid: Some(123),
            ..LoopStateSnapshot::default()
        };

        assert!(apply_autoloop_stopped(
            &mut state,
            "autopilot_loop=stopped reason=max_iterations iterations=1",
            99,
        ));

        assert!(!state.running);
        assert!(!state.stopping);
        assert_eq!(state.pid, None);
        assert_eq!(state.stopped_at_ms, Some(99));
    }
}
