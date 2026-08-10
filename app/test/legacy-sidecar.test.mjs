import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';

const configUrl = new URL('../src-tauri/tauri.conf.json', import.meta.url);
const packageUrl = new URL('../package.json', import.meta.url);
const stageScript = new URL('../../scripts/stage-legacy-sidecar.sh', import.meta.url);

test('Tauri bundle declares the versioned Legacy sidecar', () => {
  const config = JSON.parse(readFileSync(configUrl, 'utf8'));

  assert.equal(config.bundle.active, true);
  assert.deepEqual(config.bundle.externalBin, ['binaries/shea-symphony-legacy']);
});

test('local bundle script stages Legacy before building the App bundle', () => {
  const packageJson = JSON.parse(readFileSync(packageUrl, 'utf8'));

  assert.match(packageJson.scripts['bundle:legacy'], /stage-legacy-sidecar\.sh/);
  assert.match(packageJson.scripts['bundle:legacy'], /tauri build --bundles app/);
});

test('packaging preflight fails clearly when the target artifact is absent', () => {
  const result = spawnSync(stageScript.pathname, ['--check'], {
    encoding: 'utf8',
    env: {
      ...process.env,
      SHEA_LEGACY_SIDECAR_TARGET: 'missing-artifact-test-target'
    }
  });

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /missing target-specific Legacy sidecar/);
  assert.match(result.stderr, /missing-artifact-test-target/);
});
