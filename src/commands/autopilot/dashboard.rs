use crossterm::terminal::size as terminal_size;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;
use serde::Serialize;

use super::looping::{
    AutopilotLoopIterationResult, AutopilotLoopLaneResult, AutopilotLoopStatusSnapshot,
};
use super::{AutopilotIssueSummary, AutopilotParkedQueue, AutopilotRetryRecord};

const DASHBOARD_MIN_WIDTH: u16 = 84;
const DASHBOARD_MAX_WIDTH: u16 = 124;
const DASHBOARD_HEIGHT: u16 = 38;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotDashboardSnapshot {
    pub(crate) schema_version: u8,
    pub(crate) source: String,
    pub(crate) workflow_path: String,
    pub(crate) iteration: usize,
    pub(crate) mode: String,
    pub(crate) phase: String,
    pub(crate) message: String,
    pub(crate) next_poll_in_ms: Option<u64>,
    pub(crate) lane_cards: Vec<AutopilotDashboardLaneCard>,
    pub(crate) queue_cards: Vec<AutopilotDashboardQueueCard>,
    pub(crate) retry_rows: Vec<AutopilotDashboardRetryRow>,
    pub(crate) event_rows: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotDashboardLaneCard {
    pub(crate) lane: String,
    pub(crate) status: String,
    pub(crate) action: String,
    pub(crate) issue: Option<String>,
    pub(crate) target_state: Option<String>,
    pub(crate) max_concurrent: usize,
    pub(crate) recover: bool,
    pub(crate) evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotDashboardQueueCard {
    pub(crate) name: String,
    pub(crate) state: String,
    pub(crate) count: usize,
    pub(crate) issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotDashboardRetryRow {
    pub(crate) lane: String,
    pub(crate) issue: String,
    pub(crate) attempt: u32,
    pub(crate) due_in_ms: u64,
    pub(crate) error: String,
}

pub(super) fn render_autopilot_loop_status_tui(status: &AutopilotLoopStatusSnapshot) -> String {
    render_autopilot_dashboard(&AutopilotDashboardSnapshot::from_status(status))
}

pub(super) fn render_autopilot_loop_iteration_tui(result: &AutopilotLoopIterationResult) -> String {
    render_autopilot_dashboard(&AutopilotDashboardSnapshot::from_iteration(result))
}

impl AutopilotDashboardSnapshot {
    fn from_status(status: &AutopilotLoopStatusSnapshot) -> Self {
        let mut event_rows = vec![status.message.clone()];
        event_rows.extend(
            status
                .blocked_reasons
                .iter()
                .take(5)
                .map(|reason| format!("blocked: {reason}")),
        );
        event_rows.extend(
            status
                .recent_transient_failures
                .iter()
                .take(5)
                .map(|failure| {
                    format!(
                        "transient attempt={} delay_ms={} policy={} error={}",
                        failure.attempt, failure.delay_ms, failure.recovery_policy, failure.error
                    )
                }),
        );
        event_rows.extend(status.active_issues.iter().take(5).map(|issue| {
            format!(
                "active {} lane={} backend={} session={}",
                issue.identifier,
                issue.lane,
                issue.backend,
                issue.session_id.as_deref().unwrap_or("n/a")
            )
        }));

        Self {
            schema_version: 1,
            source: "status".into(),
            workflow_path: status.workflow_path.clone(),
            iteration: status.iteration,
            mode: status.mode.clone(),
            phase: status.phase.clone(),
            message: status.message.clone(),
            next_poll_in_ms: status.polling.next_poll_in_ms,
            lane_cards: lane_cards_from_status(status),
            queue_cards: status
                .parked_queues
                .iter()
                .map(queue_card_from_parked_queue)
                .collect(),
            retry_rows: status.retrying.iter().map(retry_row_from_record).collect(),
            event_rows,
        }
    }

    fn from_iteration(result: &AutopilotLoopIterationResult) -> Self {
        let mut event_rows = vec![format!(
            "iteration {} finished; order={}",
            result.iteration,
            result.execution_order.join(",")
        )];
        for lane in &result.lanes {
            event_rows.extend(
                lane.evidence
                    .iter()
                    .take(2)
                    .map(|evidence| format!("{}: {evidence}", lane.lane)),
            );
        }

        Self {
            schema_version: 1,
            source: "iteration".into(),
            workflow_path: String::new(),
            iteration: result.iteration,
            mode: result.mode.clone(),
            phase: "iteration_result".into(),
            message: "lane ticks completed for this foreground iteration".into(),
            next_poll_in_ms: None,
            lane_cards: result
                .lanes
                .iter()
                .map(lane_card_from_iteration_result)
                .collect(),
            queue_cards: result
                .parked_queues
                .iter()
                .map(queue_card_from_parked_queue)
                .collect(),
            retry_rows: Vec::new(),
            event_rows,
        }
    }
}

fn lane_cards_from_status(status: &AutopilotLoopStatusSnapshot) -> Vec<AutopilotDashboardLaneCard> {
    let mut cards = status
        .lane_activity
        .iter()
        .map(|lane| AutopilotDashboardLaneCard {
            lane: lane.lane.clone(),
            status: lane.status.clone(),
            action: lane.action.clone(),
            issue: lane.selected_issue.as_ref().map(issue_label),
            target_state: None,
            max_concurrent: lane_worker_limit(status, &lane.lane),
            recover: lane.lane != "review",
            evidence: vec![lane.reason.clone()],
        })
        .collect::<Vec<_>>();

    for lane in ["main", "review", "merge"] {
        if cards.iter().all(|card| card.lane != lane) {
            cards.push(AutopilotDashboardLaneCard {
                lane: lane.into(),
                status: status.phase.clone(),
                action: "polling".into(),
                issue: None,
                target_state: None,
                max_concurrent: lane_worker_limit(status, lane),
                recover: lane != "review",
                evidence: vec![status.message.clone()],
            });
        }
    }
    cards
}

fn lane_card_from_iteration_result(lane: &AutopilotLoopLaneResult) -> AutopilotDashboardLaneCard {
    AutopilotDashboardLaneCard {
        lane: lane.lane.clone(),
        status: lane.status.clone(),
        action: lane.action.clone(),
        issue: lane.selected_issue.as_ref().map(issue_label),
        target_state: lane.target_state.clone(),
        max_concurrent: lane.max_concurrent,
        recover: lane.recover,
        evidence: lane.evidence.iter().take(3).cloned().collect(),
    }
}

fn queue_card_from_parked_queue(queue: &AutopilotParkedQueue) -> AutopilotDashboardQueueCard {
    AutopilotDashboardQueueCard {
        name: queue.name.clone(),
        state: queue.state.clone(),
        count: queue.count,
        issues: queue.issues.iter().take(4).map(issue_label).collect(),
    }
}

fn retry_row_from_record(retry: &AutopilotRetryRecord) -> AutopilotDashboardRetryRow {
    AutopilotDashboardRetryRow {
        lane: retry.lane.clone(),
        issue: retry.issue_identifier.as_deref().unwrap_or("n/a").into(),
        attempt: retry.attempt,
        due_in_ms: retry.due_in_ms,
        error: retry.error.clone(),
    }
}

fn lane_worker_limit(status: &AutopilotLoopStatusSnapshot, lane: &str) -> usize {
    match lane {
        "main" => status.settings.main_max_concurrent,
        "review" => status.settings.review_max_concurrent,
        "merge" => status.settings.merge_max_concurrent,
        _ => 1,
    }
}

fn issue_label(issue: &AutopilotIssueSummary) -> String {
    format!("{} {}", issue.identifier, issue.title)
}

fn render_autopilot_dashboard(snapshot: &AutopilotDashboardSnapshot) -> String {
    let width = dashboard_width();
    let backend = TestBackend::new(width, DASHBOARD_HEIGHT);
    let mut terminal = Terminal::new(backend).expect("test backend is infallible");
    terminal
        .draw(|frame| {
            let area = frame.area();
            render_dashboard_frame(frame, area, snapshot);
        })
        .expect("test backend draw is infallible");
    buffer_to_string(terminal.backend().buffer())
}

fn dashboard_width() -> u16 {
    terminal_size()
        .ok()
        .map(|(width, _)| width)
        .unwrap_or(DASHBOARD_MAX_WIDTH)
        .clamp(DASHBOARD_MIN_WIDTH, DASHBOARD_MAX_WIDTH)
}

fn render_dashboard_frame(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    snapshot: &AutopilotDashboardSnapshot,
) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(11),
            Constraint::Min(16),
            Constraint::Length(4),
        ])
        .split(area);

    frame.render_widget(header(snapshot), outer[0]);

    let lane_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(outer[1]);
    for (index, lane) in snapshot.lane_cards.iter().take(3).enumerate() {
        frame.render_widget(lane_card(lane), lane_chunks[index]);
    }

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(outer[2]);
    frame.render_widget(queue_panel(snapshot), body_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(8)])
        .split(body_chunks[1]);
    frame.render_widget(retry_panel(snapshot), right_chunks[0]);
    frame.render_widget(event_panel(snapshot), right_chunks[1]);

    frame.render_widget(footer(snapshot), outer[3]);
}

