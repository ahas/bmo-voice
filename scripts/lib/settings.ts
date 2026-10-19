import { existsSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

// The settings the scripts read: the same file the service and the unit command read (docs/layout.md,
// 「Settings」), the file `BMO_VOICE_SETTINGS` names or `settings.toml` at the top of the release the scripts stand
// in. Every token stands in its own file under the data root's `env/`, and every file the voice keeps under the
// data root's `voice/` folder; where the voices stand on this machine and on a node is `weights/checkpoints.toml`.

// A unit the scripts call: its base address, and the file under the data root's `env/` its token stands in.
export interface Unit {
  address: string;
  token_file?: string;
}

export interface Settings {
  database: string;
  data_root: string;
  synth?: string;
  units: { node: Unit };
}

// Where the voices stand (`weights/checkpoints.toml`): under the data root here, and under the account's home on a
// voice node.
interface Checkpoints {
  voices: string;
  node_place: string;
}

// The top of the release the scripts stand in.
export const releaseRoot = join(import.meta.dir, "../..");

export const settingsVariable = "BMO_VOICE_SETTINGS";

let read: Settings | null = null;

// The settings where they stand, read once.
export function settings(): Settings {
  if (read !== null) {
    return read;
  }

  const named = process.env[settingsVariable];
  const path = named === undefined || named === "" ? join(releaseRoot, "settings.toml") : named;

  if (!existsSync(path)) {
    throw new Error(`there are no settings at ${path}; name them in ${settingsVariable}`);
  }

  read = Bun.TOML.parse(readFileSync(path, "utf8")) as Settings;

  return read;
}

const checkpoints = Bun.TOML.parse(readFileSync(join(releaseRoot, "weights/checkpoints.toml"), "utf8")) as Checkpoints;

// The voice's own folder under the data root, where everything it keeps stands.
export function voiceFolder(): string {
  return join(settings().data_root, "voice");
}

// The installed voices, a folder per checkpoint and the reference clips beside them.
export function voicesFolder(): string {
  return join(settings().data_root, checkpoints.voices);
}

// The locks the voice takes: the synthesis on this machine's card, and one per voice node.
export function locksFolder(): string {
  return join(voiceFolder(), "locks");
}

// The token the voice servers on the voice nodes check.
export function nodeTokenPath(): string {
  return join(voiceFolder(), "voice.token");
}

// The server's folder under the account's home on a voice node.
export const nodePlace: string = checkpoints.node_place;

// The folder of the voice's Python side: the one the settings name, else `synth/` beside the scripts.
export function synthFolder(): string {
  return settings().synth ?? join(releaseRoot, "synth");
}

// The language data the scripts read, `data/<language>/<name>`.
export function dataFile(language: string, name: string): string {
  return join(releaseRoot, "data", language, name);
}

// The token a file holds, trimmed, where it is one: readable by its owner alone and not empty.
export function readTokenFile(path: string): string | null {
  try {
    if ((statSync(path).mode & 0o077) !== 0) {
      return null;
    }

    const token = readFileSync(path, "utf8").trim();

    return token === "" ? null : token;
  } catch {
    return null;
  }
}

// The token a unit is called with, where it names a file under the data root's `env/`.
export function unitToken(unit: Unit): string | null {
  if (unit.token_file === undefined) {
    return null;
  }

  const token = readFileSync(join(settings().data_root, "env", unit.token_file), "utf8").trim();

  return token === "" ? null : token;
}
