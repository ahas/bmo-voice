import { SQL } from "bun";

import { settings } from "./settings";

// The voice's own tables as the scripts read and write them (docs/store.md): the voices, the names a voice says
// and the voice node list, in the voice's database at the address the settings name. The service runs the
// migrations that make them; the scripts only read and write rows.

// The languages a voice may carry a continuation prompt for: the two a line's stretches are told apart as
// (docs/voice-plan.md, 「Which sentences are English」).
export type VoiceLanguage = "ko" | "en";

// A continuation prompt: the path of a clip of the voice speaking one language, and the words the clip says.
// Every piece in that language is spoken as what comes next after it.
export interface VoicePrompt {
  clip: string;
  words: string;
}

// A voice's continuation prompts by language; a language it has none for is absent.
export type VoicePrompts = Partial<Record<VoiceLanguage, VoicePrompt>>;

// One voice a line can be spoken in: the trained weights, the clip they are conditioned on, and the settings
// the synthesiser is asked for. `current` is the one that speaks where nothing names another; exactly one row
// carries it.
export interface VoiceRow {
  name: string;
  // The voice's folder: its `voice.toml` says its engine, and a served voice's adapter is its `lora.pt`.
  checkpoint: string;
  // A retired VoxCPM2 voice's reference clip, its guidance and its steps; empty and naught on a served voice, which
  // speaks with no reference.
  reference: string;
  cfg: number;
  steps: number;
  // The delivery instruction the voice is spoken under, as plain words, or empty for none.
  style: string;
  // Which of the model's deliveries of a line this voice speaks: the same line, seed and settings give back the
  // same samples, so the seed a voice was chosen on by ear is the one it keeps.
  seed: number;
  prompts: VoicePrompts;
  // Whether a line is spoken a sentence at a time, each on from the voice's continuation prompt, rather than in one
  // pass: one pass of a long line grows a scratchy layer over the vowels toward its end.
  bySentence: boolean;
  current: boolean;
}

// One name a voice says its own way: the spelling, and what a Korean stretch and an English stretch say in its
// place, each empty where that language reads the spelling as it stands. The spelling is matched whatever its case.
export interface VoiceName {
  written: string;
  ko: string;
  en: string;
}

// Where each language's prompt stands in a row: its clip's column and its words' column, each empty where the
// voice has none.
const PROMPT_COLUMNS: Record<VoiceLanguage, { clip: string; words: string }> = {
  ko: { clip: "prompt_clip", words: "prompt_words" },
  en: { clip: "prompt_en_clip", words: "prompt_en_words" },
};
const LANGUAGES = Object.keys(PROMPT_COLUMNS) as VoiceLanguage[];

type StoredVoice = Record<string, unknown>;

// A pair with either half filled is handed on as it stands, so a prompt half written is refused by the mouth
// rather than quietly spoken without.
function voiceRow(row: StoredVoice): VoiceRow {
  const prompts: VoicePrompts = {};

  for (const language of LANGUAGES) {
    const clip = String(row[PROMPT_COLUMNS[language].clip] ?? "");
    const words = String(row[PROMPT_COLUMNS[language].words] ?? "");

    if (clip !== "" || words !== "") {
      prompts[language] = { clip, words };
    }
  }

  return {
    name: String(row.name),
    checkpoint: String(row.checkpoint),
    reference: String(row.reference),
    cfg: Number(row.cfg),
    steps: Number(row.steps),
    style: String(row.style),
    seed: Number(row.seed),
    prompts,
    bySentence: Number(row.by_sentence) === 1,
    current: Number(row.current) === 1,
  };
}

export class VoiceStore {
  constructor(private readonly sql: SQL) {}

  // A voice put on the table unless one of its name is there already, whose settings then stand.
  async seedVoice(row: VoiceRow, at: number): Promise<void> {
    await this.sql.begin(async (tx) => {
      const inserted = await tx`
        INSERT INTO voices (name, checkpoint, reference, cfg, steps, style, seed, by_sentence, current, set_at)
        VALUES (${row.name}, ${row.checkpoint}, ${row.reference}, ${row.cfg}, ${row.steps}, ${row.style}, ${row.seed},
          ${row.bySentence ? 1 : 0}, ${row.current ? 1 : 0}, ${at})
        ON CONFLICT (name) DO NOTHING
      `;

      if (inserted.count > 0) {
        for (const language of LANGUAGES) {
          const { clip, words } = PROMPT_COLUMNS[language];
          const prompt = row.prompts[language];

          await tx.unsafe(`UPDATE voices SET ${clip} = $1, ${words} = $2 WHERE name = $3`, [prompt?.clip ?? "", prompt?.words ?? "", row.name]);
        }
      }
    });
  }

  // The voice that speaks where nothing names another, or null where no row is current.
  async currentVoice(): Promise<VoiceRow | null> {
    const [row] = await this.sql`SELECT * FROM voices WHERE current = 1 LIMIT 1`;

    return row === undefined ? null : voiceRow(row as StoredVoice);
  }

  // One voice by the name it is known by, whether or not it is the current one.
  async voice(name: string): Promise<VoiceRow | null> {
    const [row] = await this.sql`SELECT * FROM voices WHERE name = ${name}`;

    return row === undefined ? null : voiceRow(row as StoredVoice);
  }

