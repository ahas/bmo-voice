// How the names a voice says are read aloud: the voice name table, listed and changed (docs/voice-plan.md, 「How
// names are said」).
//
// A row is a spelling and what a Korean stretch and an English stretch say in its place. Every line a voice speaks
// has each spelling the table holds written that way before anything else reads it (the reading gate, the mouths),
// so a name read wrong is mended here, for both languages at once, and nowhere else. An empty reading leaves the
// spelling as written in that language.
//
//   bun scripts/voice-names.ts
//   bun scripts/voice-names.ts --set <spelling> '<in Korean>' '<in English>'
//   bun scripts/voice-names.ts --remove <spelling>

import { nowSeconds, store } from "./lib/store";

const args = process.argv.slice(2);
const usage = "usage: bun scripts/voice-names.ts [--set <spelling> '<in Korean>' '<in English>' | --remove <spelling>]";

function refuse(why: string): never {
  console.error(`voice-names: ${why}`);
  process.exit(1);
}

// What a reading may not carry. A Korean reading with a roman letter would send the stretch to the reading gate for
// the very word it was meant to settle, and an English reading with Hangul would turn an English sentence into a
// Korean one for the mouths.
function readingRefusal(ko: string, en: string): string | null {
  if (/[A-Za-z]/.test(ko)) {
    return `a Korean reading is written without roman letters: ${ko}`;
  }

  if (/\p{Script=Hangul}/u.test(en)) {
    return `an English reading is written without Hangul: ${en}`;
  }

  if (ko === "" && en === "") {
    return "a name with no reading in either language is taken off with --remove";
  }

  return null;
}

const verb = args[0];

if (verb === "--set") {
  const [written, ko, en] = args.slice(1).map((word) => word.trim());

  if (written === undefined || written === "" || ko === undefined || en === undefined) {
    refuse(usage);
  }

  const refusal = readingRefusal(ko, en);

  if (refusal !== null) {
    refuse(refusal);
  }

  await store().setVoiceName({ written, ko, en }, nowSeconds());
} else if (verb === "--remove") {
  const written = args[1] ?? refuse(usage);

  if (!await store().removeVoiceName(written)) {
    refuse(`the table holds no name spelt ${written}`);
  }
} else if (verb !== undefined) {
  refuse(usage);
}

for (const name of await store().voiceNames()) {
  console.log(`voice-names: ${name.written} · Korean ${name.ko || "(as written)"} · English ${name.en || "(as written)"}`);
}

process.exit(0);
