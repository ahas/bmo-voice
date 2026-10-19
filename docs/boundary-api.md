# Boundary API

Everything the voice serves and everything it calls. Another unit reaches the voice only through what this
document states, and the voice reaches another unit only through the routes, commands and protocols that unit's
own document states, named here with the section they stand in.

## Versioning

- **The boundary carries the repository's one version,** in semver, the version in `VERSION` and the crate's
  manifest.
- **The major moves on a change a caller reads:** a route taken away or renamed, a field taken away or given
  another meaning, a refusal's code changed, the stream's frames changed, a command's words or exit codes changed.
- **The minor moves on an addition:** a route, a field, a frame kind, a code. The patch moves on anything else.
- **A change to this document is made in the same commit as the code that serves it,** and moves the version as
  above.

## Conventions

- **Bodies are JSON both ways,** keys in snake_case, times in epoch milliseconds, a field written with a question
  mark optional or null.
- **Every route but `GET /health` wants the bearer token** the settings' `token_file` names under the data root's
  `env/`; a request without it is answered 401 `unauthorised`.
- **A refusal** is a status and a body of `error`, one English sentence, and `code`, snake_case. The codes are in
  「Codes」 below. Words a person reads in another language are a console's language pack, keyed by the code.

## Settings

The service and the unit command read one TOML file (`docs/layout.md`, 「Settings」):

| Key | Meaning |
|---|---|
| `listen` | where the service listens |
| `address` | the service's own address, as the units it hands work to call it back |
| `token_file` | the file under the data root's `env/` holding the token callers present |
| `database` | the address of the voice's own database (`docs/store.md`) |
| `data_root` | the runtime data root; everything the voice keeps stands under its `voice/` folder |
| `agent` | the registered agent whose voice speaks where nothing names another, by its id |
| `[units.<unit>]` | `address` and `token_file?` of each unit called: `gate`, `receiver`, `agency`, `artifact`, `node`, `pocket`, and `web` for the script studio's sign-in |

## What the voice serves

### Health

| Route | Answer |
|---|---|
| `GET /health` | `ok`, `version` |

### Briefings

A briefing is a line said out loud in a room in an agent's trained voice (`docs/voice-briefing.md`). Launching one
answers at once; the speaking runs apart, under a run of agency's ledger.

| Route | Request | Answer |
|---|---|---|
| `POST /briefings` | a launch (below) | 201: `run`, the briefing's run, and `unit`, the name of the run that speaks it |
| `GET /briefings?chat=&awaiting=` | `chat`, a room; `awaiting=true` for only the summaries still awaiting their line | `briefings`: each `run`, `chat`, `message?` (the line it voices or awaits), `voice_message?` (the voice message it went out as), `kind`, `title?`, `awaits_line`, `started_at`, `finished` |
| `POST /briefings/{run}/line` | `message`, the line a summary's round sent | 204; 404 `no_briefing` for a run the voice keeps no briefing of |
| `GET /briefings/{run}/sound` | | the kept sound of a full reading or a narration, OGG/Opus, answering ranges; 404 `no_sound` where none is kept |
| `POST /briefings/stop` | `chat`, `message`: the voice message a listener stopped | 204, and a `briefing-stopped` frame on the stream |
| `GET /briefings/events` | `Last-Event-ID` or `?after=` to resume | the listener stream (below), `text/event-stream` |

- **A run is named by its ledger id written as a string,** in every answer and path, as the receiver reads it.
- **A listed briefing's `kind`** is `summary` (a summary or a notice turn's opening) or `full` (a line's full
  reading); a briefing of a session or a narration belongs to no line and is not listed.

- **A launch:** `chat` (`chat_id` is read as the same field), `text`, the words to speak, and `started_by` (`by`
  is read as the same field), naming what started it: `chat:<chatId>` for a session, `notice:<chatId>` for the
  receiver's own summary of a notice turn, `console:<chatId>` for a line's full reading, `search:<document>` or
  `archive-doc:<document>` for a narration, or the launcher's own name. Beside them:
  - `kind?`: `briefing` (a session's, where none is named), `notice` (a notice turn's opening the receiver speaks
    itself), `summary` (a notice turn's summary; a launch naming a `title` is one), `reading` (a line's full
    reading, kept on its line and sent to no road), `narration` (a document's sound; a launch naming a `document`
    is one);
  - `title?`: what a summary is about, in one line;
  - `message?`: the line a reading voices, or the line a voice answers;
  - `awaits_line?`: true where a summary awaits the line its round has yet to send;
  - `document?`: the artifact document a narration is kept with;
  - `emotion?`: one of the emotions of `policy/voice.toml`; a word outside them is refused;
  - `agent?`: the registered agent whose voice speaks, the settings' `agent` where none is named;
  - `via?`: the road the turn that launched it answers on.
