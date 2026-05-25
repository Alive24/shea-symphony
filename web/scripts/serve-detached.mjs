import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, openSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { get } from 'node:http';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptRoot = dirname(fileURLToPath(import.meta.url));
const webRoot = resolve(scriptRoot, '..');
const buildRoot = join(webRoot, 'build');
const runRoot = join(webRoot, '.shea-web');
const pidFile = join(runRoot, 'server.pid');
const urlFile = join(runRoot, 'server.url');
const logFile = join(runRoot, 'server.log');
const command = process.argv[2] ?? 'start';

function ensureRunRoot() {
  mkdirSync(runRoot, { recursive: true });
}

function readPid() {
  if (!existsSync(pidFile)) return null;
  const pid = Number(readFileSync(pidFile, 'utf8').trim());
  return Number.isInteger(pid) && pid > 0 ? pid : null;
}

function isAlive(pid) {
  if (!pid) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (_) {
    return false;
  }
}

function readKnownUrl() {
  if (existsSync(urlFile)) return readFileSync(urlFile, 'utf8').trim();
  return null;
}

function cleanupStalePid() {
  const pid = readPid();
  if (pid && isAlive(pid)) return pid;
  rmSync(pidFile, { force: true });
  return null;
}

async function printStatus() {
  const pid = cleanupStalePid();
  const url = await findResponsiveUrl();
  if (url) writeFileSync(urlFile, `${url}\n`);
  if (pid || url) {
    console.log(`shea_web_server=running${pid ? ` pid=${pid}` : ''}${url ? ` url=${url}` : ''} log=${logFile}`);
    return 0;
  }
  console.log(`shea_web_server=stopped log=${logFile}`);
  return 1;
}

function stopServer() {
  const pid = cleanupStalePid();
  if (!pid) {
    console.log('shea_web_server=stopped');
    return 0;
  }
  process.kill(pid, 'SIGTERM');
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    if (!isAlive(pid)) break;
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 100);
  }
  if (isAlive(pid)) {
    console.error(`shea_web_server=still_running pid=${pid}`);
    return 1;
  }
  rmSync(pidFile, { force: true });
  console.log(`shea_web_server=stopped pid=${pid}`);
  return 0;
}

function httpGetJson(url) {
  return new Promise((resolveGet) => {
    const request = get(url, { timeout: 1200 }, (response) => {
      let body = '';
      response.setEncoding('utf8');
      response.on('data', (chunk) => {
        body += chunk;
      });
      response.on('end', () => {
        try {
          resolveGet({ ok: response.statusCode === 200, body: JSON.parse(body) });
        } catch (_) {
          resolveGet({ ok: false, body: null });
        }
      });
    });
    request.on('timeout', () => {
      request.destroy();
      resolveGet({ ok: false, body: null });
    });
    request.on('error', () => resolveGet({ ok: false, body: null }));
  });
}

async function findResponsiveUrl() {
  const known = readKnownUrl();
  const candidates = [
    known,
    'http://localhost:5173',
    'http://127.0.0.1:5173',
    ...Array.from({ length: 6 }, (_, index) => `http://localhost:${5174 + index}`),
    ...Array.from({ length: 6 }, (_, index) => `http://127.0.0.1:${5174 + index}`)
  ].filter(Boolean);

  for (const url of candidates) {
    const health = await httpGetJson(`${url}/api/health`);
    if (health.ok && health.body?.ok) return url;
  }
  return null;
}

function buildIfNeeded() {
  if (process.argv.includes('--no-build') && existsSync(buildRoot)) return;
  const result = spawnSync('npm', ['run', 'build'], {
    cwd: webRoot,
    env: process.env,
    stdio: 'inherit'
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function waitForLogUrl(startOffset) {
  const deadline = Date.now() + 8000;
  while (Date.now() < deadline) {
    if (existsSync(logFile)) {
      const log = readFileSync(logFile, 'utf8').slice(startOffset);
      const match = log.match(/listening at (http:\/\/[^\s]+)/);
      if (match) return match[1];
    }
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 100);
  }
  return null;
}

async function startServer() {
  ensureRunRoot();
  const existingPid = cleanupStalePid();
  const existingUrl = await findResponsiveUrl();
  if (existingPid || existingUrl) {
    if (existingUrl) writeFileSync(urlFile, `${existingUrl}\n`);
    console.log(
      `shea_web_server=running${existingPid ? ` pid=${existingPid}` : ''}${existingUrl ? ` url=${existingUrl}` : ''} log=${logFile}`
    );
    return 0;
  }

  buildIfNeeded();

  const previousLogBytes = existsSync(logFile) ? readFileSync(logFile).length : 0;
  const logFd = openSync(logFile, 'a');
  const child = spawn(process.execPath, ['server.mjs'], {
    cwd: webRoot,
    detached: true,
    env: process.env,
    stdio: ['ignore', logFd, logFd]
  });
  child.unref();
  writeFileSync(pidFile, `${child.pid}\n`);

  const loggedUrl = waitForLogUrl(previousLogBytes);
  const responsiveUrl = loggedUrl ? await findResponsiveUrl() : null;
  const url = responsiveUrl ?? loggedUrl;
  if (url) writeFileSync(urlFile, `${url}\n`);

  console.log(`shea_web_server=started pid=${child.pid}${url ? ` url=${url}` : ''} log=${logFile}`);
  return 0;
}

if (command === 'status') {
  ensureRunRoot();
  process.exit(await printStatus());
}
if (command === 'stop') {
  process.exit(stopServer());
}
if (command !== 'start') {
  console.error('usage: npm run serve:detached [-- status|stop]');
  process.exit(2);
}

process.exit(await startServer());
