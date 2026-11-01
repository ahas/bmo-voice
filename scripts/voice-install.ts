// The trained voices, brought where a synthesis may rely on them and written into the table that says which one
// speaks (docs/voice-plan.md, 「The voices」).
//
// The weights are trained in an experiment directory, a scratch area another job may prune, rewrite or clean. The
// engine is a live service, so what it speaks with lives in the runtime data root instead: outside every checkout,
// taken by the deploy's own backup. This copies what a voice needs, its folder's `voice.toml` and its adapter
// `lora.pt`, and nothing else.
//
// `--add <name> --checkpoint <dir>` brings one trained adapter in under a name of its own; a folder whose `voice.toml`
// names another engine than the policy's is refused, and a voice speaks with no reference clip. The first voice on an
// empty table is the current one; any later one is not, so it can be heard by name and then chosen with `--use`.
// Running it again for a name the table holds is refused, and a file already in place is left alone.
//
// The current voice speaks wherever nothing names another; `--use <name>` moves the mark and copies nothing, and a
// retired voice is refused. An agent whose registration names a voice of its own speaks in that one, which is the
// registration's to change. `--style <name> '<instruction>'` rewrites the delivery instruction a voice is spoken under,
// and an empty one speaks it with none. `--seed <name> <n>` sets the seed it is spoken at. `--sentences <name> on|off`
// says whether it speaks a line a sentence at a time or in passes. `--prompt` and `--prompt-en` are refused: no served
// voice takes a continuation prompt. A voice spoken on the node needs its files there too, which
// `bun scripts/voice-node.ts push` brings at once and the next deployment brings otherwise.
//
//   bun scripts/voice-install.ts
//   bun scripts/voice-install.ts --add <name> --checkpoint <dir> [--dry-run]
//   bun scripts/voice-install.ts --use <name>
//   bun scripts/voice-install.ts --style <name> '<instruction>'
//   bun scripts/voice-install.ts --seed <name> <n>
//   bun scripts/voice-install.ts --sentences <name> on|off

import { copyFileSync, existsSync, mkdirSync, statSync } from "node:fs";
import { basename, dirname, join } from "node:path";

import { voicesFolder } from "./lib/settings";
import { nowSeconds, store } from "./lib/store";
import { deliveryRefusal, servedEngine, voiceEngine } from "./lib/voice";

const args = process.argv.slice(2);
const argValue = (name: string): string | null => {
  const at = args.indexOf(name);

  return at === -1 || at + 1 >= args.length ? null : (args[at + 1] as string);
};
const dryRun = args.includes("--dry-run");
const use = argValue("--use");
const styleOf = argValue("--style");
const seedOf = argValue("--seed");
const added = argValue("--add");
const promptOf = argValue("--prompt");
const promptEnOf = argValue("--prompt-en");
const sentencesOf = argValue("--sentences");

async function list(): Promise<void> {
  for (const row of await store().voices()) {
    const engine = voiceEngine(row.checkpoint);
    const style = row.style === "" ? "no delivery instruction" : `[${row.style}]`;
    const spoken = row.bySentence ? "a sentence at a time" : "in passes";
    const served = engine === servedEngine ? engine : `${engine}, retired and never spoken`;

    console.log(`voice-install: ${row.current ? ">" : " "} ${row.name} · ${served} · ${row.checkpoint} · seed ${row.seed} · ${style} · ${spoken}`);
  }
}

async function refuseUnknown(name: string): Promise<never> {
  console.error(`voice-install: there is no voice called ${name}. The table holds: ${(await store().voices()).map((row) => row.name).join(", ") || "nothing yet"}`);
  process.exit(1);
}

function refuse(why: string): never {
  console.error(`voice-install: ${why}`);
  process.exit(1);
}

if (seedOf !== null) {
  const given = args[args.indexOf("--seed") + 2] ?? "";

  if (!/^\d+$/.test(given) || !Number.isSafeInteger(Number(given))) {
    refuse("--seed <name> <n>: a seed is a whole number from 0 up");
  }

  if (!await store().setVoiceSeed(seedOf, Number(given), nowSeconds())) {
    await refuseUnknown(seedOf);
  }

  await list();
  process.exit(0);
}

