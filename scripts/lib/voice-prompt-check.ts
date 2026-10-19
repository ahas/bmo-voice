import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";

import leven from "leven";

import { hangulSyllables, syllableRange } from "./hangul";
import { shellWord } from "./node-shell";
import { dataFile } from "./settings";
import { VOICE_PLACE, speakThroughFreeNode, voiceNodeNames, voiceNodeShell } from "./voice-node";
import { soundSeconds, speakOnCard, voiceCapMs, voiceEntry, voiceQueueWaitMs } from "./voice";

import type { VoiceLanguage, VoiceRow } from "./store";

// Whether a continuation prompt speaks a briefing before any briefing is spoken on it (docs/voice-plan.md, 「How a
// prompt is checked before it is installed」). A handful of lines of a briefing's length are spoken the way a
// briefing would speak them, with the new clip and its words in place of that language's prompt, a recogniser takes
// each down, and a line whose words did not come through, that runs one sound over and over, or that runs far
// longer or shorter than its words take to say is a fault. A clip with any faulty line is not installed.

// What a line may come to and still pass. A clip that speaks a briefing lands well inside each, and a clip that
// falls apart lands far outside: the bars sit between the two, not at the edge of either.
export const checkBars = {
  // Levenshtein distance over the bare letters, against the line's own length.
  cer: 0.12,
  // The same unit of one to four letters (syllables in Korean) said this many times in a row.
  run: 5,
  // Syllables of the line over the seconds of the sound.
  pace: { ko: [3, 10], en: [2, 8] } as Record<VoiceLanguage, [number, number]>,
};

export interface SpokenLine {
  text: string;
  // The sound as a file this machine can read, and how long it runs.
  file: string;
  seconds: number;
}

export interface LineVerdict extends SpokenLine {
  heard: string;
  cer: number;
  pace: number;
  // The unit said over and over where there is one, as heard.
  run: string | null;
  faults: string[];
}

// The letters a line is compared on: Hangul syllables and Latin letters, nothing else.
export function bareLetters(text: string): string {
  return text.toLowerCase().replace(new RegExp(`[^${syllableRange}a-z]`, "g"), "");
}

