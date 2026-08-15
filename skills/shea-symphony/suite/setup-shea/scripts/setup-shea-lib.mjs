import { createHash } from "node:crypto";
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

export const SETUP_SCHEMA_VERSION = 1;
export const MANAGED_MARKER = "setup-shea:v1";
export const REQUIRED_PROJECT_FIELDS = ["Main Agent", "Review Agent", "Merging Agent"];
export const REQUIRED_STATUSES = [
  "Backlog",
  "Todo",
  "Need to Clarify",
  "In Progress",
  "Need Human Input",
  "Agent Review",
  "Human Review",
  "Rework",
  "Merging",
  "Done",
];
export const SUPPORTED_HARNESSES = ["codex", "claude-code", "antigravity"];
export const SUPPORTED_MAIN_BACKENDS = ["codex", "claude-code"];
export const SUPPORTED_REVIEW_BACKENDS = ["codex-app-server", "claude-code", "agy-cli"];

const SUITE_ARCHIVE_LIMITS = {
  SKILLS_DOWNLOAD_MAX_BYTES: String(128 * 1024 * 1024),
  SKILLS_EXTRACT_MAX_BYTES: String(256 * 1024 * 1024),
  SKILLS_EXTRACT_MAX_FILES: "10000",
};

const skillAssetPath = fileURLToPath(
  new URL("../assets/normal-skills.json", import.meta.url),
);

export function normalSkills() {
  const asset = readJson(skillAssetPath);
  if (asset.schema_version !== 1 || !Array.isArray(asset.skills) || asset.skills.length === 0) {
    throw new Error(`invalid normal skill-set metadata: ${skillAssetPath}`);
  }
  return asset.skills;
}

export function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

export function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

export function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function writeJson(path, value) {
  atomicWrite(path, `${JSON.stringify(value, null, 2)}\n`);
}

function atomicWrite(path, content) {
  mkdirSync(dirname(path), { recursive: true });
  const temporary = `${path}.setup-shea-${process.pid}`;
  writeFileSync(temporary, content);
  renameSync(temporary, path);
}

function command(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd,
    encoding: "utf8",
    env: options.env,
    stdio: options.inherit ? "inherit" : "pipe",
  });
  if (result.error) throw result.error;
  if (result.status !== 0 && !options.allowFailure) {
    const stderr = String(result.stderr || "").trim();
    throw new Error(`${command} ${args.join(" ")} failed (${result.status}): ${stderr}`);
  }
  return result;
}

function stdout(commandName, args, options = {}) {
  return String(command(commandName, args, options).stdout || "").trim();
}

function commandExists(name) {
  return command("sh", ["-c", `command -v ${shellWord(name)}`], { allowFailure: true }).status === 0;
}

function shellWord(value) {
  if (!/^[A-Za-z0-9._/+:-]+$/.test(value)) throw new Error(`unsafe command name: ${value}`);
  return value;
}

function ensureRepo(path) {
  const root = stdout("git", ["-C", resolve(path), "rev-parse", "--show-toplevel"]);
  if (!root) throw new Error(`not a git repository: ${path}`);
  return resolve(root);
}

function git(repo, args, options = {}) {
  return stdout("git", ["-C", repo, ...args], options);
}

function parseRepositoryId(remote) {
  const match = remote.match(/(?:github\.com[:/])([^/]+)\/([^/]+?)(?:\.git)?$/);
  return match ? `${match[1]}/${match[2]}` : null;
}

function defaultTarget() {
  const key = `${process.platform}/${process.arch}`;
  const targets = {
    "darwin/arm64": "aarch64-apple-darwin",
    "darwin/x64": "x86_64-apple-darwin",
    "linux/x64": "x86_64-unknown-linux-gnu",
  };
  const target = targets[key];
  if (!target) throw new Error(`unsupported first-slice platform/architecture: ${key}`);
  return target;
}

function targetPlatform(target) {
  if (target.endsWith("apple-darwin")) return "macos";
  if (target.endsWith("unknown-linux-gnu")) return "linux";
  throw new Error(`unsupported first-slice target: ${target}`);
}

function targetArchitecture(target) {
  if (target.startsWith("aarch64-")) return "aarch64";
  if (target.startsWith("x86_64-")) return "x86_64";
  throw new Error(`unsupported target architecture: ${target}`);
}

export function validateRuntimeIdentity(identity, expected = {}) {
  const failures = [];
  if (!identity || typeof identity !== "object") failures.push("malformed identity JSON");
  else {
    if (identity.schema_version !== 1) failures.push("schema_version must be 1");
    if (identity.binary_role !== "legacy_cli") failures.push("binary_role must be legacy_cli");
    if (identity.compatibility !== "shea-legacy-cli-v1") {
      failures.push("compatibility must be shea-legacy-cli-v1");
    }
    for (const [field, value] of Object.entries(expected)) {
      if (value != null && identity[field] !== value) {
        failures.push(`${field} expected ${value}, found ${identity[field] ?? "missing"}`);
      }
    }
    for (const field of [
      "cli_version",
      "source_revision",
      "target",
      "platform",
      "architecture",
    ]) {
      if (typeof identity[field] !== "string" || identity[field].trim() === "") {
        failures.push(`${field} is missing`);
      }
    }
  }
  if (failures.length) throw new Error(`Legacy runtime identity rejected: ${failures.join("; ")}`);
  return identity;
}

export function inspectRuntime(path, expected = {}) {
  if (!existsSync(path)) throw new Error(`runtime is unavailable: ${path}`);
  const result = command(path, ["--runtime-info"], { allowFailure: true });
  if (result.status !== 0) throw new Error(`runtime-info failed for ${path}`);
  let identity;
  try {
    identity = JSON.parse(String(result.stdout).trim());
  } catch {
    throw new Error(`Legacy runtime identity rejected: malformed identity JSON`);
  }
  return validateRuntimeIdentity(identity, expected);
}

