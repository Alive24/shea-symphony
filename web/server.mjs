import { createServer } from 'node:http';
import { createReadStream, existsSync, statSync } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import { extname, join, normalize, resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

const webRoot = resolve(import.meta.dirname);
const repoRoot = resolve(webRoot, '..');
const buildRoot = resolve(webRoot, 'build');
const defaultBinaryPath = resolve(repoRoot, 'target', 'debug', 'shea-symphony');
const workflowPath = process.env.SHEA_WORKFLOW ?? 'workflows/shea-symphony.md';
const port = Number(process.env.PORT ?? 5173);
const host = process.env.HOST ?? 'localhost';
const portFallbackLimit = Number(process.env.SHEA_WEB_PORT_FALLBACKS ?? 6);
const maxBodyBytes = 128 * 1024;
const overviewTimeoutMs = Number(process.env.SHEA_WEB_OVERVIEW_TIMEOUT_MS ?? 15000);
const fastOverviewTimeoutMs = Number(process.env.SHEA_WEB_FAST_OVERVIEW_TIMEOUT_MS ?? 3000);
const githubQueueTimeoutMs = Number(process.env.SHEA_WEB_GITHUB_QUEUE_TIMEOUT_MS ?? 5000);

const mimeTypes = new Map([
  ['.html', 'text/html; charset=utf-8'],
  ['.js', 'text/javascript; charset=utf-8'],
  ['.css', 'text/css; charset=utf-8'],
  ['.json', 'application/json; charset=utf-8'],
  ['.svg', 'image/svg+xml'],
  ['.png', 'image/png'],
  ['.jpg', 'image/jpeg'],
  ['.jpeg', 'image/jpeg'],
  ['.webp', 'image/webp']
]);

let overviewCache = null;

const surfaceCommands = new Map([
  ['autopilot', ['autopilot', 'plan', workflowPath, '--json']],
  ['doctor', ['doctor', workflowPath, '--json']],
  ['review', ['review', 'status', workflowPath, '--json']],
  ['skills', ['skills', 'status', workflowPath, '--json']],
  ['sessions', ['session', 'list', workflowPath]]
]);

function fixtureEnabled() {
  return process.env.SHEA_WEB_FIXTURE === '1';
}

function jsonResponse(response, status, payload) {
  response.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'cache-control': 'no-store'
  });
  response.end(JSON.stringify(payload));
}

function readRequestBody(request) {
  return new Promise((resolveBody, rejectBody) => {
    let body = '';
    request.on('data', (chunk) => {
      body += chunk;
      if (Buffer.byteLength(body) > maxBodyBytes) {
        request.destroy();
        rejectBody(new Error('request body is too large'));
      }
    });
    request.on('end', () => {
      try {
        resolveBody(body ? JSON.parse(body) : {});
      } catch (error) {
        rejectBody(new Error(`invalid JSON request body: ${error.message}`));
      }
    });
    request.on('error', rejectBody);
  });
}

function runShea(args, options = {}) {
  const timeoutMs = options.timeoutMs ?? 120000;
  const startedAt = Date.now();
  const localBinaryAvailable = existsSync(defaultBinaryPath);
  const command = process.env.SHEA_CLI ?? (localBinaryAvailable ? defaultBinaryPath : 'cargo');
  const commandArgs = process.env.SHEA_CLI || localBinaryAvailable
    ? args
    : ['run', '--quiet', '--', ...args];

  return new Promise((resolveRun) => {
    let timedOut = false;
    const child = spawn(command, commandArgs, {
      cwd: repoRoot,
      env: process.env,
      stdio: ['ignore', 'pipe', 'pipe']
    });
    let stdout = '';
    let stderr = '';
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill('SIGTERM');
    }, timeoutMs);

    child.stdout.on('data', (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString();
    });
    child.on('error', (error) => {
      clearTimeout(timer);
      resolveRun({
        ok: false,
        args,
        stdout,
        stderr: stderr || error.message,
        exitCode: null,
        timedOut: false,
        durationMs: Date.now() - startedAt
      });
    });
    child.on('close', (exitCode, signal) => {
      clearTimeout(timer);
      resolveRun({
        ok: exitCode === 0,
        args,
        stdout,
        stderr,
        exitCode,
        signal,
        timedOut,
        durationMs: Date.now() - startedAt
      });
    });
  });
}

