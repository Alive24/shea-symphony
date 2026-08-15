import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

import {
  MANAGED_MARKER,
  REQUIRED_PROJECT_FIELDS,
  REQUIRED_STATUSES,
  applySetup,
  buildSetupPlan,
  normalSkills,
  publicPlan,
  sha256,
  validateRuntimeIdentity,
} from "./setup-shea-lib.mjs";

const REVISION = "0123456789abcdef0123456789abcdef01234567";

function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8" });
  assert.equal(result.status, 0, `${command} failed: ${result.stderr}`);
  return String(result.stdout).trim();
}

function fixture() {
  const repo = mkdtempSync(join(tmpdir(), "setup-shea-test-"));
  run("git", ["init", "--initial-branch=main"], repo);
  writeFileSync(join(repo, "package.json"), "{}\n");
  run("git", ["add", "package.json"], repo);
  run("git", ["-c", "user.name=Setup Test", "-c", "user.email=setup@example.invalid", "commit", "-m", "fixture"], repo);
  const log = join(repo, "legacy.log");
  const cli = join(repo, "fake-shea-symphony-legacy");
  const identity = {
    schema_version: 1,
    binary_role: "legacy_cli",
    cli_version: "0.1.0",
    source_revision: REVISION,
    target: "aarch64-apple-darwin",
    platform: "macos",
    architecture: "aarch64",
    compatibility: "shea-legacy-cli-v1",
  };
  writeFileSync(
    cli,
    `#!/bin/sh\nif [ "${"$1"}" = "--runtime-info" ]; then\n  printf '%s\\n' '${JSON.stringify(identity)}'\n  exit 0\nfi\nprintf '%s\\n' "${"$*"}" >> '${log}'\nexit 0\n`,
  );
  chmodSync(cli, 0o755);
  const request = {
    schema_version: 1,
    run_id: "fixture-run",
    source: {
      suite_version: "2026.08.15",
      source_revision: REVISION,
      suite_url: `https://github.com/Alive24/shea-symphony/archive/${REVISION}.tar.gz`,
    },
    runtime: {
      cli_path: cli,
      discovery_path: join(repo, "missing-runtime-discovery.json"),
      target: identity.target,
      architecture: identity.architecture,
    },
    harnesses: ["codex"],
    backends: { main: "codex", review: "codex-app-server", merge: "codex" },
    tracker: {
      repository: "example/target",
      project_owner: "example",
      project_owner_type: "user",
      project_number: 7,
      base_branch: "main",
    },
    runtime_profile: {
      schema_version: 1,
      profile_id: "fixture-runtime",
      generated_at: "2026-08-15T00:00:00Z",
      repository: { kind: "github", id: "example/target" },
      requirement_sources: [{ path: "package.json", git_blob: "fixture" }],
      tools: [{ name: "node", path: process.execPath, version_args: ["--version"], observed_version: process.version, version_requirement: process.version }],
      environment: {},
    },
    verification: { blocking: ["true"], advisory: ["false"] },
  };
  const discovery = {
    repository_id: "example/target",
    base_branch: "main",
    github_authenticated: true,
    issues_available: true,
    harnesses: { codex: true, "claude-code": false, antigravity: false },
    project: {
      available: true,
      fields: [
        ...REQUIRED_PROJECT_FIELDS.map((name) => ({ name, dataType: "TEXT" })),
        { name: "Status", dataType: "SINGLE_SELECT", options: REQUIRED_STATUSES.map((name) => ({ name })) },
      ],
    },
  };
  return { repo, request, discovery, cli, log, identity };
}

test("runtime identity validation fails closed for every trust-boundary mismatch", () => {
  const good = fixture().identity;
  assert.equal(validateRuntimeIdentity(good).binary_role, "legacy_cli");
  const cases = [
    [{ ...good, schema_version: 2 }, "schema_version"],
    [{ ...good, binary_role: "temporal_worker" }, "binary_role"],
    [{ ...good, compatibility: "shea-temporal-worker-v1" }, "compatibility"],
    [{ ...good, target: "x86_64-apple-darwin" }, "target"],
    [{ ...good, architecture: "x86_64" }, "architecture"],
    [{ ...good, source_revision: "stale" }, "source_revision"],
    [{ ...good, cli_version: "9.9.9" }, "cli_version"],
  ];
  for (const [identity, field] of cases) {
    assert.throws(() => validateRuntimeIdentity(identity, { [field]: good[field] }), /rejected/);
  }
  assert.throws(() => validateRuntimeIdentity(null), /malformed identity/);
});

