import { Database } from "bun:sqlite";
import { copyFileSync, existsSync, lstatSync, mkdirSync, rmSync, symlinkSync } from "node:fs";
import { join, resolve, sep } from "node:path";

import { studioRoot } from "./settings";

// The copy of the data root development works against, so nothing a check does reaches the real records
// (docs/development.md). The records are copied; the media, subtitles and transcripts are large, only ever read by
// the server, and linked.

/** The studio's data root, which the copy is made from. */
export function realDataRoot(): string {
  return studioRoot();
}

/** Where the copy stands where none is named: beside the data root, never inside it. */
export function devDataRoot(): string {
  return `${resolve(studioRoot())}-workbench`;
}

const COPIED = ["source-manifest.json", "downloads.json", "research-index.json"];
const LINKED = ["media", "subtitles", "asr"];

export interface DevDataReport {
  copied: string[];
  linked: string[];
  reviews: number;
}

/** Makes or refreshes the copy at `to` from the data root at `from`. */
export function refreshDevData(from: string, to: string): DevDataReport {
  const source = resolve(from);
  const target = resolve(to);
  if (target === source || target.startsWith(source + sep)) throw new Error(`the copy may not stand inside the data root it copies: ${target}`);
  mkdirSync(target, { recursive: true });

  const copied: string[] = [];
  for (const name of COPIED) {
    if (!existsSync(join(source, name))) continue;
    copyFileSync(join(source, name), join(target, name));
    copied.push(name);
  }

  // The review database through SQLite itself, which reads through the write-ahead log, so the copy holds every
  // committed write whether or not it has been checkpointed into the main file.
  for (const suffix of ["", "-wal", "-shm"]) rmSync(join(target, `studio.db${suffix}`), { force: true });
  let reviews = 0;
  if (existsSync(join(source, "studio.db"))) {
    const db = new Database(join(source, "studio.db"), { readonly: true });
    try {
      db.run(`VACUUM INTO '${join(target, "studio.db").replaceAll("'", "''")}'`);
    } finally {
      db.close();
    }
    const copy = new Database(join(target, "studio.db"), { readonly: true });
    try {
      reviews = (copy.query("SELECT count(*) AS n FROM reviews").get() as { n: number }).n;
    } finally {
      copy.close();
    }
  }

  const linked: string[] = [];
  for (const name of LINKED) {
    if (!existsSync(join(source, name))) continue;
    const at = join(target, name);
    let stands = false;
    try {
      stands = lstatSync(at) !== undefined;
    } catch {
      // Nothing stands there yet.
    }
    if (stands) rmSync(at, { recursive: false, force: true });
    symlinkSync(join(source, name), at);
    linked.push(name);
  }

  return { copied, linked, reviews };
}
