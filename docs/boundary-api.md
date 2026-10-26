# The voice engine's boundary

Fixed at 0.1.0-rev.4.

Every route this engine answers, its event stream, the calls it makes, its harness plugin, its command line
and its settings. The engine is an upstream: a coding harness takes it as its first entry point through its
MCP server, with no console and no other unit running. The engine keeps briefings, each a line spoken for a
room and kept with that room, its line and its moment, and tells its listeners when one is spoken; when a
briefing is spoken, and what it says, is its caller's decision, and speaking to listeners on a schedule is a
caller's work too.

`docs/openapi.json` is the contract: an OpenAPI 3.1 document written from the service's own types, stating
every route with its parameters, its body, its answers and the codes it can answer, the bodies' schemas, the
events under `x-events`, the harness plugin under `x-harness` and its tools under `x-tools`. `bmo-voice
openapi` prints it as the repository keeps it. A change to anything here changes this document and that one
in the same commit, and moves the version (「Versions」).

## Conventions

The common wire format and the API design rules are bmo-chat's `docs/conventions.md`; this engine keeps them.

- **Every route, `GET /health` among them, needs `Authorization: Bearer <token>`,** the token of the
  settings' token file; a missing or wrong token is answered 401 `unauthorized`. A token file that holds
  nothing lets no caller in.
- **Bodies are JSON, both ways, with field names in camelCase.**
- **Times are integers of epoch milliseconds,** each field ending in `At`; spans end in `Ms` and counts in
  `Count`.
- **A failure** is a non-2xx answer whose body is `code`, a kebab-case code a caller matches on, and
  `error`, an English sentence for a log (「Codes」). A method and path no route answers is 404 `no-route`.
- **A list is a page,** `{"items": [...], "more": boolean}`. `more` says whether anything stands past the
  last item. A list takes `after`, the cursor its route states, and `limit`, 1 to 1,000 and 100 where
  absent.
- **A byte answer** is a member's `sound`, carries OGG/Opus, and honours `Range`.
- **Nothing in the API names an agent, an owner or a worker.** Who a caller speaks for is its own business. A
  briefing carries the room and the line its caller names, as the numbers the caller gives, which the engine
  keeps and matches on and never reads into.

## Health

`GET /health` answers `ready`, `reason` (null while ready) and `version`. A service that answers has its
store open and migrated.

## Events

`GET /events` is the service's one stream, server-sent events, or a page of the same events to a reader
asking `Accept: application/json`.

| Event | Data |
|---|---|
| `synthesis.created` | the synthesis as `GET /syntheses/{synthesisId}` answers it, state `waiting` |
| `synthesis.ended` | the synthesis, state `done` or `failed` |
| `transcription.ended` | the transcription, state `done` or `failed` |
| `voice.changed` | a voice as `GET /voices/{voiceName}` answers it |
| `briefing.created` | the briefing as `GET /briefings/{briefingId}` answers it, state `waiting` |
| `briefing.ended` | the briefing, state `done` or `failed` |
| `briefing.changed` | the briefing, after its line was named or let go |

- **A frame's `event` is the kind,** its `id` the event number, a signed 64-bit integer that only grows,
  and its `data` is `{"id", "kind", "at", "data"}`, the inner `data` being the member.
- **A reader resumes** with `Last-Event-ID` or `?after=`, and narrows with `kinds`, a comma-separated list.
  The last 1,000 events are kept; a resume from before them is answered 410 `gone`, and the reader reads
  the collections again.
- **A page** is read with `after` and `limit`.
- **A comment line every fifteen seconds** keeps a stream open.

## Voices

| Route | Query | Answer |
|---|---|---|
| `GET /voices` | `after`, `limit` | a page of voices, in name order |
| `GET /voices/{voiceName}` | | one voice |