function parseJsonOutput(result) {
  if (!result.ok) return null;
  try {
    return JSON.parse(result.stdout);
  } catch (_) {
    return null;
  }
}

export async function buildOverview(force = false, scope = 'full') {
  if (fixtureEnabled()) return fixtureOverview();
  if (scope === 'fast') return buildFastOverview();
  if (!force && overviewCache && Date.now() - overviewCache.generatedAtMs < 15000) {
    return overviewCache.payload;
  }

  const commandsToRun = [...surfaceCommands.entries()];
  const entries = existsSync(defaultBinaryPath) || process.env.SHEA_CLI
    ? await Promise.all(commandsToRun.map(async ([name, args]) => [name, await runShea(args, { timeoutMs: overviewTimeoutMs })]))
    : [];

  if (entries.length === 0) {
    for (const [name, args] of commandsToRun) {
      entries.push([name, await runShea(args, { timeoutMs: overviewTimeoutMs })]);
    }
  }

  const results = Object.fromEntries(entries);
  const { autopilot, doctor, review, skills, sessions } = results;
  const [local, githubQueue] = await Promise.all([buildLocalStatus(), buildGithubQueueStatus()]);

  const payload = {
    generatedAt: new Date().toISOString(),
    workflowPath,
    commands: {
      autopilot: summarizeResult(autopilot),
      doctor: summarizeResult(doctor),
      review: summarizeResult(review),
      skills: summarizeResult(skills),
      sessions: summarizeResult(sessions),
      local: local.command,
      githubQueue: githubQueue.command
    },
    autopilot: parseJsonOutput(autopilot),
    doctor: parseJsonOutput(doctor),
    review: parseJsonOutput(review),
    skills: parseJsonOutput(skills),
    sessionsText: sessions.stdout.trim(),
    localStatus: local.parsed,
    githubQueue: githubQueue.parsed,
    healthy: [autopilot, doctor, review, skills].some((result) => result.ok)
  };
  overviewCache = { generatedAtMs: Date.now(), payload };
  return payload;
}

async function buildFastOverview() {
  const fastCommands = ['skills', 'sessions'].map((name) => [name, surfaceCommands.get(name)]);
  const entries = await Promise.all(
    fastCommands.map(async ([name, args]) => [name, await runShea(args, { timeoutMs: fastOverviewTimeoutMs })])
  );
  const results = Object.fromEntries(entries);
  const [local, githubQueue] = await Promise.all([buildLocalStatus(), buildGithubQueueStatus()]);
  const pending = {
    autopilot: pendingResult(['autopilot', 'plan', workflowPath, '--json'], 'Deferred to full overview.'),
    doctor: pendingResult(['doctor', workflowPath, '--json'], 'Deferred to full overview.'),
    review: pendingResult(['review', 'status', workflowPath, '--json'], 'Deferred to full overview.')
  };

  return {
    generatedAt: new Date().toISOString(),
    workflowPath,
    scope: 'fast',
    commands: {
      autopilot: pending.autopilot,
      doctor: pending.doctor,
      review: pending.review,
      skills: summarizeResult(results.skills),
      sessions: summarizeResult(results.sessions),
      local: local.command,
      githubQueue: githubQueue.command
    },
    autopilot: null,
    doctor: null,
    review: null,
    skills: parseJsonOutput(results.skills),
    sessionsText: results.sessions.stdout.trim(),
    localStatus: local.parsed,
    githubQueue: githubQueue.parsed,
    healthy: [results.skills, results.sessions].some((result) => result.ok)
  };
}

export async function buildReadSurface(name, force = false) {
  if (name === 'local') {
    return buildLocalStatus();
  }
  if (name === 'githubQueue') {
    return buildGithubQueueStatus();
  }

  const args = surfaceCommands.get(name);
  if (!args) {
    throw new Error(`unknown read surface: ${name || '<empty>'}`);
  }

  if (fixtureEnabled()) {
    const fixture = fixtureOverview();
    return surfacePayload(name, fixture.commands[name], {
      parsed: fixture[name] ?? null,
      text: name === 'sessions' ? fixture.sessionsText : ''
    });
  }

  const timeoutMs = ['skills', 'sessions'].includes(name) ? fastOverviewTimeoutMs : overviewTimeoutMs;
  const result = await runShea(args, { timeoutMs });
  return surfacePayload(name, summarizeResult(result), {
    parsed: parseJsonOutput(result),
    text: result.stdout.trim()
  });
}

