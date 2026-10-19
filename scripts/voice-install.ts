// The trained voices, brought where a briefing may rely on them and written into the table that says which one
// speaks.
//
// The weights are trained in an experiment directory, a scratch area another job may prune, rewrite or clean. A
// briefing is a live service, so what it speaks with lives in the runtime data root instead: outside every checkout,
// taken by the deploy's own backup. This copies what a voice actually needs (the LoRA weights and the config that
// names their base model, and the clip the voice is conditioned on) and nothing else. `resume.pt` is optimiser state
// for resuming training and has no part in speaking, so it stays where it was made.
//
// `--add <name> --checkpoint <dir> --reference <clip.wav>` brings one trained checkpoint in under a name of its own.
// The first voice on an empty table is the current one; any later one is not, so it can be heard by name and then
// chosen with `--use`. Running it again for a name the table holds is refused, and a file already in place is left
// alone.
//
// The current voice speaks wherever nothing names another; `--use <name>` moves the mark and copies nothing. An agent
// whose registration names a voice of its own speaks in that one, which is the registration's to change.
// `--style <name> '<instruction>'` rewrites the delivery instruction a voice is spoken under, and an empty one speaks
// it with none. `--seed <name> <n>` sets the seed it is spoken at, which is what names the one delivery of a line its
// row was chosen on. `--prompt <name> <clip.wav> '<words>'` gives a voice a continuation prompt (a clip of it speaking
// Korean and the words the clip says) which every Korean stretch of a line is then spoken on from; `--prompt-en`
// gives it the English one its English stretches are spoken on from. Either is given only once whole briefings
// spoken on the clip came through, and `--dry-run` asks that of a clip without giving it. `--sentences <name> on|off`
// says whether it speaks a line a sentence at a time, each on from that prompt, or in one pass. A voice spoken on the
// node needs its files there too, which `bun scripts/voice-node.ts push` brings at once and the next deployment
// brings otherwise.
//
//   bun scripts/voice-install.ts
//   bun scripts/voice-install.ts --add <name> --checkpoint <dir> --reference <clip.wav> [--dry-run]
//   bun scripts/voice-install.ts --use <name>
//   bun scripts/voice-install.ts --style <name> '<instruction>'
//   bun scripts/voice-install.ts --seed <name> <n>
//   bun scripts/voice-install.ts --prompt <name> <clip.wav> '<words>' [--dry-run] | none
//   bun scripts/voice-install.ts --prompt-en <name> <clip.wav> '<words>' [--dry-run] | none
//   bun scripts/voice-install.ts --sentences <name> on|off

import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join } from "node:path";

import { voicesFolder } from "./lib/settings";
import { nowSeconds, store } from "./lib/store";
import { deliveryRefusal, trainedDelivery } from "./lib/voice";
import { checkFile, checkLines, checkPrompt, putClipOnNodes, speakForCheck, transcribeFiles } from "./lib/voice-prompt-check";

import type { LineVerdict } from "./lib/voice-prompt-check";
import type { VoiceLanguage, VoiceRow } from "./lib/store";

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

// The settings a voice starts with: the ones the training run chose together with the delivery instruction.
const cfg = 1.5;
const steps = 16;