test("fresh apply is no-claim, cleans only its marked staging run, and reruns as a no-op", async () => {
  const data = fixture();
  data.request.runtime_profile.environment.SETUP_SHEA_FIXTURE = "ready";
  data.request.verification.blocking = [
    "test \"$SETUP_SHEA_FIXTURE\" = ready && test \"$SHEA_SYMPHONY_RUNTIME_PROFILE_ID\" = fixture-runtime",
  ];
  mkdirSync(join(data.repo, ".shea", "local"), { recursive: true });
  writeFileSync(join(data.repo, ".shea", "local", "unrelated.txt"), "preserve me\n");
  const first = await buildSetupPlan(data);
  assert.equal(first.conflicts.length, 0);
  assert(first.actions.some((action) => action.classification === "create"));
  const applied = await applySetup({ ...data, confirm: first.plan_id, skipSkills: true });
  assert.equal(applied.readiness.status, "ready");
  assert.equal(applied.readiness.no_claim, true);
  assert.equal(
    JSON.parse(readFileSync(join(data.repo, ".shea", "setup.json"), "utf8"))
      .last_successful_readiness.status,
    "ready",
  );
  assert(existsSync(join(data.repo, ".shea", "local", "unrelated.txt")));
  assert(!existsSync(join(data.repo, ".shea", "local", "setup", data.request.run_id)));
  assert(readFileSync(data.log, "utf8").includes("validate"));
  assert(!/claim|set-state| main | review | merge /.test(readFileSync(data.log, "utf8")));

  for (const name of normalSkills()) {
    const root = join(data.repo, ".agents", "skills", name);
    mkdirSync(root, { recursive: true });
    writeFileSync(join(root, "SKILL.md"), `---\nname: ${name}\ndescription: fixture\n---\n`);
  }
  const statusBefore = run("git", ["status", "--porcelain=v1", "--untracked-files=all"], data.repo);
  const second = await buildSetupPlan(data);
  assert(second.actions.every((action) => action.classification === "no-op"));
  const appliedAgain = await applySetup({ ...data, confirm: second.plan_id, skipSkills: true });
  assert.equal(appliedAgain.readiness.status, "ready");
  const statusAfter = run("git", ["status", "--porcelain=v1", "--untracked-files=all"], data.repo);
  assert.equal(statusAfter, statusBefore);
});

test("overlapping operator edits become focused conflicts and are preserved", async () => {
  const data = fixture();
  const first = await buildSetupPlan(data);
  await applySetup({ ...data, confirm: first.plan_id, skipSkills: true });
  const readme = join(data.repo, ".shea", "README.md");
  writeFileSync(readme, `${readFileSync(readme, "utf8")}operator edit\n`);
  const setupManifest = join(data.repo, ".shea", "setup.json");
  const editedManifest = JSON.parse(readFileSync(setupManifest, "utf8"));
  editedManifest.operator_note = "preserve me";
  writeFileSync(setupManifest, `${JSON.stringify(editedManifest, null, 2)}\n`);
  const next = await buildSetupPlan(data);
  const conflict = next.conflicts.find((item) => item.path === ".shea/README.md");
  assert.equal(conflict.reason, "operator edit overlaps a managed file");
  assert(next.conflicts.some((item) =>
    item.path === ".shea/setup.json"
      && item.reason === "operator edit overlaps the setup manifest"
  ));
  await assert.rejects(
    applySetup({ ...data, confirm: next.plan_id, skipSkills: true }),
    /unresolved conflicts/,
  );
  assert(readFileSync(readme, "utf8").includes("operator edit"));
  assert(readFileSync(setupManifest, "utf8").includes("preserve me"));
});

test("failed blocking verification never records a successful readiness", async () => {
  const data = fixture();
  data.request.verification.blocking = ["false"];
  const plan = await buildSetupPlan(data);
  await assert.rejects(
    applySetup({ ...data, confirm: plan.plan_id, skipSkills: true }),
    /blocking baseline verification failed/,
  );
  const manifest = JSON.parse(readFileSync(join(data.repo, ".shea", "setup.json"), "utf8"));
  assert.equal(manifest.last_successful_readiness, null);
});

test("Project mutation is previewed separately and never hidden in local apply", async () => {
  const data = fixture();
  data.discovery.project.fields = [];
  const plan = await buildSetupPlan(data);
  assert.equal(plan.external_actions.length, 4);
  assert(plan.external_actions.some((action) => action.kind === "create_status_field"));
  const visible = publicPlan(plan);
  assert.equal(visible.project_plan_id, plan.project_plan_id);
  assert.equal(visible.source.source_revision, REVISION);
  assert.deepEqual(visible.harnesses, ["codex"]);
  assert.equal(visible.staging.path, ".shea/local/setup/fixture-run");
  assert.notEqual(plan.plan_id, plan.project_plan_id);
});