if (sentencesOf !== null) {
  const given = args[args.indexOf("--sentences") + 2];

  if (given !== "on" && given !== "off") {
    refuse("--sentences <name> on|off: on speaks a line a sentence at a time, off in one pass");
  }

  if (!await store().setVoiceBySentence(sentencesOf, given === "on", nowSeconds())) {
    await refuseUnknown(sentencesOf);
  }

  await list();
  process.exit(0);
}

if (styleOf !== null) {
  const given = args[args.indexOf("--style") + 2];

  // Clearing an instruction is said out loud with an empty one, never inferred from a missing word.
  if (given === undefined) {
    refuse("--style <name> '<instruction>': an empty instruction speaks the voice with none");
  }

  const style = given.trim();
  const refusal = deliveryRefusal(style);

  if (refusal !== null) {
    refuse(refusal);
  }

  if (!await store().setVoiceStyle(styleOf, style, nowSeconds())) {
    await refuseUnknown(styleOf);
  }

  await list();
  process.exit(0);
}

if (promptOf !== null || promptEnOf !== null) {
  refuse("no served voice takes a continuation prompt: a voice of the engine speaks with none (docs/voice-plan.md, 「The voices」)");
}

if (use !== null) {
  const row = (await store().voice(use)) ?? (await refuseUnknown(use));
  const engine = voiceEngine(row.checkpoint);

  if (engine !== servedEngine) {
    refuse(`${use} is a ${engine} voice, an engine the voice no longer speaks; it is kept and never made current`);
  }

  await store().setCurrentVoice(use, nowSeconds());

  // The voice an agent speaks in is its registration's where that names one (docs/voice-plan.md, 「The voices」, 「Whose
  // voice」); agency states no route to write it, so that stays the registration's to change.
  console.log(`voice-install: ${use} is the current voice; an agent whose registration names a voice of its own still speaks in that one`);
  await list();
  process.exit(0);
}

// What a voice is made of on disk, and the whole of what gets copied.
const voiceFiles = ["voice.toml", "lora.pt"];

function bring(from: string, to: string): void {
  if (existsSync(to)) {
    console.log(`voice-install: already here, left alone: ${to}`);

    return;
  }

  if (!existsSync(from)) {
    refuse(`nothing to copy from ${from}`);
  }

  if (dryRun) {
    console.log(`voice-install: would copy ${from} -> ${to} (${statSync(from).size} bytes)`);

    return;
  }

  mkdirSync(dirname(to), { recursive: true });
  copyFileSync(from, to);
  console.log(`voice-install: copied ${basename(from)} -> ${to} (${statSync(to).size} bytes)`);
}

// A name is a directory under the voices folder beside `reference/`, so it is one plain word and never that one.
if (added !== null) {
  const from = argValue("--checkpoint") ?? "";

  if (!/^[a-z0-9][a-z0-9-]*$/.test(added) || added === "reference") {
    refuse(`a voice's name is lower-case letters, digits and hyphens, and not "reference": ${added}`);
  }

  if (from === "") {
    refuse("--add <name> --checkpoint <dir>: the folder the adapter and its voice.toml come from");
  }

  if (voiceFiles.some((file) => !existsSync(join(from, file)))) {
    refuse(`${from} does not hold ${voiceFiles.join(" and ")}`);
  }

  const engine = voiceEngine(from);

  if (engine !== servedEngine) {
    refuse(`${from} is a ${engine} voice, and the voice speaks ${servedEngine} voices only`);
  }

  if (await store().voice(added) !== null) {
    refuse(`the table already holds a voice called ${added}`);
  }

  const checkpoint = join(voicesFolder(), added);

  for (const file of voiceFiles) {
    bring(join(from, file), join(checkpoint, file));
  }

  const current = (await store().currentVoice()) === null;

  if (dryRun) {
    console.log(`voice-install: would add ${added}${current ? " as the current voice" : ""}`);
    process.exit(0);
  }

  // A served voice speaks with no reference clip and reads no guidance or steps, so those columns stand empty.
  await store().seedVoice({ name: added, checkpoint, reference: "", cfg: 0, steps: 0, style: "", seed: 0, prompts: {}, bySentence: true, current }, nowSeconds());
}

await list();
process.exit(0);
