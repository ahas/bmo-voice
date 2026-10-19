// The development bench: the studio's server on 127.0.0.1:3215 against the development copy of the data root, and
// the screen's Nuxt dev server on 127.0.0.1:3216, which passes the API, media and subtitles through to it
// (docs/development.md). When either ends, the other is ended too, so whatever runs this starts both again together.
import { existsSync } from "node:fs";
import { join } from "node:path";

import { devDataRoot, realDataRoot, refreshDevData } from "../lib/dev-data";
import { releaseRoot } from "../lib/settings";

const API_PORT = 3215;
const SCREEN_PORT = 3216;
const copy = devDataRoot();

if (!existsSync(join(copy, "studio.db"))) {
  const report = refreshDevData(realDataRoot(), copy);
  console.log(`dev: made the development copy at ${copy} (${report.reviews} reviews)`);
}

const env = {
  ...process.env,
  BMO_SCRIPT_DATA: copy,
  BMO_SCRIPT_PORT: String(API_PORT),
  BMO_SCRIPT_DEV_API: `http://127.0.0.1:${API_PORT}`,
};

// The server built and started by Cargo from the repository's one crate; a change to it is taken up by starting the
// bench again.
const server = Bun.spawn(["cargo", "run", "--locked", "--bin", "script-studio"], { cwd: releaseRoot, env, stdout: "inherit", stderr: "inherit" });
const screen = Bun.spawn(["bun", "--bun", "nuxi", "dev", "--host", "127.0.0.1", "--port", String(SCREEN_PORT)], {
  cwd: join(releaseRoot, "studio", "client"),
  env,
  stdout: "inherit",
  stderr: "inherit",
});

const stop = () => {
  server.kill();
  screen.kill();
};
process.on("SIGINT", stop);
process.on("SIGTERM", stop);

const first = await Promise.race([server.exited.then((code) => ["server", code] as const), screen.exited.then((code) => ["screen", code] as const)]);
console.error(`dev: the ${first[0]} ended (${first[1]}); ending the other`);
stop();
await Promise.all([server.exited, screen.exited]);
process.exit(first[1] === 0 ? 1 : first[1]);