test("existing target contract without ownership markers is operator-owned", async () => {
  const data = fixture();
  mkdirSync(join(data.repo, ".shea", "workflows"), { recursive: true });
  writeFileSync(join(data.repo, ".shea", "workflows", "shea-symphony.md"), "operator workflow\n");
  const plan = await buildSetupPlan(data);
  assert(plan.conflicts.some((item) => item.path === ".shea/workflows/shea-symphony.md"));
  assert.equal(readFileSync(join(data.repo, ".shea", "workflows", "shea-symphony.md"), "utf8"), "operator workflow\n");
});

test("normal skill metadata includes setup and excludes research-only Dream and HALO", () => {
  const skills = normalSkills();
  assert(skills.includes("setup-shea"));
  assert(skills.includes("shea-symphony-manual-main"));
  assert(!skills.includes("shea-symphony-issue-forge-dream"));
  assert(!skills.includes("shea-halo-research-seed"));
  assert.equal(MANAGED_MARKER, "setup-shea:v1");
  const manifest = readFileSync(new URL("../../../manifest.toml", import.meta.url), "utf8");
  const defaults = manifest
    .split("[[skills]]")
    .slice(1)
    .filter((entry) => !entry.includes("default_install = false"))
    .map((entry) => entry.match(/name = "([^"]+)"/)?.[1])
    .filter(Boolean)
    .sort();
  assert.deepEqual([...skills].sort(), defaults);
});

test("setup source URL must be the exact pinned commit archive", async () => {
  const data = fixture();
  data.request.source.suite_url =
    "https://github.com/Alive24/shea-symphony/tree/main/skills/shea-symphony/suite";
  await assert.rejects(buildSetupPlan(data), /must use the exact pinned GitHub commit archive/);
  data.request.source.source_revision = REVISION.slice(0, 12);
  await assert.rejects(buildSetupPlan(data), /must be a full pinned git revision/);
});

test("standard Skills CLI receives the pinned archive, full-depth discovery, and bounded limits", async () => {
  const data = fixture();
  const bin = join(data.repo, "fake-bin");
  const invocation = join(data.repo, "npx-invocation.json");
  const fakeNpx = join(bin, "npx");
  mkdirSync(bin);
  writeFileSync(
    fakeNpx,
    `#!/usr/bin/env node
const { mkdirSync, writeFileSync } = require("node:fs");
const { join } = require("node:path");
const args = process.argv.slice(2);
writeFileSync(${JSON.stringify(invocation)}, JSON.stringify({
  args,
  download: process.env.SKILLS_DOWNLOAD_MAX_BYTES,
  extract: process.env.SKILLS_EXTRACT_MAX_BYTES,
  files: process.env.SKILLS_EXTRACT_MAX_FILES,
}));
for (let index = 0; index < args.length; index += 1) {
  if (args[index] !== "--skill") continue;
  const name = args[index + 1];
  const root = join(process.cwd(), ".agents", "skills", name);
  mkdirSync(root, { recursive: true });
  writeFileSync(join(root, "SKILL.md"), "---\\nname: " + name + "\\ndescription: fixture\\n---\\n");
}
`,
  );
  chmodSync(fakeNpx, 0o755);
  const originalPath = process.env.PATH;
  process.env.PATH = `${bin}:${originalPath}`;
  try {
    const plan = await buildSetupPlan(data);
    const applied = await applySetup({ ...data, confirm: plan.plan_id });
    assert.equal(applied.readiness.status, "ready");
  } finally {
    process.env.PATH = originalPath;
  }
  const observed = JSON.parse(readFileSync(invocation, "utf8"));
  assert.deepEqual(observed.args.slice(0, 4), [
    "skills",
    "add",
    data.request.source.suite_url,
    "--full-depth",
  ]);
  assert.equal(observed.download, String(128 * 1024 * 1024));
  assert.equal(observed.extract, String(256 * 1024 * 1024));
  assert.equal(observed.files, "10000");
});