- **Refusals before anything is opened:** 400 `bad_launch` (a field missing or of the wrong kind), 400
  `nothing_to_say`, 400 `nobody_named` (no `started_by`), 422 `unknown_emotion`, 409 `switched_off` (the voice
  briefing's switch stands off), 409 `voiceless` (the agent's registration names no voice).
- **A summary of the same room still waiting** when a newer one is launched is closed as `superseded`.

### The listener stream

`GET /briefings/events` is what a listener holds open: the console's briefing player, a face display, any client
that plays briefings. It carries only these frames, each with an id a client resumes from:

| Event | Data |
|---|---|
| `briefing` | `chat`, `message` (the voice message), `text` (the words spoken), `title?`, `seconds`, `sent_at`, `emotion?`, `face?` (the face the face unit's entry answers for the emotion), `autoplay` |
| `briefing-stopped` | `chat`, `message` |
| `presence` | `state` (`writing`, `thinking` or `idle`) and `since` |

- **A keep frame** (a comment line) goes every five seconds, so a client that hears nothing for twenty knows the
  connection dropped.
- **`autoplay`** is the voice briefing's autoplay switch as it stands when the frame is filed, read from the
  receiver; a client never reads the switch itself.
- **`presence`** is worked out from the turn events the receiver's stream carries: any turn writing is
  `writing`, otherwise any turn running is `thinking`, otherwise `idle`. It is filed once a stream has the
  picture and again only when the state moves, and carries nothing of any room.

### A line's script reading

| Route | Request | Answer |
|---|---|---|
| `POST /scripts/reading` | `text`, a line as a room received it; `full?`, true for a full reading | `script`, the line made a script for speaking; `spans`, the spans the reading gate would be asked about, each `n`, `text` and `code` (true where it was code); `polish?`, for a full reading, the script as the polishing gate is shown it, with each piece of code as a numbered placeholder |

- It reads the line by the voice's own script rules (`docs/voice-plan.md`, 「A line is spoken as a script」) and
  asks no gate. The gate's evaluation set reads its voice members' items through it.

### The voice's policy values

| Route | Answer |
|---|---|
| `GET /policy` | `run_cap_ms`, `emotions`, `failed_kind`: the values of `policy/voice.toml` |

- The voice owns these values; a unit above it that needs one reads it here.

### A briefing heard, from its own runner

| Route | Request | Answer |
|---|---|---|
| `POST /briefings/heard` | a `briefing` frame's data (「The listener stream」) | 204, the frame filed on the listener stream |

- A briefing's runner runs apart from the service, so it files the frame of what it put in the room here, with the
  service's own token. No other unit calls it.

### The unit command

`bmo-voice` is the command `bmo` hands the voice's words to (bmo-cli's boundary, 「Unit commands」). Asked
`nouns`, it prints `voice say`, `voice brief` and `runner voice`; each noun's two words, or its one-word name, run
it.

| Words | Name and alias | What it does |
|---|---|---|
| `voice say --text <words> [--chat <chat>] [--voice <name>]`, or `--file <sound> [--caption <words>]` | `voice-say`, `voice` | says one line out loud in a room while the caller waits, or sends a sound file it is handed; the room is the owner's private chat where none is named |
| `voice brief --chat <chat> --by <who> [--text <words>] [--emotion <emotion>] [--title <title>] [--document <id>] [--via <road>]` | `voice-brief` | launches a briefing as `POST /briefings` does, the words on standard input where `--text` names none, the agent the one `BMO_TURN_AGENT` names, and ends once the run is up |
| `runner voice <chat> <words-file> [<run> [<unit>]] [--reply-to <message>] [--voices <line>] [--no-transcript] [--emotion <emotion>] [--summary] [--keep] [--document <id>] [--agent <agent>] [--via <road>]` | `voice-runner` | the run a launch started: reads, speaks and sends the line, and closes its run |

- Exit 0 on success, 1 on a refusal, which is printed on standard error as its English sentence, 2 on words that
  do not read.
- **A briefing's run** is opened in agency's ledger under the agent `voice` and launched there as this command's
  `voice-runner`, with `BMO_VOICE_SETTINGS` naming the settings the launcher read.

### The node scripts and the card adapter

