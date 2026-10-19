import { Database } from "bun:sqlite";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

import tags from "../../data/ko/studio-tags.json";

// The studio's reading of its records, the one the import script writes with (docs/script-studio.md, 「Recording a
// line」 and 「Review state」): the same rules, codes and sentences as the server's Rust reading, held to the stored
// records in `fixtures/reviews.json`.

// Why a review does not read: its code, and the English sentence the server refuses it with too.
export class ReviewFault extends Error {
  constructor(readonly code: string, sentence: string) {
    super(sentence);
    this.name = "ReviewFault";
  }
}

const FAULTS = {
  bad_time: "A time is not written in a form the studio reads.",
  time_form: "Write a time as minutes:seconds.milliseconds.",
  time_sixty: "Seconds and minutes are below 60.",
  time_range: "A time is outside the range the studio takes.",
  text_length: "A text is longer than the studio takes, or is not text.",
  duplicate_line: "A line's id is empty or repeated.",
  review_status: "A line's review state is neither pending nor done.",
  span: "Give a span's start and end together, the end after the start.",
  record_form: "The record is not in the form the studio keeps.",
  edited_elsewhere: "Another window saved this since it was read. Load it again, then save.",
} as const;

function fault(code: keyof typeof FAULTS): ReviewFault {
  return new ReviewFault(code, FAULTS[code]);
}

// The review database has several writers, the server and the import script, so a writer waits for the lock rather
// than failing, and the write-ahead log is kept from growing without bound.
export function shareDatabase(db: Database): void {
  db.run("PRAGMA busy_timeout = 10000");
  db.run(`PRAGMA journal_size_limit = ${32 * 1024 * 1024}`);
}

export function seconds(value: unknown): number | null {
  if (value === "" || value === null || value === undefined) return null;
  if (typeof value !== "string" && typeof value !== "number") throw fault("bad_time");
  const parts = String(value).trim().split(":");
  if (parts.length > 3 || parts.some((p) => !/^\d+(\.\d{1,3})?$/.test(p))) throw fault("time_form");
  if (parts.length > 1 && parts.slice(1).some((p) => Number(p) >= 60)) throw fault("time_sixty");
  const n = parts.reduce((sum, p) => sum * 60 + Number(p), 0);
  if (!Number.isFinite(n) || n > 86400) throw fault("time_range");
  return n;
}

export interface Asset { id: string; path: string; title: string; group: string; size: number; video: boolean }

// Every file of the source manifest, named by the hash of its path there and read without the folder `prefix` its
// paths stand under.
export function assets(root: string, prefix: string): Asset[] {
  if (!existsSync(join(root, "source-manifest.json"))) return [];
  const rows = JSON.parse(readFileSync(join(root, "source-manifest.json"), "utf8")) as { path: string; size: number }[];
  return rows.map((row) => {
    const path = prefix !== "" && row.path.startsWith(prefix) ? row.path.slice(prefix.length) : row.path;
    if (path.includes("..") || path.startsWith("/")) throw Error(`${row.path} leaves the material`);
    return {
      id: createHash("sha256").update(row.path).digest("hex"),
      path,
      title: path.split("/").at(-1)!,
      group: path.split("/").slice(0, -1).join("/") || tags.topGroup,
      size: row.size,
      video: /\.mp4$/i.test(path),
    };
  });
}

export interface Line { id: string; text: string; annotated: string; start: number | null; end: number | null; emotion: string; delivery: string; notes: string; file?: string; source?: string; events?: string; music?: string; overlap?: string; reviewStatus?: "pending" | "done" }
export interface Review { version: number; appearance: string; speech: string; evidence: string; lines: Line[] }
export const emptyReview = (): Review => ({ version: 0, appearance: "unknown", speech: "unknown", evidence: "", lines: [] });

export function validateReview(value: unknown): Review {
  const v = value as Review;
  if (!v || !Number.isSafeInteger(v.version) || v.version < 0 || !["unknown", "yes", "no"].includes(v.appearance) || !["unknown", "yes", "no"].includes(v.speech) || !Array.isArray(v.lines) || v.lines.length > 3000) throw fault("record_form");
  const text = (s: unknown, limit: number) => {
    if (typeof s !== "string" || s.length > limit) throw fault("text_length");
    return s;
  };
  const ids = new Set<string>();
  const lines = v.lines.map((l) => {
    const id = text(l.id, 100);
    if (!id || ids.has(id)) throw fault("duplicate_line");
    ids.add(id);
    if (l.reviewStatus !== undefined && l.reviewStatus !== "pending" && l.reviewStatus !== "done") throw fault("review_status");
    const start = seconds(l.start), end = seconds(l.end);
    if ((start === null) !== (end === null) || (start !== null && end !== null && end <= start)) throw fault("span");
    return { id, text: text(l.text, 10000), annotated: text(l.annotated, 10000), start, end, emotion: text(l.emotion, 100).trim() || tags.defaults.emotion, delivery: text(l.delivery, 100).trim() || tags.defaults.delivery, notes: text(l.notes, 10000), file: text(l.file ?? "", 1000), source: text(l.source ?? "", 2000), events: text(l.events ?? "", 500), music: text(l.music ?? "", 100), overlap: text(l.overlap ?? "", 100), reviewStatus: l.reviewStatus ?? "pending" };
  });
  return { version: v.version, appearance: v.appearance, speech: v.speech, evidence: text(v.evidence, 20000), lines };
}

export class Studio {
  db: Database;
  constructor(readonly root: string) {
    this.db = new Database(join(root, "studio.db"));
    shareDatabase(this.db);
    this.db.exec("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS reviews(id TEXT PRIMARY KEY, version INTEGER NOT NULL, body TEXT NOT NULL, updated_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS sessions(token TEXT PRIMARY KEY, name TEXT NOT NULL, expires INTEGER NOT NULL);");
  }
  read(id: string): Review {
    const row = this.db.query("SELECT body FROM reviews WHERE id=?").get(id) as { body: string } | null;
    return row ? validateReview(JSON.parse(row.body)) : emptyReview();
  }
  write(id: string, body: unknown): Review {
    const v = validateReview(body);
    return this.db.transaction(() => {
      if (this.read(id).version !== v.version) throw fault("edited_elsewhere");
      const next = { ...v, version: v.version + 1 };
      this.db.query("INSERT INTO reviews VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET version=excluded.version,body=excluded.body,updated_at=excluded.updated_at").run(id, next.version, JSON.stringify(next), Date.now());
      return next;
    }).immediate();
  }
}
