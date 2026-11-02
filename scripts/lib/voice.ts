import { mkdir } from "node:fs/promises";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { parseFile } from "music-metadata";

import { releaseRoot, settings, synthFolder } from "./settings";

import type { VoiceRow } from "./store";

// What the scripts share with the service about speaking a line (docs/voice-plan.md): the line in front of this
// machine's card, the engine a served voice belongs to, and the voice's own entries
// in `synth/`, which do the speaking, encoding and transcribing.

// The voice's own policy values (`policy/voice.toml`).
const policy = Bun.TOML.parse(readFileSync(join(releaseRoot, "policy/voice.toml"), "utf8")) as {
  emotions: string[];
  engine: string;
};

// The engine every served voice belongs to (`policy/voice.toml`, docs/voice-plan.md 「The synthesiser」).
export const servedEngine = policy.engine;

// The engine a voice folder's `voice.toml` names; `voxcpm2` for a folder that holds none, which is what VoxCPM2's
// folders are (docs/voice-plan.md, 「The voices」).
export function voiceEngine(folder: string): string {
  const declaration = join(folder, "voice.toml");

  if (!existsSync(declaration)) {
    return "voxcpm2";
  }

  return String((Bun.TOML.parse(readFileSync(declaration, "utf8")) as { engine?: unknown }).engine ?? "");
}

// The emotions a voice may be spoken with.
export const voiceEmotions: readonly string[] = policy.emotions;

// -------------------------------------------------------------- the line in front of the card

// A lock file there, made by whichever run comes first, so a line does not depend on another having run before
// it. A node a run speaks through is held by a lock file of its own beside the card's.
export async function readyVoiceLock(path: string): Promise<void> {
  await mkdir(dirname(path), { recursive: true }).catch(async () => undefined);
  await Bun.write(path, "");
}

// What flock exits with when a lock is somebody else's, kept clear of anything the process it guards exits with.
export const voiceQueueExit = 75;

// What the trained mouth refuses in an instruction, said before a row is written that no voice could speak: a
// bracket would close the instruction early and have the rest read aloud, and the mouth takes a sentence rather
// than a script. The mouth holds the same two limits on its side (`synth/voice-fish.py`).
export const mostDelivery = 300;

export function deliveryRefusal(style: string): string | null {
  if (/[[\]]/.test(style) || style.length > mostDelivery) {
    return `a delivery instruction is plain words of at most ${mostDelivery} characters, without brackets`;
  }

  return null;
}

// ------------------------------------------------------------ the voice's entries

// Every line spoken on this machine's card, every wav encoded and every file taken down is the work of the voice's
// own tool entry, `synth/voice-tool.py`, handed a call the way the tool host hands it (docs/voice-plan.md, 「The
// voice as a tool」).

// The words a line is handed to the mouth as: a file, or the line itself.
export type VoiceWords = { textFile: string } | { text: string };

// What the mouth is called with: the words, where the sound goes, and the whole of the trained voice's row.
export function voiceMouthArguments(words: VoiceWords, out: string, voice: VoiceRow): Record<string, unknown> {
  return {
    ...("textFile" in words ? { text_file: words.textFile } : { text: words.text }),
    out,
    checkpoint: voice.checkpoint,
    seed: voice.seed,
    ...(voice.style === "" ? {} : { style: voice.style }),
    sentences: voice.bySentence,
  };
}

// An entry's answer, as the call protocol has it.
export interface ToolAnswer {
  content?: { type: string; text?: string }[];
  isError?: boolean;
  structuredContent?: unknown;
}

// One of the voice's entries run to its end, however long that takes, handed the call as the tool host hands it: the
// entry, its arguments, and a record naming the data root, under whose voice folder the card's lock stands. Its
// answer is handed back where it did the work, and a failure throws an Error.
export async function voiceEntry(entry: string, args: Record<string, unknown>): Promise<ToolAnswer> {
  const folder = synthFolder();
  const handed = {
    protocol: 1,
    kind: "call",
    entry: `voice/${entry}`,
    arguments: args,
    session: { line: "voice", capabilities: [], dataRoot: settings().data_root },
  };
  const run = Bun.spawn(["python3", join(folder, "voice-tool.py")], {
    cwd: folder,
    stdin: new TextEncoder().encode(JSON.stringify(handed)),
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, stderr, code] = await Promise.all([new Response(run.stdout).text(), new Response(run.stderr).text(), run.exited]);
  let answer: ToolAnswer;

  try {
    answer = JSON.parse(stdout) as ToolAnswer;
  } catch {
    throw new Error(`the voice's ${entry} entry gave no answer (exit ${code}): ${stderr.trim().split("\n").pop() ?? ""}`);
  }

  if (answer.isError !== true) return answer;

  const said = (answer.content ?? []).map((piece) => piece.text ?? "").join("\n");

  throw new Error(said.replace(/^ERROR: /, ""));
}

// One line spoken on this machine's card by the voice's mouth, which stands it in the card's line.
export async function speakOnCard(words: VoiceWords, out: string, voice: VoiceRow): Promise<void> {
  await voiceEntry("mouth", voiceMouthArguments(words, out, voice));
}

// A wav already spoken by a node written out as OGG/Opus by the voice's encoder. It stands in no line: no model is
// loaded and the card is not touched.
export async function encodeWav(wav: string, out: string): Promise<void> {
  await voiceEntry("encoder", { from_wav: wav, out });
}

// How long a sound file runs, read by the maintained reader of audio containers; null where it tells none.
export async function soundSeconds(file: string): Promise<number | null> {
  try {
    return (await parseFile(file, { duration: true })).format.duration ?? null;
  } catch {
    return null;
  }
}