export function syllables(text: string, language: VoiceLanguage): number {
  if (language === "ko") {
    return hangulSyllables(text).length;
  }

  // An English word's syllables as its groups of vowels, a silent final e left out.
  return (text.toLowerCase().match(/[a-z']+/g) ?? []).reduce((sum, word) => sum + Math.max(1, (word.replace(/e$/, "").match(/[aeiouy]+/g) ?? []).length), 0);
}

// The edits between the bare letters, against the line's own length. Every bare letter is one UTF-16 unit, a Hangul
// syllable or a Latin letter, so the string distance is the distance in letters.
export function characterErrorRate(asked: string, heard: string): number {
  const reference = bareLetters(asked);

  return leven(reference, bareLetters(heard)) / Math.max(1, reference.length);
}

// The first unit of one to four letters said `times` times in a row, or null. Korean is read by its syllables and
// English by its letters, both off the bare letters.
export function repeatedRun(text: string, times: number = checkBars.run): string | null {
  const letters = [...bareLetters(text)];

  for (let size = 1; size <= 4; size += 1) {
    for (let start = 0; start + size * times <= letters.length; start += 1) {
      const unit = letters.slice(start, start + size).join("");
      let count = 1;

      while (letters.slice(start + size * count, start + size * (count + 1)).join("") === unit) {
        count += 1;
      }

      if (count >= times) {
        return unit;
      }
    }
  }

  return null;
}

export function judgeLine(line: SpokenLine, heard: string, language: VoiceLanguage): LineVerdict {
  const cer = characterErrorRate(line.text, heard);
  const pace = syllables(line.text, language) / Math.max(0.01, line.seconds);
  // A run the line itself carries is the line's, not the voice's.
  const run = repeatedRun(line.text) === null ? repeatedRun(heard) : null;
  const [slowest, fastest] = checkBars.pace[language];
  const faults: string[] = [];

  if (cer > checkBars.cer) {
    faults.push(`CER ${cer.toFixed(2)} over ${checkBars.cer}`);
  }

  if (run !== null) {
    faults.push(`"${run}" said ${checkBars.run} times or more in a row`);
  }

  if (pace < slowest || pace > fastest) {
    faults.push(`${pace.toFixed(1)} syllables a second, outside ${slowest}-${fastest}`);
  }

  return { ...line, heard, cer, pace, run, faults };
}

export interface PromptCheck {
  language: VoiceLanguage;
  lines: readonly string[];
  // One line spoken with the new prompt, written somewhere this machine can read it.
  speak(text: string, index: number): Promise<SpokenLine>;
  // What a listener took down from each file, in the order given.
  transcribe(files: readonly string[], language: VoiceLanguage): Promise<string[]>;
}

// Every line spoken side by side, taken down in one pass of the recogniser, and each judged.
export async function checkPrompt(check: PromptCheck): Promise<LineVerdict[]> {
  const spoken = await Promise.all(check.lines.map((text, index) => check.speak(text, index)));
  const heard = await check.transcribe(spoken.map((line) => line.file), check.language);

  return spoken.map((line, index) => judgeLine(line, heard[index] ?? "", check.language));
}

// ------------------------------------------------------------------- the roads the check takes

// The lines a prompt is checked on in one language, `data/<language>/voice-prompt-check-lines.json`: whole
// briefings, with no figure in them, since a recogniser writes a figure as digits and the letters would then differ
// where the voice did not.
export function checkLines(language: VoiceLanguage): string[] {
  const file = dataFile(language, "voice-prompt-check-lines.json");
  const lines = JSON.parse(readFileSync(file, "utf8")) as unknown;

  if (!Array.isArray(lines) || lines.length === 0) {
    throw new Error(`${file} holds no lines`);
  }

  return lines as string[];
}

// The files taken down by the voice's transcriber, each with what was heard in the order given; a file it did not
// name comes back as heard empty.
export async function transcribeFiles(files: readonly string[], language: VoiceLanguage): Promise<string[]> {
  const answer = await voiceEntry("transcriber", { language, files: [...files] });
  const taken = (answer.structuredContent as { heard?: { file: string; heard: string }[] } | undefined)?.heard ?? [];
  const heard = new Map(taken.map((one) => [one.file, one.heard] as const));

  return files.map((file) => heard.get(file) ?? "");
}

// The clip put where every voice node on the list keeps its copy of the voices directory, at the same place under
// it, before any row names it: a node then holds it by the time a briefing asks for it. A node that could not be
// given it is named with what it met, and holds no file of that name.
export async function putClipOnNodes(clip: string, root: string): Promise<string[]> {
  const bytes = new Uint8Array(readFileSync(clip));
  const place = relative(root, clip);
  const missed: string[] = [];

  await Promise.all((await voiceNodeNames()).map(async (name) => {
    try {
      const shell = await voiceNodeShell(name);
      const home = (await shell.run(`printf %s "$HOME"`)).stdout.trim();
      const at = `${home}/${VOICE_PLACE}/voices/${place}`;

      await shell.run(`mkdir -p ${shellWord(dirname(at))}`);
      await shell.put(at, bytes);
    } catch (error) {
      missed.push(`${name}: ${error instanceof Error ? error.message : String(error)}`);
    }
  }));

  return missed;
}

// One line spoken in the candidate row the way a briefing would speak it: on a free voice node where the list has
// any, on this machine's card behind the briefings' own line where it is empty.
export async function speakForCheck(voice: VoiceRow, text: string, out: string): Promise<SpokenLine> {
  mkdirSync(dirname(out), { recursive: true });

  if ((await voiceNodeNames()).length > 0) {
    const spoken = await speakThroughFreeNode({ voice, text, out, capMs: voiceCapMs, waitMs: voiceQueueWaitMs });

    return { text, file: out, seconds: spoken.seconds };
  }

  await speakOnCard({ text }, out, voice).catch((error: unknown) => {
    throw new Error(`the synthesiser did not speak the line: ${error instanceof Error ? error.message : String(error)}`);
  });

  return { text, file: out, seconds: (await soundSeconds(out)) ?? 0 };
}

export function checkFile(room: string, index: number): string {
  return join(room, `line-${index + 1}.ogg`);
}