async function readResource(location) {
  if (/^https:\/\//.test(location)) {
    const response = await fetch(location, { redirect: "error" });
    if (!response.ok) throw new Error(`download failed ${response.status}: ${location}`);
    return Buffer.from(await response.arrayBuffer());
  }
  return readFileSync(location);
}

function releaseLocation(runtime, name) {
  if (runtime.release_dir) return join(resolve(runtime.release_dir), name);
  const base = String(runtime.release_base_url || "https://github.com/Alive24/shea-symphony/releases/download").replace(/\/$/, "");
  return `${base}/${encodeURIComponent(runtime.release_tag)}/${name}`;
}

export async function loadRelease(runtime) {
  if (!runtime.release_tag) throw new Error("runtime.release_tag is required without a compatible existing runtime");
  const manifestName = "legacy-release.json";
  const [manifestBytes, checksumBytes] = await Promise.all([
    readResource(releaseLocation(runtime, manifestName)),
    readResource(releaseLocation(runtime, "SHA256SUMS")),
  ]);
  const metadata = JSON.parse(manifestBytes.toString("utf8"));
  if (metadata.schema_version !== 1 || metadata.release_tag !== runtime.release_tag) {
    throw new Error("release metadata schema/tag mismatch");
  }
  if (metadata.compatibility !== "shea-legacy-cli-v1") {
    throw new Error("release metadata compatibility mismatch");
  }
  if (typeof metadata.cli_version !== "string" || metadata.cli_version.trim() === "") {
    throw new Error("release metadata cli_version is missing");
  }
  if (!/^[a-f0-9]{7,64}$/.test(metadata.source_revision || "")) {
    throw new Error("release metadata source_revision is missing or malformed");
  }
  if (!Array.isArray(metadata.artifacts) || metadata.artifacts.length === 0) {
    throw new Error("release metadata has no artifacts");
  }
  const checksums = new Map();
  for (const line of checksumBytes.toString("utf8").split(/\r?\n/).filter(Boolean)) {
    const match = line.match(/^([a-f0-9]{64})  ([^/\\]+)$/);
    if (!match || checksums.has(match[2])) {
      throw new Error("SHA256SUMS is malformed or contains duplicate entries");
    }
    checksums.set(match[2], match[1]);
  }
  for (const artifact of metadata.artifacts) {
    if (!artifact.archive || basename(artifact.archive) !== artifact.archive) {
      throw new Error("release metadata contains an unsafe archive name");
    }
    if (!/^[a-f0-9]{64}$/.test(artifact.sha256 || "")) {
      throw new Error("release artifact checksum is missing or malformed");
    }
    if (checksums.get(artifact.archive) !== artifact.sha256) {
      throw new Error(`SHA256SUMS does not authenticate ${artifact.archive}`);
    }
  }
  return metadata;
}

function validateReleaseArtifact(metadata, target) {
  const artifact = metadata.artifacts.find((candidate) => candidate.target === target);
  if (!artifact) throw new Error(`release ${metadata.release_tag} has no verified artifact for ${target}`);
  if (artifact.platform !== targetPlatform(target)) throw new Error("release artifact platform mismatch");
  if (artifact.architecture !== targetArchitecture(target)) throw new Error("release artifact architecture mismatch");
  if (!/^[a-f0-9]{64}$/.test(artifact.sha256 || "")) throw new Error("release artifact checksum is missing or malformed");
  for (const field of ["archive", "binary"]) {
    if (!artifact[field] || basename(artifact[field]) !== artifact[field]) {
      throw new Error(`unsafe release artifact ${field}`);
    }
  }
  return artifact;
}

function discoveredRuntimeCandidates(request) {
  const candidates = [];
  if (request.runtime.cli_path) candidates.push({ source: "explicit", path: resolve(request.runtime.cli_path) });
  const discoveryPath = resolve(
    request.runtime.discovery_path || join(homedir(), ".shea-symphony", "runtime-discovery.json"),
  );
  if (existsSync(discoveryPath)) {
    try {
      const record = readJson(discoveryPath);
      if (record.cli_path || record.executable_path) {
        candidates.push({
          source: "app_discovery",
          path: resolve(record.cli_path || record.executable_path),
          discovery: record,
        });
      }
    } catch {
      // A stale discovery record is ignored; every candidate is verified below.
    }
  }
  return candidates;
}

function validateAppDiscovery(candidate) {
  const record = candidate.discovery;
  if (!record || record.schema_version !== 1) throw new Error("App runtime discovery schema mismatch");
  if (record.binary_role !== "legacy_cli" || record.compatibility !== "shea-legacy-cli-v1") {
    throw new Error("App runtime discovery role/compatibility mismatch");
  }
  if (!isAbsolute(record.cli_path || record.executable_path || "")) {
    throw new Error("App runtime discovery executable path must be absolute");
  }
  if (!/^[a-f0-9]{64}$/.test(record.sha256 || "")) {
    throw new Error("App runtime discovery checksum is missing or malformed");
  }
  if (sha256(readFileSync(candidate.path)) !== record.sha256) {
    throw new Error("App runtime discovery digest mismatch");
  }
  return {
    cli_version: record.cli_version,
    source_revision: record.source_revision,
    target: record.target,
    platform: record.platform,
    architecture: record.architecture,
    compatibility: record.compatibility,
  };
}