async function list(): Promise<void> {
  for (const row of await store().voices()) {
    const style = row.style === "" ? "no delivery instruction" : `(${row.style})`;
    const spoken = row.bySentence ? "a sentence at a time" : "in one pass";
    const prompted = Object.keys(row.prompts);
    const prompts = prompted.length === 0 ? "no continuation prompt" : `prompts ${prompted.join(", ")}`;

    console.log(`voice-install: ${row.current ? ">" : " "} ${row.name} · ${row.checkpoint} · cfg ${row.cfg} · ${row.steps} steps · seed ${row.seed} · ${style} · ${prompts} · ${spoken}`);
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

// A continuation prompt is the voices row's, one pair of columns a language; `none` empties the pair and takes the
// clip away. A clip is copied into the voice's own directory under a name made of its content
// (`prompt-<digest>.wav` for Korean, `prompt-en-<digest>.wav` for English) and put in the same place on every voice
// node, then checked by speaking whole briefings on it (docs/voice-plan.md, 「How a prompt is checked before it is
// installed」). Only a clip that passed is written into the row with its words, which is all the mouth reads, and
// `--dry-run` stops short of that. Named by its content, a clip a node was not given is refused by that node rather
// than spoken against the words of the clip before it.
const promptFlag = promptOf !== null ? "--prompt" : promptEnOf !== null ? "--prompt-en" : null;

if (promptFlag !== null) {
  const name = (promptOf ?? promptEnOf) as string;
  const [language, stem, languageName]: [VoiceLanguage, string, string] = promptFlag === "--prompt" ? ["ko", "prompt", "Korean"] : ["en", "prompt-en", "English"];
  const clip = args[args.indexOf(promptFlag) + 2] ?? "";
  const words = (args[args.indexOf(promptFlag) + 3] ?? "").trim();
  const row = (await store().voice(name)) ?? (await refuseUnknown(name));
  const before = row.prompts[language]?.clip ?? null;

  if (clip === "none") {
    await store().setVoicePrompt(name, language, null, nowSeconds());

    if (before !== null) {
      rmSync(before, { force: true });
    }

    console.log(`voice-install: ${name} speaks its ${languageName} from its reference alone`);
    await list();
    process.exit(0);
  }

  if (!clip.endsWith(".wav") || !existsSync(clip) || words === "") {
    refuse(`${promptFlag} <name> <clip.wav> '<words>': a wav on this machine and the words it says, or none`);
  }

  if (!existsSync(row.checkpoint)) {
    refuse(`${name}'s directory is not on this machine: ${row.checkpoint}`);
  }

  const clipAt = join(row.checkpoint, `${stem}-${createHash("sha256").update(readFileSync(clip)).digest("hex").slice(0, 12)}.wav`);
  const broughtHere = !existsSync(clipAt);

  if (broughtHere) {
    copyFileSync(clip, clipAt);
  }

  // What a failed or dry check leaves behind: nothing it brought, and the row as it was.
  const takeBack = (): void => {
    if (broughtHere) {
      rmSync(clipAt, { force: true });
    }
  };

  for (const missed of await putClipOnNodes(clipAt, voicesFolder())) {
    console.error(`voice-install: the clip did not reach ${missed}; that node refuses a line on it until voice-node.ts sync brings it`);
  }

  const candidate: VoiceRow = { ...row, prompts: { ...row.prompts, [language]: { clip: clipAt, words } } };
  const room = mkdtempSync(join(tmpdir(), "voice-prompt-check-"));
  let verdicts: LineVerdict[];

  console.log(`voice-install: checking ${basename(clip)} on whole briefings before ${name} speaks on it`);

  try {
    verdicts = await checkPrompt({
      language,
      lines: checkLines(language),
      speak: (text, index) => speakForCheck(candidate, text, checkFile(room, index)),
      transcribe: (files, spoken) => transcribeFiles(files, spoken),
    });
  } catch (error) {
    takeBack();
    refuse(`the check could not be run, so nothing was installed: ${error instanceof Error ? error.message : String(error)}`);
  } finally {
    rmSync(room, { recursive: true, force: true });
  }

  for (const [index, verdict] of verdicts.entries()) {
    console.log(`voice-install: line ${index + 1} · ${verdict.seconds.toFixed(1)}s · ${verdict.pace.toFixed(1)} syllables a second · CER ${verdict.cer.toFixed(3)} · ${verdict.faults.length === 0 ? "passed" : verdict.faults.join("; ")}`);
    console.log(`voice-install:   heard: ${verdict.heard}`);
  }

  const failed = verdicts.filter((verdict) => verdict.faults.length > 0).length;

  if (failed > 0 || dryRun) {
    takeBack();
    console.log(failed > 0
      ? `voice-install: refused: ${failed} of ${verdicts.length} lines came out wrong on ${basename(clip)}; ${name} speaks as it did`
      : `voice-install: would give ${name} ${basename(clip)} for its ${languageName}; every line passed`);
    process.exit(failed > 0 ? 1 : 0);
  }

  await store().setVoicePrompt(name, language, { clip: clipAt, words }, nowSeconds());

  if (before !== null && before !== clipAt) {
    rmSync(before, { force: true });
  }

  console.log(`voice-install: ${name} speaks its ${languageName} on from ${basename(clip)} (${statSync(clipAt).size} bytes)`);
  await list();
  process.exit(0);
}

if (use !== null) {
  if (!await store().setCurrentVoice(use, nowSeconds())) {
    await refuseUnknown(use);
  }

  // The voice an agent speaks in is its registration's where that names one (docs/voice-briefing.md, 「Whose
  // voice」); agency states no route to write it, so that stays the registration's to change.
  console.log(`voice-install: ${use} is the current voice; an agent whose registration names a voice of its own still speaks in that one`);
  await list();
  process.exit(0);
}

// What a voice is made of on disk, and the whole of what gets copied.
const weightFiles = ["lora_config.json", "lora_weights.safetensors"];

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
  const clip = argValue("--reference") ?? "";

  if (!/^[a-z0-9][a-z0-9-]*$/.test(added) || added === "reference") {
    refuse(`a voice's name is lower-case letters, digits and hyphens, and not "reference": ${added}`);
  }

  if (from === "" || !clip.endsWith(".wav")) {
    refuse("--add <name> --checkpoint <dir> --reference <clip.wav>: the checkpoint directory the weights come from and the clip they are conditioned on");
  }

  if (weightFiles.some((file) => !existsSync(join(from, file)))) {
    refuse(`${from} does not hold ${weightFiles.join(" and ")}`);
  }

  if (await store().voice(added) !== null) {
    refuse(`the table already holds a voice called ${added}`);
  }

  const checkpoint = join(voicesFolder(), added);
  const reference = join(voicesFolder(), "reference", basename(clip));

  bring(clip, reference);

  for (const file of weightFiles) {
    bring(join(from, file), join(checkpoint, file));
  }

  const current = (await store().currentVoice()) === null;

  if (dryRun) {
    console.log(`voice-install: would add ${added}${current ? " as the current voice" : ""}`);
    process.exit(0);
  }

  await store().seedVoice({ name: added, checkpoint, reference, cfg, steps, style: trainedDelivery, seed: 0, prompts: {}, bySentence: false, current }, nowSeconds());
}

await list();
process.exit(0);
