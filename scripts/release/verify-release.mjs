import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, readdirSync, copyFileSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

export const RELEASE_MANIFEST = 'release-manifest.json';
export const CHECKSUM_MANIFEST = 'SHA256SUMS';

const TARGETS = Object.freeze({
  'aarch64-apple-darwin': {
    platform: 'macos',
    architecture: 'aarch64',
    assetName: (tag) => `Shea-Symphony-App-${tag}-macos-arm64.zip`
  },
  'x86_64-pc-windows-msvc': {
    platform: 'windows',
    architecture: 'x86_64',
    assetName: (tag) => `Shea-Symphony-App-${tag}-windows-x64-setup.exe`
  }
});

function fail(message) {
  throw new Error(message);
}

function cargoPackageVersion(text, source) {
  const packageStart = text.match(/^\[package\]\s*$/m);
  if (!packageStart) fail(`could not find [package] in ${source}`);
  const remainder = text.slice(packageStart.index + packageStart[0].length);
  const nextTable = remainder.search(/^\[/m);
  const packageBlock = nextTable === -1 ? remainder : remainder.slice(0, nextTable);
  const version = packageBlock?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  if (!version) fail(`could not read [package] version from ${source}`);
  return version;
}

export function readVersionContract(repositoryRoot) {
  const root = resolve(repositoryRoot);
  return {
    rootCargo: cargoPackageVersion(readFileSync(join(root, 'Cargo.toml'), 'utf8'), 'Cargo.toml'),
    appPackage: JSON.parse(readFileSync(join(root, 'app/package.json'), 'utf8')).version,
    appCargo: cargoPackageVersion(
      readFileSync(join(root, 'app/src-tauri/Cargo.toml'), 'utf8'),
      'app/src-tauri/Cargo.toml'
    ),
    tauri: JSON.parse(readFileSync(join(root, 'app/src-tauri/tauri.conf.json'), 'utf8')).version
  };
}

export function validateVersionContract(repositoryRoot, tag) {
  if (!/^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/.test(tag)) {
    fail(`release tag must be a stable semantic version: ${tag}`);
  }
  const versions = readVersionContract(repositoryRoot);
  const unique = new Set(Object.values(versions));
  if (unique.size !== 1) {
    fail(`release versions disagree: ${JSON.stringify(versions)}`);
  }
  const version = versions.rootCargo;
  if (tag !== `v${version}`) {
    fail(`release tag ${tag} does not match repository version ${version}`);
  }
  return { tag, version, versions };
}

function validateRevision(revision) {
  if (!/^[0-9a-f]{40}$/.test(revision)) {
    fail(`release source revision must be a full lowercase Git commit: ${revision}`);
  }
}

export function sha256File(path) {
  const hash = createHash('sha256');
  hash.update(readFileSync(path));
  return hash.digest('hex');
}

function runtimeIdentity(runtimePath) {
  const result = spawnSync(runtimePath, ['--runtime-info'], { encoding: 'utf8' });
  if (result.status !== 0) {
    fail(`Legacy runtime identity probe failed for ${runtimePath}: ${result.stderr.trim()}`);
  }
  try {
    return JSON.parse(result.stdout.trim());
  } catch (error) {
    fail(`Legacy runtime emitted invalid identity JSON: ${error.message}`);
  }
}

function validateRuntimeIdentity(identity, { version, revision, target }) {
  const spec = TARGETS[target];
  if (!spec) fail(`unsupported release target: ${target}`);
  const expected = {
    schema_version: 1,
    binary_role: 'legacy_cli',
    cli_version: version,
    source_revision: revision,
    target,
    platform: spec.platform,
    architecture: spec.architecture,
    compatibility: 'shea-legacy-cli-v1'
  };
  for (const [key, value] of Object.entries(expected)) {
    if (identity[key] !== value) {
      fail(`Legacy runtime ${key} mismatch: expected ${value}, received ${identity[key]}`);
    }
  }
  return spec;
}

export function createBuildMetadata({ repositoryRoot, tag, revision, target, runtimePath, assetPath }) {
  validateRevision(revision);
  const { version } = validateVersionContract(repositoryRoot, tag);
  const identity = runtimeIdentity(runtimePath);
  const spec = validateRuntimeIdentity(identity, { version, revision, target });
  const expectedAsset = spec.assetName(tag);
  if (basename(assetPath) !== expectedAsset) {
    fail(`release asset must be named ${expectedAsset}, received ${basename(assetPath)}`);
  }
  if (!statSync(assetPath).isFile() || statSync(assetPath).size === 0) {
    fail(`release asset is missing or empty: ${assetPath}`);
  }
  return {
    schema_version: 1,
    tag,
    version,
    source_revision: revision,
    target,
    platform: spec.platform,
    architecture: spec.architecture,
    unsigned: true,
    asset: {
      name: expectedAsset,
      size: statSync(assetPath).size,
      sha256: sha256File(assetPath)
    },
    runtime: {
      ...identity,
      sha256: sha256File(runtimePath)
    }
  };
}

function walkFiles(root) {
  return readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    const path = join(root, entry.name);
    return entry.isDirectory() ? walkFiles(path) : [path];
  });
}