- **`weights/voice.card.toml`** declares the voice server to the card arbiter, its adapter the node scripts'
  `voice-node.ts adapter` (`docs/voice-plan.md`, 「The voice nodes」).
- **The adapter answers** the arbiter's verbs `install`, `uninstall`, `start`, `stop`, `health` and `placed`, run
  as `<adapter words> <verb> --node <node> --placed <node>,…`, its last line one JSON object: `ok`, `node`,
  `state` (`running`, `stopped`, `absent` or `failing`), `detail`, and for `placed` the `nodes` in order.

### The tool entries

`synth/tool.toml` declares the voice's entries for the system alone, called through the tool host's control
socket (bmo-pocket's `docs/api.md`, 「The control socket」): `voice/mouth`, `voice/encoder`, `voice/transcriber` and
`voice/briefing` (`docs/voice-plan.md`, 「The voice as a tool」).

## What the voice calls

Each call goes to the unit's address under `[units]` with the unit's token as a bearer where it names a token file,
in the conventions that unit's document gives the route's family. A refusal is read as the unit's document gives
it, carried as the voice's error naming the unit, its status, its code and its sentence; a unit that does not answer
is reported with the failure's own words, and the briefing it served fails as `docs/voice-briefing.md` says.

### bmo-gate (`docs/boundary-api.md`, 「For the voice unit: the voice gates」 and 「For the console」)

| Call | Route | Used for |
|---|---|---|
| one run of a gate | `POST /stages/{stage}/runs` with `prompt`, `chat?`, `message?`, `agent?`, `thinking`, `alongside` | the reading gate, `voice-reading`, and the polishing gate, `voice-polish`, each with the stage's own instructions; `answer` null is no answer, 409 `stopped` a stop, anything else that is no answer the gate's fault |
| a gate's options | `GET /options/{key}` | whether the polishing gate is switched on: off only where `enabled` is false or zero |
| the sanitiser | `POST /sanitised` with `text`, `label`, `chat?` | a briefing's words and a voice message's caption made safe to hand on, before anything is made of them; where the gate does not answer, the words go on as they are and the run's log says so |

### bmo-receiver (`docs/boundary-api.md`)

| Call | Route | Used for |
|---|---|---|
| voice a notice | `POST /rooms/{chat}/notices` with `fact`, `intent`, `follow_up?`, `optional`, `kind?`, `expires_at?`, `events`, `rides_until?` | a briefing that did not go out told to its room by the room's voice; answers the instruction's `id` |
| the switch | `GET /voice-briefing` | `on` and `autoplay`: whether briefings are on, and whether they play by themselves |
| a room's answerers | `GET /rooms/{chat}/answerers` | `voice`, the agent that speaks for a room |
| the owner | `GET /owner` | `chat`, the owner's private chat, where a line nobody asked for goes |
| a send with files | `POST /say-file`, a form of `chatId`, `text?`, `replyTo?` and the `file` | a voice message put in a room, its reading as the caption; answers `messageId`, `sent` and `failed` |
| a message | `GET /rooms/{chat}/messages/{message}` | `text`, the line a full reading voices; 404 for a message the receiver does not keep |
| the event stream | `GET /events` | the turn events presence is worked out from |
| a room's name | `GET /console/rooms/{chat}/title` | `title`, what a briefing's run is called where a person watches it |
| a message changed | `POST /event` with `type` `message`, `chatId` and `messageId` | a screen showing the room reads the line a briefing voices again once the briefing's run ends |

### bmo-agency (`docs/api.md`)

| Call | Route | Used for |
|---|---|---|
| the agents | `GET /agents` | each agent's `name` and the `voice` its row names, or none |
| open a run | `POST /runs` with `agent`, `chat_id?`, `unit`, `depth`, `cap_ms?`, `started_at`, `started_by?` | the ledger row of a briefing's run; answers its `id` |
| launch it | `POST /runs/{id}/launch` with `title`, `agent` and `command` (`program`, `args`, `cwd`, `environment`) | the briefing's runner started in its agent's host |
| close it | `POST /runs/{id}/close` with `finished_at` and `outcome` | how the run ended: `done`, `failed`, `refused`, `superseded`; a run closed already, 409, keeps its closing |
| its clock | `GET /runs/clock?run=` | `capEnds` and `keptUntil`: how long a run past its cap is kept, before a line is cut |
| a note | `POST /log/notes` with `agent`, `run`, `chat?`, `text`, `state?` | what the runner tells its run's log on its way |

### bmo-artifact (`docs/api.md`, 「Documents (HTTP)」)

| Call | Route | Used for |
|---|---|---|
| a document | `GET /api/archive/{id}` | a narration's title |

### bmo-node (`docs/api.md`)

| Call | Route | Used for |
|---|---|---|
| a node's target | `GET /nodes/{name}/target` | how a voice node is reached: `kind`, then an ssh node's `host`, `port`, `user`, `keyPath?` and `shell?`, or a hub node's `hubUrl`, `tokenFile` and `agent` |
| a node's login | `POST /nodes/login` with `node` | `ssh`, the arguments that open a batch SSH login to an ssh node, or null |

- A refusal of the node service is its answer: `answered` false and a `refusal` carrying its `code` and `message`,
  carried as the voice's error like any other.

### bmo-arbiter (`docs/api.md`)

- **The declaration** `weights/voice.card.toml` and **the adapter verbs** above.
- **The turn client** `turn.py` on each node, which the voice server holds a turn with, declaring
  `BMO_CARD_SERVICE=voice`; **the card lock** `~/.gpu.lock`.

### bmo-pocket (`docs/api.md`, 「The control socket」)

| Call | Route | Used for |
|---|---|---|
| an entry described | `GET /catalogue/{namespace}/{name}` | the capabilities the entry asks, which the call's record holds |
| a system entry | `POST /calls` with `session` (a record of `line` `voice`, those capabilities, `dataRoot` and a privileged turn), `tool` `face/expression` and its `arguments` | the face a briefing shows for its emotion, filed on its `briefing` frame |

- The control socket is the path `[units.pocket]`'s `address` names; it takes no token.

### bmo-web (`docs/boundary.md`, 「sumida's own routes」)

| Call | Route | Used for |
|---|---|---|
| sign-in | `POST /sumida/auth/session`, then `DELETE /sumida/auth/session` | the script studio's sign-in: a name and a password the account service takes are a login; the studio keeps its own session after |

## Codes

| Code | Status | Meaning |
|---|---|---|
| `unauthorised` | 401 | no token, or another one |
| `bad_launch` | 400 | a launch whose fields do not read |
| `nothing_to_say` | 400 | a launch with no words |
| `nobody_named` | 400 | a launch naming nobody who started it |
| `unknown_emotion` | 422 | an emotion outside the policy's |
| `switched_off` | 409 | the voice briefing's switch stands off |
| `voiceless` | 409 | the agent's registration names no voice |
| `no_briefing` | 404 | no briefing of that run |
| `no_sound` | 404 | no kept sound for that run |

## Contract gaps

Statements here a sibling's document does not yet match, each for that sibling to take up:

- **bmo-receiver** sends a launch with `kind` `notice` or `summary` and no `started_by`; the voice reads such a
  launch's `started_by` as `notice:<chat>` or `chat:<chat>` until it sends one. It keeps its own copy of
  `run_cap_ms`, `failed_kind` and `emotions` in its `chat-round.toml`, which it is to read from `GET /policy`. Its
  port reads a listed briefing's `kind` as `notice` or `summary` only and its `title` as never null, so a full
  reading listed with `kind` `full` and no title does not read there until it takes both.
- **bmo-receiver's `POST /say-file`** takes a caption and files but says nothing of sending an OGG/Opus file as a
  voice message, of folding a caption past a road's limit into replies, or of the road a line nobody asked for
  goes on; the voice sends its briefings there and leaves those to the receiver's roads. Nor does any route say
  before a send whether a room's road carries a voice, so a road that carries none is met at the send and told to
  the room as a briefing that did not go out.
- **bmo-receiver's sends** name no room-file rule for a sound file `voice say` is handed, so the voice sends what it
  is handed and the receiver's roads keep their own rules.
- **bmo-artifact** sends `chat_id` and no `kind`; the voice reads a body naming a `document` as a narration. It
  serves no route to attach a narration to a document, so a narration's sound is kept by the voice and served at
  `GET /briefings/{run}/sound` until it does.
- **bmo-agency** stands a run's program in its agent's host; a briefing's run names the agent `voice`, which is
  to stand as agency's registration for the voice's runs.
- **bmo-node** serves no route that runs a command on a hub node; the voice reaches a hub node through the hub
  itself, from the target's `hubUrl`, `tokenFile` and `agent`: the hub's `POST /exec?agent=` with the bearer token
  the token file holds under `JAKE_CONTROL_TOKEN`, its answer a server-sent stream of `started`, `stdout`,
  `stderr`, `exit`, `error` and `ended` events.
- **The script studio's data root** is shared with the song bench's; the studio keeps its records under the
  voice's own folder and the bench reads none of them.
