#!/usr/bin/env node

import { resolve } from "node:path";
import {
  applyProjectPlan,
  applySetup,
  buildSetupPlan,
  publicPlan,
  readJson,
} from "./setup-shea-lib.mjs";

function usage() {
  return `Usage:
  setup-shea.mjs plan --repo <path> --request <request.json>
  setup-shea.mjs apply --repo <path> --request <request.json> --confirm <plan-id>
  setup-shea.mjs project-apply --repo <path> --request <request.json> --confirm <project-plan-id>

The plan command is read-only. apply requires the exact visible plan id. Project
writes require a separate project-plan confirmation. Readiness never claims or
transitions an issue.
`;
}

function parseArgs(argv) {
  if (argv.length === 1 && (argv[0] === "--help" || argv[0] === "-h")) {
    console.log(usage());
    process.exit(0);
  }
  const command = argv.shift();
  const options = { command };
  while (argv.length) {
    const flag = argv.shift();
    if (["--repo", "--request", "--confirm"].includes(flag)) {
      if (!argv.length) throw new Error(`${flag} requires a value`);
      options[flag.slice(2)] = argv.shift();
    } else if (flag === "--help" || flag === "-h") {
      console.log(usage());
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${flag}`);
    }
  }
  if (!options.command || !["plan", "apply", "project-apply"].includes(options.command)) {
    throw new Error(usage());
  }
  if (!options.repo || !options.request) throw new Error("--repo and --request are required");
  if (options.command !== "plan" && !options.confirm) throw new Error("--confirm is required for writes");
  return options;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const repo = resolve(options.repo);
  const request = readJson(resolve(options.request));
  let result;
  if (options.command === "plan") {
    result = publicPlan(await buildSetupPlan({ repo, request }));
  } else if (options.command === "apply") {
    result = await applySetup({
      repo,
      request,
      confirm: options.confirm,
    });
  } else {
    result = await applyProjectPlan({ repo, request, confirm: options.confirm });
  }
  console.log(JSON.stringify(result, null, 2));
}

main().catch((error) => {
  console.error(`setup_shea_error=${error.message}`);
  process.exit(1);
});
