// The Korean lines a voice is trained on, written by Codex (docs/voice-lines.md).
//
//   bun scripts/voice-lines.ts run --persona <file> --model <model> --effort <effort> [--out <folder>]
//       [--prefix <id prefix>] [--per-cell <n>] [--at-once <n>] [--rounds <n>] [--emotion <emotion>]
//   bun scripts/voice-lines.ts status [--out <folder>]
//
// The persona is the deployment's own file saying who the agent is and how it talks, with the people it may name and
// its own invented example lines: it names the agent and the people around it, so no repository holds it.

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { voiceFolder } from "./lib/settings";
import { store } from "./lib/store";
import { briefingEmotions } from "./lib/voice";
import { readMaterial, run, statusText } from "./lib/voice-lines";

const usage = [
  "usage: bun scripts/voice-lines.ts run --persona <file> --model <model> --effort <effort> [--out <folder>]",
  "           [--prefix <id prefix>] [--per-cell <n>] [--at-once <n>] [--rounds <n>] [--emotion <emotion>]",
  "       bun scripts/voice-lines.ts status [--out <folder>]",
].join("\n");
const args = process.argv.slice(2);

function fail(why: string, code = 2): never {
  console.error(`voice-lines: ${why}`);
  process.exit(code);
}

function flag(name: string): string | null {
  const at = args.indexOf(name);

  return at === -1 ? null : (args[at + 1] ?? null);
}

function wholeFlag(name: string, fallback: number): number {
  const value = flag(name);
  const number = value === null ? fallback : Number(value);

  if (!Number.isInteger(number) || number < 1) {
    fail(`${name} is a whole number from 1 up: ${value}`);
  }

  return number;
}

const out = flag("--out") ?? join(voiceFolder(), "voice-lines");
const material = readMaterial();

if (args[0] === "status") {
  console.log(statusText(out, briefingEmotions, material));
  process.exit(0);
}

if (args[0] !== "run") {
  fail(usage);
}

const only = flag("--emotion");
const persona = flag("--persona") ?? fail(`name the agent's persona with --persona\n${usage}`);
const model = flag("--model") ?? fail(`name the Codex model with --model\n${usage}`);
const effort = flag("--effort") ?? fail(`name the Codex reasoning effort with --effort\n${usage}`);

if (only !== null && !briefingEmotions.includes(only)) {
  fail(`--emotion is one of ${briefingEmotions.join(", ")}`);
}

if (!existsSync(persona)) {
  fail(`there is no persona at ${persona}`);
}

try {
  process.exit(
    await run({
      out,
      perCell: wholeFlag("--per-cell", 20),
      atOnce: wholeFlag("--at-once", 2),
      rounds: wholeFlag("--rounds", 5),
      emotions: only === null ? briefingEmotions : [only],
      allEmotions: briefingEmotions,
      prefix: flag("--prefix") ?? "ko",
      codex: { model, effort },
      persona: readFileSync(persona, "utf8"),
      names: await store().voiceNames(),
      material,
    }),
  );
} catch (error) {
  fail(error instanceof Error ? error.message : String(error), 1);
}