async function buildLocalStatus() {
  const startedAt = Date.now();
  const [status, branch, head, worktrees] = await Promise.all([
    runLocalCommand('git', ['status', '--porcelain']),
    runLocalCommand('git', ['branch', '--show-current']),
    runLocalCommand('git', ['rev-parse', '--short', 'HEAD']),
    runLocalCommand('git', ['worktree', 'list', '--porcelain'])
  ]);
  const dirtyLines = status.stdout.split('\n').filter((line) => line.trim());
  const worktreeCount = worktrees.stdout.split('\n').filter((line) => line.startsWith('worktree ')).length;
  const ok = [status, branch, head, worktrees].every((result) => result.ok);
  const parsed = {
    branch: branch.stdout.trim() || 'unknown',
    head: head.stdout.trim() || 'unknown',
    dirtyCount: dirtyLines.length,
    worktreeCount,
    buildPresent: existsSync(buildRoot),
    binaryPresent: existsSync(defaultBinaryPath),
    dirtyPreview: dirtyLines.slice(0, 8)
  };
  return surfacePayload('local', {
    ok,
    args: ['local', 'status'],
    exitCode: ok ? 0 : 1,
    signal: null,
    timedOut: false,
    durationMs: Date.now() - startedAt,
    stderr: [status.stderr, branch.stderr, head.stderr, worktrees.stderr].filter(Boolean).join('\n'),
    stdoutPreview: JSON.stringify(parsed, null, 2)
  }, {
    parsed,
    text: JSON.stringify(parsed)
  });
}

async function buildGithubQueueStatus() {
  const startedAt = Date.now();
  const args = [
    'issue',
    'list',
    '--repo',
    'Alive24/shea-symphony',
    '--state',
    'open',
    '--limit',
    '100',
    '--json',
    'number,title,projectItems,assignees,labels,updatedAt,url'
  ];
  const result = await runCommandWithTimeout('gh', args, { timeoutMs: githubQueueTimeoutMs });
  const parsed = result.ok ? parseGithubQueue(result.stdout) : null;
  return surfacePayload('githubQueue', {
    ok: result.ok && parsed !== null,
    args: ['gh', ...args],
    exitCode: result.exitCode,
    signal: result.signal ?? null,
    timedOut: result.timedOut === true,
    durationMs: Date.now() - startedAt,
    stderr: result.stderr.trim(),
    stdoutPreview: parsed ? JSON.stringify(parsed, null, 2).slice(0, 6000) : result.stdout.trim().slice(0, 6000)
  }, {
    parsed,
    text: parsed ? JSON.stringify(parsed) : result.stdout.trim()
  });
}

function parseGithubQueue(stdout) {
  let issues;
  try {
    issues = JSON.parse(stdout);
  } catch (_) {
    return null;
  }
  if (!Array.isArray(issues)) return null;

  const projectTitle = 'Shea Symphony Tracker';
  const stateCounts = {};
  const queueIssues = issues.map((issue) => {
    const projectItem = (issue.projectItems ?? []).find((item) => item.title === projectTitle) ?? issue.projectItems?.[0];
    const state = projectItem?.status?.name ?? 'No Project';
    stateCounts[state] = (stateCounts[state] ?? 0) + 1;
    return {
      identifier: `#${issue.number}`,
      number: issue.number,
      title: issue.title,
      url: issue.url,
      state,
      assignees: (issue.assignees ?? []).map((assignee) => assignee.login).filter(Boolean),
      labels: (issue.labels ?? []).map((label) => label.name).filter(Boolean),
      updatedAt: issue.updatedAt
    };
  });

  const laneCounts = {
    main: countStates(stateCounts, ['Todo', 'Rework', 'In Progress']),
    review: countStates(stateCounts, ['Agent Review', 'Human Review']),
    merge: countStates(stateCounts, ['Merging'])
  };
  const operatorIssues = queueIssues.filter((issue) => ['Need Human Input', 'Human Review'].includes(issue.state));

  return {
    totalOpen: queueIssues.length,
    stateCounts,
    laneCounts,
    operatorIssues,
    issues: queueIssues.slice(0, 30),
    source: 'gh issue list --json projectItems'
  };
}

function countStates(stateCounts, states) {
  return states.reduce((sum, state) => sum + Number(stateCounts[state] ?? 0), 0);
}

