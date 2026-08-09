#!/usr/bin/env node

const fs = require("fs");
const os = require("os");
const path = require("path");
const readline = require("readline");

const repoRoot = path.resolve(__dirname, "..");
const defaultSuiteDir = path.join(repoRoot, "skills", "shea-symphony");
const ignoredFileNames = new Set([".DS_Store"]);

function usage() {
  return `Usage: node scripts/install-shea-symphony-skills.js [options]

Install or validate the repo-owned Shea Symphony skill suite.

Options:
  --suite-dir <path>   Override the suite directory.
  --codex-dir <path>   Override the Codex skills target root.
  --gemini-dir <path>  Override the Gemini local-skills target root.
  --skip-codex         Do not install or validate Codex target.
  --skip-gemini        Do not install or validate Gemini target.
  --dry-run            Preview detected targets and planned writes.
  --validate           Compare active local skills with the repo-owned suite.
  --yes                Install without an interactive confirmation.
  --help               Show this help.
`;
}

function parseArgs(argv) {
  const args = {
    suiteDir: defaultSuiteDir,
    codexDir: undefined,
    geminiDir: undefined,
    skipCodex: false,
    skipGemini: false,
    dryRun: false,
    validate: false,
    yes: false,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = () => {
      i += 1;
      if (i >= argv.length) {
        throw new Error(`${arg} requires a value`);
      }
      return argv[i];
    };

    if (arg === "--suite-dir") args.suiteDir = path.resolve(next());
    else if (arg === "--codex-dir") args.codexDir = path.resolve(next());
    else if (arg === "--gemini-dir") args.geminiDir = path.resolve(next());
    else if (arg === "--skip-codex") args.skipCodex = true;
    else if (arg === "--skip-gemini") args.skipGemini = true;
    else if (arg === "--dry-run") args.dryRun = true;
    else if (arg === "--validate") args.validate = true;
    else if (arg === "--yes") args.yes = true;
    else if (arg === "--help" || arg === "-h") {
      console.log(usage());
      process.exit(0);
    } else {
      throw new Error(`unknown option: ${arg}`);
    }
  }

  return args;
}

function detectCodexDir(args) {
  if (args.codexDir) return args.codexDir;
  if (process.env.CODEX_HOME) return path.join(process.env.CODEX_HOME, "skills");
  return path.join(os.homedir(), ".codex", "skills");
}

function detectGeminiDir(args) {
  if (args.geminiDir) return args.geminiDir;
  if (process.env.GEMINI_HOME) {
    return path.join(process.env.GEMINI_HOME, "local-skills");
  }
  return path.join(os.homedir(), ".gemini", "local-skills");
}

function readManifest(suiteDir) {
  const manifestPath = path.join(suiteDir, "manifest.toml");
  const text = fs.readFileSync(manifestPath, "utf8");
  const version = text.match(/^version = "([^"]+)"/m)?.[1] || "unknown";
  const releaseDate = text.match(/^release_date = "([^"]+)"/m)?.[1] || "unknown";
  const skills = [];
  let current;
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line === "[[skills]]") {
      if (current) skills.push(current);
      current = {};
      continue;
    }
    if (!current) continue;
    const match = line.match(/^(name|path|summary) = "([^"]*)"$/);
    if (match) current[match[1]] = match[2];
  }
  if (current) skills.push(current);
  if (skills.length === 0) {
    throw new Error(`manifest contains no [[skills]] entries: ${manifestPath}`);
  }
  return { manifestPath, version, releaseDate, skills };
}

function readSkills(suiteDir, manifest) {
  const suiteRoot = path.join(suiteDir, "suite");
  const sourceNames = fs
    .readdirSync(suiteRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
  const manifestNames = manifest.skills.map((entry) => entry.name).sort();
  if (new Set(manifestNames).size !== manifestNames.length) {
    throw new Error("manifest contains duplicate skill names");
  }
  if (JSON.stringify(sourceNames) !== JSON.stringify(manifestNames)) {
    throw new Error(
      `manifest/source skill mismatch: manifest=${manifestNames.join(",")} source=${sourceNames.join(",")}`,
    );
  }

  return manifest.skills.map((entry) => {
    if (!entry.name || !entry.path || !entry.summary) {
      throw new Error("each [[skills]] entry requires name, path, and summary");
    }
    const source = path.resolve(suiteDir, entry.path);
    if (path.dirname(source) !== suiteRoot || path.basename(source) !== entry.name) {
      throw new Error(`unsafe or mismatched manifest path for ${entry.name}: ${entry.path}`);
    }
    const skillFile = path.join(source, "SKILL.md");
    if (!fs.existsSync(skillFile)) {
      throw new Error(`missing SKILL.md for ${entry.name}`);
    }
    return { name: entry.name, source };
  });
}

function validateSourceSuite(skills) {
  for (const skill of skills) {
    const skillPath = path.join(skill.source, "SKILL.md");
    const skillText = fs.readFileSync(skillPath, "utf8");
    const frontmatter = skillText.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n/);
    if (!frontmatter) throw new Error(`invalid SKILL.md frontmatter for ${skill.name}`);
    const declaredName = frontmatter[1].match(/^name:\s*(.+)$/m)?.[1]?.trim();
    const description = frontmatter[1].match(/^description:\s*(.+)$/m)?.[1]?.trim();
    if (declaredName !== skill.name) {
      throw new Error(`SKILL.md name mismatch for ${skill.name}: ${declaredName || "missing"}`);
    }
    if (!description) throw new Error(`SKILL.md description is missing for ${skill.name}`);

    const metadataPath = path.join(skill.source, "agents", "openai.yaml");
    if (!fs.existsSync(metadataPath)) {
      throw new Error(`missing agents/openai.yaml for ${skill.name}`);
    }
    const metadata = fs.readFileSync(metadataPath, "utf8");
    for (const field of ["display_name", "short_description", "default_prompt"]) {
      if (!new RegExp(`^\\s*${field}:`, "m").test(metadata)) {
        throw new Error(`agents/openai.yaml missing ${field} for ${skill.name}`);
      }
    }
  }
}

