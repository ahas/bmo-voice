// Makes or refreshes the development copy of the data root (docs/development.md).
// usage: bun studio/scripts/dev-data.ts [--from <data root>] [--to <copy>]
import { devDataRoot, realDataRoot, refreshDevData } from "../lib/dev-data";

const args = process.argv.slice(2);
const value = (flag: string) => {
  const at = args.indexOf(flag);
  return at < 0 ? undefined : args[at + 1];
};
const from = value("--from") ?? realDataRoot();
const to = value("--to") ?? devDataRoot();

const report = refreshDevData(from, to);
console.log(`dev-data: ${to} <- ${from}: copied ${report.copied.join(", ") || "nothing"}; studio.db with ${report.reviews} reviews; linked ${report.linked.join(", ") || "nothing"}`);