function runCommandWithTimeout(command, args, options = {}) {
  const timeoutMs = options.timeoutMs ?? 10000;
  const startedAt = Date.now();
  return new Promise((resolveRun) => {
    let timedOut = false;
    const child = spawn(command, args, {
      cwd: repoRoot,
      env: process.env,
      stdio: ['ignore', 'pipe', 'pipe']
    });
    let stdout = '';
    let stderr = '';
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill('SIGTERM');
    }, timeoutMs);
    child.stdout.on('data', (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString();
    });
    child.on('error', (error) => {
      clearTimeout(timer);
      resolveRun({
        ok: false,
        stdout,
        stderr: stderr || error.message,
        exitCode: null,
        signal: null,
        timedOut: false,
        durationMs: Date.now() - startedAt
      });
    });
    child.on('close', (exitCode, signal) => {
      clearTimeout(timer);
      resolveRun({
        ok: exitCode === 0,
        stdout,
        stderr,
        exitCode,
        signal,
        timedOut,
        durationMs: Date.now() - startedAt
      });
    });
  });
}

function runLocalCommand(command, args) {
  return new Promise((resolveRun) => {
    const child = spawn(command, args, {
      cwd: repoRoot,
      env: process.env,
      stdio: ['ignore', 'pipe', 'pipe']
    });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString();
    });
    child.on('error', (error) => {
      resolveRun({ ok: false, stdout, stderr: stderr || error.message });
    });
    child.on('close', (exitCode) => {
      resolveRun({ ok: exitCode === 0, stdout, stderr });
    });
  });
}

function surfacePayload(name, command, { parsed = null, text = '' } = {}) {
  return {
    generatedAt: new Date().toISOString(),
    workflowPath,
    name,
    command,
    parsed,
    text
  };
}

function summarizeResult(result) {
  return {
    ok: result.ok,
    args: result.args,
    exitCode: result.exitCode,
    signal: result.signal ?? null,
    timedOut: result.timedOut === true,
    durationMs: result.durationMs,
    stderr: result.stderr.trim(),
    stdoutPreview: result.stdout.trim().slice(0, 6000)
  };
}

function pendingResult(args, reason) {
  return {
    ok: false,
    pending: true,
    args,
    exitCode: null,
    signal: null,
    timedOut: false,
    durationMs: 0,
    stderr: reason,
    stdoutPreview: ''
  };
}

export function buildHealth() {
  return {
    ok: true,
    generatedAt: new Date().toISOString(),
    workflowPath,
    fixture: fixtureEnabled(),
    buildPresent: existsSync(buildRoot),
    cli: {
      mode: process.env.SHEA_CLI ? 'env' : existsSync(defaultBinaryPath) ? 'binary' : 'cargo',
      path: process.env.SHEA_CLI ?? (existsSync(defaultBinaryPath) ? defaultBinaryPath : 'cargo run')
    },
    server: {
      host,
      port,
      maxBodyBytes,
      overviewTimeoutMs,
      fastOverviewTimeoutMs
    }
  };
}

function cleanIssueRef(issue) {
  const value = String(issue ?? '').trim();
  if (!/^#?\d+$/.test(value)) {
    throw new Error('issue must look like #123');
  }
  return value.startsWith('#') ? value : `#${value}`;
}

function cleanState(state) {
  const value = String(state ?? '').trim();
  const allowed = new Set([
    'Backlog',
    'Todo',
    'In Progress',
    'Agent Review',
    'Human Review',
    'Merging',
    'Rework',
    'Need Human Input',
    'Done',
    'backlog',
    'todo',
    'in_progress',
    'agent_review',
    'human_review',
    'merging',
    'rework',
    'need_human_input',
    'done'
  ]);
  if (!allowed.has(value)) {
    throw new Error('state is not an allowed Shea Symphony Project status');
  }
  return value;
}

function cleanForgeStatus(status) {
  const value = String(status ?? 'Todo').trim();
  if (!new Set(['Backlog', 'Todo', 'backlog', 'todo']).has(value)) {
    throw new Error('forge status must be Backlog or Todo');
  }
  return value;
}

