# The store

Everything the voice keeps stands in its own database and in its own folder under the data root, `voice/`, and
nowhere else. No other unit reads them; they reach what is in them through the API (`docs/boundary-api.md`).

## The database

- **A PostgreSQL database of the voice's own,** at the address the settings' `database` names, read and written
  through sea-orm. The service runs the migrations as it starts; the unit command runs them too before it reads
  or writes.
- **The node and training scripts** read and write the same tables, at the same address, through the settings
  file the service reads.

### `voices`

One row per trained voice.

| Column | What it holds |
|---|---|
| `name` | the voice's name, the key |
| `checkpoint` | its checkpoint directory, as a name under the voices folder |
| `reference` | its reference clip, as a name under the voices folder (`reference/<key>.wav`) |
| `cfg` | how hard the model holds to its conditioning |
| `steps` | diffusion steps per patch |
| `current` | 1 for the one current voice, 0 for every other; at most one row is current |
| `set_at` | when the row was last written |
| `style` | its delivery instruction, plain words without brackets, empty for none |
| `seed` | the seed every line is spoken on, from 0 up |
| `by_sentence` | 1 where a line is spoken a sentence at a time |
| `prompt_clip`, `prompt_words` | its Korean continuation prompt: a clip in its own directory and the words it says, both empty for none |
| `prompt_en_clip`, `prompt_en_words` | its English continuation prompt, the same way |

### `voice_names`

One row per spelling whose reading is settled (`docs/voice-plan.md`, 「How names are said」).

| Column | What it holds |
|---|---|
| `written` | the spelling, matched whatever its case, the key |
| `ko` | what a Korean stretch says in its place, empty to leave it as written |
| `en` | what an English stretch says in its place, empty to leave it as written |
| `set_at` | when the row was last written |

A table first made is given the seed names of `data/ko/voice-names.toml`; a table that stands is left as it is.

### `voice_nodes`

The voice node list: the nodes whose voice server speaks a line (`docs/voice-plan.md`, 「The voice nodes」).

| Column | What it holds |
|---|---|
| `name` | a node, by its name in the node service's list, the key |
| `position` | its place in the list, the order free nodes are tried in |
| `added_at` | when it was put on the list |

It starts empty. The card arbiter changes it through the voice's adapter; nothing else keeps a list of voice nodes.

### `voice_briefings`

One row per briefing that belongs to a line (`docs/voice-briefing.md`, 「Where each voice is written down」).

| Column | What it holds |
|---|---|
| `run_id` | the briefing's run, the key |
| `chat_id` | the room |
| `message_id` | the line it voices or awaits; none yet for a summary launched while its turn is still writing |
| `kind` | `summary` or `full` (a line's full reading) |
| `title` | a summary's title |
| `voice_message_id` | the voice message it went out as |
| `sent_at` | when it went out |
| `seconds` | the length of its sound |
| `sound_path` | the kept sound of a full reading or a narration, under the voice's folder |
| `awaits_line` | 1 while a summary still awaits the line its round will send |
| `started_at` | when it was launched |
| `finished_at` | when its run ended, however it ended; none while it runs |

- **The run is agency's ledger row,** and its id is the row's key; the start and end are kept here as well, so the
  rules of one summary at a time in a room, a summary still being made and a full reading still making are read
  from this table alone.
- **A notice turn's opening** is kept as a `summary`, as the summary it stands in for is.
- The runner writes the voice message the moment it is saved, before any screen is told the message landed.

## The migrations

- **One migration makes every table** with its indexes, column for column in the shape the rows already kept read
  in, so rows copied in keep every column their readers name. Its record stands in `voice_migrations`, so the
  database may be shared with another service's tables. The `citext` extension is created where it is missing.
- **A table's `rowid`** is the identity the rows already kept carry; nothing reads it.

## The folder

Under the data root's `voice/`:

| Place | What it holds |
|---|---|
| `voices/` | the installed voices, a folder per checkpoint, and `reference/` (`weights/checkpoints.toml`) |
| `sounds/` | the kept sounds of full readings and narrations, by run |
| `locks/` | `voice-synth.lock`, the line in front of this machine's card, and `voice-node-<node>.lock`, one per voice node, taken by the run that speaks through it |
| `voice.token` | the bearer token every voice server checks, `0600` |
| `studio/` | the script studio's data root where the settings name no other (`docs/script-studio.md`) |
| `voice-lines/` | the training lines' files (`docs/voice-lines.md`) |

Nothing here is committed and no test writes it.