function validateBuildMetadata(metadata, tag, revision) {
  validateRevision(revision);
  if (metadata.schema_version !== 1 || metadata.tag !== tag || metadata.source_revision !== revision) {
    fail(`build metadata does not match release ${tag} at ${revision}`);
  }
  const spec = TARGETS[metadata.target];
  if (!spec) fail(`build metadata has unsupported target ${metadata.target}`);
  if (
    metadata.platform !== spec.platform ||
    metadata.architecture !== spec.architecture ||
    metadata.asset?.name !== spec.assetName(tag) ||
    metadata.runtime?.binary_role !== 'legacy_cli' ||
    metadata.runtime?.compatibility !== 'shea-legacy-cli-v1' ||
    metadata.runtime?.source_revision !== revision ||
    metadata.runtime?.target !== metadata.target ||
    metadata.runtime?.platform !== spec.platform ||
    metadata.runtime?.architecture !== spec.architecture ||
    metadata.runtime?.cli_version !== metadata.version ||
    metadata.unsigned !== true
  ) {
    fail(`build metadata identity is inconsistent for ${metadata.target}`);
  }
  for (const digest of [metadata.asset?.sha256, metadata.runtime?.sha256]) {
    if (!/^[0-9a-f]{64}$/.test(digest ?? '')) fail(`build metadata has an invalid SHA-256 digest`);
  }
}

function validateManifest(manifest, tag, revision) {
  validateRevision(revision);
  if (
    manifest.schema_version !== 1 ||
    manifest.tag !== tag ||
    manifest.source_revision !== revision ||
    manifest.unsigned !== true ||
    !Array.isArray(manifest.builds) ||
    manifest.builds.length !== Object.keys(TARGETS).length
  ) {
    fail(`release manifest does not match ${tag} at ${revision}`);
  }
  const targets = new Set();
  for (const build of manifest.builds) {
    validateBuildMetadata(build, tag, revision);
    if (build.version !== manifest.version || tag !== `v${manifest.version}`) {
      fail(`release manifest versions disagree for ${build.target}`);
    }
    if (targets.has(build.target)) fail(`release manifest repeats target ${build.target}`);
    targets.add(build.target);
  }
  for (const target of Object.keys(TARGETS)) {
    if (!targets.has(target)) fail(`release manifest is missing target ${target}`);
  }
}

export function aggregateRelease({ inputDir, outputDir, tag, revision }) {
  validateRevision(revision);
  const metadataPaths = walkFiles(inputDir).filter((path) => path.endsWith('.release.json'));
  if (metadataPaths.length !== Object.keys(TARGETS).length) {
    fail(`expected ${Object.keys(TARGETS).length} build metadata files, found ${metadataPaths.length}`);
  }
  const builds = metadataPaths
    .map((path) => ({ path, value: JSON.parse(readFileSync(path, 'utf8')) }))
    .sort((left, right) => left.value.target.localeCompare(right.value.target));
  const seenTargets = new Set();
  mkdirSync(outputDir, { recursive: true });
  for (const build of builds) {
    validateBuildMetadata(build.value, tag, revision);
    if (seenTargets.has(build.value.target)) fail(`duplicate target ${build.value.target}`);
    seenTargets.add(build.value.target);
    const assetPath = join(dirname(build.path), build.value.asset.name);
    if (sha256File(assetPath) !== build.value.asset.sha256) {
      fail(`staged asset digest mismatch for ${build.value.asset.name}`);
    }
    if (statSync(assetPath).size !== build.value.asset.size) {
      fail(`staged asset size mismatch for ${build.value.asset.name}`);
    }
    copyFileSync(assetPath, join(outputDir, build.value.asset.name));
  }
  for (const target of Object.keys(TARGETS)) {
    if (!seenTargets.has(target)) fail(`missing release target ${target}`);
  }
  const manifest = {
    schema_version: 1,
    tag,
    version: builds[0].value.version,
    source_revision: revision,
    unsigned: true,
    builds: builds.map((entry) => entry.value)
  };
  validateManifest(manifest, tag, revision);
  const manifestPath = join(outputDir, RELEASE_MANIFEST);
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  const checksummed = [...builds.map((entry) => entry.value.asset.name), RELEASE_MANIFEST].sort();
  const checksums = checksummed
    .map((name) => `${sha256File(join(outputDir, name))}  ${name}`)
    .join('\n');
  writeFileSync(join(outputDir, CHECKSUM_MANIFEST), `${checksums}\n`);
  return manifest;
}

