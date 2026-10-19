import tags from "../../data/ko/studio-tags.json";
import { ReviewFault, emptyReview, validateReview } from "./script-studio";

// The stored review records the server's Rust reading is held to (docs/script-studio.md, 「Access and service」):
// each as a writer may have left it in the review database, with what this project's own reading makes of it or the
// code and sentence it refuses it with. `bun scripts/fixtures.ts` writes them into `fixtures/`. The tag values are the
// language material's, so a case carries the Korean the records do.

const line = (overrides: Record<string, unknown> = {}) => ({
  id: "line_1",
  text: "Hello, friend",
  annotated: `<${tags.events[0]}>hello`,
  start: 10,
  end: 12.5,
  emotion: tags.emotion[1],
  delivery: tags.defaults.delivery,
  notes: "",
  ...overrides,
});
const review = (lines: unknown[], overrides: Record<string, unknown> = {}) => ({ ...emptyReview(), version: 2, speech: "yes", lines, ...overrides });

const CASES: [string, unknown][] = [
  ["an empty review", emptyReview()],
  ["a line of every field", review([line({ file: "clips/line_1.wav", source: "S01E12", events: tags.events[0], music: tags.music[1], overlap: tags.overlap[0], reviewStatus: "done" })], { appearance: "yes", evidence: "matched against the subtitles" })],
  ["times written as text, in minutes and in hours", review([line({ start: "02:13.450", end: "1:02:03" })])],
  ["a line with no span", review([line({ start: null, end: null }), line({ id: "line_2", start: "", end: "" })])],
  ["an old line without the later fields, its empty labels given the defaults", review([line({ emotion: " ", delivery: "" })])],
  ["fields the reading does not know are dropped", review([line({ speaker: "Speaker" })], { extra: true })],
  ["labels at their limit counted as the screen counts them", review([line({ emotion: "😀".repeat(50) })])],
  ["no record at all", null],
  ["a version below zero", review([], { version: -1 })],
  ["a version that is no whole number", review([], { version: 1.5 })],
  ["an appearance it does not know", review([], { appearance: "maybe" })],
  ["lines that are no list", review([], { lines: {} })],
  ["a line number twice", review([line(), line()])],
  ["an empty line number", review([line({ id: "" })])],
  ["a review state it does not know", review([line({ reviewStatus: "later" })])],
  ["a start without an end", review([line({ end: null })])],
  ["an end before its start", review([line({ start: 12, end: 10 })])],
  ["seconds past fifty-nine", review([line({ start: "2:99", end: "3:00" })])],
  ["a time that is no time", review([line({ start: "abc" })])],
  ["a time of the wrong kind", review([line({ start: true })])],
  ["a time with more than three decimals", review([line({ start: 1.2345 })])],
  ["a time past a day", review([line({ end: "24:00:01" })])],
  ["a label past its limit counted as the screen counts it", review([line({ emotion: "😀".repeat(51) })])],
  ["a text that is no text", review([line({ notes: 7 })])],
  ["evidence that is no text", review([], { evidence: null })],
];

/** Each case with what this project's reading makes of it. */
export function fixtureCases(): { name: string; stored: unknown; read: { ok: unknown } | { code: string; error: string } }[] {
  return CASES.map(([name, stored]) => {
    try {
      return { name, stored, read: { ok: validateReview(structuredClone(stored)) } };
    } catch (error) {
      if (!(error instanceof ReviewFault)) throw error;
      return { name, stored, read: { code: error.code, error: error.message } };
    }
  });
}

/** Every fixture file by its name under `fixtures/`, as it should stand: one case to a line. */
export function fixtureFiles(): Record<string, string> {
  return { "reviews.json": `[\n${fixtureCases().map((one) => JSON.stringify(one)).join(",\n")}\n]\n` };
}