test("release resolution rejects missing checksums, missing targets, unavailable metadata, and wrong digests", async () => {
  const data = fixture();
  const release = mkdtempSync(join(tmpdir(), "setup-shea-release-"));
  delete data.request.runtime.cli_path;
  data.request.runtime = {
    release_tag: "legacy-v0.1.0",
    release_dir: release,
    discovery_path: join(data.repo, "missing-runtime-discovery.json"),
    install_root: mkdtempSync(join(tmpdir(), "setup-shea-runtimes-")),
    target: data.identity.target,
  };
  const metadata = {
    schema_version: 1,
    release_tag: "legacy-v0.1.0",
    cli_version: data.identity.cli_version,
    source_revision: data.identity.source_revision,
    compatibility: data.identity.compatibility,
    artifacts: [{
      target: data.identity.target,
      platform: data.identity.platform,
      architecture: data.identity.architecture,
      archive: "shea-symphony-legacy-0.1.0-aarch64-apple-darwin.tar.gz",
      binary: "shea-symphony-legacy",
    }],
  };
  await assert.rejects(buildSetupPlan(data), /ENOENT|no such file/i);
  writeFileSync(join(release, "legacy-release.json"), `${JSON.stringify(metadata)}\n`);
  writeFileSync(join(release, "SHA256SUMS"), `${"0".repeat(64)}  ${metadata.artifacts[0].archive}\n`);
  await assert.rejects(buildSetupPlan(data), /checksum is missing/);
  metadata.artifacts[0].sha256 = "0".repeat(64);
  writeFileSync(join(release, "SHA256SUMS"), `${"1".repeat(64)}  ${metadata.artifacts[0].archive}\n`);
  writeFileSync(join(release, "legacy-release.json"), `${JSON.stringify(metadata)}\n`);
  await assert.rejects(buildSetupPlan(data), /does not authenticate/);
  writeFileSync(join(release, "SHA256SUMS"), `${metadata.artifacts[0].sha256}  ${metadata.artifacts[0].archive}\n`);
  metadata.artifacts[0].target = "x86_64-apple-darwin";
  writeFileSync(join(release, "legacy-release.json"), `${JSON.stringify(metadata)}\n`);
  await assert.rejects(buildSetupPlan(data), /no verified artifact/);
  metadata.artifacts[0].target = data.identity.target;
  writeFileSync(join(release, "legacy-release.json"), `${JSON.stringify(metadata)}\n`);
  writeFileSync(join(release, metadata.artifacts[0].archive), "not the expected archive");
  const plan = await buildSetupPlan(data);
  await assert.rejects(
    applySetup({ ...data, confirm: plan.plan_id, skipSkills: true }),
    /digest mismatch/,
  );
});

test("release checksum verification rejects malformed checksum manifests", async () => {
  const data = fixture();
  const release = mkdtempSync(join(tmpdir(), "setup-shea-release-"));
  const archive = "shea-symphony-legacy-0.1.0-aarch64-apple-darwin.tar.gz";
  delete data.request.runtime.cli_path;
  data.request.runtime = {
    release_tag: "legacy-v0.1.0",
    release_dir: release,
    discovery_path: join(data.repo, "missing-runtime-discovery.json"),
    install_root: mkdtempSync(join(tmpdir(), "setup-shea-runtimes-")),
    target: data.identity.target,
  };
  const metadata = {
    schema_version: 1,
    release_tag: "legacy-v0.1.0",
    cli_version: data.identity.cli_version,
    source_revision: data.identity.source_revision,
    compatibility: data.identity.compatibility,
    artifacts: [{
      target: data.identity.target,
      platform: data.identity.platform,
      architecture: data.identity.architecture,
      archive,
      binary: "shea-symphony-legacy",
      sha256: sha256("fixture"),
    }],
  };
  writeFileSync(join(release, "legacy-release.json"), `${JSON.stringify(metadata)}\n`);
  writeFileSync(join(release, "SHA256SUMS"), `not-a-checksum  ${archive}\n`);
  await assert.rejects(buildSetupPlan(data), /SHA256SUMS is malformed/);
});