fn header(snapshot: &AutopilotDashboardSnapshot) -> Paragraph<'_> {
    let next_poll = snapshot
        .next_poll_in_ms
        .map(|value| format!("{value}ms"))
        .unwrap_or_else(|| "n/a".into());
    Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "SHEA SYMPHONY AUTOPILOT",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "  iteration={} mode={} phase={}",
                snapshot.iteration, snapshot.mode, snapshot.phase
            )),
        ]),
        Line::from(format!(
            "next_poll={} source={} workflow={}",
            next_poll,
            snapshot.source,
            empty_as_dash(&snapshot.workflow_path)
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .wrap(Wrap { trim: true })
}

fn lane_card(lane: &AutopilotDashboardLaneCard) -> Paragraph<'_> {
    let title = format!(" {} ", lane.lane.to_ascii_uppercase());
    let issue = lane.issue.as_deref().unwrap_or("none");
    let target = lane.target_state.as_deref().unwrap_or("unchanged");
    let evidence = lane.evidence.first().map(String::as_str).unwrap_or("n/a");
    Paragraph::new(vec![
        Line::from(format!("status  {}", lane.status)),
        Line::from(format!("action  {}", lane.action)),
        Line::from(format!("issue   {issue}")),
        Line::from(format!("target  {target}")),
        Line::from(format!(
            "slots   {}   recover={}",
            lane.max_concurrent, lane.recover
        )),
        Line::from(format!("note    {evidence}")),
    ])
    .block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(lane_border_style(&lane.status)),
    )
    .wrap(Wrap { trim: true })
}

