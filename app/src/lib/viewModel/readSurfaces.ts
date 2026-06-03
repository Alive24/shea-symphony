import { labelForCommand } from './commandHelpers.ts';
import { parseSessionCount, parseSessionReadState } from './sessionParsers.ts';
import { timeLabel } from './text.ts';

type LooseRecord = Record<string, any>;

export function buildTrackerSignals(overview: any, commands: LooseRecord) {
  const autopilot = overview?.autopilot;
  const doctor = overview?.doctor;
  const review = overview?.review;
  return [
    {
      label: 'ProjectV2 tracker',
      status: commands.autopilot?.ok ? 'Readable' : 'Unknown',
      tone: commands.autopilot?.ok ? 'success' : 'warn',
      detail: autopilot?.readiness?.reason ?? 'Autopilot plan is the primary tracker read for queue posture.'
    },
    {
      label: 'Status metadata',
      status: commands.doctor?.ok && !doctor?.blockers ? 'Ready' : commands.doctor?.ok ? 'Needs attention' : 'Unknown',
      tone: commands.doctor?.ok && !doctor?.blockers ? 'success' : 'warn',
      detail: doctor?.blockers
        ? `${doctor.blockers} doctor blocker${doctor.blockers === 1 ? '' : 's'} visible.`
        : 'Status option and workflow readiness are checked through doctor evidence.'
    },
    {
      label: 'Review evidence',
      status: commands.review?.ok ? 'Readable' : 'Unknown',
      tone: commands.review?.ok ? 'success' : 'warn',
      detail: Array.isArray(review?.recent)
        ? `${review.recent.length} recent review record${review.recent.length === 1 ? '' : 's'} surfaced.`
        : 'Review status feeds Agent Review and Human Review visibility.'
    },
    {
      label: 'Session surface',
      status: commands.sessions?.ok ? 'Readable' : 'Unknown',
      tone: commands.sessions?.ok ? 'success' : 'warn',
      detail: overview?.sessionsText || 'Session list is used to detect active foreground work.'
    }
  ];
}

export function buildFallbackTrackerSignals(reason: string) {
  return [
    {
      label: 'Local API',
      status: 'Offline',
      tone: 'danger',
      detail: reason
    },
    {
      label: 'ProjectV2 tracker',
      status: 'Not checked',
      tone: 'warn',
      detail: 'Start the local server to read live ProjectV2 posture.'
    },
    {
      label: 'Review evidence',
      status: 'Fixture only',
      tone: 'warn',
      detail: 'Fallback data is useful for layout, not live routing.'
    }
  ];
}

export function buildDataSource(overview: any, commands: LooseRecord, generatedAt: Date | null) {
  if (overview?.fixture === true) {
    return {
      mode: 'fixture',
      label: 'Fixture data',
      tone: 'warn',
      freshness: generatedAt ? `Generated ${timeLabel(generatedAt)}` : 'Generated locally',
      trust: 'Safe for visual QA, not tracker routing',
      detail: 'GitHub reads and tracker writes are intentionally bypassed.'
    };
  }

  const commandResults = Object.values(commands) as any[];
  const cooldown = projectReadCooldown(overview, commands);
  const passed = commandResults.filter((result) => result?.ok).length;
  const pending = commandResults.filter((result) => result?.pending).length;
  const total = commandResults.length;
  const allPassed = total > 0 && passed === total;
  if (cooldown) {
    return {
      mode: passed > 0 ? 'live' : 'degraded',
      label: 'Project reads paused',
      tone: 'warn',
      freshness: generatedAt ? `Checked ${timeLabel(generatedAt)}` : 'Not checked',
      trust: 'Using last stable Project queue for operator awareness only',
      detail: `GitHub Project read paused until ${cooldownLabel(cooldown.untilMs)}.`
    };
  }
  return {
    mode: passed > 0 ? 'live' : 'degraded',
    label: allPassed ? 'Live tracker data' : pending > 0 ? 'Fast live data' : passed > 0 ? 'Partial live data' : 'No live data',
    tone: allPassed ? 'success' : passed > 0 ? 'warn' : 'danger',
    freshness: generatedAt ? `Checked ${timeLabel(generatedAt)}` : 'Not checked',
    trust:
      allPassed
        ? 'Usable for operator awareness'
        : pending > 0
          ? 'Fast readback only; full overview loading'
          : 'Confirm in chat Skills before routing',
    detail: `${passed}/${total || 0} overview commands passed${pending ? ` · ${pending} pending slow reads` : ''}.`
  };
}