  // Every voice on record.
  async voices(): Promise<VoiceRow[]> {
    return ((await this.sql`SELECT * FROM voices ORDER BY name`) as StoredVoice[]).map(voiceRow);
  }

  // Moving the current mark is one act: the named row becomes the only current one.
  async setCurrentVoice(name: string, at: number): Promise<boolean> {
    return await this.sql.begin(async (tx) => {
      const [row] = await tx`SELECT name FROM voices WHERE name = ${name}`;

      if (row === undefined) {
        return false;
      }

      await tx`UPDATE voices SET current = 0 WHERE current = 1`;
      await tx`UPDATE voices SET current = 1, set_at = ${at} WHERE name = ${name}`;

      return true;
    });
  }

  // A voice's delivery instruction rewritten; an empty one speaks the line with none.
  async setVoiceStyle(name: string, style: string, at: number): Promise<boolean> {
    return (await this.sql`UPDATE voices SET style = ${style}, set_at = ${at} WHERE name = ${name}`).count > 0;
  }

  // A voice's seed rewritten.
  async setVoiceSeed(name: string, seed: number, at: number): Promise<boolean> {
    return (await this.sql`UPDATE voices SET seed = ${seed}, set_at = ${at} WHERE name = ${name}`).count > 0;
  }

  // A voice's continuation prompt in one language rewritten; null leaves that language with none.
  async setVoicePrompt(name: string, language: VoiceLanguage, prompt: VoicePrompt | null, at: number): Promise<boolean> {
    const { clip, words } = PROMPT_COLUMNS[language];
    const changed = await this.sql.unsafe(`UPDATE voices SET ${clip} = $1, ${words} = $2, set_at = $3 WHERE name = $4`, [prompt?.clip ?? "", prompt?.words ?? "", at, name]);

    return changed.count > 0;
  }

  // Whether a voice speaks a line a sentence at a time, rewritten.
  async setVoiceBySentence(name: string, bySentence: boolean, at: number): Promise<boolean> {
    return (await this.sql`UPDATE voices SET by_sentence = ${bySentence ? 1 : 0}, set_at = ${at} WHERE name = ${name}`).count > 0;
  }

  // How the names a voice says are read aloud.
  async voiceNames(): Promise<VoiceName[]> {
    return (await this.sql`SELECT written::text AS written, ko, en FROM voice_names ORDER BY written`) as VoiceName[];
  }

  // A name's readings written, the row made where the spelling has none.
  async setVoiceName(name: VoiceName, at: number): Promise<void> {
    await this.sql`
      INSERT INTO voice_names (written, ko, en, set_at) VALUES (${name.written}, ${name.ko}, ${name.en}, ${at})
      ON CONFLICT (written) DO UPDATE SET ko = excluded.ko, en = excluded.en, set_at = excluded.set_at
    `;
  }

  // A name taken off the table; false where no row had that spelling.
  async removeVoiceName(written: string): Promise<boolean> {
    return (await this.sql`DELETE FROM voice_names WHERE written = ${written}`).count > 0;
  }

  // The voice node list: the nodes whose voice servers speak the trained voice, in the order a free one is taken.
  async voiceNodes(): Promise<string[]> {
    return ((await this.sql`SELECT name FROM voice_nodes ORDER BY position, name`) as { name: string }[]).map((row) => row.name);
  }

  // A node put last on the list; false where it is already on it.
  async addVoiceNode(name: string, at: number): Promise<boolean> {
    const added = await this.sql`
      INSERT INTO voice_nodes (name, position, added_at)
      SELECT ${name}, COALESCE(MAX(position), 0) + 1, ${at} FROM voice_nodes
      ON CONFLICT (name) DO NOTHING
    `;

    return added.count > 0;
  }

  // A node taken off the list; false where it was not on it.
  async removeVoiceNode(name: string): Promise<boolean> {
    return (await this.sql`DELETE FROM voice_nodes WHERE name = ${name}`).count > 0;
  }
}

let opened: VoiceStore | null = null;

// The address the settings name, as Bun's SQL opens it. A libpq address of a Unix socket (`postgres:///db?host=/dir`)
// is opened by the socket's path, since the URL form alone would be tried over TCP against the local host, where
// the socket's peer authentication does not hold; any other address is handed over as it stands.
export function sqlOptions(database: string): string | Bun.SQL.Options {
  let url: URL;
  try {
    url = new URL(database);
  } catch {
    return database;
  }
  const host = url.searchParams.get("host");
  if (!host || !host.startsWith("/")) return database;
  const options: Bun.SQL.Options = {
    adapter: "postgres",
    path: `${host.replace(/\/$/, "")}/.s.PGSQL.${url.searchParams.get("port") ?? (url.port || "5432")}`,
    database: decodeURIComponent(url.pathname.replace(/^\//, "")) || undefined,
  };
  if (url.username) options.username = decodeURIComponent(url.username);
  if (url.password) options.password = decodeURIComponent(url.password);

  return options;
}

// The store at the address the settings name, opened when first asked, so a command refused for its flags opens
// no connection.
export function store(): VoiceStore {
  opened ??= new VoiceStore(new SQL(sqlOptions(settings().database) as string));

  return opened;
}

// The present moment as the tables keep it, in whole seconds.
export function nowSeconds(): number {
  return Math.floor(Date.now() / 1000);
}