export async function commandArgsFor(body) {
  const action = String(body.action ?? '');
  const write = body.write === true;
  switch (action) {
    case 'autopilot-plan':
      return ['autopilot', 'plan', workflowPath, '--json'];
    case 'doctor':
      return ['doctor', workflowPath, '--json'];
    case 'review-status':
      return ['review', 'status', workflowPath, '--json'];
    case 'review-pass': {
      const issue = cleanIssueRef(body.issue);
      const evidence = String(body.markdown ?? '').trim();
      if (!evidence) throw new Error('review evidence markdown is required');
      const path = join(tmpdir(), `shea-web-review-pass-${Date.now()}-${issue.replace('#', '')}.md`);
      await writeFile(path, evidence);
      const args = ['review', 'pass', workflowPath, issue, '--evidence-file', path];
      args.push(write ? '--write' : '--dry-run');
      return args;
    }
    case 'review-reject': {
      const issue = cleanIssueRef(body.issue);
      const evidence = String(body.markdown ?? '').trim();
      if (!evidence) throw new Error('review evidence markdown is required');
      const targetState = cleanReviewRejectTarget(body.targetState ?? body.state ?? 'agent_review');
      const path = join(tmpdir(), `shea-web-review-reject-${Date.now()}-${issue.replace('#', '')}.md`);
      await writeFile(path, evidence);
      const args = [
        'review',
        'reject',
        workflowPath,
        issue,
        '--evidence-file',
        path,
        '--target-state',
        targetState
      ];
      args.push(write ? '--write' : '--dry-run');
      return args;
    }
    case 'skills-status':
      return ['skills', 'status', workflowPath, '--json'];
    case 'session-list':
      return ['session', 'list', workflowPath];
    case 'workspace-list':
      return ['workspace', 'list', workflowPath];
    case 'clean-audit':
      return ['clean', 'audit', workflowPath];
    case 'doctor-repair': {
      const args = ['doctor', 'repair', cleanIssueRef(body.issue).replace(/^#/, '')];
      if (body.repairAction === 'move_need_human_input') args.push('--move-need-human-input');
      if (body.repairAction === 'mark_pr_ready') args.push('--mark-pr-ready', '--confirm-handoff-ready');
      args.push(write ? '--write' : '--dry-run');
      return args;
    }
    case 'forge-validate': {
      const issue = String(body.issue ?? '').trim();
      const title = String(body.title ?? '').trim();
      const markdown = String(body.markdown ?? '').trim();
      const args = ['forge', 'validate', '--workflow', workflowPath, '--status', cleanForgeStatus(body.forgeStatus)];
      if (issue) args.push('--issue', cleanIssueRef(issue));
      if (title) args.push('--title', title);
      if (markdown) {
        const path = join(tmpdir(), `shea-web-forge-validate-${Date.now()}.md`);
        await writeFile(path, markdown);
        args.push('--body-file', path);
      }
      if (!issue && !title) throw new Error('forge validate requires an issue or title');
      return args;
    }
    case 'forge-create': {
      const title = String(body.title ?? '').trim();
      const markdown = String(body.markdown ?? '').trim();
      if (!title) throw new Error('forge create requires title');
      if (!markdown) throw new Error('forge create requires body markdown');
      const path = join(tmpdir(), `shea-web-forge-create-${Date.now()}.md`);
      await writeFile(path, markdown);
      const args = [
        'forge',
        'create',
        '--workflow',
        workflowPath,
        '--status',
        cleanForgeStatus(body.forgeStatus),
        '--title',
        title,
        '--body-file',
        path
      ];
      for (const assignee of cleanList(body.assignees)) args.push('--assignee', assignee);
      args.push(write ? '--write' : '--dry-run');
      return args;
    }
    case 'project-issue':
      return ['project', 'issue', workflowPath, cleanIssueRef(body.issue), '--json'];
    case 'project-inspect': {
      const args = ['project', 'inspect', workflowPath, cleanIssueRef(body.issue)];
      if (body.lane) args.push('--lane', String(body.lane));
      return args;
    }
    case 'quality-gate': {
      const args = ['gate', workflowPath, cleanIssueRef(body.issue)];
      args.push(write ? '--write' : '--dry-run');
      return args;
    }
    case 'set-state': {
      const args = ['project', 'set-state', workflowPath, cleanIssueRef(body.issue), cleanState(body.state)];
      args.push(write ? '--write' : '--dry-run');
      return args;
    }
    case 'autopilot-loop-once': {
      const args = ['autopilot', 'loop', workflowPath, '--once', '--json'];
      args.push(write ? '--write' : '--dry-run');
      return args;
    }
    case 'merge-once': {
      const args = ['merge', 'once', workflowPath];
      args.push(write ? '--write' : '--dry-run');
      return args;
    }
    case 'timeline-comment': {
      const issue = cleanIssueRef(body.issue);
      const text = String(body.markdown ?? '').trim();
      if (!text) throw new Error('timeline comment markdown is required');
      const path = join(tmpdir(), `shea-web-${Date.now()}-${issue.replace('#', '')}.md`);
      await writeFile(path, text);
      const args = ['project', 'timeline-comment', workflowPath, issue, path];
      args.push(write ? '--write' : '--dry-run');
      return args;
    }
    default:
      throw new Error(`unsupported action: ${action || '<empty>'}`);
  }
}

function cleanList(value) {
  if (!value) return [];
  if (Array.isArray(value)) return value.map((item) => String(item).trim()).filter(Boolean);
  return String(value)
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean);
}

function cleanReviewRejectTarget(target) {
  const value = String(target ?? '').trim();
  const allowed = new Set(['agent_review', 'rework', 'need_human_input', 'Agent Review', 'Rework', 'Need Human Input']);
  if (!allowed.has(value)) {
    throw new Error('review reject target must be agent_review, rework, or need_human_input');
  }
  return value;
}

function fixtureOverview() {
  const now = new Date().toISOString();
  const command = (args) => ({
    ok: true,
    args,
    exitCode: 0,
    signal: null,
    durationMs: 12,
    stderr: '',
    stdoutPreview: 'fixture output'
  });
  return {
    generatedAt: now,
    workflowPath,
    fixture: true,
    commands: {
      autopilot: command(['autopilot', 'plan', workflowPath, '--json']),
      doctor: command(['doctor', workflowPath, '--json']),
      review: command(['review', 'status', workflowPath, '--json']),
      skills: command(['skills', 'status', workflowPath, '--json']),
      sessions: command(['session', 'list', workflowPath]),
      local: command(['local', 'status']),
      githubQueue: command(['gh', 'issue', 'list', '--json', 'projectItems'])
    },
    autopilot: {
      readiness: {
        status: 'ready',
        reason: 'Fixture mode is exercising the operator UI without GitHub writes.'
      },
      lanes: [
        {
          lane: 'main',
          status: 'ready',
          selected_issue: '#418',
          action: 'Run quality gate before dispatch',
          reason: 'Todo issue is executable with current fixture evidence.',
          target_state: 'In Progress'
        },
        {
          lane: 'review',
          status: 'blocked',
          selected_issue: '#421',
          action: 'Human review routing decision needed',
          reason: 'Independent review evidence is present; operator must choose pass or reject.',
          target_state: 'Human Review'
        },
        {
          lane: 'merge',
          status: 'idle',
          selected_issue: null,
          action: 'No approved PR ready',
          reason: 'Merging queue is empty.',
          target_state: 'Merging'
        }
      ],
      parked_queues: [
        {
          state: 'Need Human Input',
          reason: 'Operator confirmation required before status mutation.',
          issues: [
            {
              identifier: '#421',
              title: 'Agent Review evidence needs Human Review routing',
              reason: 'Review evidence exists but no human decision has been recorded.',
              evidence: 'Fixture: review pass checklist is present; PR readback is fresh.'
            }
          ]
        }
      ],
      active_issues: [
        {
          issue: '#418',
          lane: 'main',
          title: 'ProjectV2 metadata refresh recovery',
          action: 'Quality gate dry-run',
          status: 'ready',
          evidence: 'Fixture: REST metadata was refreshed and readback succeeded.',
          target: 'In Progress'
        }
      ]
    },
    doctor: { blockers: 0, warnings: 0 },
    review: { recent: [] },
    skills: { status: 'ready' },
    sessionsText: 'agent_session_list=none',
    localStatus: {
      branch: 'main',
      head: 'fixture',
      dirtyCount: 0,
      worktreeCount: 1,
      buildPresent: true,
      binaryPresent: true,
      dirtyPreview: []
    },
    githubQueue: {
      totalOpen: 4,
      stateCounts: {
        Todo: 1,
        'Need Human Input': 1,
        'Agent Review': 1,
        Merging: 1
      },
      laneCounts: {
        main: 1,
        review: 1,
        merge: 1
      },
      operatorIssues: [
        {
          identifier: '#421',
          number: 421,
          title: 'Agent Review evidence needs Human Review routing',
          state: 'Need Human Input',
          updatedAt: now
        }
      ],
      issues: [],
      source: 'fixture GitHub queue'
    },
    healthy: true
  };
}

async function handleApi(request, response, url) {
  try {
    if (request.method === 'GET' && url.pathname === '/api/overview') {
      jsonResponse(
        response,
        200,
        await buildOverview(url.searchParams.get('force') === '1', url.searchParams.get('scope') ?? 'full')
      );
      return;
    }

    if (request.method === 'GET' && url.pathname === '/api/read-surface') {
      jsonResponse(
        response,
        200,
        await buildReadSurface(url.searchParams.get('name'), url.searchParams.get('force') === '1')
      );
      return;
    }

    if (request.method === 'GET' && url.pathname === '/api/health') {
      jsonResponse(response, 200, buildHealth());
      return;
    }

    if (request.method === 'POST' && url.pathname === '/api/command') {
      const body = await readRequestBody(request);
      const args = await commandArgsFor(body);
      if (fixtureEnabled()) {
        overviewCache = null;
        jsonResponse(response, 200, {
          ok: true,
          args,
          exitCode: 0,
          signal: null,
          durationMs: 8,
          stderr: '',
          stdout: `fixture_command=ok args=${JSON.stringify(args)}`,
          stdoutPreview: `fixture_command=ok args=${JSON.stringify(args)}`,
          parsed: { fixture: true, args }
        });
        return;
      }
      const result = await runShea(args, { timeoutMs: 180000 });
      overviewCache = null;
      jsonResponse(response, result.ok ? 200 : 500, {
        ...summarizeResult(result),
        stdout: result.stdout,
        parsed: parseJsonOutput(result)
      });
      return;
    }

    jsonResponse(response, 404, { error: 'unknown API route' });
  } catch (error) {
    jsonResponse(response, 400, { error: error.message });
  }
}

async function serveStatic(request, response, url) {
  if (!existsSync(buildRoot)) {
    jsonResponse(response, 503, {
      error: 'web/build is missing; run `npm run build` from web first'
    });
    return;
  }

  const requested = url.pathname === '/' ? '/index.html' : url.pathname;
  const normalized = normalize(decodeURIComponent(requested)).replace(/^(\.\.[/\\])+/, '');
  let filePath = resolve(join(buildRoot, normalized));
  if (!filePath.startsWith(buildRoot)) {
    response.writeHead(403);
    response.end('Forbidden');
    return;
  }

  if (!existsSync(filePath) || statSync(filePath).isDirectory()) {
    filePath = resolve(join(buildRoot, '200.html'));
  }

  response.writeHead(200, {
    'content-type': mimeTypes.get(extname(filePath)) ?? 'application/octet-stream'
  });
  createReadStream(filePath).pipe(response);
}

export function createSheaWebServer() {
  return createServer(async (request, response) => {
    const url = new URL(request.url ?? '/', `http://${host}:${port}`);
    if (url.pathname.startsWith('/api/')) {
      await handleApi(request, response, url);
      return;
    }
    await serveStatic(request, response, url);
  });
}

async function startServer() {
  const packageJson = JSON.parse(await readFile(join(webRoot, 'package.json'), 'utf8'));
  const hosts = process.env.HOST ? [host] : ['127.0.0.1', 'localhost', '0.0.0.0'];
  const attempts = [];

  for (let portOffset = 0; portOffset <= portFallbackLimit; portOffset += 1) {
    for (const candidateHost of hosts) {
      const candidatePort = port + portOffset;
      const server = createSheaWebServer();
      try {
        await new Promise((resolveListen, rejectListen) => {
          server.once('error', rejectListen);
          server.listen(candidatePort, candidateHost, resolveListen);
        });
        const displayHost = candidateHost === '0.0.0.0' ? '127.0.0.1' : candidateHost;
        if (candidateHost !== host || candidatePort !== port) {
          console.warn(
            `Preferred bind ${host}:${port} was unavailable; using ${candidateHost}:${candidatePort}.`
          );
        }
        console.log(`${packageJson.name} listening at http://${displayHost}:${candidatePort}`);
        return;
      } catch (error) {
        server.close();
        attempts.push(`${candidateHost}:${candidatePort} ${error.code ?? error.message}`);
      }
    }
  }

  console.error(`Cannot start ${packageJson.name}. Tried: ${attempts.join(', ')}`);
  console.error('Set PORT=... or HOST=... to force a specific bind target.');
  process.exit(1);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  startServer();
}
