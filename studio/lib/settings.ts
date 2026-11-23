import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

// The settings the studio's media tools read: the voice's one settings file, the one `BMO_VOICE_SETTINGS` names or
// `settings.toml` at the top of the release they stand in, for the data root and the studio's deployment values
// (docs/script-studio.md, 「Access and service」).

export interface StudioSettings {
  source_root?: string;
  source_url?: string;
  speakers?: string[];
  cloud?: string;
  cloud_home?: string;
}

interface Settings {
  data_root: string;
  studio?: StudioSettings;
}

// The top of the release the studio's tools stand in.
export const releaseRoot = join(import.meta.dir, "../..");

let read: Settings | null = null;

function settings(): Settings {
  if (read !== null) {
    return read;
  }

  const named = process.env.BMO_VOICE_SETTINGS;
  const path = named === undefined || named === "" ? join(releaseRoot, "settings.toml") : named;

  if (!existsSync(path)) {
    throw new Error(`there are no settings at ${path}; name them in BMO_VOICE_SETTINGS`);
  }

  read = Bun.TOML.parse(readFileSync(path, "utf8")) as Settings;

  return read;
}

// The studio's data root: the folder `BMO_VOICE_STUDIO_DATA` names, else the voice folder's `studio/`.
export function studioRoot(): string {
  const named = process.env.BMO_VOICE_STUDIO_DATA;

  return named === undefined || named === "" ? join(settings().data_root, "voice", "studio") : named;
}

// The studio's deployment values.
export function studioSettings(): StudioSettings {
  return settings().studio ?? {};
}

// The folder the source manifest's paths stand under, with its slash, or nothing where they stand at the top.
export function sourcePrefix(): string {
  const root = (studioSettings().source_root ?? "").replace(/^\/+|\/+$/g, "");

  return root === "" ? "" : `${root}/`;
}
