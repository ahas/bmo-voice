// A step of a voice-training experiment put on a free card among the nodes, and what the nodes' cards are doing now
// (docs/voice-plan.md, 「Placing a step on a card」).
//
//   bun scripts/voice-train.ts nodes [--node <name>]...
//   bun scripts/voice-train.ts run --experiment <name> --step <label> --node <name>... --home <name>
//       [--carry <path under the home>]... [--bring <path under the experiment>]... -- <command...>
//
// A step runs only on the nodes its `--node` flags name, in that order, so which cards an experiment may use is
// written in its own pipeline; `nodes` looks at every node the node service lists unless told. The home is the node
// the experiment stands on, which its pipeline names with `--home`; the nodes that serve the voice are another list.
// The record is the experiment's folder under the script studio's data root (docs/script-studio.md).

import { mkdirSync } from "node:fs";
import { join } from "node:path";

import { nodeNames } from "./lib/node";
import { locksFolder, studioFolder } from "./lib/settings";
import { voiceNodeShell } from "./lib/voice-node";
import { cardKind, cardState, lookAll, runStep, trainingNode, withLockFile } from "./lib/voice-train";

import type { TrainNode } from "./lib/voice-train";

const usage = [
  "usage: bun scripts/voice-train.ts nodes [--node <name>]...",
  "       bun scripts/voice-train.ts run --experiment <name> --step <label> --node <name>... --home <name>",
  "           [--carry <path>]... [--bring <path>]... -- <command...>",
].join("\n");

const all = process.argv.slice(2);
const split = all.indexOf("--");
const args = split === -1 ? all : all.slice(0, split);
const command = split === -1 ? "" : all.slice(split + 1).join(" ").trim();
const verb = args[0] ?? "";

function fail(why: string): never {
  console.error(`voice-train: ${why}`);
  process.exit(1);
}

function values(flag: string): string[] {
  return args.flatMap((one, at) => (one === flag && at + 1 < args.length ? [args[at + 1] as string] : []));
}

function value(flag: string): string | null {
  return values(flag).at(-1) ?? null;
}

async function node(name: string): Promise<TrainNode> {
  try {
    return { name, shell: await voiceNodeShell(name) };
  } catch (error) {
    fail(error instanceof Error ? error.message : String(error));
  }
}

// The nodes `--node` names, else every node the node service lists, in that order; a node out of the voice's reach
// is passed over.
async function candidates(): Promise<TrainNode[]> {
  const named = values("--node");
  const names = named.length > 0 ? named : await nodeNames();
  const reached = await Promise.all(names.map(async (name) => {
    try {
      return [{ name, shell: await voiceNodeShell(name) }];
    } catch {
      return [];
    }
  }));

  return reached.flat();
}

async function tar(argv: string[], input?: Uint8Array): Promise<Uint8Array> {
  const run = Bun.spawn(["tar", ...argv], { stdin: input === undefined ? "ignore" : input, stdout: "pipe", stderr: "pipe" });
  const [out, err, code] = await Promise.all([new Response(run.stdout).bytes(), new Response(run.stderr).text(), run.exited]);

  if (code !== 0) fail(`tar ${argv[0]} failed: ${err.trim().slice(-500)}`);

  return out;
}

async function nodes(): Promise<void> {
  for (const { node: one, probe } of await lookAll(await candidates())) {
    if (probe === null) {
      console.log(`${one.name}\tnot reached`);
    } else {
      console.log(`${one.name}\t${cardState(probe)}${trainingNode(probe) ? `\t${cardKind(probe)} card` : ""}`);
    }
  }
}

async function run(): Promise<void> {
  const experiment = value("--experiment") ?? fail(`--experiment is required\n${usage}`);
  const step = value("--step") ?? fail(`--step is required\n${usage}`);
  const home = value("--home") ?? fail(`name the node the experiment stands on with --home\n${usage}`);

  if (command === "") fail(`the command goes after --\n${usage}`);
  if (values("--node").length === 0) fail(`name the nodes the step may run on with --node\n${usage}`);

  const record = join(studioFolder(), "experiments", experiment);

  mkdirSync(locksFolder(), { recursive: true });

  const end = await runStep(
    {
      experiment,
      step,
      command,
      carry: values("--carry"),
      bring: values("--bring"),
      record,
      home: await node(home),
      candidates: await candidates(),
    },
    {
      sleep: Bun.sleep,
      now: () => new Date(),
      say: (line) => console.log(`voice-train: ${line}`),
      lock: (work) => withLockFile(join(locksFolder(), "voice-train.lock"), work),
      pack: (dir, files) => tar(["czf", "-", "-C", dir, ...files]),
      unpack: async (dir, bytes) => {
        mkdirSync(dir, { recursive: true });
        await tar(["xzf", "-", "-C", dir], bytes);
      },
    },
  );

  process.exit(end.code === 0 ? 0 : 1);
}

switch (verb) {
  case "nodes":
    await nodes();
    break;
  case "run":
    await run();
    break;
  default:
    fail(usage);
}

process.exit(0);