function parseChecksums(text) {
  const entries = new Map();
  for (const line of text.trim().split('\n')) {
    const match = line.match(/^([0-9a-f]{64})  ([^/\\]+)$/);
    if (!match || entries.has(match[2])) fail(`invalid or duplicate checksum line: ${line}`);
    entries.set(match[2], match[1]);
  }
  return entries;
}

export function verifyPublishedRelease({ releaseDir, tag, revision }) {
  const manifest = JSON.parse(readFileSync(join(releaseDir, RELEASE_MANIFEST), 'utf8'));
  validateManifest(manifest, tag, revision);
  const expectedAssets = new Set([
    ...manifest.builds.map((build) => build.asset.name),
    RELEASE_MANIFEST,
    CHECKSUM_MANIFEST
  ]);
  const actualAssets = new Set(
    readdirSync(releaseDir, { withFileTypes: true })
      .filter((entry) => entry.isFile())
      .map((entry) => entry.name)
  );
  if (
    expectedAssets.size !== actualAssets.size ||
    [...expectedAssets].some((name) => !actualAssets.has(name))
  ) {
    fail(`published asset set is incomplete or unexpected`);
  }
  const checksums = parseChecksums(readFileSync(join(releaseDir, CHECKSUM_MANIFEST), 'utf8'));
  const checksummedAssets = [...expectedAssets].filter((name) => name !== CHECKSUM_MANIFEST).sort();
  if (checksums.size !== checksummedAssets.length) fail(`checksum manifest has the wrong asset count`);
  for (const name of checksummedAssets) {
    if (checksums.get(name) !== sha256File(join(releaseDir, name))) {
      fail(`published checksum mismatch for ${name}`);
    }
  }
  for (const build of manifest.builds) {
    if (build.asset.sha256 !== sha256File(join(releaseDir, build.asset.name))) {
      fail(`published asset disagrees with release manifest for ${build.asset.name}`);
    }
  }
  return manifest;
}

function options(argv) {
  const parsed = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith('--') || value === undefined) fail(`invalid arguments`);
    parsed[key.slice(2)] = value;
  }
  return parsed;
}

function required(values, name) {
  if (!values[name]) fail(`missing --${name}`);
  return values[name];
}

function main(argv) {
  const [command, ...rest] = argv;
  const values = options(rest);
  if (command === 'preflight') {
    validateRevision(required(values, 'revision'));
    console.log(JSON.stringify(validateVersionContract(required(values, 'root'), required(values, 'tag'))));
    return;
  }
  if (command === 'build') {
    const metadata = createBuildMetadata({
      repositoryRoot: required(values, 'root'),
      tag: required(values, 'tag'),
      revision: required(values, 'revision'),
      target: required(values, 'target'),
      runtimePath: required(values, 'runtime'),
      assetPath: required(values, 'asset')
    });
    writeFileSync(required(values, 'metadata'), `${JSON.stringify(metadata, null, 2)}\n`);
    console.log(`release_build_verify=ok target=${metadata.target} asset=${metadata.asset.name}`);
    return;
  }
  if (command === 'aggregate') {
    const manifest = aggregateRelease({
      inputDir: required(values, 'input'),
      outputDir: required(values, 'output'),
      tag: required(values, 'tag'),
      revision: required(values, 'revision')
    });
    console.log(`release_aggregate_verify=ok builds=${manifest.builds.length}`);
    return;
  }
  if (command === 'published') {
    const manifest = verifyPublishedRelease({
      releaseDir: required(values, 'input'),
      tag: required(values, 'tag'),
      revision: required(values, 'revision')
    });
    console.log(`release_readback_verify=ok tag=${manifest.tag} revision=${manifest.source_revision}`);
    return;
  }
  fail(`usage: verify-release.mjs preflight|build|aggregate|published [options]`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