async function resolveRuntimePlan(request) {
  for (const candidate of discoveredRuntimeCandidates(request)) {
    try {
      const discoveryExpected = candidate.source === "app_discovery"
        ? validateAppDiscovery(candidate)
        : {};
      const identity = inspectRuntime(candidate.path, {
        ...discoveryExpected,
        target: request.runtime.target || undefined,
        architecture: request.runtime.architecture || undefined,
      });
      return { action: "no-op", source: candidate.source, path: candidate.path, identity };
    } catch (error) {
      if (candidate.source === "explicit") throw error;
    }
  }

  const target = request.runtime.target || defaultTarget();
  const metadata = await loadRelease(request.runtime);
  const artifact = validateReleaseArtifact(metadata, target);
  const installRoot = resolve(
    request.runtime.install_root || join(homedir(), ".local", "share", "shea-symphony", "runtimes"),
  );
  const path = join(installRoot, metadata.cli_version, target, artifact.binary);
  const expected = {
    cli_version: metadata.cli_version,
    source_revision: metadata.source_revision,
    target,
    platform: artifact.platform === "macos" ? "macos" : artifact.platform,
    architecture: artifact.architecture,
  };
  if (existsSync(path)) {
    const identity = inspectRuntime(path, expected);
    return { action: "no-op", source: "installed_release", path, identity, metadata, artifact };
  }
  return {
    action: "install",
    source: "github_release",
    path,
    identity: {
      schema_version: 1,
      binary_role: "legacy_cli",
      compatibility: "shea-legacy-cli-v1",
      ...expected,
    },
    metadata,
    artifact,
    url: releaseLocation(request.runtime, artifact.archive),
  };
}

function normalizeRequest(input) {
  const request = structuredClone(input);
  if (request.schema_version !== SETUP_SCHEMA_VERSION) throw new Error("request.schema_version must be 1");
  request.run_id ||= `setup-${Date.now()}`;
  request.harnesses ||= [];
  request.backends ||= { main: "codex", review: "codex-app-server", merge: "codex" };
  request.verification ||= { blocking: [], advisory: [] };
  request.source ||= {};
  request.runtime ||= {};
  request.tracker ||= {};
  if (!/^[A-Za-z0-9._-]{1,80}$/.test(request.run_id)) throw new Error("request.run_id is unsafe");
  if (!request.source.suite_version || !request.source.source_revision) {
    throw new Error("request.source requires suite_version and source_revision");
  }
  if (!/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(request.source.source_revision)) {
    throw new Error("request.source.source_revision must be a full pinned git revision");
  }
  const suiteArchiveUrl = `https://github.com/Alive24/shea-symphony/archive/${request.source.source_revision}.tar.gz`;
  if (request.source.suite_url && request.source.suite_url !== suiteArchiveUrl) {
    throw new Error("request.source.suite_url must use the exact pinned GitHub commit archive");
  }
  request.source.suite_url ||= suiteArchiveUrl;
  request.harnesses = [...new Set(request.harnesses)].sort();
  for (const harness of request.harnesses) {
    if (!SUPPORTED_HARNESSES.includes(harness)) throw new Error(`unsupported harness: ${harness}`);
  }
  if (request.harnesses.length === 0) throw new Error("select at least one supported interactive skill harness");
  if (!SUPPORTED_MAIN_BACKENDS.includes(request.backends.main)) throw new Error(`unsupported Main backend: ${request.backends.main}`);
  if (!SUPPORTED_MAIN_BACKENDS.includes(request.backends.merge)) throw new Error(`unsupported Merge backend: ${request.backends.merge}`);
  if (!SUPPORTED_REVIEW_BACKENDS.includes(request.backends.review)) throw new Error(`unsupported Review backend: ${request.backends.review}`);
  if (request.backends.main === "antigravity" || request.backends.merge === "antigravity") {
    throw new Error("Antigravity skill discovery is not an unattended lane backend");
  }
  if (!request.runtime_profile || request.runtime_profile.schema_version !== 1) {
    throw new Error("request.runtime_profile is required for no-claim readiness");
  }
  if (typeof request.runtime_profile.profile_id !== "string" || request.runtime_profile.profile_id.trim() === "") {
    throw new Error("request.runtime_profile.profile_id is required");
  }
  if (
    request.runtime_profile.environment == null
    || typeof request.runtime_profile.environment !== "object"
    || Array.isArray(request.runtime_profile.environment)
  ) {
    throw new Error("request.runtime_profile.environment must be an object");
  }
  for (const bucket of ["blocking", "advisory"]) {
    if (!Array.isArray(request.verification[bucket])) throw new Error(`verification.${bucket} must be an array`);
  }
  return request;
}

export function discoverSetup(repoPath, request, fixture = null) {
  const repo = ensureRepo(repoPath);
  if (fixture) return { ...fixture, repo };
  const remote = git(repo, ["remote", "get-url", "origin"], { allowFailure: true });
  const repositoryId = parseRepositoryId(remote) || request.tracker.repository;
  const baseBranch = request.tracker.base_branch || git(repo, ["branch", "--show-current"], { allowFailure: true }) || "main";
  const harnesses = {
    codex: commandExists("codex"),
    "claude-code": commandExists("claude"),
    antigravity: commandExists("agy") || commandExists("antigravity"),
  };
  let project = { available: false, fields: [], error: "GitHub Project discovery not configured" };
  if (request.tracker.project_number && request.tracker.project_owner && commandExists("gh")) {
    const result = command(
      "gh",
      [
        "project",
        "field-list",
        String(request.tracker.project_number),
        "--owner",
        request.tracker.project_owner,
        "--format",
        "json",
        "--limit",
        "100",
      ],
      { cwd: repo, allowFailure: true },
    );
    if (result.status === 0) {
      const parsed = JSON.parse(String(result.stdout));
      project = { available: true, fields: parsed.fields || parsed, error: null };
    } else {
      project = { available: false, fields: [], error: String(result.stderr).trim() || "field discovery failed" };
    }
  }
  return {
    repo,
    repository_id: repositoryId,
    base_branch: baseBranch,
    github_authenticated:
      commandExists("gh")
      && command("gh", ["auth", "status"], { cwd: repo, allowFailure: true }).status === 0,
    issues_available: Boolean(repositoryId),
    harnesses,
    project,
  };
}