test("verified release archive installs to a versioned user-local root and then resolves as a no-op", async () => {
  const data = fixture();
  const release = mkdtempSync(join(tmpdir(), "setup-shea-release-"));
  const payload = join(release, "payload");
  const archive = "shea-symphony-legacy-0.1.0-aarch64-apple-darwin.tar.gz";
  mkdirSync(payload);
  writeFileSync(join(payload, "shea-symphony-legacy"), readFileSync(data.cli));
  chmodSync(join(payload, "shea-symphony-legacy"), 0o755);
  run("tar", ["-czf", join(release, archive), "-C", payload, "shea-symphony-legacy"], data.repo);
  const digest = sha256(readFileSync(join(release, archive)));
  const metadata = {
    schema_version: 1,
    release_tag: "legacy-v0.1.0",
    cli_version: data.identity.cli_version,
    source_revision: data.identity.source_revision,
    compatibility: data.identity.compatibility,
    artifacts: [{
      target: data.identity.target,
      platform: data.identity.platform,
      architecture: data.identity.architecture,
      archive,
      binary: "shea-symphony-legacy",
      sha256: digest,
    }],
  };
  writeFileSync(join(release, "legacy-release.json"), `${JSON.stringify(metadata)}\n`);
  writeFileSync(join(release, "SHA256SUMS"), `${digest}  ${archive}\n`);
  delete data.request.runtime.cli_path;
  data.request.runtime = {
    release_tag: metadata.release_tag,
    release_dir: release,
    discovery_path: join(data.repo, "missing-runtime-discovery.json"),
    install_root: data.repo,
    target: data.identity.target,
  };

  await assert.rejects(
    buildSetupPlan(data),
    /runtime installation must remain outside the target repository/,
  );
  data.request.runtime.install_root = mkdtempSync(join(tmpdir(), "setup-shea-runtimes-"));
  const plan = await buildSetupPlan(data);
  assert.equal(plan.runtime.action, "install");
  assert.equal(plan.runtime.identity.binary_role, "legacy_cli");
  const applied = await applySetup({ ...data, confirm: plan.plan_id, skipSkills: true });
  assert.equal(applied.readiness.status, "ready");
  assert(existsSync(join(
    data.request.runtime.install_root,
    data.identity.cli_version,
    data.identity.target,
    "shea-symphony-legacy",
  )));
  const rerun = await buildSetupPlan(data);
  assert.equal(rerun.runtime.action, "no-op");
  assert(rerun.actions
    .filter((action) => action.kind === "runtime" || action.kind === "file")
    .every((action) => action.classification === "no-op"));
});

test("validated App discovery avoids a release download", async () => {
  const data = fixture();
  const discoveryRecord = join(data.repo, "runtime-discovery.json");
  writeFileSync(discoveryRecord, `${JSON.stringify({
    schema_version: 1,
    binary_role: "legacy_cli",
    app_version: "0.1.0",
    cli_version: data.identity.cli_version,
    source_revision: data.identity.source_revision,
    target: data.identity.target,
    platform: data.identity.platform,
    architecture: data.identity.architecture,
    compatibility: data.identity.compatibility,
    executable_path: data.cli,
    sha256: sha256(readFileSync(data.cli)),
  })}\n`);
  delete data.request.runtime.cli_path;
  data.request.runtime.discovery_path = discoveryRecord;
  const plan = await buildSetupPlan(data);
  assert.equal(plan.runtime.source, "app_discovery");
  assert.equal(plan.runtime.action, "no-op");
});

test("harness add and remove use standard Skills CLI classifications", async () => {
  const data = fixture();
  const initial = await buildSetupPlan(data);
  await applySetup({ ...data, confirm: initial.plan_id, skipSkills: true });
  for (const name of normalSkills()) {
    const root = join(data.repo, ".agents", "skills", name);
    mkdirSync(root, { recursive: true });
    writeFileSync(join(root, "SKILL.md"), `---\nname: ${name}\ndescription: fixture\n---\n`);
  }
  data.request.harnesses = ["codex", "claude-code"];
  data.discovery.harnesses["claude-code"] = true;
  const added = await buildSetupPlan(data);
  assert(added.actions.some((action) => action.kind === "skills" && action.harness === "claude-code" && action.classification === "create"));
  data.request.harnesses = ["claude-code"];
  const removed = await buildSetupPlan(data);
  assert(removed.actions.some((action) =>
    action.kind === "skills"
      && action.harness === "codex"
      && action.classification === "remove"
      && action.installation_method === "remove_named_shea_set_then_reinstall_selected_harnesses"
  ));
  assert(removed.actions.some((action) =>
    action.kind === "skills"
      && action.harness === "claude-code"
      && action.classification !== "no-op"
  ));
});

test("an interrupted current marked run is safely restarted without touching siblings", async () => {
  const data = fixture();
  const runDir = join(data.repo, ".shea", "local", "setup", data.request.run_id);
  const sibling = join(data.repo, ".shea", "local", "other-producer");
  mkdirSync(runDir, { recursive: true });
  mkdirSync(sibling, { recursive: true });
  writeFileSync(join(runDir, ".shea-setup-marker"), `${MANAGED_MARKER}\n`);
  writeFileSync(join(runDir, "partial"), "interrupted\n");
  writeFileSync(join(sibling, "keep"), "unrelated\n");
  const plan = await buildSetupPlan(data);
  await applySetup({ ...data, confirm: plan.plan_id, skipSkills: true });
  assert(!existsSync(runDir));
  assert(existsSync(join(sibling, "keep")));
});
