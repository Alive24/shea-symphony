import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  aggregateRelease,
  CHECKSUM_MANIFEST,
  RELEASE_MANIFEST,
  sha256File,
  validateVersionContract,
  verifyPublishedRelease
} from '../../scripts/release/verify-release.mjs';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const revision = '0123456789abcdef0123456789abcdef01234567';
const tag = 'v0.1.0';

function buildFixture(root, target, contents) {
  const spec = target === 'aarch64-apple-darwin'
    ? { platform: 'macos', architecture: 'aarch64', asset: `Shea-Symphony-App-${tag}-macos-arm64.zip` }
    : { platform: 'windows', architecture: 'x86_64', asset: `Shea-Symphony-App-${tag}-windows-x64-setup.exe` };
  const directory = join(root, spec.platform);
  mkdirSync(directory, { recursive: true });
  const assetPath = join(directory, spec.asset);
  writeFileSync(assetPath, contents);
  const metadata = {
    schema_version: 1,
    tag,
    version: '0.1.0',
    source_revision: revision,
    target,
    platform: spec.platform,
    architecture: spec.architecture,
    unsigned: true,
    asset: {
      name: spec.asset,
      size: Buffer.byteLength(contents),
      sha256: sha256File(assetPath)
    },
    runtime: {
      schema_version: 1,
      binary_role: 'legacy_cli',
      cli_version: '0.1.0',
      source_revision: revision,
      target,
      platform: spec.platform,
      architecture: spec.architecture,
      compatibility: 'shea-legacy-cli-v1',
      sha256: 'a'.repeat(64)
    }
  };
  writeFileSync(join(directory, `${spec.platform}.release.json`), `${JSON.stringify(metadata)}\n`);
  return { assetPath, metadata };
}

function completeInput() {
  const root = mkdtempSync(join(tmpdir(), 'shea-release-input-'));
  const macos = buildFixture(root, 'aarch64-apple-darwin', 'macOS package');
  const windows = buildFixture(root, 'x86_64-pc-windows-msvc', 'Windows package');
  return { root, macos, windows };
}

test('repository and App versions agree with the stable tag', () => {
  const result = validateVersionContract(repositoryRoot, tag);

  assert.equal(result.version, '0.1.0');
  assert.deepEqual(new Set(Object.values(result.versions)), new Set(['0.1.0']));
  assert.throws(() => validateVersionContract(repositoryRoot, 'v0.1.1'), /does not match/);
  assert.throws(() => validateVersionContract(repositoryRoot, 'v0.1.0-rc.1'), /stable semantic/);
});

test('native desktop icon resources are present', () => {
  const icons = join(repositoryRoot, 'app/src-tauri/icons');

  assert.ok(existsSync(join(icons, 'icon.icns')), 'macOS icon resource is missing');
  assert.ok(existsSync(join(icons, 'icon.ico')), 'Windows icon resource is missing');
});

test('release workflow is native, draft-first, and state-last', () => {
  const workflow = readFileSync(join(repositoryRoot, '.github/workflows/release.yml'), 'utf8');
  const draft = workflow.indexOf('gh release create "$RELEASE_TAG"');
  const readback = workflow.indexOf('verify-release.mjs published');
  const publish = workflow.indexOf('-F draft=false');

  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /pull_request:/);
  assert.match(workflow, /runs-on: macos-15/);
  assert.match(workflow, /RELEASE_TARGET: aarch64-apple-darwin/);
  assert.match(workflow, /runs-on: windows-2025/);
  assert.match(workflow, /RELEASE_TARGET: x86_64-pc-windows-msvc/);
  assert.match(workflow, /permissions:\n\s+contents: read/);
  assert.match(workflow, /publish:[\s\S]*permissions:\n\s+contents: write/);
  assert.match(workflow, /publish:[\s\S]*if: github\.event_name == 'workflow_dispatch'/);
  assert.ok(draft > 0 && readback > draft && publish > readback);
  assert.doesNotMatch(workflow, /shea-symphony-legacy[^\n]*(upload|release-output)/i);
});

test('aggregation requires both native builds and produces complete checksums', () => {
  const input = completeInput();
  const output = mkdtempSync(join(tmpdir(), 'shea-release-output-'));

  const manifest = aggregateRelease({ inputDir: input.root, outputDir: output, tag, revision });

  assert.deepEqual(
    manifest.builds.map((build) => build.target),
    ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc']
  );
  assert.match(readFileSync(join(output, CHECKSUM_MANIFEST), 'utf8'), new RegExp(RELEASE_MANIFEST));
  assert.equal(verifyPublishedRelease({ releaseDir: output, tag, revision }).tag, tag);
});

test('aggregation fails closed for missing builds and version mismatches', () => {
  const missing = mkdtempSync(join(tmpdir(), 'shea-release-missing-'));
  buildFixture(missing, 'aarch64-apple-darwin', 'macOS package');
  const output = mkdtempSync(join(tmpdir(), 'shea-release-output-'));
  assert.throws(
    () => aggregateRelease({ inputDir: missing, outputDir: output, tag, revision }),
    /expected 2 build metadata files/
  );

  const mismatched = completeInput();
  const windowsMetadataPath = join(mismatched.root, 'windows/windows.release.json');
  const metadata = JSON.parse(readFileSync(windowsMetadataPath, 'utf8'));
  metadata.version = '0.1.1';
  metadata.runtime.cli_version = '0.1.1';
  writeFileSync(windowsMetadataPath, JSON.stringify(metadata));
  assert.throws(
    () => aggregateRelease({ inputDir: mismatched.root, outputDir: output, tag, revision }),
    /versions disagree/
  );
});

test('published readback rejects tampered and partial asset sets', () => {
  const input = completeInput();
  const output = mkdtempSync(join(tmpdir(), 'shea-release-output-'));
  aggregateRelease({ inputDir: input.root, outputDir: output, tag, revision });
  writeFileSync(join(output, input.macos.metadata.asset.name), 'tampered');

  assert.throws(
    () => verifyPublishedRelease({ releaseDir: output, tag, revision }),
    /checksum mismatch|disagrees/
  );

  const complete = mkdtempSync(join(tmpdir(), 'shea-release-output-'));
  aggregateRelease({ inputDir: input.root, outputDir: complete, tag, revision });
  writeFileSync(join(complete, 'unexpected.txt'), 'unexpected');
  assert.throws(
    () => verifyPublishedRelease({ releaseDir: complete, tag, revision }),
    /incomplete or unexpected/
  );
});
