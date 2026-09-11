import { invoke } from '@tauri-apps/api/core';

export type ReviewAction = 'once' | 'recover';
export type CommandResult = { ok: boolean; stderr: string; stdoutPreview: string };
export type ReviewActionState = { running: boolean; issue?: string; action?: ReviewAction; workspace?: string; result?: CommandResult };
export type ReviewPreview = { token: string; issue: string; action: ReviewAction; workspace: string; summary: CommandResult };
type Publication = { issue_ref: string; state?: string; run_id?: string; terminal_result_captured?: boolean; diagnostic?: string };
type ReviewJob = { issue_identifier: string; job_id?: string; job_state?: string; review_outcome?: string; pid_alive?: boolean };
export type ReviewStatus = {
  issues?: { identifier: string; state: string }[];
  publications?: Publication[];
  pending_publications?: Publication[];
  running_slots?: ReviewJob[];
  recent_jobs?: ReviewJob[];
};
export type ReviewLifecycle = { label: string; detail: string; runId?: string; canStart: boolean; canRecover: boolean };

export function reviewLifecycle(status: ReviewStatus | null, issue: string): ReviewLifecycle {
  const blocked = { canStart: false, canRecover: false };
  if (!status) return { ...blocked, label: 'Not checked', detail: 'Read Review status before choosing an action.' };
  const tracker = status.issues?.find((entry) => entry.identifier === issue);
  const pending = status.pending_publications?.find((entry) => entry.issue_ref === issue);
  const receipt = pending ?? status.publications?.find((entry) => entry.issue_ref === issue);
  if (receipt) {
    const base = { ...blocked, runId: receipt.run_id };
    if (receipt.state === 'Starting' || receipt.state === 'Running') {
      return { ...base, label: receipt.state === 'Starting' ? 'Starting Review' : 'Review dispatched', detail: receipt.diagnostic || 'A dispatch receipt exists. If progress has stopped, use Shea Doctor; do not launch a duplicate.' };
    }
    if (receipt.state === 'ResultReady' || receipt.state === 'EvidencePublished') {
      return { ...base, label: 'Publication pending', detail: receipt.diagnostic || 'The captured result still needs publication or final routing. Recover this run without starting another reviewer.', canRecover: receipt.terminal_result_captured === true };
    }
    if (receipt.state === 'Complete' && receipt.terminal_result_captured === true) {
      return { ...base, label: 'Publication complete', detail: tracker?.state === 'Agent Review' ? 'The recorded run completed. This Issue is in Agent Review again; a fresh preflight is required.' : 'Review evidence publication and routing completed for this recorded run.', canStart: tracker?.state === 'Agent Review' };
    }
    return { ...base, label: 'Needs diagnosis', detail: receipt.diagnostic || 'The recorded result is superseded or unreadable. Use Shea Doctor before retrying.' };
  }
  const running = status.running_slots?.find((entry) => entry.issue_identifier === issue);
  if (running) return { ...blocked, label: 'Review job recorded', runId: running.job_id, detail: 'A running job or claim exists. Check its progress before dispatching another Review.' };
  const recent = status.recent_jobs?.find((entry) => entry.issue_identifier === issue);
  if (recent) return { ...blocked, label: 'Publication unverified', runId: recent.job_id, detail: 'A backend result exists without a final publication receipt. Use Shea Doctor to inspect its evidence.' };
  return { ...blocked, label: 'Not dispatched', detail: 'Agent Review status alone does not launch the reviewer.', canStart: tracker?.state === 'Agent Review' };
}

export const getIssueReviewStatus = (issue: string) => invoke<ReviewStatus>('get_issue_review_status', { issue });
export const getReviewActionState = () => invoke<ReviewActionState>('get_review_action_state');
export const prepareReviewAction = (issue: string, action: ReviewAction) => invoke<ReviewPreview>('prepare_review_action', { issue, action });
export const executeReviewAction = (token: string) => invoke<ReviewActionState>('execute_review_action', { token });
export const getAppReadiness = () => invoke<{ ready: boolean; runtime: { source_revision: string }; scope: string }>('get_app_readiness');