export function buildLiveSignals(overview: any, commands: LooseRecord) {
  const skills = overview?.skills;
  const githubQueue = overview?.githubQueue;
  const cooldown = projectReadCooldown(overview, commands);
  const skillSummary = skills?.summary ?? {};
  const sessionsText = overview?.sessionsText ?? '';
  const sessionReadState = parseSessionReadState(sessionsText);
  const activeSessionCount = parseSessionCount(sessionsText);
  const slowReads = ['autopilot', 'doctor', 'review'].map((id) => {
    const result = commands[id];
    return {
      id,
      label: labelForCommand(id),
      status: result?.ok ? 'live' : result?.pending ? 'loading' : result?.timedOut ? 'timeout' : result ? 'degraded' : 'unknown',
      tone: result?.ok ? 'success' : result?.pending || result?.timedOut ? 'warn' : 'danger'
    };
  });

  return [
    {
      id: 'sessions',
      label: 'Workers',
      value: sessionReadState.status === 'unavailable' ? '!' : activeSessionCount == null ? 'Read' : String(activeSessionCount),
      shortDetail:
        sessionReadState.status === 'unavailable'
          ? 'unavailable'
          : activeSessionCount === 0
            ? 'none active'
            : 'readable',
      detail:
        sessionReadState.status === 'unavailable'
          ? sessionReadState.detail
          : activeSessionCount === 0
            ? 'No foreground agent sessions'
            : sessionsText || 'Session list is readable',
      tone: commands.sessions?.ok && sessionReadState.status !== 'unavailable' ? 'success' : 'warn'
    },
    {
      id: 'skills',
      label: 'Skills',
      value: skillSummary.expected_skills == null ? 'Read' : `${skillSummary.expected_skills}`,
      shortDetail:
        skillSummary.blockers == null
          ? commands.skills?.ok
            ? 'readable'
            : 'pending'
          : `${skillSummary.blockers} blocker${skillSummary.blockers === 1 ? '' : 's'}`,
      detail:
        skillSummary.blockers == null
          ? commands.skills?.ok
            ? 'Skill surface readable'
            : 'Skill surface pending'
          : `${skillSummary.blockers} blocker${skillSummary.blockers === 1 ? '' : 's'} · ${skillSummary.codex_status ?? 'unknown'}`,
      tone: skillSummary.blockers > 0 ? 'warn' : commands.skills?.ok ? 'success' : 'warn'
    },
    {
      id: 'slow',
      label: 'Queue',
      value: githubQueue?.totalOpen == null ? 'Read' : String(githubQueue.totalOpen),
      shortDetail: cooldown
        ? `paused until ${cooldownLabel(cooldown.untilMs)}`
        : githubQueue?.laneCounts
        ? `${githubQueue.laneCounts.main ?? 0}/${githubQueue.laneCounts.review ?? 0}/${githubQueue.laneCounts.merge ?? 0} lanes`
        : summarizeSlowReads(slowReads),
      detail: cooldown
        ? `GitHub Project reads are paused after rate limit. Last stable queue remains visible when available.`
        : githubQueue?.stateCounts
        ? Object.entries(githubQueue.stateCounts).map(([state, count]) => `${state}: ${count}`).join(' · ')
        : slowReads.map((item) => `${item.label}: ${item.status}`).join(' · '),
      tone: commands.githubQueue?.ok
        ? 'success'
        : cooldown || slowReads.some((item) => item.status === 'timeout' || item.status === 'degraded')
          ? 'warn'
          : 'success'
    },
    {
      id: 'status',
      label: 'Status',
      value: overview?.localStatus?.branch ?? 'Read',
      shortDetail: localStatusShortDetail(overview?.localStatus, commands.status),
      detail: localStatusDetail(overview?.localStatus, commands.status),
      tone: commands.status?.ok ? (overview?.localStatus?.dirtyCount > 0 ? 'warn' : 'success') : 'danger'
    }
  ];
}

function projectReadCooldown(overview: any, commands: LooseRecord) {
  return cooldownFromParsed(overview?.githubQueue)
    ?? cooldownFromCommand(commands.githubQueue)
    ?? cooldownFromCommand(commands.autopilot)
    ?? cooldownFromCommand(commands.doctor)
    ?? cooldownFromCommand(commands.review);
}

function cooldownFromCommand(command: any) {
  if (!command?.projectReadPaused && command?.signal !== 'project-rate-limit-cooldown') return null;
  return cooldownShape(command.rateLimitResetAtMs);
}

function cooldownFromParsed(parsed: any) {
  if (!parsed?.projectReadPaused && parsed?.failureKind !== 'rate_limit') return null;
  return cooldownShape(parsed.rateLimitResetAtMs);
}

function cooldownShape(value: any) {
  const untilMs = Number(value);
  if (!Number.isFinite(untilMs) || untilMs <= 0) return { untilMs: Date.now() + 10 * 60 * 1000 };
  return { untilMs };
}

function cooldownLabel(untilMs: number) {
  return new Date(untilMs).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

function summarizeSlowReads(slowReads: any[]) {
  const timeouts = slowReads.filter((item) => item.status === 'timeout').length;
  if (timeouts) return `${timeouts} timeout${timeouts === 1 ? '' : 's'}`;
  const loading = slowReads.filter((item) => item.status === 'loading').length;
  if (loading) return `${loading} loading`;
  const live = slowReads.filter((item) => item.status === 'live').length;
  return `${live} live`;
}

function localStatusShortDetail(localStatus: any, command: any) {
  if (!command) return 'not checked';
  if (!command.ok) return 'unavailable';
  if (!localStatus) return 'readable';
  return `${localStatus.dirtyCount ?? 0} dirty`;
}

function localStatusDetail(localStatus: any, command: any) {
  if (!command) return 'Local repo status not checked';
  if (!command.ok) return 'Local repo status unavailable';
  if (!localStatus) return 'Local repo readable';
  const dirty = `${localStatus.dirtyCount ?? 0} dirty`;
  const worktrees = `${localStatus.worktreeCount ?? 0} worktrees`;
  const binary = localStatus.binaryPresent ? 'binary ready' : 'binary missing';
  return `${localStatus.head ?? 'unknown'} · ${dirty} · ${worktrees} · ${binary}`;
}