fn queue_panel(snapshot: &AutopilotDashboardSnapshot) -> Paragraph<'_> {
    let mut lines = Vec::new();
    if snapshot.queue_cards.is_empty() {
        lines.push(Line::from("no parked operator queues"));
    }
    for queue in &snapshot.queue_cards {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", queue.name),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("state={} count={}", queue.state, queue.count)),
        ]));
        if queue.issues.is_empty() {
            lines.push(Line::from("  none"));
        } else {
            for issue in &queue.issues {
                lines.push(Line::from(format!("  {issue}")));
            }
        }
    }
    Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Operator Queues ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .wrap(Wrap { trim: true })
}

fn retry_panel(snapshot: &AutopilotDashboardSnapshot) -> Paragraph<'_> {
    let mut lines = Vec::new();
    if snapshot.retry_rows.is_empty() {
        lines.push(Line::from("no retry/backoff records"));
    }
    for retry in &snapshot.retry_rows {
        lines.push(Line::from(format!(
            "{} {} attempt={} due={}ms {}",
            retry.lane, retry.issue, retry.attempt, retry.due_in_ms, retry.error
        )));
    }
    Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Retry / Backoff ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: true })
}

fn event_panel(snapshot: &AutopilotDashboardSnapshot) -> Paragraph<'_> {
    let lines = if snapshot.event_rows.is_empty() {
        vec![Line::from("no events yet")]
    } else {
        snapshot
            .event_rows
            .iter()
            .take(8)
            .map(|row| Line::from(row.clone()))
            .collect()
    };
    Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Event Log ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        )
        .wrap(Wrap { trim: true })
}