function projectPlan(request, discovery) {
  const actions = [];
  const fields = discovery.project?.fields || [];
  const byName = new Map(fields.map((field) => [field.name, field]));
  for (const name of REQUIRED_PROJECT_FIELDS) {
    if (!byName.has(name)) {
      actions.push({
        kind: "create_text_field",
        name,
        command: [
          "gh",
          "project",
          "field-create",
          String(request.tracker.project_number),
          "--owner",
          request.tracker.project_owner,
          "--name",
          name,
          "--data-type",
          "TEXT",
        ],
      });
    }
  }
  const status = byName.get("Status");
  if (!status) {
    actions.push({
      kind: "create_status_field",
      name: "Status",
      command: [
        "gh",
        "project",
        "field-create",
        String(request.tracker.project_number),
        "--owner",
        request.tracker.project_owner,
        "--name",
        "Status",
        "--data-type",
        "SINGLE_SELECT",
        "--single-select-options",
        REQUIRED_STATUSES.join(","),
      ],
    });
  } else {
    const options = new Set((status.options || []).map((option) => option.name || option));
    const missing = REQUIRED_STATUSES.filter((name) => !options.has(name));
    if (missing.length) {
      actions.push({
        kind: "manual_status_option_repair",
        name: "Status",
        missing,
        command: null,
        reason: "GitHub CLI cannot safely append options to an existing Project single-select field",
      });
    }
  }
  return actions;
}

function yaml(value) {
  return JSON.stringify(String(value));
}

function renderedWorkflow(request, discovery) {
  const repoId = discovery.repository_id || request.tracker.repository;
  if (!repoId || !repoId.includes("/")) throw new Error("target repository identity is unresolved");
  const [owner, repo] = repoId.split("/", 2);
  const main = request.backends.main;
  const review = request.backends.review;
  const merge = request.backends.merge;
  const commands = request.verification.blocking.map((entry) =>
    typeof entry === "string" ? entry : entry.command,
  );
  return `---
tracker:
  kind: github_project_v2
  owner: ${yaml(owner)}
  repo: ${yaml(repo)}
  project_owner: ${yaml(request.tracker.project_owner || owner)}
  project_owner_type: ${yaml(request.tracker.project_owner_type || "user")}
  project_number: ${Number(request.tracker.project_number)}
  status_field: Status
  state_map:
    backlog: Backlog
    todo: Todo
    need_to_clarify: Need to Clarify
    in_progress: In Progress
    need_human_input: Need Human Input
    agent_review: Agent Review
    human_review: Human Review
    rework: Rework
    merging: Merging
    done: Done
  active_states: [Todo, Rework]
  terminal_states: [Done, Closed, Cancelled, Canceled, Duplicate]
  workpad:
    source: issue_comment
    marker: "<!-- shea-symphony-workpad -->"
git:
  base_branch: ${yaml(discovery.base_branch || request.tracker.base_branch || "main")}
prompts:
  main_agent: ../prompts/main-agent.md
  review_agent: ../prompts/review-agent.md
  merge_agent: ../prompts/merge-agent.md
workpad_templates:
  agent_review_run: ../template/workpad/agent-review.md
  merge_run: ../template/workpad/merge-run.md
  rework_run: ../template/workpad/rework-run.md
artifacts:
  root: ../artifacts
workspace:
  root: ../worktrees
main_lane:
  backend: ${yaml(main)}
  max_concurrent_agents: 1
codex:
  command: ${yaml(request.backend_commands?.codex || "codex app-server")}
  approval_policy: never
claude:
  command: ${yaml(request.backend_commands?.claude || "claude")}
review_lane:
  backend: ${yaml(review)}
  max_concurrent_workers: 1
  codex_approval_policy: never
  codex_thread_sandbox: read-only
  agy_command: ${yaml(request.backend_commands?.agy || "agy")}
merge_lane:
  agent_backend: ${yaml(merge)}
  max_concurrent_workers: 1
verification:
  commands:${commands.length ? `\n${commands.map((value) => `    - ${yaml(value)}`).join("\n")}` : " []"}
runtime_profile:
  path: ../runtime-profile.json
  required: true
  timeout_ms: 10000
---

<!-- managed by ${MANAGED_MARKER} -->
# Shea Symphony Workflow

This repository-owned contract contains target identity, lane backends, and blocking verification. Agent behavior remains in the installed Shea skills; machine-local runtime paths remain outside this file.
`;
}

function prompt(title, skill, boundary) {
  return `<!-- managed by ${MANAGED_MARKER} -->
# ${title}

Use the \`${skill}\` skill for {{ issue.identifier }} {{ issue.title }}. Resolve the active repository profile and workflow before acting. ${boundary}
`;
}

function workpad(title, sections) {
  return `<!-- managed by ${MANAGED_MARKER} -->
## ${title}

${sections.map((section) => `### ${section}\n\n- <record owned evidence>`).join("\n\n")}
`;
}

function setupReadme(request, discovery) {
  return `<!-- managed by ${MANAGED_MARKER} -->
# Shea Symphony Setup

- Repository: \`${discovery.repository_id}\`
- Project: \`${request.tracker.project_owner}/${request.tracker.project_number}\`
- Selected interactive harnesses: ${request.harnesses.map((value) => `\`${value}\``).join(", ")}
- Lane backends: Main \`${request.backends.main}\`, Review \`${request.backends.review}\`, Merge \`${request.backends.merge}\`
- Suite: \`${request.source.suite_version}\` at \`${request.source.source_revision}\`

Run the \`setup-shea\` skill again for install, update, harness/backend changes, or normal reconciliation. An unchanged rerun must be a no-op. Use Shea Symphony Doctor for unusual post-setup drift, not to finish a normal first run.

Machine-local files such as \`.shea/app-profile.local.json\`, \`.shea/runtime-profile.json\`, setup staging, logs, worktrees, sessions, and runtime binaries are ignored. Do not commit credentials or absolute executable paths.
`;
}