function targets(args) {
  const result = [];
  if (!args.skipCodex) {
    result.push({ label: "codex", root: detectCodexDir(args) });
  }
  if (!args.skipGemini) {
    result.push({ label: "gemini", root: detectGeminiDir(args) });
  }
  if (result.length === 0) {
    throw new Error("no targets selected");
  }
  return result;
}

function listFiles(root) {
  if (!fs.existsSync(root)) return [];
  const files = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      if (ignoredFileNames.has(entry.name)) continue;
      const absolute = path.join(dir, entry.name);
      const relative = path.relative(root, absolute);
      if (entry.isDirectory()) walk(absolute);
      else if (entry.isFile()) files.push(relative);
    }
  };
  walk(root);
  return files.sort();
}

function compareSkill(source, destination) {
  const sourceFiles = listFiles(source);
  const destFiles = listFiles(destination);
  const sourceSet = new Set(sourceFiles);
  const destSet = new Set(destFiles);
  const missing = sourceFiles.filter((file) => !destSet.has(file));
  const extra = destFiles.filter((file) => !sourceSet.has(file));
  const different = sourceFiles.filter((file) => {
    if (!destSet.has(file)) return false;
    const left = fs.readFileSync(path.join(source, file));
    const right = fs.readFileSync(path.join(destination, file));
    return !left.equals(right);
  });
  return { missing, extra, different };
}

function copyDir(source, destination) {
  fs.mkdirSync(destination, { recursive: true });
  for (const entry of fs.readdirSync(source, { withFileTypes: true })) {
    if (ignoredFileNames.has(entry.name)) continue;
    const from = path.join(source, entry.name);
    const to = path.join(destination, entry.name);
    if (entry.isDirectory()) {
      copyDir(from, to);
    } else if (entry.isFile()) {
      fs.mkdirSync(path.dirname(to), { recursive: true });
      fs.copyFileSync(from, to);
    }
  }
}

function printPlan(manifest, skills, selectedTargets, mode) {
  console.log(`Shea Symphony skill suite ${manifest.version} (${manifest.releaseDate})`);
  console.log(`Suite: ${path.dirname(manifest.manifestPath)}`);
  console.log(`Mode: ${mode}`);
  console.log("");
  console.log("Skills:");
  for (const skill of skills) {
    console.log(`- ${skill.name}`);
  }
  console.log("");
  console.log("Targets:");
  for (const target of selectedTargets) {
    console.log(`- ${target.label}: ${target.root}`);
  }
}

async function confirmInstall() {
  if (!process.stdin.isTTY) {
    throw new Error("interactive confirmation unavailable; rerun with --yes after checking the printed targets");
  }
  const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
  const answer = await new Promise((resolve) => {
    rl.question("Install or update these local skills? Type yes to continue: ", resolve);
  });
  rl.close();
  if (answer.trim().toLowerCase() !== "yes") {
    throw new Error("installation cancelled");
  }
}

function validate(skills, selectedTargets) {
  let ok = true;
  for (const target of selectedTargets) {
    console.log("");
    console.log(`[${target.label}] ${target.root}`);
    for (const skill of skills) {
      const destination = path.join(target.root, skill.name);
      const result = compareSkill(skill.source, destination);
      const clean =
        result.missing.length === 0 &&
        result.different.length === 0 &&
        result.extra.length === 0;
      if (clean) {
        console.log(`- ${skill.name}: ok`);
        continue;
      }
      ok = false;
      console.log(`- ${skill.name}: drift`);
      for (const file of result.missing) console.log(`  missing: ${file}`);
      for (const file of result.different) console.log(`  different: ${file}`);
      for (const file of result.extra) console.log(`  extra: ${file}`);
    }
  }
  return ok;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const manifest = readManifest(args.suiteDir);
  const skills = readSkills(args.suiteDir, manifest);
  validateSourceSuite(skills);
  const selectedTargets = targets(args);
  const mode = args.validate ? "validate" : args.dryRun ? "dry-run" : "install";

  printPlan(manifest, skills, selectedTargets, mode);
  console.log("");
  console.log(`[suite] source contract: ok (${skills.length} manifest-backed skills)`);

  if (args.validate) {
    const ok = validate(skills, selectedTargets);
    if (!ok) process.exitCode = 1;
    return;
  }

  if (args.dryRun) {
    console.log("");
    for (const target of selectedTargets) {
      for (const skill of skills) {
        console.log(`would install ${skill.name} -> ${path.join(target.root, skill.name)}`);
      }
    }
    return;
  }

  if (!args.yes) {
    await confirmInstall();
  }

  for (const target of selectedTargets) {
    fs.mkdirSync(target.root, { recursive: true });
    for (const skill of skills) {
      const destination = path.join(target.root, skill.name);
      copyDir(skill.source, destination);
      console.log(`installed ${skill.name} -> ${destination}`);
    }
  }

  console.log("");
  console.log("Install complete. Run with --validate to compare active local copies.");
}

main().catch((error) => {
  console.error(`error: ${error.message}`);
  process.exit(1);
});