fn footer(snapshot: &AutopilotDashboardSnapshot) -> Paragraph<'_> {
    Paragraph::new(Line::from(format!(
        "{} | foreground bounded loop | ctrl-c stops the operator command",
        snapshot.message
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    )
    .wrap(Wrap { trim: true })
}

fn lane_border_style(status: &str) -> Style {
    match status {
        "ready" | "completed" | "running" => Style::default().fg(Color::Green),
        "blocked" | "error" => Style::default().fg(Color::Red),
        "retrying" => Style::default().fg(Color::Yellow),
        "idle" => Style::default().fg(Color::DarkGray),
        _ => Style::default().fg(Color::Cyan),
    }
}

fn empty_as_dash(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
}

fn buffer_to_string(buffer: &Buffer) -> String {
    let width = buffer.area.width as usize;
    let mut lines = buffer
        .content()
        .chunks(width)
        .map(|cells| {
            cells
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>();
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::autopilot::looping::{
        AutopilotLaneActivity, AutopilotLoopCounts, AutopilotLoopSettings,
        AutopilotLoopStatusSnapshot,
    };
    use shea_symphony::model::PollingSnapshot;

    #[test]
    fn autopilot_tui_status_surfaces_lanes_queues_and_retries() {
        let status = AutopilotLoopStatusSnapshot {
            schema_version: 1,
            workflow_path: "workflows/shea-symphony.md".into(),
            iteration: 2,
            supervisor_cycle: 2,
            mode: "dry-run".into(),
            phase: "running".into(),
            message: "one or more lanes have useful work ready".into(),
            cancellation_requested: false,
            work_unit_limit: Some(4),
            completed_work_units: 1,
            lane_work_units: [("main".into(), 1)].into_iter().collect(),
            polling: PollingSnapshot {
                checking: false,
                next_poll_in_ms: Some(30_000),
                poll_interval_ms: 30_000,
            },
            settings: AutopilotLoopSettings {
                write: false,
                dry_run: true,
                recover: false,
                poll_interval_ms: 30_000,
                main_max_concurrent: 2,
                review_max_concurrent: 1,
                merge_max_concurrent: 1,
            },
            lane_activity: vec![
                lane_activity("main", "ready", "claim_main_issue"),
                lane_activity("review", "idle", "idle"),
                lane_activity("merge", "retrying", "wait_for_mergeability"),
            ],
            counts: AutopilotLoopCounts {
                running: 1,
                retrying: 1,
                blocked: 0,
                idle: 1,
            },
            selected_issues: Vec::new(),
            active_issues: Vec::new(),
            parked_queues: vec![
                parked_queue("Human Review", "Human Review", 2),
                parked_queue("Need Human Input", "Need Human Input", 1),
            ],
            blocked_reasons: Vec::new(),
            retrying: vec![AutopilotRetryRecord {
                lane: "merge".into(),
                issue_identifier: Some("#398".into()),
                attempt: 2,
                due_in_ms: 15_000,
                next_retry_at_ms: 45_000,
                error: "mergeability unknown".into(),
            }],
            recent_transient_failures: Vec::new(),
        };

        let rendered = render_autopilot_loop_status_tui(&status);

        assert!(rendered.contains("SHEA SYMPHONY AUTOPILOT"));
        assert!(rendered.contains("MAIN"));
        assert!(rendered.contains("REVIEW"));
        assert!(rendered.contains("MERGE"));
        assert!(rendered.contains("Human Review"));
        assert!(rendered.contains("Need Human Input"));
        assert!(rendered.contains("Retry / Backoff"));
    }

    fn lane_activity(lane: &str, status: &str, action: &str) -> AutopilotLaneActivity {
        AutopilotLaneActivity {
            lane: lane.into(),
            status: status.into(),
            action: action.into(),
            selected_issue: None,
            reason: "test".into(),
        }
    }

    fn parked_queue(name: &str, state: &str, count: usize) -> AutopilotParkedQueue {
        AutopilotParkedQueue {
            name: name.into(),
            state: state.into(),
            count,
            issues: Vec::new(),
        }
    }
}
