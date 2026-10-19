import notes from "../../data/ko/studio-notes.json";
import { validateReview, type Line, type Review } from "./script-studio";

export interface ImportBatch { episodes: { id: string; source: string; lines: Line[] }[] }

/** Validate the entire batch before writing. Stable IDs make repeated collection idempotent. */
export function importLines(batch: ImportBatch, known: Set<string>, read: (id: string) => Review) {
  if (!batch || !Array.isArray(batch.episodes)) throw Error("Invalid import");
  const seen = new Set<string>();
  return batch.episodes.map((e) => {
    if (!known.has(e.id) || seen.has(e.id)) throw Error("Unknown or duplicate episode");
    seen.add(e.id);
    const url = new URL(e.source);
    if (url.protocol !== "https:") throw Error("Invalid source");
    const before = read(e.id), ids = new Set(before.lines.map((l) => l.id));
    // Validate incoming rows even when their stable IDs already exist.
    validateReview({ ...before, lines: e.lines });
    const added = e.lines.filter((l) => !ids.has(l.id));
    const evidence = notes.importEvidence.replaceAll("{source}", e.source);
    const review = validateReview({ ...before, lines: [...before.lines, ...added], evidence: added.length ? [before.evidence, evidence].filter(Boolean).join("\n") : before.evidence });
    // Automatic speaker candidates do not change the episode's human judgement.
    return { id: e.id, review, inserted: added.length, duplicates: e.lines.length - added.length };
  });
}
