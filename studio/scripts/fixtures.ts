// Writes the stored review records the server's Rust reading is held to into `fixtures/`, from this project's own
// reading (docs/script-studio.md, 「Access and service」). Run after the reading changes.
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

import { fixtureFiles } from "../lib/fixtures";

const folder = resolve(import.meta.dir, "..", "fixtures");

mkdirSync(folder, { recursive: true });

for (const [name, text] of Object.entries(fixtureFiles())) {
  writeFileSync(join(folder, name), text);
  console.log(`fixtures: ${join(folder, name)}`);
}