function managedGitignore(existing) {
  const start = "# setup-shea managed begin";
  const end = "# setup-shea managed end";
  const block = `${start}\n.shea/*.local.json\n.shea/runtime-profile.json\n.shea/local/\n.shea/artifacts/\n.shea/logs/\n.shea/worktrees/\n.shea/sessions/\n${end}`;
  const startIndex = existing.indexOf(start);
  const endIndex = existing.indexOf(end);
  if ((startIndex >= 0) !== (endIndex >= 0) || (startIndex >= 0 && endIndex < startIndex)) {
    throw new Error("conflicting setup-shea .gitignore managed markers");
  }
  if (startIndex < 0) return `${existing.replace(/\s*$/, "")}\n${block}\n`.replace(/^\n/, "");
  const after = endIndex + end.length;
  return `${existing.slice(0, startIndex)}${block}${existing.slice(after)}`;
}

function withSetupManifestIntegrity(manifest) {
  const value = structuredClone(manifest);
  delete value.setup_integrity_sha256;
  value.setup_integrity_sha256 = sha256(stableStringify(value));
  return value;
}

function setupManifestIntegrityIsValid(text) {
  try {
    const value = JSON.parse(text);
    if (!value.setup_integrity_sha256) return true;
    const recorded = value.setup_integrity_sha256;
    delete value.setup_integrity_sha256;
    return recorded === sha256(stableStringify(value));
  } catch {
    return false;
  }
}

function desiredRepositoryFiles(request, discovery, runtimePlan, previous) {
  const files = {
    ".shea/workflows/shea-symphony.md": renderedWorkflow(request, discovery),
    ".shea/prompts/main-agent.md": prompt(
      "Main Agent",
      "shea-symphony-manual-main",
      "Implement only accepted scope and stop at Agent Review.",
    ),
    ".shea/prompts/review-agent.md": prompt(
      "Review Agent",
      "shea-symphony-manual-review",
      "Review independently; never merge or claim Main authority.",
    ),
    ".shea/prompts/merge-agent.md": prompt(
      "Merging Agent",
      "shea-symphony-manual-merge",
      "Merge only approved work and keep merge-lane repair out of Main.",
    ),
    ".shea/prompts/human-review-handoff.md": prompt(
      "Human Review Handoff",
      "shea-symphony-human-review",
      "Refresh evidence, brief the operator, and wait for an explicit decision.",
    ),
    ".shea/prompts/need-human-input-handoff.md": `<!-- managed by ${MANAGED_MARKER} -->\n# Need Human Input\n\nExplain the missing credential, external authority, sample, or product decision without attempting a lane claim.\n`,
    ".shea/prompts/need-to-clarify-handoff.md": `<!-- managed by ${MANAGED_MARKER} -->\n# Need to Clarify\n\nExplain the incomplete issue contract and route it back to Issue Forge without claiming implementation.\n`,
    ".shea/template/workpad/agent-review.md": workpad("Shea Symphony Agent Review", ["Scope Reviewed", "Findings", "Verification", "Decision", "Handoff"]),
    ".shea/template/workpad/human-review.md": workpad("Shea Symphony Human Review Decision", ["Problem", "Delivered Change", "Resulting Effect", "Evidence", "Human Decision Needed"]),
    ".shea/template/workpad/merge-run.md": workpad("Shea Symphony Merge Run", ["Plan", "Freshness", "Merge Evidence", "Verification", "Handoff"]),
    ".shea/template/workpad/rework-run.md": workpad("Shea Symphony Rework Run", ["Requested Revision", "Plan", "Changed Files", "Verification", "Handoff"]),
    ".shea/README.md": setupReadme(request, discovery),
    ".shea/app-profile.local.json": `${JSON.stringify({ workflow_path: ".shea/workflows/shea-symphony.md", cli_path: runtimePlan.path }, null, 2)}\n`,
    ".shea/runtime-profile.json": `${JSON.stringify(request.runtime_profile, null, 2)}\n`,
  };
  const managedHashes = Object.fromEntries(
    Object.entries(files)
      .filter(([path]) => !path.endsWith(".local.json") && path !== ".shea/runtime-profile.json")
      .map(([path, content]) => [path, sha256(content)]),
  );
  const manifest = withSetupManifestIntegrity({
    schema_version: 1,
    managed_by: MANAGED_MARKER,
    source: {
      suite_version: request.source.suite_version,
      source_revision: request.source.source_revision,
      suite_url: request.source.suite_url,
    },
    harnesses: request.harnesses,
    backends: request.backends,
    runtime: {
      release_tag: request.runtime.release_tag || null,
      cli_version: runtimePlan.identity.cli_version,
      source_revision: runtimePlan.identity.source_revision,
      target: runtimePlan.identity.target,
      architecture: runtimePlan.identity.architecture,
      compatibility: runtimePlan.identity.compatibility,
    },
    verification: request.verification,
    managed_files: managedHashes,
    last_successful_readiness: previous?.last_successful_readiness || null,
  });
  files[".shea/setup.json"] = `${JSON.stringify(manifest, null, 2)}\n`;
  return files;
}

function safeRelativePath(path) {
  if (isAbsolute(path) || path.split(/[\\/]/).some((part) => part === "..")) {
    throw new Error(`unsafe managed path: ${path}`);
  }
  return path;
}

function isWithin(parent, child) {
  const canonical = (path) => {
    let existing = resolve(path);
    const suffix = [];
    while (!existsSync(existing)) {
      suffix.unshift(basename(existing));
      const next = dirname(existing);
      if (next === existing) break;
      existing = next;
    }
    return join(realpathSync(existing), ...suffix);
  };
  const candidate = relative(canonical(parent), canonical(child));
  return candidate === "" || (!candidate.startsWith("..") && !isAbsolute(candidate));
}

function existingManifest(repo) {
  const path = join(repo, ".shea", "setup.json");
  if (!existsSync(path)) return null;
  try {
    const value = readJson(path);
    return value.managed_by === MANAGED_MARKER ? value : null;
  } catch {
    return null;
  }
}

