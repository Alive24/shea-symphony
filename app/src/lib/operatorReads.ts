import { buildFixtureOverview, buildFixtureReadSurface } from './operatorFixtures.ts';
import { getDataMode, recordCliLog, updateCliLog } from './uiState.ts';
import { getOperatorOverview, getReadSurface, isTauriRuntime } from './tauriAutoloop.ts';

export async function loadOverview(force = false, scope = 'full') {
  if (getDataMode() === 'fixture') return buildFixtureOverview(force);
  if (isTauriRuntime()) {
    const logId = recordCliLog({
      surface: `overview:${scope}`,
      phase: 'start',
      status: 'running',
      detail: 'Requesting non-blocking operator overview from Tauri.'
    });
    const startedAt = performance.now();
    try {
      const overview = await getOperatorOverview(force, scope);
      if (overview) {
        updateCliLog(logId, {
          surface: `overview:${scope}`,
          phase: 'finish',
          status: 'ok',
          detail: 'Overview returned; slow CLI surfaces may still be pending.',
          durationMs: Math.round(performance.now() - startedAt)
        });
        return overview;
      }
    } catch (error) {
      updateCliLog(logId, {
        surface: `overview:${scope}`,
        phase: 'error',
        status: 'failed',
        detail: errorMessage(error),
        durationMs: Math.round(performance.now() - startedAt)
      });
      throw error;
    }
  }
  return buildFixtureOverview(force);
}

export async function loadHealth() {
  return {
    ok: true,
    generatedAt: Date.now(),
    workflowPath: 'workflows/shea-symphony.md',
    fixture: !isTauriRuntime(),
    buildPresent: false,
    cli: {
      mode: isTauriRuntime() ? 'tauri' : 'fixture',
      path: isTauriRuntime() ? 'Tauri allowlisted CLI commands' : 'Browser fixture preview'
    },
    runtime: {
      host: isTauriRuntime() ? 'desktop' : 'browser',
      bridge: isTauriRuntime() ? 'tauri' : 'fixture'
    }
  };
}

export async function loadReadSurface(name, force = false) {
  if (getDataMode() === 'fixture') return buildFixtureReadSurface(name, force);
  if (isTauriRuntime()) {
    const logId = recordCliLog({
      surface: name,
      phase: 'start',
      status: 'running',
      detail: `Starting CLI read surface: ${name}.`
    });
    const startedAt = performance.now();
    try {
      const surface = await getReadSurface(name, force);
      if (surface) {
        const command = (surface.command ?? {}) as any;
        updateCliLog(logId, {
          surface: name,
          phase: 'finish',
          status: command.ok ? 'ok' : command.timedOut ? 'timeout' : command.pending ? 'pending' : 'failed',
          detail: command.stderr || command.stdoutPreview || `Finished CLI read surface: ${name}.`,
          args: command.args ?? [],
          durationMs: command.durationMs ?? Math.round(performance.now() - startedAt)
        });
        return surface;
      }
    } catch (error) {
      updateCliLog(logId, {
        surface: name,
        phase: 'error',
        status: 'failed',
        detail: errorMessage(error),
        durationMs: Math.round(performance.now() - startedAt)
      });
      throw error;
    }
  }
  return buildFixtureReadSurface(name, force);
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error ?? 'unknown error');
}