- **A voice:** `name` (its key), `language` (the language data's code for the language it speaks),
  `checkpoint` (where its weights stand), `emotions` (the emotions it may be asked to speak with) and
  `changedAt` (epoch milliseconds of its last change).
- **The cursor is the last voice's `name`.** A name no voice holds is answered 404 `not-found`.
- **The emotions are the engine's policy values** (`policy/voice.toml`, 「The policy」), the same for every
  voice.

## Syntheses

| Route | Body or query | Answer |
|---|---|---|
| `POST /syntheses` | `voiceName`, `text`, `emotion?`, `style?`, `seed?` | 201, the synthesis |
| `GET /syntheses/{synthesisId}` | | one synthesis |
| `GET /syntheses/{synthesisId}/sound` | | the sound, OGG/Opus, with `Range` |

- **A synthesis:** `id`, `voiceName`, `text`, `emotion`, `state`, `durationMs` (null until done) and
  `createdAt`. Its `state` is `waiting` (taken in), `speaking` (a voice server is speaking it), `done` (its
  sound stands) or `failed`.
- **A synthesis is answered at once;** the speaking runs apart on one free voice server of the voice nodes,
  and the caller reads the member again until its state is `done` or `failed`. `done` carries
  `durationMs`, the sound's length read from its Ogg pages.
- **`text` must hold something to say.** A voice no list holds is answered 404 `not-found`; an `emotion`
  outside the voice's is answered 422 `unknown-emotion`. `style` and `seed` are laid over the voice's row
  for this one synthesis.
- **The sound** is answered only once the state is `done`; before that, and where a failed synthesis left
  none, 404 `not-found`. A sound is kept under the data root, never in the database.
- **One synthesis speaks one text.** There is no list route: a caller that wants to know a synthesis it
  started reads its member or listens to the stream.

## Briefings

| Route | Body or query | Answer |
|---|---|---|
| `POST /briefings` | `voiceName`, `text`, `chatId`, `messageId?`, `kind`, `title?`, `emotion?`, `plays?` | 201, the briefing |
| `GET /briefings` | `chatId?`, `messageId?`, `kind?`, `awaitsLine?`, `after`, `limit` | a page of briefings, newest first |
| `GET /briefings/{briefingId}` | | one briefing |
| `POST /briefings/{briefingId}/line` | `messageId`, a number or null | the briefing |
| `GET /briefings/{briefingId}/sound` | | the sound, OGG/Opus, with `Range` |

- **A briefing is a line spoken for a room in a voice,** kept with the room and the line it speaks: `id`,
  `chatId`, `messageId` (the line, null while it awaits one), `kind`, `title` (null where none was given),
  `voiceName`, `text` (what was spoken), `emotion`, `synthesisId`, `state`, `durationMs` (null until done),
  `plays`, `awaitsLine`, `voiceMessageId`, `startedAt` and `finishedAt` (null until it ends).
- **Two kinds.** A `summary` is what a listener hears live: a line or two of a line's gist, written to be heard,
  with a one-line title a listener shows in place of the words. A `full` reading is a whole line read out, kept on
  that line, which a listener plays only when asked; it names its line, and one that names none is answered 400
  `bad-request`.
- **It is spoken as a synthesis is.** `POST /briefings` answers at once, state `waiting`; the engine reads the
  words into a script by its own reading rules (「Script readings」) and speaks the script in the named voice on a
  free voice server, and `text` is the script spoken. Its `state` and `durationMs` are its synthesis's, and
  `synthesisId` names it. `text` must hold something to say; a voice no list holds is 404 `not-found`, and an
  emotion outside the policy's is 422 `unknown-emotion`.
- **Whether it plays by itself is the caller's to say** in `plays`, true where absent, and the engine keeps what
  it was told. A listener plays by itself only a `summary` whose `plays` holds, once `briefing.ended` says it is
  `done`; it plays a `full` reading only when somebody asks for it, and shows a briefing it learns of more than
  two minutes after it ended rather than playing it.
- **A summary may be spoken before its line stands.** One taken in with no `messageId` awaits its line, and
  `POST /briefings/{briefingId}/line` names it, or lets it go with null; either way it no longer awaits one, and
  `briefing.changed` says so. A full reading's line is never moved; asking is 409 `conflict`.
- **The page** is read newest first; the cursor is the last briefing's `id`, as a number, and the page starts
  below it. `chatId`, `messageId`, `kind` and `awaitsLine` narrow it.
- **A briefing the engine did not speak,** a row kept from before it spoke briefings, names no synthesis and no
  voice, reads `done`, and serves no sound; `voiceMessageId` names the voice message the room heard it as.
- **The sound** is answered only once the state is `done` and the engine spoke it; before that, 404
  `not-found`. It is the synthesis's own sound, kept under the data root.

## Transcriptions

| Route | Body | Answer |
|---|---|---|
| `POST /transcriptions` | `language`, `files` | 201, the transcription |
| `GET /transcriptions/{transcriptionId}` | | one transcription |

- **A transcription:** `id`, `language`, `state`, `text` (null until done) and `createdAt`. Its state is
  `waiting`, `running`, `done` or `failed`.
- **`files` are paths on the engine's own machine,** one or more, in the order their words are answered. A
  language other than `ko` or `en` is answered 400 `bad-request`.
- **The answer is immediate,** state `waiting`; the transcription runs apart and ends `done`, with `text`
  holding one file's words to a line in the order given, or `failed`. The end is published as
  `transcription.ended`.

## Encodings

`POST /encodings` with `fromWav`, the path of a reference recording on the engine's machine, writes it out
as an OGG/Opus sound and answers 201 with an encoding: `id`, `state` (`done`) and `createdAt`. A path that
is no readable file is answered 422 `unreadable-source`; a failed encode is 500 `failed`. The sound is kept
under the data root; no route serves it.

## Script readings

`POST /script-readings` with `text` and `language?` answers 200 with `script`, the text made speakable by the
engine's own reading rules, and `sentences`, the script cut after each of its end marks, their marks kept.
It asks no model, takes no card turn and keeps nothing. A language other than `ko` or `en` is answered 400
`bad-request`.

## Voice servers

`GET /voice-servers` answers a page of the voice servers on the voice nodes, in the node list's order.

- **A voice server:** `nodeName` (its key), `state` and `voiceNames`. `state` is `running` where the node's
  server answers, `absent` where the node cannot be reached or its server does not answer.
- **`voiceNames`** are the voices whose checkpoint the server holds loaded, in name order; empty where it
  holds none or does not answer.
- **The cursor is the last server's `nodeName`.**

## Codes

| Code | Status | Meaning |
|---|---|---|
| `bad-request` | 400 | the body or the query is not the shape the route takes; the sentence names what is wrong |
| `unauthorized` | 401 | the token is missing or wrong |
| `no-route` | 404 | no route answers the method and path |
| `not-found` | 404 | nothing stands at the key the path names |
| `conflict` | 409 | the change asked for does not fit the member as it stands |
| `gone` | 410 | the events since the number asked for are no longer kept |
| `unknown-emotion` | 422 | the emotion is not one of the engine's policy values |
| `unreadable-source` | 422 | an encoding's wav is not a file the engine can read |
| `failed` | 500 | the engine failed; the sentence is the failure |

## The voice servers and the card

- **Each voice node runs `synth/voice-vox.py` on the voice environment** as the user unit
  `bmo-voice.service`, listening on the node's loopback and taking a line on the node's own shell
  (`docs/voice-plan.md`, 「The voice nodes」).
- **A voice server takes its turn on the node's card through the card arbiter's own turn client,** installed
  on the node by the arbiter, and holds the card lock while it speaks. The engine reaches no arbiter route
  itself; the card turn is the node's.
- **Which node speaks a line is read at every synthesis** from the voice node list, and how each node is
  reached is the node service's answer for it. A node out of reach is never quietly replaced by this
  machine's card.

## What the engine calls

### bmo-node

| Call | Route | Used for |
|---|---|---|
| a node's target | `GET /nodes/{name}/target` | how a voice node is reached: `kind`, then an ssh node's `host`, `port`, `user`, `keyPath?` and `shell?`, or a hub node's `hubAddress`, `hubTokenFile` and `agentName` |
| a node's login | `POST /nodes/login` with `node` | `ssh`, the arguments that open a batch SSH login to an ssh node, or null |

A refusal of the node service is read as its own answer, `answered` false and a `refusal` carrying `code` and
`message`, and carried as the engine's failure.

### bmo-web

| Call | Route | Used for |
|---|---|---|
| sign-in | `POST /sumida/auth/session` with `name` and `password`, then `DELETE /sumida/auth/session` with the `bmo.sid` cookie it answered | the script studio's sign-in: a name and a password the account service takes are a login, a 401 is a wrong one; the studio keeps its own session after |

## The harness plugin

- **The MCP server is `bmo-voice mcp`,** on standard input and output. It calls the engine at the settings'
  address with the settings' token; `--address` and `--token` name them instead.
- **Its tools,** each a thin call of one route through the engine's own client:

| Tool | Route |
|---|---|
| `voice_voices` | `GET /voices` |
| `voice_synthesize` | `POST /syntheses` |
| `voice_synthesis` | `GET /syntheses/{synthesisId}` |
| `voice_transcribe` | `POST /transcriptions` |
| `voice_script` | `POST /script-readings` |

- **The `bmo-voice` skill** tells a harness when and how to use them. A tool's failure carries the status,
  code and sentence of the refusal, or the sentence saying why nothing answered.

## The command line

`bmo-voice` serves the harness plugin and the document; it speaks nothing by itself.

| Word | What it does |
|---|---|
| `health` | asks the service the settings name for `GET /health`, with its token, and exits 0 where it answers ready, 1 otherwise |
| `mcp` | runs the harness plugin's MCP server; `--address` and `--token` name the engine |
| `openapi` | prints `docs/openapi.json` as the repository keeps it |

## The policy

`policy/voice.toml` holds the engine's own policy values, read once at start: `run_cap_ms`, how long one
synthesis may take, and `emotions`, the emotions a voice may be asked to speak with. No route serves them;
the voices list them, and a synthesis outside them is refused.

## Settings

The service and the command read one TOML file, `settings.toml` at the top of the release or the file
`BMO_VOICE_SETTINGS` names.

| Key | Meaning |
|---|---|
| `listen` | where the service listens |
| `address` | the service's own address, as the command and the plugin call it |
| `token_file` | the file under the data root's `env/` holding the token callers present |
| `database` | the address of the engine's own database (`docs/store.md`) |
| `data_root` | the runtime data root; everything the engine keeps stands under its `voice/` folder |
| `synth` | optional, the folder of the engine's Python side, `synth/` where it is not in the release |
| `[units.node]` | `address` and `token_file?` of the node service |
| `[units.web]` | `address` and `token_file?` of the account service the script studio signs in against |
| `[studio]` | optional, the script studio's deployment values (`docs/script-studio.md`, 「Access and service」) |

Every token stands in its own file under the data root's `env/`, never in the settings, a log, a command
line or an environment variable.

## The script studio

The script studio has its own server and screen (`docs/script-studio.md`). It is not part of the engine's
upstream API: its screen speaks a language pack the repository holds, its API refuses with its own codes,
and its server calls the engine's synthesis and reading rules through the crate rather than HTTP.

## Versions

- **The repository has one version,** in `VERSION` and the crate's manifest, shared by the engine, its
  client, its plugin and its OpenAPI document.
- **Before its first release it is `0.1.0` and counts revisions:** the first `0.1.0` is the base, and every
  later bump is one revision, written `0.1.0-rev.<n>` with `n` counting from 1. Major, minor and patch never
  move before the first release, a breaking change included.
- **The first release drops the suffix.** From then on the version moves by semver; a removed or renamed
  route or field, or a field that becomes required, is a major version.
