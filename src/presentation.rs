use std::collections::BTreeMap;

use crossterm::terminal::size as terminal_size;
use ratatui::layout::Rect;

use crate::doctor::{AuditSeverity, ProjectAuditReport};
use crate::handoff::IssueHandoffPlan;
use crate::model::{RuntimeSnapshot, TrackerIssue};

const DEFAULT_PANEL_WIDTH: u16 = 96;
const MIN_PANEL_WIDTH: u16 = 60;

#[derive(Debug, Clone)]
pub struct RunLoopPanel<'a> {
    pub snapshot: &'a RuntimeSnapshot,
    pub issue: Option<&'a TrackerIssue>,
    pub handoff: Option<&'a IssueHandoffPlan>,
    pub actor_role: &'a str,
    pub mode: &'a str,
    pub pool: usize,
    pub selected_pool: usize,
}

pub fn render_run_loop_panel(panel: RunLoopPanel<'_>) -> String {
    let mut sections = vec![PanelSection {
        title: "Run Loop".into(),
        rows: vec![
            row("mode", panel.mode),
            row("actor", panel.actor_role),
            row("pool", format!("{}", panel.pool)),
            row("selected_pool", format!("{}", panel.selected_pool)),
        ],
    }];

    if let Some(issue) = panel.issue {
        sections.push(PanelSection {
            title: "Issue".into(),
            rows: vec![
                row("selected", format!("{} {}", issue.identifier, issue.title)),
                row("state", &issue.state),
                row("next", "claim -> In Progress -> Agent Review"),
            ],
        });
    }

    if let Some(handoff) = panel.handoff {
        sections.push(PanelSection {
            title: "Workspace".into(),
            rows: vec![
                row("path", handoff.workspace_path.display().to_string()),
                row("branch", &handoff.branch_name),
                row(
                    "pr_plan",
                    format!(
                        "{} -> {}",
                        handoff.pull_request.head_branch, handoff.pull_request.base_branch
                    ),
                ),
            ],
        });
    }

    sections.push(PanelSection {
        title: "Activity".into(),
        rows: vec![
            row("running", format!("{}", panel.snapshot.running.len())),
            row("retrying", format!("{}", panel.snapshot.retrying.len())),
            row("skipped", format!("{}", panel.snapshot.skipped.len())),
        ],
    });

    if !panel.snapshot.integration_gaps.is_empty() {
        sections.push(PanelSection {
            title: "Warnings".into(),
            rows: panel
                .snapshot
                .integration_gaps
                .iter()
                .map(|gap| row("integration_gap", gap))
                .collect(),
        });
    }

    render_panel("JADE SYMPHONY OPERATOR PANEL", sections)
}

pub fn render_project_state_panel(issues: &[TrackerIssue], integration_gaps: &[String]) -> String {
    let mut state_counts = BTreeMap::new();
    for issue in issues {
        let state = issue.state.trim();
        let state = if state.is_empty() { "(unknown)" } else { state };
        *state_counts.entry(state.to_string()).or_insert(0usize) += 1;
    }

    let mut sections = vec![
        PanelSection {
            title: "Project".into(),
            rows: vec![
                row("issues", format!("{}", issues.len())),
                row("empty_queue", format!("{}", issues.is_empty())),
            ],
        },
        PanelSection {
            title: "States".into(),
            rows: state_counts
                .into_iter()
                .map(|(state, count)| row(state, format!("{}", count)))
                .collect(),
        },
    ];

    if !integration_gaps.is_empty() {
        sections.push(PanelSection {
            title: "Warnings".into(),
            rows: integration_gaps
                .iter()
                .map(|gap| row("integration_gap", gap))
                .collect(),
        });
    }

    render_panel("JADE SYMPHONY PROJECT PANEL", sections)
}