function fileActions(repo, desired, previous) {
  const actions = [];
  const previousHashes = previous?.managed_files || {};
  for (const [path, content] of Object.entries(desired)) {
    safeRelativePath(path);
    const absolute = join(repo, path);
    if (!existsSync(absolute)) {
      actions.push({ kind: "file", classification: "create", path, sha256: sha256(content) });
      continue;
    }
    const current = readFileSync(absolute, "utf8");
    if (current === content) {
      actions.push({ kind: "file", classification: "no-op", path, sha256: sha256(content) });
      continue;
    }
    const prior = previousHashes[path];
    const marked = current.includes(`managed by ${MANAGED_MARKER}`) || current.includes(`"managed_by": "${MANAGED_MARKER}"`);
    if (path === ".shea/setup.json" && marked && !setupManifestIntegrityIsValid(current)) {
      actions.push({ kind: "file", classification: "conflict", path, reason: "operator edit overlaps the setup manifest" });
    } else if (prior && sha256(current) !== prior) {
      actions.push({ kind: "file", classification: "conflict", path, reason: "operator edit overlaps a managed file" });
    } else if (!prior && !marked && !path.endsWith(".local.json") && path !== ".shea/runtime-profile.json") {
      actions.push({ kind: "file", classification: "conflict", path, reason: "existing operator-owned file has no setup-shea ownership marker" });
    } else {
      actions.push({ kind: "file", classification: "update", path, sha256: sha256(content) });
    }
  }
  for (const [path, hash] of Object.entries(previousHashes)) {
    if (Object.hasOwn(desired, path)) continue;
    const absolute = join(repo, safeRelativePath(path));
    if (!existsSync(absolute)) continue;
    const currentHash = sha256(readFileSync(absolute));
    actions.push({
      kind: "file",
      classification: currentHash === hash ? "remove" : "conflict",
      path,
      reason: currentHash === hash ? "no longer managed" : "retired managed file has operator edits",
    });
  }
  return actions;
}

function standardSkillRoot(repo, harness) {
  if (harness === "claude-code") return join(repo, ".claude", "skills");
  return join(repo, ".agents", "skills");
}

function skillActions(repo, request, previous) {
  const skills = normalSkills();
  const actions = [];
  const removedHarnesses = (previous?.harnesses || []).filter(
    (harness) => !request.harnesses.includes(harness),
  );
  for (const harness of request.harnesses) {
    const root = standardSkillRoot(repo, harness);
    const complete = skills.every((name) => existsSync(join(root, name, "SKILL.md")));
    const unchanged = previous?.harnesses?.includes(harness) && previous?.source?.source_revision === request.source.source_revision;
    actions.push({
      kind: "skills",
      harness,
      classification: complete && unchanged && removedHarnesses.length === 0 ? "no-op" : complete ? "update" : "create",
      root,
      ownership: "standard_skills_cli",
      installation_method: "pinned_commit_archive_via_standard_skills_cli",
      archive_limits: SUITE_ARCHIVE_LIMITS,
      skills,
    });
  }
  for (const harness of removedHarnesses) {
    actions.push({
      kind: "skills",
      harness,
      classification: "remove",
      root: standardSkillRoot(repo, harness),
      ownership: "standard_skills_cli",
      installation_method: "remove_named_shea_set_then_reinstall_selected_harnesses",
      skills,
    });
  }
  return actions;
}

export async function buildSetupPlan({ repo: repoPath, request: input, discovery: fixture = null }) {
  const request = normalizeRequest(input);
  const discovery = discoverSetup(repoPath, request, fixture);
  const repo = discovery.repo;
  const runtime = await resolveRuntimePlan(request);
  if (runtime.action === "install" && isWithin(repo, runtime.path)) {
    throw new Error("release runtime installation must remain outside the target repository");
  }
  const previous = existingManifest(repo);
  const desired = desiredRepositoryFiles(request, discovery, runtime, previous);
  const gitignorePath = join(repo, ".gitignore");
  const gitignoreCurrent = existsSync(gitignorePath) ? readFileSync(gitignorePath, "utf8") : "";
  const gitignoreDesired = managedGitignore(gitignoreCurrent);
  const actions = [
    { kind: "runtime", classification: runtime.action, path: runtime.path, source: runtime.source, url: runtime.url || null },
    { kind: "file", classification: gitignoreCurrent === gitignoreDesired ? "no-op" : existsSync(gitignorePath) ? "update" : "create", path: ".gitignore", sha256: sha256(gitignoreDesired) },
    ...fileActions(repo, desired, previous),
    ...skillActions(repo, request, previous),
  ];
  for (const harness of request.harnesses) {
    if (!discovery.harnesses?.[harness]) actions.push({ kind: "harness", classification: "conflict", harness, reason: "selected harness is not available on this machine" });
  }
  const external_actions = projectPlan(request, discovery);
  const planCore = {
    schema_version: 1,
    run_id: request.run_id,
    repository: discovery.repository_id,
    base_branch: discovery.base_branch,
    source: request.source,
    harnesses: request.harnesses,
    backends: request.backends,
    verification: request.verification,
    staging: {
      path: `.shea/local/setup/${request.run_id}`,
      ignored: true,
      marker: MANAGED_MARKER,
    },
    runtime: { ...runtime, metadata: undefined },
    actions,
    external_actions,
    conflicts: actions.filter((action) => action.classification === "conflict"),
  };
  const plan_id = sha256(stableStringify(planCore));
  const project_plan_id = sha256(stableStringify(external_actions));
  return { ...planCore, plan_id, project_plan_id, request, discovery, desired, gitignoreDesired, runtimeInternal: runtime };
}

