import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { get } from 'node:http';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { homedir } from 'node:os';

const scriptRoot = dirname(fileURLToPath(import.meta.url));
const webRoot = resolve(scriptRoot, '..');
const runRoot = join(webRoot, '.shea-web');
const logRoot = join(homedir(), 'Library', 'Logs', 'SheaSymphony');
const label = 'com.shea-symphony.operator-desk';
const plistPath = join(homedir(), 'Library', 'LaunchAgents', `${label}.plist`);
const uid = process.getuid?.();
const domain = Number.isInteger(uid) ? `gui/${uid}` : 'gui';
const command = process.argv[2] ?? 'status';

function xmlEscape(value) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&apos;');
}

function run(program, args, options = {}) {
  return spawnSync(program, args, {
    cwd: webRoot,
    env: process.env,
    encoding: 'utf8',
    stdio: options.inherit ? 'inherit' : 'pipe'
  });
}

function launchctl(args, options = {}) {
  return run('launchctl', args, options);
}

function buildPlist() {
  const nodePath = process.execPath;
  const stdoutPath = join(logRoot, 'operator-desk.out.log');
  const stderrPath = join(logRoot, 'operator-desk.err.log');

  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>${label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>${xmlEscape(nodePath)}</string>
    <string>server.mjs</string>
  </array>
  <key>WorkingDirectory</key>
  <string>${xmlEscape(webRoot)}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOST</key>
    <string>127.0.0.1</string>
    <key>PORT</key>
    <string>5173</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ThrottleInterval</key>
  <integer>10</integer>
  <key>StandardOutPath</key>
  <string>${xmlEscape(stdoutPath)}</string>
  <key>StandardErrorPath</key>
  <string>${xmlEscape(stderrPath)}</string>
</dict>
</plist>
`;
}

function httpHealth(url) {
  return new Promise((resolveHealth) => {
    const request = get(url, { timeout: 1200 }, (response) => {
      response.resume();
      response.on('end', () => resolveHealth(response.statusCode === 200));
    });
    request.on('timeout', () => {
      request.destroy();
      resolveHealth(false);
    });
    request.on('error', () => resolveHealth(false));
  });
}

async function printStatus() {
  const healthy = await httpHealth('http://127.0.0.1:5173/api/health');
  const printed = launchctl(['print', `${domain}/${label}`]);
  const loaded = printed.status === 0;
  console.log(
    `shea_web_autostart=${loaded ? 'loaded' : 'unloaded'} health=${healthy ? 'ok' : 'unreachable'} plist=${plistPath}`
  );
  return loaded && healthy ? 0 : 1;
}

function install() {
  mkdirSync(dirname(plistPath), { recursive: true });
  mkdirSync(runRoot, { recursive: true });
  mkdirSync(logRoot, { recursive: true });

  const build = run('npm', ['run', 'build'], { inherit: true });
  if (build.status !== 0) return build.status ?? 1;

  writeFileSync(plistPath, buildPlist());
  launchctl(['bootout', domain, plistPath]);
  const bootstrap = launchctl(['bootstrap', domain, plistPath]);
  if (bootstrap.status !== 0) {
    process.stderr.write(bootstrap.stderr || bootstrap.stdout);
    return bootstrap.status ?? 1;
  }

  const kickstart = launchctl(['kickstart', '-k', `${domain}/${label}`]);
  if (kickstart.status !== 0) {
    process.stderr.write(kickstart.stderr || kickstart.stdout);
    return kickstart.status ?? 1;
  }

  console.log(`shea_web_autostart=installed label=${label} plist=${plistPath}`);
  return 0;
}

function uninstall() {
  if (existsSync(plistPath)) {
    launchctl(['bootout', domain, plistPath]);
  }
  console.log(`shea_web_autostart=uninstalled label=${label} plist=${plistPath}`);
  return 0;
}

if (command === 'install') {
  process.exit(install());
}

if (command === 'uninstall') {
  process.exit(uninstall());
}

if (command === 'status') {
  process.exit(await printStatus());
}

if (command === 'logs') {
  const out = join(logRoot, 'operator-desk.out.log');
  const err = join(logRoot, 'operator-desk.err.log');
  if (existsSync(out)) process.stdout.write(readFileSync(out, 'utf8').slice(-4000));
  if (existsSync(err)) process.stderr.write(readFileSync(err, 'utf8').slice(-4000));
  process.exit(0);
}

console.error('usage: npm run autostart:[install|status|uninstall|logs]');
process.exit(2);