pub fn render_doctor_panel(report: &ProjectAuditReport) -> String {
    let mut sections = vec![PanelSection {
        title: "Doctor".into(),
        rows: vec![
            row("issues", format!("{}", report.total_issues)),
            row("violations", format!("{}", report.violations.len())),
            row("blockers", format!("{}", report.blocker_count())),
        ],
    }];

    if report.violations.is_empty() {
        sections.push(PanelSection {
            title: "Summary".into(),
            rows: vec![row("status", "Project invariants look clean.")],
        });
    } else {
        sections.push(PanelSection {
            title: "Findings".into(),
            rows: report
                .violations
                .iter()
                .map(|violation| {
                    let severity = match violation.severity {
                        AuditSeverity::Warning => "warning",
                        AuditSeverity::Blocker => "blocker",
                    };
                    row(
                        format!("{} {}", violation.issue_ref, severity),
                        format!("{}: {}", violation.code, violation.message),
                    )
                })
                .collect(),
        });
    }

    if !report.integration_gaps.is_empty() {
        sections.push(PanelSection {
            title: "Warnings".into(),
            rows: report
                .integration_gaps
                .iter()
                .map(|gap| row("integration_gap", gap))
                .collect(),
        });
    }

    render_panel("JADE SYMPHONY DOCTOR PANEL", sections)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PanelSection {
    title: String,
    rows: Vec<(String, String)>,
}

fn row(label: impl Into<String>, value: impl Into<String>) -> (String, String) {
    (label.into(), value.into())
}

fn render_panel(title: &str, sections: Vec<PanelSection>) -> String {
    let width = panel_width();
    let inner_width = usize::from(width.saturating_sub(4));
    let border = format!("+{}+", "-".repeat(usize::from(width.saturating_sub(2))));
    let mut lines = vec![
        border.clone(),
        panel_line(title, inner_width),
        border.clone(),
    ];

    for section in sections {
        lines.push(panel_line(&format!("[{}]", section.title), inner_width));
        if section.rows.is_empty() {
            lines.push(panel_line("(none)", inner_width));
        }
        for (label, value) in section.rows {
            let content = format!("{label}: {value}");
            for wrapped in wrap_ascii(&content, inner_width) {
                lines.push(panel_line(&wrapped, inner_width));
            }
        }
        lines.push(border.clone());
    }

    lines.join("\n")
}

fn panel_width() -> u16 {
    let terminal_width = terminal_size()
        .ok()
        .map(|(width, _)| width)
        .unwrap_or(DEFAULT_PANEL_WIDTH);
    let rect = Rect::new(0, 0, terminal_width, 0);
    rect.width.clamp(MIN_PANEL_WIDTH, DEFAULT_PANEL_WIDTH)
}

fn panel_line(content: &str, inner_width: usize) -> String {
    let clipped = clip_ascii(content, inner_width);
    format!("| {clipped:<inner_width$} |")
}

fn wrap_ascii(content: &str, width: usize) -> Vec<String> {
    if content.len() <= width {
        return vec![content.to_string()];
    }

    let mut lines = Vec::new();
    let mut remaining = content.trim();
    while remaining.len() > width {
        let split_at = remaining[..width]
            .rfind(' ')
            .filter(|index| *index > 0)
            .unwrap_or(width);
        lines.push(remaining[..split_at].trim_end().to_string());
        remaining = remaining[split_at..].trim_start();
    }
    if !remaining.is_empty() {
        lines.push(remaining.to_string());
    }
    lines
}

fn clip_ascii(content: &str, width: usize) -> String {
    if content.len() <= width {
        content.to_string()
    } else {
        content[..width].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doctor::{AuditSeverity, ProjectAuditViolation};
    use crate::model::{PollingSnapshot, RuntimeSnapshot, TokenTotals};

    #[test]
    fn run_loop_panel_surfaces_issue_handoff_and_warning_sections() {
        let issue = TrackerIssue {
            tracker_kind: "github_project_v2".into(),
            id: "GHI_196".into(),
            item_id: None,
            identifier: "#196".into(),
            title: "Add TUI foundation".into(),
            description: None,
            url: None,
            state: "Todo".into(),
            labels: Vec::new(),
            assignees: vec!["Alive24".into()],
            priority: None,
            branch_name: None,
            linked_pull_requests: Vec::new(),
            blocked_by: Vec::new(),
            project_fields: BTreeMap::new(),
            created_at: None,
            updated_at: None,
        };
        let handoff = IssueHandoffPlan {
            issue_ref: "#196".into(),
            issue_title: issue.title.clone(),
            workspace_key: "issue-196-tui".into(),
            workspace_path: "/tmp/issue-196-tui".into(),
            branch_name: "feature/issue-196-tui".into(),
            pull_request: crate::handoff::PullRequestHandoffPlan {
                title: "#196: Add TUI foundation".into(),
                head_branch: "feature/issue-196-tui".into(),
                base_branch: "main".into(),
                issue_ref: "#196".into(),
                body: String::new(),
            },
            continuation: None,
        };
        let snapshot = RuntimeSnapshot {
            polling: PollingSnapshot {
                checking: false,
                next_poll_in_ms: Some(30_000),
                poll_interval_ms: 30_000,
            },
            codex_totals: TokenTotals::default(),
            integration_gaps: vec!["GitHub Project v2 PR linking is comment based".into()],
            ..RuntimeSnapshot::default()
        };

        let rendered = render_run_loop_panel(RunLoopPanel {
            snapshot: &snapshot,
            issue: Some(&issue),
            handoff: Some(&handoff),
            actor_role: "Main Agent",
            mode: "dry-run",
            pool: 1,
            selected_pool: 1,
        });

        assert!(rendered.contains("JADE SYMPHONY OPERATOR PANEL"));
        assert!(rendered.contains("selected: #196 Add TUI foundation"));
        assert!(rendered.contains("branch: feature/issue-196-tui"));
        assert!(rendered.contains("integration_gap"));
    }

    #[test]
    fn project_state_panel_counts_states() {
        let mut issue = minimal_issue("#195", "Todo");
        let other = minimal_issue("#196", "Agent Review");
        issue.title = "Rewrite README".into();

        let rendered = render_project_state_panel(&[issue, other], &[]);

        assert!(rendered.contains("JADE SYMPHONY PROJECT PANEL"));
        assert!(rendered.contains("Todo: 1"));
        assert!(rendered.contains("Agent Review: 1"));
    }

    #[test]
    fn doctor_panel_surfaces_blockers() {
        let report = ProjectAuditReport {
            total_issues: 1,
            integration_gaps: Vec::new(),
            violations: vec![ProjectAuditViolation {
                issue_ref: "#57".into(),
                title: "Missing PR".into(),
                state: "Agent Review".into(),
                severity: AuditSeverity::Blocker,
                code: "agent_review_missing_pr_handoff".into(),
                message: "Agent Review issue has no linked PR URL.".into(),
                suggestion: "Repair the missing PR link.".into(),
            }],
        };

        let rendered = render_doctor_panel(&report);

        assert!(rendered.contains("JADE SYMPHONY DOCTOR PANEL"));
        assert!(rendered.contains("#57 blocker"));
        assert!(rendered.contains("agent_review_missing_pr_handoff"));
    }

    fn minimal_issue(identifier: &str, state: &str) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: identifier.into(),
            item_id: None,
            identifier: identifier.into(),
            title: "Issue".into(),
            description: None,
            url: None,
            state: state.into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            priority: None,
            branch_name: None,
            linked_pull_requests: Vec::new(),
            blocked_by: Vec::new(),
            project_fields: BTreeMap::new(),
            created_at: None,
            updated_at: None,
        }
    }
}