async function installRuntime(runtime, request) {
  if (runtime.action !== "install") return inspectRuntime(runtime.path, runtime.identity);
  const stage = mkdtempSync(join(tmpdir(), "setup-shea-runtime-"));
  try {
    const archive = await readResource(releaseLocation(request.runtime, runtime.artifact.archive));
    if (archive.length > 128 * 1024 * 1024) throw new Error("release archive exceeds the 128 MiB setup limit");
    if (sha256(archive) !== runtime.artifact.sha256) throw new Error("release archive digest mismatch");
    const archivePath = join(stage, runtime.artifact.archive);
    writeFileSync(archivePath, archive);
    const listing = stdout("tar", ["-tzf", archivePath]).split(/\r?\n/).filter(Boolean);
    if (listing.length !== 1 || listing[0] !== runtime.artifact.binary) {
      throw new Error(`release archive contains unexpected entries: ${listing.join(",")}`);
    }
    const extracted = spawnSync("tar", ["-xOzf", archivePath, runtime.artifact.binary], {
      encoding: null,
      maxBuffer: 128 * 1024 * 1024,
      stdio: ["ignore", "pipe", "pipe"],
    });
    if (extracted.error) throw extracted.error;
    if (extracted.status !== 0 || !extracted.stdout?.length) {
      throw new Error(`release archive payload extraction failed (${extracted.status})`);
    }
    const binary = join(stage, runtime.artifact.binary);
    writeFileSync(binary, extracted.stdout);
    chmodSync(binary, 0o755);
    inspectRuntime(binary, runtime.identity);
    mkdirSync(dirname(runtime.path), { recursive: true });
    const temporary = `${runtime.path}.setup-shea-${process.pid}`;
    cpSync(binary, temporary);
    chmodSync(temporary, 0o755);
    renameSync(temporary, runtime.path);
    return inspectRuntime(runtime.path, runtime.identity);
  } finally {
    rmSync(stage, { recursive: true, force: true });
  }
}

function directorySize(path) {
  if (!existsSync(path)) return 0;
  let total = 0;
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const child = join(path, entry.name);
    total += entry.isDirectory() ? directorySize(child) : statSync(child).size;
  }
  return total;
}

function exactRunDirectory(repo, runId) {
  const root = resolve(repo, ".shea", "local", "setup");
  const path = resolve(root, runId);
  if (dirname(path) !== root) throw new Error("unsafe setup run directory");
  return path;
}

function cleanRunDirectory(runDir) {
  const marker = join(runDir, ".shea-setup-marker");
  if (!existsSync(runDir)) return;
  if (!existsSync(marker) || !readFileSync(marker, "utf8").includes(MANAGED_MARKER)) {
    throw new Error(`refusing to clean unmarked setup directory: ${runDir}`);
  }
  rmSync(runDir, { recursive: true });
}

function runSkills(repo, request, actions) {
  const changed = actions.filter((action) => action.kind === "skills" && action.classification !== "no-op");
  if (!changed.length) return [];
  if (!commandExists("npx")) throw new Error("standard Skills CLI requires npx");
  const results = [];
  const skills = normalSkills();
  const removed = changed.filter((action) => action.classification === "remove");
  if (removed.length) {
    command("npx", ["skills", "remove", ...skills, "-y"], { cwd: repo, inherit: true });
    results.push({
      harnesses: removed.map((action) => action.harness),
      skills,
      action: "removed_named_set_before_selected_harness_reinstall",
    });
  }
  const selected = changed.filter((action) => action.classification !== "remove").map((action) => action.harness);
  if (selected.length) {
    const source = request.source.suite_url;
    command(
      "npx",
      [
        "skills",
        "add",
        source,
        "--full-depth",
        ...skills.flatMap((skill) => ["--skill", skill]),
        ...selected.flatMap((harness) => ["--agent", harness]),
        "-y",
      ],
      {
        cwd: repo,
        inherit: true,
        env: { ...process.env, ...SUITE_ARCHIVE_LIMITS },
      },
    );
    results.push({ harnesses: selected, skills, action: "installed_or_updated", source });
  }
  return results;
}

function runVerification(repo, entries, advisory, environment) {
  const results = [];
  for (const entry of entries) {
    const spec = typeof entry === "string" ? { command: entry } : entry;
    if (!spec.command || typeof spec.command !== "string") throw new Error("verification command is missing");
    const result = command("sh", ["-lc", spec.command], {
      cwd: resolve(repo, spec.working_directory || "."),
      env: environment,
      inherit: true,
      allowFailure: true,
    });
    results.push({ command: spec.command, advisory, exit_code: result.status });
    if (result.status !== 0 && !advisory) throw new Error(`blocking baseline verification failed: ${spec.command}`);
  }
  return results;
}

