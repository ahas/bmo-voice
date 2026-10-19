// Registers a batch of automatic lines into the studio's reviews (docs/script-studio.md, 「Importing speakers from web
// scripts」), backing the records up first and writing an audit of what it did.
//
// usage: bun studio/scripts/bmo-script-import.ts --input batch.json [--root <data root>] [--dry-run]
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { importLines, type ImportBatch } from "../lib/script-import";
import { Studio, assets } from "../lib/script-studio";
import { sourcePrefix, studioRoot } from "../lib/settings";

const args = process.argv.slice(2);
const value = (flag: string) => {
  const i = args.indexOf(flag);
  return i < 0 ? undefined : args[i + 1];
};
const input = value("--input");
if (!input) throw Error("usage: bun studio/scripts/bmo-script-import.ts --input batch.json [--root <data root>] [--dry-run]");
const root = value("--root") ?? studioRoot();
const batch = JSON.parse(readFileSync(input, "utf8")) as ImportBatch;
const studio = new Studio(root);

try {
  const known = new Set(assets(root, sourcePrefix()).map((a) => a.id));
  const plan = importLines(batch, known, (id) => studio.read(id));
  const report = { at: new Date().toISOString(), episodes: plan.length, inserted: plan.reduce((n, p) => n + p.inserted, 0), duplicates: plan.reduce((n, p) => n + p.duplicates, 0) };
  if (!args.includes("--dry-run")) {
    const audit = join(root, "imports");
    mkdirSync(audit, { recursive: true });
    const stamp = Date.now();
    writeFileSync(join(audit, `${stamp}-before.json`), JSON.stringify(plan.map((p) => ({ id: p.id, review: studio.read(p.id) })), null, 2));
    studio.db.transaction(() => {
      for (const p of plan) if (p.inserted) studio.write(p.id, p.review);
    })();
    writeFileSync(join(audit, `${stamp}-report.json`), JSON.stringify(report, null, 2));
  }
  console.log(JSON.stringify(report));
} finally {
  studio.db.close();
}
