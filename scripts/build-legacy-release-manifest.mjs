#!/usr/bin/env node

import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const SUPPORTED_TARGETS = [
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  "x86_64-unknown-linux-gnu",
];

function args(argv) {
  const options = { allowPartial: false };
  while (argv.length) {
    const flag = argv.shift();
    if (flag === "--allow-partial") options.allowPartial = true;
    else if (["--input-dir", "--release-tag", "--output"].includes(flag)) {
      if (!argv.length) throw new Error(`${flag} requires a value`);
      options[flag.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = argv.shift();
    } else throw new Error(`unknown argument: ${flag}`);
  }
  if (!options.inputDir || !options.releaseTag || !options.output) {
    throw new Error("--input-dir, --release-tag, and --output are required");
  }
  return options;
}

function main() {
  const options = args(process.argv.slice(2));
  const input = resolve(options.inputDir);
  const records = readdirSync(input)
    .filter((name) => name.endsWith(".identity.json"))
    .map((name) => JSON.parse(readFileSync(resolve(input, name), "utf8")))
    .sort((left, right) => left.target.localeCompare(right.target));
  if (!records.length) throw new Error("no Legacy release identity records found");
  const first = records[0];
  for (const record of records) {
    for (const [key, value] of Object.entries({
      schema_version: 1,
      binary_role: "legacy_cli",
      compatibility: "shea-legacy-cli-v1",
      release_tag: options.releaseTag,
      cli_version: first.cli_version,
      source_revision: first.source_revision,
    })) {
      if (record[key] !== value) throw new Error(`${record.target} ${key} mismatch`);
    }
    if (!SUPPORTED_TARGETS.includes(record.target)) throw new Error(`unsupported target record: ${record.target}`);
  }
  const actual = records.map((record) => record.target).sort();
  if (!options.allowPartial && JSON.stringify(actual) !== JSON.stringify([...SUPPORTED_TARGETS].sort())) {
    throw new Error(`release target matrix incomplete: ${actual.join(",")}`);
  }
  const manifest = {
    schema_version: 1,
    release_tag: options.releaseTag,
    cli_version: first.cli_version,
    source_revision: first.source_revision,
    compatibility: "shea-legacy-cli-v1",
    artifacts: records.map((record) => ({
      target: record.target,
      platform: record.platform,
      architecture: record.architecture,
      archive: record.archive,
      binary: record.binary,
      sha256: record.sha256,
    })),
  };
  writeFileSync(resolve(options.output), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`legacy_release_manifest=ok artifacts=${records.length} revision=${first.source_revision}`);
}

try {
  main();
} catch (error) {
  console.error(error.message);
  process.exit(1);
}