function runNoClaimReadiness(repo, plan, options) {
  const cli = plan.runtime.path;
  const workflow = join(repo, ".shea", "workflows", "shea-symphony.md");
  const invoked = [];
  const runCli = (args) => {
    const forbidden = new Set(["claim", "once", "loop", "set-state", "review", "merge", "main"]);
    if (args.some((arg) => forbidden.has(arg))) throw new Error(`no-claim readiness forbids command: ${args.join(" ")}`);
    command(cli, args, { cwd: repo, inherit: true });
    invoked.push(args);
  };
  inspectRuntime(cli, plan.runtime.identity);
  runCli(["validate", workflow]);
  runCli(["profiles", workflow]);
  const skillTargets = plan.request.harnesses.map((harness) => {
    const root = standardSkillRoot(repo, harness);
    const missing = normalSkills().filter((name) => !existsSync(join(root, name, "SKILL.md")));
    return { harness, root, status: missing.length ? "missing" : "ready", missing };
  });
  if (!options.skipSkills && skillTargets.some((target) => target.missing.length)) {
    throw new Error(
      `selected skill targets are incomplete: ${skillTargets
        .filter((target) => target.missing.length)
        .map((target) => `${target.harness}=${target.missing.join(",")}`)
        .join("; ")}`,
    );
  }
  if (!options.skipSkills && plan.request.harnesses.some((harness) => harness !== "claude-code")) {
    runCli(["skills", "status", workflow, "--codex-dir", join(repo, ".agents", "skills")]);
  }
  const verificationEnvironment = {
    ...process.env,
    ...(plan.request.runtime_profile.environment || {}),
    SHEA_SYMPHONY_RUNTIME_PROFILE_ID: plan.request.runtime_profile.profile_id,
    SHEA_SYMPHONY_RUNTIME_PROFILE_PATH: join(repo, ".shea", "runtime-profile.json"),
  };
  const verification = [
    ...runVerification(repo, plan.request.verification.blocking, false, verificationEnvironment),
    ...runVerification(repo, plan.request.verification.advisory, true, verificationEnvironment),
  ];
  const projectReady = plan.external_actions.length === 0;
  return {
    status: projectReady ? "ready" : "blocked_project_actions_pending",
    no_claim: true,
    repository: plan.discovery.repository_id,
    project_schema_ready: projectReady,
    selected_harnesses: plan.request.harnesses,
    skill_targets: skillTargets,
    runtime: { path: cli, source: plan.runtime.source, identity: inspectRuntime(cli, plan.runtime.identity) },
    backends: plan.request.backends,
    runtime_profile: "ready",
    verification,
    managed_files: Object.keys(plan.desired).sort(),
    conflicts: plan.conflicts,
    invoked_cli_commands: invoked,
  };
}

function recordSuccessfulReadiness(repo, plan, readiness) {
  const path = join(repo, ".shea", "setup.json");
  const manifest = readJson(path);
  manifest.last_successful_readiness = {
    status: "ready",
    no_claim: true,
    suite_revision: plan.request.source.source_revision,
    runtime: {
      cli_version: readiness.runtime.identity.cli_version,
      source_revision: readiness.runtime.identity.source_revision,
      target: readiness.runtime.identity.target,
      compatibility: readiness.runtime.identity.compatibility,
    },
    harnesses: plan.request.harnesses,
    verification: readiness.verification,
  };
  writeJson(path, withSetupManifestIntegrity(manifest));
}

export async function applySetup({ repo: repoPath, request: input, discovery = null, confirm, skipSkills = false }) {
  const plan = await buildSetupPlan({ repo: repoPath, request: input, discovery });
  if (confirm !== plan.plan_id) throw new Error(`confirmation mismatch: expected plan ${plan.plan_id}`);
  if (plan.conflicts.length) throw new Error(`setup has unresolved conflicts: ${plan.conflicts.map((item) => item.path || item.harness).join(", ")}`);
  const repo = plan.discovery.repo;
  await installRuntime(plan.runtimeInternal, plan.request);
  atomicWrite(join(repo, ".gitignore"), plan.gitignoreDesired);
  const runDir = exactRunDirectory(repo, plan.run_id);
  const marker = join(runDir, ".shea-setup-marker");
  const ignoredProbe = `.shea/local/setup/${plan.run_id}/.shea-setup-marker`;
  if (command("git", ["-C", repo, "check-ignore", "-q", ignoredProbe], { allowFailure: true }).status !== 0) {
    throw new Error(`setup staging is not ignored: ${join(repo, ignoredProbe)}`);
  }
  mkdirSync(runDir, { recursive: true });
  writeFileSync(marker, `${MANAGED_MARKER}\nrun_id=${plan.run_id}\n`);
  const before = { git_status: git(repo, ["status", "--porcelain=v1", "--untracked-files=all"]), setup_namespace_bytes: directorySize(join(repo, ".shea", "local", "setup")) };
  try {
    const drafts = join(runDir, "files");
    for (const [path, content] of Object.entries(plan.desired)) {
      const draft = join(drafts, safeRelativePath(path));
      mkdirSync(dirname(draft), { recursive: true });
      writeFileSync(draft, content);
    }
    for (const action of plan.actions.filter((item) => item.kind === "file" && item.classification === "remove")) {
      rmSync(join(repo, safeRelativePath(action.path)));
    }
    for (const [path, content] of Object.entries(plan.desired)) atomicWrite(join(repo, safeRelativePath(path)), content);
    const skill_results = skipSkills ? [{ action: "skipped_for_fixture" }] : runSkills(repo, plan.request, plan.actions);
    const readiness = runNoClaimReadiness(repo, plan, { skipSkills });
    if (readiness.status !== "ready") {
      return { applied: true, plan_id: plan.plan_id, project_plan_id: plan.project_plan_id, before, skill_results, readiness, cleanup: "current_run_removed_after_handled_blocker" };
    }
    recordSuccessfulReadiness(repo, plan, readiness);
    return { applied: true, plan_id: plan.plan_id, project_plan_id: plan.project_plan_id, before, skill_results, readiness, cleanup: "current_run_removed" };
  } finally {
    cleanRunDirectory(runDir);
  }
}

export async function applyProjectPlan({ repo: repoPath, request: input, discovery = null, confirm }) {
  const plan = await buildSetupPlan({ repo: repoPath, request: input, discovery });
  if (confirm !== plan.project_plan_id) throw new Error(`project confirmation mismatch: expected ${plan.project_plan_id}`);
  const manual = plan.external_actions.filter((action) => !action.command);
  if (manual.length) throw new Error(`Project repair requires operator input: ${manual.map((item) => item.missing?.join(",") || item.name).join("; ")}`);
  const applied = [];
  for (const action of plan.external_actions) {
    const [program, ...args] = action.command;
    command(program, args, { cwd: plan.discovery.repo, inherit: true });
    applied.push(action.kind);
  }
  return { applied, readback_required: true, no_issue_claim_or_status_transition: true };
}

export function publicPlan(plan) {
  const { request: _request, discovery: _discovery, desired: _desired, runtimeInternal: _runtime, ...safe } = plan;
  return safe;
}
