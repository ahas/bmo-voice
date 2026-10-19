import { appendFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { findBanned } from "bmo-markup/wording";

import { hangulSyllables } from "./hangul";
import { dataFile, releaseRoot } from "./settings";

import type { BannedTerm } from "bmo-markup/wording";
import type { VoiceName } from "./store";

// Korean lines for training a voice, written by Codex run straight from here rather than through any provider
// another unit switches between (docs/voice-lines.md). A run is hours, carried by a unit of its own, and always
// resumes from the files it has written.

// What a kind is, whatever label the language material gives it.
export type KindRole = "statement" | "question" | "exclamation" | "mixed";
const KIND_ROLES: readonly KindRole[] = ["statement", "question", "exclamation", "mixed"];

export interface LengthBand {
  name: string;
  least: number;
  most: number;
  // What a line of the band is, as the instructions say it.
  is: string;
}

// The language material the instructions are filled in with and a row is held to (`data/ko/voice-lines.toml`).
export interface LinesMaterial {
  everyday: string;
  meta: string[];
  letter_runs: string;
  laughter: string;
  reading: string;
  kinds: Record<KindRole, string>;
  band: LengthBand[];
  emotions: Record<string, string>;
  banned: { term: string; instead: string; keeps?: string[] }[];
}

export function readMaterial(): LinesMaterial {
  return Bun.TOML.parse(readFileSync(dataFile("ko", "voice-lines.toml"), "utf8")) as LinesMaterial;
}

// The material's banned words as the wording check reads an entry.
export function bannedTerms(material: LinesMaterial): BannedTerm[] {
  return material.banned.map((entry) => ({ lang: "ko", term: entry.term, instead: entry.instead, ...(entry.keeps === undefined ? {} : { keeps: entry.keeps }) }));
}

// One row as Codex hands it back.
export interface AskedLine {
  kind: string;
  text: string;
  reading: string;
}

export interface KeptLine {
  id: string;
  emotion: string;
  band: string;
  kind: string;
  text: string;
  reading: string;
}

export type Refusal = "shape" | "dash" | "reading" | "kind" | "banned" | "meta" | "length" | "duplicate" | "near-duplicate" | "full";

// What the status line calls each refusal.
export const refusalWords: Record<Refusal, string> = {
  shape: "shape (line break, tab, markdown)",
  dash: "dash",
  reading: "reading holds more than Hangul",
  kind: "kind does not hold",
  banned: "banned word",
  meta: "meta talk",
  length: "outside every band",
  duplicate: "duplicate",
  "near-duplicate": "near duplicate",
  full: "cell already full",
};

export function syllables(text: string): number {
  return hangulSyllables(text).length;
}

// A dash between words. A hyphen inside a word, a command's flag and a minus sign before a figure are not dashes,
// so only the dash characters themselves and a hyphen standing alone are.
const dash = /[‒-―⸺⸻]|(^|\s)-+(\s|$)/;
const markdown = /[`*]|^\s*(#|[-+] )/;
// The marks a reading may hold beside Hangul syllables and spaces.
const readingMarks = /[ .,?!]/g;

function lastMark(text: string): string {
  return text.trim().slice(-1);
}

// The rules a row is held to: the material, and the readings the voice name table gives a name. Said for that name,
// a reading is not a use of the word it happens to spell, which the banned words may ban as a written word.
export class LineRules {
  readonly kindLabels: string[];
  private readonly roleOf: Map<string, KindRole>;

  constructor(
    readonly material: LinesMaterial,
    readonly banned: readonly BannedTerm[],
    private readonly nameReadings: ReadonlySet<string>,
  ) {
    this.kindLabels = KIND_ROLES.map((role) => material.kinds[role]);
    this.roleOf = new Map(KIND_ROLES.map((role) => [material.kinds[role], role]));
  }

  bandOf(reading: string): LengthBand | null {
    const count = syllables(reading);

    return this.material.band.find((band) => count >= band.least && count <= band.most) ?? null;
  }

  // Why a row cannot stand as a line of the kind it claims, before it is compared with anything.
  shapeRefusal(line: AskedLine): Refusal | null {
    const { text, reading } = line;

    if (text.trim() === "" || /[\n\t]/.test(text) || /[\n\t]/.test(reading) || markdown.test(text)) {
      return "shape";
    }

    if (dash.test(text) || dash.test(reading)) {
      return "dash";
    }

    const spoken = reading.trim();

    if (spoken === "" || syllables(spoken) + (spoken.match(readingMarks) ?? []).length !== [...spoken].length) {
      return "reading";
    }

    const role = this.roleOf.get(line.kind);
    const kindHolds =
      (role === "question" && lastMark(text) === "?") ||
      (role === "exclamation" && lastMark(text) === "!") ||
      (role === "statement" && !["?", "!"].includes(lastMark(text))) ||
      (role === "mixed" && text.trim() !== reading.trim());

    if (!kindHolds) {
      return "kind";
    }

    const readingHits = findBanned("", reading, this.banned).filter((hit) => !this.nameReadings.has(hit.term.term));

    if (findBanned("", text, this.banned).length > 0 || readingHits.length > 0) {
      return "banned";
    }

    if (this.material.meta.some((word) => text.includes(word) || reading.includes(word))) {
      return "meta";
    }

    return null;
  }
}

// What a reading is compared by: its Hangul alone, so spacing and marks make no new line.
export function sameKey(reading: string): string {
  return hangulSyllables(reading).join("");
}

function pairsOf(key: string): Set<string> {
  const pairs = new Set<string>();

  for (let at = 0; at + 1 < key.length; at += 1) {
    pairs.add(key.slice(at, at + 2));
  }

  return pairs;
}

// Below this many syllables two readings are only ever the same or different: a short cry shares most of its pairs
// with another one and is still a line of its own.
export const nearLeast = 6;
export const nearOverlap = 0.7;

export function overlap(a: Set<string>, b: Set<string>): number {
  let shared = 0;

  for (const pair of a) {
    if (b.has(pair)) {
      shared += 1;
    }
  }

  return shared / (a.size + b.size - shared);
}

function cellKey(emotion: string, band: string, kind: string): string {
  return `${emotion}/${band}/${kind}`;
}

// Every line kept so far, and the one place a new row is judged against them.
export class Keeper {
  readonly lines: KeptLine[] = [];
  private readonly keys = new Set<string>();
  private readonly near: { size: number; pairs: Set<string> }[] = [];
  private readonly cells = new Map<string, number>();

  constructor(
    readonly perCell: number,
    private readonly rules: LineRules,
    // What a new line's id starts with.
    private readonly prefix: string,
  ) {}

  count(emotion: string, band: string, kind: string): number {
    return this.cells.get(cellKey(emotion, band, kind)) ?? 0;
  }

  // A line read back from the record, taken as it stands.
  restore(line: KeptLine): void {
    this.remember(line);
  }

  consider(emotion: string, asked: AskedLine): { kept: KeptLine } | { refused: Refusal } {
    const refused = this.rules.shapeRefusal(asked);

    if (refused !== null) {
      return { refused };
    }

    const band = this.rules.bandOf(asked.reading);

    if (band === null) {
      return { refused: "length" };
    }

    const key = sameKey(asked.reading);

    if (this.keys.has(key)) {
      return { refused: "duplicate" };
    }

    if (key.length >= nearLeast) {
      const pairs = pairsOf(key);

      if (this.near.some((other) => Math.min(other.size, key.length) / Math.max(other.size, key.length) >= nearOverlap && overlap(pairs, other.pairs) >= nearOverlap)) {
        return { refused: "near-duplicate" };
      }
    }

    if (this.count(emotion, band.name, asked.kind) >= this.perCell) {
      return { refused: "full" };
    }

    const line: KeptLine = {
      id: `${this.prefix}-${String(this.lines.length + 1).padStart(5, "0")}`,
      emotion,
      band: band.name,
      kind: asked.kind,
      text: asked.text.trim(),
      reading: asked.reading.trim().replace(/\s+/g, " "),
    };

    this.remember(line);

    return { kept: line };
  }

  private remember(line: KeptLine): void {
    const key = sameKey(line.reading);

    this.lines.push(line);
    this.keys.add(key);
    this.cells.set(cellKey(line.emotion, line.band, line.kind), this.count(line.emotion, line.band, line.kind) + 1);

    if (key.length >= nearLeast) {
      this.near.push({ size: key.length, pairs: pairsOf(key) });
    }
  }
}

// How many of each kind one batch asks for: what the cell lacks and a third more, so the rows the checks refuse still
// leave it full, at least five so a nearly full cell is not asked for one line, and at most twelve so a long band's
// answer stays a size one turn writes well.
export function wants(lacking: number): number {
  return lacking <= 0 ? 0 : Math.min(12, Math.max(5, Math.ceil((lacking * 4) / 3)));
}

// The instructions, `prompts/voice-lines.md`, filled in with the persona the run is handed and the language
// material. Each mark is one of the instructions' own names in braces.
export function instructions(persona: string, material: LinesMaterial, emotions: readonly string[]): string {
  const marks: Record<string, string> = {
    persona: persona.trim(),
    everyday: material.everyday,
    meta: material.meta.join(", "),
    letter_runs: material.letter_runs,
    laughter: material.laughter,
    reading: material.reading.trim().split("\n").map((line) => `  ${line}`).join("\n"),
    emotions: emotions.map((emotion) => `- **${emotion}**: ${material.emotions[emotion] ?? ""}`).join("\n"),
    bands: material.band.map((band) => `- **${band.name}**: ${band.least} to ${band.most} syllables. ${band.is}`).join("\n"),
    ...material.kinds,
  };
  let text = readFileSync(join(releaseRoot, "prompts/voice-lines.md"), "utf8");

  for (const [mark, value] of Object.entries(marks)) {
    text = text.replaceAll(`{${mark}}`, value);
  }

  return text;
}

// The thread's first message: the instructions, the names, the banned words and the emotion.
export function openingSaying(filled: string, names: readonly VoiceName[], banned: readonly BannedTerm[], emotion: string): string {
  const said = names.filter((name) => name.ko !== "").map((name) => `- ${name.written} → ${name.ko}`);
  const words = banned.map((term) => `- ${term.term} → ${term.instead}`);

  return [
    filled.trim(),
    "## Names and how they are read",
    said.join("\n"),
    "## Banned words",
    words.join("\n"),
    "## This thread",
    `Every line in this thread is spoken with the emotion **${emotion}**.`,
  ].join("\n\n");
}

// One batch's ask: the band, the count per kind, and what the emotion and band already hold.
export function batchAsk(emotion: string, band: LengthBand, counts: Record<string, number>, kinds: readonly string[], kept: readonly KeptLine[]): string {
  const asked = kinds.filter((kind) => (counts[kind] ?? 0) > 0).map((kind) => `- ${kind}: ${counts[kind]}`);
  const held = kept.filter((line) => line.emotion === emotion && line.band === band.name).map((line) => `- ${line.text}`);

  return [
    `Write new lines with the emotion **${emotion}**, in the band **${band.name}** (${band.least} to ${band.most} syllables of reading), this many of each kind:`,
    asked.join("\n"),
    held.length === 0 ? "" : `Already kept for this emotion and band, so write nothing that repeats or paraphrases these:\n${held.join("\n")}`,
    'Answer with the JSON object only: {"lines": [{"kind", "text", "reading"}, ...]}.',
  ].filter((part) => part !== "").join("\n\n");
}

// The answer's shape, closed so Codex enforces it rather than suggesting it.
export function answerSchema(kinds: readonly string[]): object {
  return {
    type: "object",
    properties: {
      lines: {
        type: "array",
        items: {
          type: "object",
          properties: {
            kind: { type: "string", enum: [...kinds] },
            text: { type: "string" },
            reading: { type: "string" },
          },
          required: ["kind", "text", "reading"],
          additionalProperties: false,
        },
      },
    },
    required: ["lines"],
    additionalProperties: false,
  };
}

export function readAnswer(text: string): AskedLine[] | null {
  try {
    const parsed = JSON.parse(text) as { lines?: unknown };

    if (!Array.isArray(parsed.lines)) {
      return null;
    }

    return parsed.lines.filter(
      (line): line is AskedLine =>
        typeof line === "object" && line !== null && typeof line.kind === "string" && typeof line.text === "string" && typeof line.reading === "string",
    );
  } catch {
    return null;
  }
}

// --- Codex -------------------------------------------------------------------------------------------

// The model and effort every call names: a resume that leaves the model out falls back to the account's default.
export interface CodexChoice {
  model: string;
  effort: string;
}

const codexTimeoutMs = 900_000;

export function codexCommand(threadId: string | null, schemaPath: string, choice: CodexChoice): string[] {
  const opened = threadId === null ? ["exec"] : ["exec", "resume", threadId];

  return [
    "codex",
    ...opened,
    "--json",
    "--skip-git-repo-check",
    "--dangerously-bypass-approvals-and-sandbox",
    "--output-schema",
    schemaPath,
    "-m",
    choice.model,
    "-c",
    `model_reasoning_effort=${choice.effort}`,
    "-",
  ];
}

export interface CodexTurn {
  threadId: string | null;
  text: string;
  tokens: { input: number; cached: number; output: number };
  failure: string | null;
}

// One `codex exec` run read off its event stream, one JSON object a line. A plain notice line is not an event and is
// passed over.
export function readCodexEvents(stream: string): CodexTurn {
  const turn: CodexTurn = { threadId: null, text: "", tokens: { input: 0, cached: 0, output: 0 }, failure: null };

  for (const line of stream.split("\n")) {
    let event: Record<string, unknown>;

    try {
      event = JSON.parse(line) as Record<string, unknown>;
    } catch {
      continue;
    }

    if (typeof event !== "object" || event === null) {
      continue;
    }

    if (event.type === "thread.started" && typeof event.thread_id === "string") {
      turn.threadId = event.thread_id;
    }

    const item = event.item as Record<string, unknown> | undefined;

    if (event.type === "item.completed" && item?.type === "agent_message" && typeof item.text === "string") {
      turn.text = item.text;
    }

    if (event.type === "turn.completed") {
      const usage = event.usage as Record<string, number> | undefined;

      turn.tokens = {
        input: usage?.input_tokens ?? 0,
        cached: usage?.cached_input_tokens ?? 0,
        output: (usage?.output_tokens ?? 0) + (usage?.reasoning_output_tokens ?? 0),
      };
    }

    if (event.type === "error" || event.type === "turn.failed") {
      const nested = event.error as Record<string, unknown> | undefined;

      turn.failure ??= (typeof event.message === "string" ? event.message : null) ?? (typeof nested?.message === "string" ? nested.message : null) ?? line;
    }
  }

  return turn;
}

// Codex reads an AGENTS.md where it starts and follows it, so it starts in a directory holding nothing, and the
// prompt goes in on stdin with the door shut behind it.
async function runCodex(command: string[], saying: string, cwd: string): Promise<CodexTurn> {
  const child = Bun.spawn(command, { cwd, stdin: new TextEncoder().encode(saying), stdout: "pipe", stderr: "pipe" });
  let ranOut = false;
  const timer = setTimeout(() => {
    ranOut = true;
    child.kill();
  }, codexTimeoutMs);

  try {
    const [out, said] = await Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text()]);
    const code = await child.exited;
    const turn = readCodexEvents(out);

    if (ranOut) {
      turn.failure ??= `no answer within ${codexTimeoutMs / 1000}s, so it was cut off`;
    }

    if (turn.failure === null && turn.text === "" && code !== 0) {
      turn.failure = said.trim() === "" ? `codex exited ${code}` : said.trim().slice(-2000);
    }

    return turn;
  } finally {
    clearTimeout(timer);
  }
}

// --- A run -------------------------------------------------------------------------------------------

export interface Progress {
  state: "running" | "finished" | "failed";
  startedAt: string;
  updatedAt: string;
  lastError: string | null;
  batches: number;
  written: number;
  kept: number;
  refused: Partial<Record<Refusal, number>>;
  tokens: { input: number; cached: number; output: number };
  threads: Partial<Record<string, string>>;
  rounds: Partial<Record<string, number>>;
}

function freshProgress(): Progress {
  const now = new Date().toISOString();

  return { state: "running", startedAt: now, updatedAt: now, lastError: null, batches: 0, written: 0, kept: 0, refused: {}, tokens: { input: 0, cached: 0, output: 0 }, threads: {}, rounds: {} };
}

function readJsonLines<T>(path: string): T[] {
  if (!existsSync(path)) {
    return [];
  }

  return readFileSync(path, "utf8").split("\n").filter((line) => line.trim() !== "").map((line) => JSON.parse(line) as T);
}

function writeWhole(path: string, text: string): void {
  writeFileSync(`${path}.tmp`, text);
  renameSync(`${path}.tmp`, path);
}

// The list a training set reads: four columns, tab-separated. Every field was refused a tab or a line break when it
// was kept, so none needs quoting.
export function listText(lines: readonly KeptLine[]): string {
  return ["id\temotion\ttext\treading", ...lines.map((line) => [line.id, line.emotion, line.text, line.reading].join("\t"))].join("\n") + "\n";
}

export interface RunOptions {
  out: string;
  perCell: number;
  atOnce: number;
  rounds: number;
  emotions: readonly string[];
  // Every emotion a briefing may be spoken with, which the status counts by.
  allEmotions: readonly string[];
  prefix: string;
  codex: CodexChoice;
  persona: string;
  names: readonly VoiceName[];
  material: LinesMaterial;
}

export async function run(options: RunOptions): Promise<number> {
  const { out, material } = options;

  mkdirSync(out, { recursive: true });

  const files = {
    lines: join(out, "lines.jsonl"),
    rejected: join(out, "rejected.jsonl"),
    list: join(out, "lines.tsv"),
    progress: join(out, "progress.json"),
    schema: join(out, "answer-schema.json"),
  };
  const banned = bannedTerms(material);
  const missing = options.emotions.filter((emotion) => material.emotions[emotion] === undefined);

  if (missing.length > 0) {
    throw new Error(`the language material describes no emotion ${missing.join(", ")}`);
  }

  const rules = new LineRules(material, banned, new Set(options.names.map((name) => name.ko).filter((ko) => ko !== "")));
  const filled = instructions(options.persona, material, options.emotions);
  const keeper = new Keeper(options.perCell, rules, options.prefix);

  for (const line of readJsonLines<KeptLine>(files.lines)) {
    keeper.restore(line);
  }

  const progress: Progress = existsSync(files.progress) ? (JSON.parse(readFileSync(files.progress, "utf8")) as Progress) : freshProgress();

  progress.state = "running";
  progress.lastError = null;
  writeWhole(files.schema, JSON.stringify(answerSchema(rules.kindLabels), null, 2));

  const cwd = mkdtempSync(join(tmpdir(), "voice-lines-"));
  const save = (): void => {
    progress.updatedAt = new Date().toISOString();
    progress.kept = keeper.lines.length;
    writeWhole(files.progress, JSON.stringify(progress, null, 2));
    writeWhole(files.list, listText(keeper.lines));
  };

  save();

  // One batch, tried three times: the second a minute later on the same thread, the third on a fresh one, since a
  // thread that has grown or gone is the likeliest cause after the throttle.
  const ask = async (emotion: string, saying: (opening: boolean) => string): Promise<AskedLine[]> => {
    let lastFailure = "";

    for (let attempt = 0; attempt < 3; attempt += 1) {
      if (attempt > 0) {
        await Bun.sleep(60_000);
      }

      if (attempt === 2) {
        delete progress.threads[emotion];
      }

      const threadId = progress.threads[emotion] ?? null;
      const turn = await runCodex(codexCommand(threadId, files.schema, options.codex), saying(threadId === null), cwd);
      const rows = turn.failure === null ? readAnswer(turn.text) : null;

      progress.tokens.input += turn.tokens.input;
      progress.tokens.cached += turn.tokens.cached;
      progress.tokens.output += turn.tokens.output;

      if (rows !== null) {
        progress.threads[emotion] = turn.threadId ?? threadId ?? undefined;

        return rows;
      }

      lastFailure = turn.failure ?? `an answer that does not read: ${turn.text.slice(0, 300)}`;
      console.error(`voice-lines: ${emotion}: the batch failed (${attempt + 1}/3): ${lastFailure}`);
    }

    throw new Error(`${emotion}: codex wrote no batch: ${lastFailure}`);
  };

  const oneEmotion = async (emotion: string): Promise<void> => {
    for (const band of material.band) {
      const roundKey = `${emotion}/${band.name}`;

      while ((progress.rounds[roundKey] ?? 0) < options.rounds) {
        const counts: Record<string, number> = {};

        for (const kind of rules.kindLabels) {
          counts[kind] = wants(options.perCell - keeper.count(emotion, band.name, kind));
        }

        if (rules.kindLabels.every((kind) => counts[kind] === 0)) {
          break;
        }

        const request = batchAsk(emotion, band, counts, rules.kindLabels, keeper.lines);
        const rows = await ask(emotion, (opening) => (opening ? `${openingSaying(filled, options.names, banned, emotion)}\n\n${request}` : request));
        let kept = 0;

        for (const row of rows) {
          const verdict = keeper.consider(emotion, row);

          if ("kept" in verdict) {
            kept += 1;
            appendFileSync(files.lines, `${JSON.stringify(verdict.kept)}\n`);
          } else {
            progress.refused[verdict.refused] = (progress.refused[verdict.refused] ?? 0) + 1;
            appendFileSync(files.rejected, `${JSON.stringify({ emotion, asked: band.name, ...row, reason: verdict.refused })}\n`);
          }
        }

        progress.batches += 1;
        progress.written += rows.length;
        progress.rounds[roundKey] = (progress.rounds[roundKey] ?? 0) + 1;
        save();
        console.log(`voice-lines: ${emotion} ${band.name} round ${progress.rounds[roundKey]}: ${rows.length} written, ${kept} kept (${keeper.lines.length} in all)`);
      }
    }
  };

  const queue = [...options.emotions];
  const failures: string[] = [];
  const worker = async (): Promise<void> => {
    for (let emotion = queue.shift(); emotion !== undefined; emotion = queue.shift()) {
      try {
        await oneEmotion(emotion);
      } catch (error) {
        failures.push(error instanceof Error ? error.message : String(error));
        progress.lastError = failures.at(-1) ?? null;
        save();
      }
    }
  };

  await Promise.all(Array.from({ length: options.atOnce }, worker));

  progress.state = failures.length === 0 ? "finished" : "failed";
  save();
  console.log(statusText(out, options.allEmotions, material));

  return failures.length === 0 ? 0 : 1;
}

// What a person reads of a run: its state, the counts and the spread.
export function statusText(out: string, emotions: readonly string[], material: LinesMaterial): string {
  const progressPath = join(out, "progress.json");

  if (!existsSync(progressPath)) {
    return `${out} holds no run yet.`;
  }

  const progress = JSON.parse(readFileSync(progressPath, "utf8")) as Progress;
  const lines = readJsonLines<KeptLine>(join(out, "lines.jsonl"));
  const states = { running: "running", finished: "finished", failed: "stopped on a failure" };
  const refusedCount = Object.values(progress.refused).reduce((sum, count) => sum + (count ?? 0), 0);
  const tally = (pick: (line: KeptLine) => string): Map<string, number> => {
    const counts = new Map<string, number>();

    for (const line of lines) {
      counts.set(pick(line), (counts.get(pick(line)) ?? 0) + 1);
    }

    return counts;
  };
  const byEmotion = tally((line) => line.emotion);
  const byBand = tally((line) => line.band);
  const byKind = tally((line) => line.kind);

  return [
    `state: ${states[progress.state]} (started ${progress.startedAt}, last written ${progress.updatedAt})`,
    progress.lastError === null ? "" : `last error: ${progress.lastError}`,
    `${progress.batches} batches, ${progress.written} written, ${lines.length} kept, ${refusedCount} refused`,
    `refused for: ${(Object.entries(progress.refused) as [Refusal, number][]).map(([reason, count]) => `${refusalWords[reason]} ${count}`).join(", ") || "nothing"}`,
    `by emotion: ${emotions.map((emotion) => `${emotion} ${byEmotion.get(emotion) ?? 0}`).join(", ")}`,
    `by band: ${material.band.map((band) => `${band.name} ${byBand.get(band.name) ?? 0}`).join(", ")}`,
    `by kind: ${KIND_ROLES.map((role) => `${material.kinds[role]} ${byKind.get(material.kinds[role]) ?? 0}`).join(", ")}`,
    `tokens: ${progress.tokens.input} in (${progress.tokens.cached} cached), ${progress.tokens.output} out`,
    `file: ${join(out, "lines.tsv")}`,
  ].filter((line) => line !== "").join("\n");
}
