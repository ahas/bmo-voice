# The voice engine's boundary

Fixed at 0.1.0-rev.15.

Every route this engine answers, its event stream, the calls it makes, its harness plugin, its hosted tool,
its command line and its settings. The engine is an upstream: a coding harness takes it as its first entry
point through its MCP server, with no console and no other unit running. The engine keeps briefings, each a
line spoken for a room and kept with that room, its line and its moment, and tells its listeners when one is
spoken; when a briefing is spoken, and what it says, is its caller's decision, and speaking to listeners on a
schedule is a caller's work too.

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
  `checkpoint` (where its weights stand), `engine` (the engine its folder's `voice.toml` names, `voxcpm2` for a
  folder that names none), `served` (true where that engine is the one the voice speaks, the policy's `engine`),
  `emotions` (the emotions it may be asked to speak with) and `changedAt` (epoch milliseconds of its last change).
- **The cursor is the last voice's `name`.** A name no voice holds is answered 404 `not-found`.
- **The emotions are the engine's policy values** (`policy/voice.toml`, 「The policy」), the same for every
  voice.

## Syntheses

| Route | Body or query | Answer |
|---|---|---|
| `POST /syntheses` | `voiceName`, `text`, `spoken?`, `language?`, `emotion?`, `style?`, `seed?`, `key?` | 201, the synthesis; 200, the one its `key` already names |
| `GET /syntheses/{synthesisId}` | | one synthesis |
| `GET /syntheses/{synthesisId}/sound` | | the sound, OGG/Opus, with `Range` |

- **A synthesis:** `id`, `voiceName`, `text` (the written words), `spoken` (the words the voice says, null until
  they stand; 「The spoken words」), `emotion`, `key` (the caller's key, null where none was given), `state`,
  `waitingOn`, `durationMs` (null until done) and `createdAt`. Its `state` is `waiting` (taken in, its card not yet granted), `speaking` (a voice
  server is speaking it), `done` (its sound stands) or `failed`.
- **A synthesis is answered at once;** the speaking runs apart on the voice server of the card the arbiter's
  master grants it (「The voice servers and the card」), and the caller reads the member again until its
  state is `done` or `failed`. `done` carries `durationMs`, the sound's length read from its Ogg pages.
- **A synthesis taken in with no `spoken` has them derived first** (「How the engine derives the spoken words」),
  before any card is asked; it is `waiting` meanwhile, and `waitingOn` says how the derivation stands.
- **A synthesis waits for its card however long the grant takes:** nothing ends the wait and nothing refuses
  it as busy. While it waits, `waitingOn` is an English sentence saying what on: the master's reason for its
  card (every card in use, a card held by a claim or a lower tier, the service starting), its place in the
  line, or the master not reached. It is null once the card is granted, and on a synthesis that ended.
- **`text` must hold something to say,** in at most 10,000 characters, and a `spoken` given holds at most 30,000;
  either longer is answered 400 `bad-request`. A `spoken` given that holds nothing, or that fails the check, is
  answered 422 `unconverted-run` (「The spoken words」). A voice no list holds is answered 404 `not-found`; a voice
  that is not served is answered 422 `retired-voice`; an `emotion` outside the voice's is answered 422
  `unknown-emotion`. `style` and `seed` are laid over the voice's row for this one synthesis.
- **A line taken in is spoken whatever its length.** Its voice server is handed the spoken words whole and speaks
  them piece by piece into one sound (`docs/voice-plan.md`, 「What a long line takes of the card」), and the time it
  is given grows with them (「The policy」); no line within the bounds above is refused or cut off for its length.
- **The sound** is answered only once the state is `done`; before that, and where a failed synthesis left
  none, 404 `not-found`. A sound is kept under the data root, never in the database.
- **A line a stopped process left is taken up by the next.** As the service starts, every synthesis still
  `waiting` or `speaking` is spoken again from its words, in the language it was taken in with, its spoken
  words derived where none stand, and a briefing spoken by it is ended and told as its synthesis ends. One whose
  voice no list holds any more, or whose voice is not served, fails. Nothing a caller took in is left `waiting`
  with no process to speak it.
- **A key makes a line once.** `key` is the caller's own name for a line, of at most 200 characters and not blank,
  in whatever shape the caller keys its work by. A `POST` naming a key that a synthesis `waiting`, `speaking` or
  `done` already holds takes nothing in and is answered 200 with that synthesis, whatever else the body says; once
  the one holding it has `failed`, the key takes a new one in. Two calls with one key at once take in one synthesis
  between them. A key that is blank or longer is answered 400 `bad-request`.
- **One synthesis speaks one text.** There is no list route: a caller that wants to know a synthesis it
  started reads its member or listens to the stream.

## The spoken words

Every line the engine speaks, a synthesis's and a briefing's alike, is two texts, and no line reaches a voice
server any other way.

- **`text` is the written words:** what people read of the line, the caption, the reading a road folds under a
  voice note, a transcript. The engine keeps them as they were given, and a briefing's as the script its reading
  rules make of them (「Script readings」); nothing else changes them.
- **`language` is the language the line is said in,** `ko` or `en`, the voice's own where absent; any other is
  answered 400 `bad-request`. A voice says a line of either language, and the spoken words are made and checked
  in the line's.
- **`spoken` is the words the voice says:** the same words as a reader of the line's language says them aloud.
  Every foreign name, acronym, product name and model name stands in that language's reading, every number,
  date, time and unit in the words a reader says, and a web address, a file path, code and markup are said in a
  few words for what they are or left out. Nothing else differs from the written words.
- **A language model makes them, with judgement; no table and no rule of the engine's does.** Context decides how
  a name, an acronym or a number is read aloud, so no reading table, substitution or romanisation produces them,
  and the engine's own reading rules (「Script readings」, `docs/voice-plan.md`) unfold markup into the written
  words and never read them. A caller writes them itself and passes `spoken`, or passes `text` alone and the
  engine derives them (「How the engine derives the spoken words」).
- **The check:** before a line is spoken its spoken words are checked for what was left unconverted. It finds,
  and the engine never mends:
  - a letter of another script than the line's language writes: a Latin letter in a Korean line, a Hangul letter
    in an English one;
  - a digit, in every voice;
  - the marks a web address, a path, code or markup is written with: a slash or a backslash, and any of
    `` ` `` `*` `_` `#` `|` `<` `>` `[` `]` `{` `}` `~` `=` `^` `@` `$` `%` `&` `+`, and a character of the
    private use area.
- **A `spoken` a caller passed that fails the check is refused** 422 `unconverted-run`, the sentence naming the
  first word that failed and what it holds, so a caller that claims to have converted a line and did not is
  caught; it corrects that word and asks again. A derived one that fails is asked of the model again.

## How the engine derives the spoken words

- **On a seat of agency's co-worker pool,** asked for the role `[spoken]` names in the settings,
  `transliterator` where they name none, with the provider they name where they name one. The role is a
  registration of agency's (its `docs/agent-roles.md`) and must stand there; the engine creates none.
- **In one thrown-away session for the whole derivation,** on the runner line `[spoken]` names, `search` where it
  names none, its instructions the engine's own (`prompts/spoken-words.md`) and no tool allowed, bound to the
  seat. Each try is one turn in it, handing the model the line's language, the agreed readings, the written
  words, and, where the answer before failed the check, the word it failed on. The session is ended and the seat
  given back once the spoken words stand; a seat or a session agency no longer holds is given up and a new one
  asked, and no try leaves a session of its own standing.
- **The agreed readings come first, the model's judgement after:** every row of the voice name table
  (`docs/store.md`, `voice_names`) with a reading in the line's language is handed over as a reading to apply
  as it stands, and the model reads whatever else the words need with its own judgement.
- **A reading the model chose is kept:** its answer names each name it read that the readings did not hold, and
  each is written into the voice name table, so the same spelling is read the same way next time. A spelling
  the table holds is never written over: the owner's correction of a row (`scripts/voice-names.ts --set`) wins
  from then on. A chosen reading that holds what the check finds is not kept.
- **Every seat asked stands in the ledger** (`docs/store.md`, 「The folder」) until it is given back, and each one a
  process left there is given back, cancelled, as the next process starts: the pool lets its line in one at a time
  from its head, so a seat left waiting by a process that is gone would hold every later line behind it. A seat
  waiting while agency is not reached is asked about again, never left for a new one; one agency no longer holds,
  or given back before it was let in, is dropped and a new one asked.
- **Derived spoken words hold at most 30,000 characters,** the bound a caller's `spoken` is held to; a longer
  answer does not stand and is asked again, as one that fails the check is.
- **Nothing gives it up.** An answer that fails the check, one that does not read, a seat or a session agency
  refuses and agency not reached are each asked again five seconds later, however often; `waitingOn` says which
  it was and why, and the synthesis stays `waiting` until its spoken words stand. They are kept as its
  `spoken`, and its card is asked after.

## Briefings

| Route | Body or query | Answer |
|---|---|---|
| `POST /briefings` | `voiceName`, `text`, `spoken?`, `language?`, `chatId`, `messageId?`, `kind`, `title?`, `emotion?`, `plays?`, `key?` | 201, the briefing; 200, the one its `key` already names |
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
  words into a script by its own reading rules (「Script readings」), and the script is the written words of a
  synthesis in the named voice, `spoken` its spoken words where the caller gave them and derived where it did
  not (「The spoken words」), spoken on the voice server of the card granted it. `text` is the script. Its `state` and
  `durationMs` are its synthesis's, and `synthesisId` names it, whose `waitingOn` says what a waiting one waits
  on. `text` must hold something to say, within a synthesis's bounds (「Syntheses」); a `spoken` that is empty or
  fails the check is 422 `unconverted-run`, a voice no list holds is 404 `not-found`, a voice that is not served is
  422 `retired-voice`, and an emotion outside the policy's is 422 `unknown-emotion`.
- **A key makes a briefing once,** as it does a synthesis (「Syntheses」): the key is its synthesis's, and a `POST`
  naming a key the synthesis of a briefing holds, not yet `failed`, is answered 200 with that briefing. A key a
  synthesis holds that speaks no briefing is answered 409 `conflict`.
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
| `retired-voice` | 422 | the voice belongs to an engine the voice no longer speaks; it is kept and never spoken |
| `unreadable-source` | 422 | an encoding's wav is not a file the engine can read |
| `unconverted-run` | 422 | the spoken words given hold nothing, or hold what the check finds; the sentence names the first word that failed |
| `failed` | 500 | the engine failed; the sentence is the failure |

## The voice servers and the card

- **Each voice node runs `synth/voice-fish.py` on the engine environment** as the user unit
  `bmo-voice-server.service`, listening on the node's loopback and taking a line on the node's own shell
  (`docs/voice-plan.md`, 「The voice nodes」).
- **A voice server takes its turn on the node's card through the card arbiter's own turn client,** installed
  on the node by the arbiter, and holds the card lock while it speaks.
- **The card declaration** is `weights/voice.card.toml`: the service `voice`, standing on every node its placement
  names (`spread` `many`), not alone and at a priority below every service that stands alone on the GPU nodes, so it
  joins the reservation of the node it is placed on (`docs/voice-plan.md`, 「Where they run」), yielding the card by
  its turn, its adapter the node scripts' own. The master reads it from
  this repository's deploy tree (`docs/deploy.md`); no node is named in it.
- **Which node speaks a line is the arbiter's master's to say.** Every synthesis asks the master a demand for
  one card of the service `voice`, naming no node, with the work key `voice:synthesis:<synthesisId>` and a
  reference naming the synthesis and its voice. The line is spoken on the node of the card granted, reached as
  the node service's answer for that node says, and the demand is given back once the line ends, spoken or
  failed.
- **One read of the master serves every waiting line.** No waiting line holds a request of its own open: while any
  line waits, the engine reads every demand the master holds once a second, and each waiting line takes its
  demand's state from that read. However many lines wait, the master is read through one request at a time.
- **A line holds one demand at a time, and forgets it only on the master's word.** A read of every demand may
  have been taken before a line's demand was asked, so a demand the read does not hold is read again by its id,
  and only the master's `404` for that id is the demand no longer held; absence from a read is never enough. An
  ask whose answer did not arrive may have been taken in, so before it is asked again the master's demands are
  read for one of the line's work key: the first found is the line's demand, and any beyond it are given back.
- **Every line runs on the service's one runtime and through its one HTTP client,** whose idle connections are
  kept to a bound, so a waiting line holds no thread, poller or socket of its own.
- **The wait has no end but the grant.** While the master holds the demand in line, or cannot be reached, the
  synthesis stays `waiting`, its `waitingOn` saying why. A master that answers `404` for the demand, one
  restarted, is asked a new one. `voice` placed on no node (`no-place`) and any other refusal of the master's
  fail the synthesis at once, the demand it held given back.
- **A card held from under the line:** where the node's server answers that other work held its card past its
  own wait (503), the demand is read again until its card is granted again, and the line is asked of that
  card's node once more; nothing bounds that either.
- **Every demand the engine holds stands in its ledger** (`docs/store.md`, 「The folder」) from its asking until
  the master answers its give-back, `204` or `404`, or answers `404` for it by its id; a give-back the master
  refuses leaves it there.
- **The master is the truth at start.** Before any line of the new process asks, every demand the ledger holds
  is given back, and so is every demand the master holds whose work key is `voice:synthesis:<synthesisId>`, since
  no line of the new process holds one yet: what a process left behind, in the ledger or not.
- **A node out of reach is never quietly replaced by this machine's card:** the synthesis fails, naming the node
  and what it met.

## What the engine calls

### bmo-node

| Call | Route | Used for |
|---|---|---|
| a node's target | `GET /nodes/{name}/target` | how a voice node is reached: `kind`, then an ssh node's `host`, `port`, `user`, `keyPath?`, `shell?` and `sshArgs`, the arguments that open a batch SSH login ahead of the command, or a hub node's `hubAddress`, `hubTokenFile` and `agentName` |

A refusal of the node service is read as its own answer, `answered` false and a `refusal` carrying `code` and
`message`, and carried as the engine's failure.

### bmo-arbiter

The arbiter's master (its `docs/api.md`, 「Demands」).

| Call | Route | Used for |
|---|---|---|
| a demand | `POST /demands` with `serviceName` `voice`, `count` 1, `workKey` and `ref` | a card of `voice` for one synthesis: `201` granted, `202` waiting, `422` `no-place` where `voice` is placed on no node |
| the demands read | `GET /demands` | every demand the master holds: read once a second while any line waits, each waiting line's demand as it stands; read for a line's work key where its ask was not answered; and read once as the service starts, for the demands of `voice:synthesis:` work keys an earlier process left |
| a demand read | `GET /demands/{demandId}` | a line's demand as it stands where a read of every demand did not hold it, or once other work held its card from under the line; `404` alone is the master holding it no more |
| the demand given back | `DELETE /demands/{demandId}` | `204`, or `404` where it is given back already |

- **A granted card** is the first of `cards` on a demand `granted` with nothing `taken`; its `nodeName` is the
  node that speaks the line.
- **The master not reached,** an answer of 502 or 503 and an answer that does not arrive are all asked again a
  second later, however often.

### bmo-agency

The co-worker pool and its thrown-away sessions (its `docs/api.md`, 「pool, seat-requests, reservations and roles」
and 「sessions」), for the spoken words a synthesis is derived (「How the engine derives the spoken words」).

| Call | Route | Used for |
|---|---|---|
| a seat | `POST /seat-requests` with `requestKey` and `owner` `voice:synthesis:<synthesisId>:<n>`, `n` counting the seats the line was asked, `role`, `selection` `auto`, `workKey` `role:<role>` and `provider` where the settings name one | the seat the derivation runs on, with the provider, model, effort and tier its role's key resolves to |
| its admission and its end | `PATCH /seat-requests/{token}` | asked with no state until it is `active`; bound to the session with `bound`; given back with `finished` and its outcome, `cancel` where no turn ran, and every seat an earlier process left in the ledger given back cancelled as the service starts |
| a session | `POST /sessions` with `scope` `apart`, `line`, `systemPrompt`, `toolsAllowed` empty and the seat's provider and model | the derivation's thrown-away session |
| its turns | `POST /sessions/{sessionId}/turns` with `prompt` and a work turn's parts: `provider` and `model` the seat's, `mapping` its `model`, `effort` and `tier`, `resume` `fresh`, `cwd` empty, `name`, `title`, `chat` 0 (the derivation stands under no room), `workKey` `role:<role>`, `hearsWords` false and `variables` empty | one try, answered once it ends; its answer is the model's |
| its end | `DELETE /sessions/{sessionId}` | the session ended once the spoken words stand; one still busy is asked again |

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

## The hosted tool

- **`voice/servers` is a tool of the tool host** (bmo-pocket's `docs/mcp-host.md`, 「Placed tools」): a session
  the shell guard keeps off every unit's loopback port, the chat agent among them, reads through it which voice
  nodes stand in the voice node list and whether their servers answer, since the tool host runs it outside every
  harness. It is the folder `tools/servers/`, the crate `voice-servers-tool`, built into the release and placed
  before the flip for this service, with the capability its entry asks for allowed (`docs/deploy.md`).
- **Its one entry** is `voice/servers`, asking for `room.tools`, read-only and taking no arguments. The executable
  speaks the tool host's call protocol 1 itself: one request on standard input, one tool result of text on
  standard output.
- **What it answers:** every page of `GET /voice-servers`, called through the engine's own client, as one line a
  voice server in the node list's order, with its node's name, its `state` and the voices it holds loaded; then a
  line saying where a line is spoken now (`docs/voice-plan.md`, 「Which road speaks a line」): on the voice server
  of the listed node the arbiter's master grants a card of `voice`, where any is listed, and, where none is,
  nowhere, since a synthesis fails and nothing speaks on the operations machine's card.
- **Its settings** are the file `BMO_VOICE_SETTINGS` names, else `settings.toml` under the link of the voice's
  deploy tree, `~/.local/share/bmo-voice/deploy`: the tool host runs the registry's copy of the placed folder,
  which stands in no release. It calls the settings' address with the settings' token.
- **What it refuses,** each as an answer marked as an error: settings or a token that do not read, with the
  sentence saying why; an engine that does not answer, or one that refuses, carrying the status, code and sentence
  of the refusal, or the sentence saying why nothing answered, as the harness plugin's failures do; and a call of
  another entry, naming its own. It serves no resources, and a kind of request it does not answer ends the call
  unclean.

## The command line

`bmo-voice` serves the harness plugin, the document and the ear and mouth groups' first-class verbs: it
declares the nouns `listen` and `brief` when asked `nouns`, so the platform hands `bmo listen` and
`bmo brief` straight to it.

| Word | What it does |
|---|---|
| `nouns` | answers `listen` and `brief`, one a line |
| `listen <file>…` | asks `POST /transcriptions` for the files in `--language` and waits, up to `--wait` seconds, for it to end; prints what was heard, or the transcription as JSON with `--json` |
| `brief` | asks `POST /briefings` for a line spoken in `--voice` for the room `--chat`, of kind `--kind` (`summary` or `full`), with `--text`, and optional `--spoken`, `--title`, `--emotion`, `--message` and `--plays`; waits, up to `--wait` seconds, for the engine to speak it, then prints its id, or the briefing as JSON with `--json` |
| `health` | asks the service the settings name for `GET /health`, with its token, and exits 0 where it answers ready, 1 otherwise |
| `mcp` | runs the harness plugin's MCP server; `--address` and `--token` name the engine |
| `openapi` | prints `docs/openapi.json` as the repository keeps it |

## The policy

`policy/voice.toml` holds the engine's own policy values, read once at start: `run_cap_ms` and
`run_ms_per_character`, which together say how long one line may take to speak once its card is granted (the first,
and the second for each character of its spoken words), `emotions`, the emotions a voice may be asked to speak
with, and `engine`, the engine every served voice belongs to (`fish-s2-pro`). No route serves them; the voices list
them, and a synthesis outside them is refused.

## Settings

The service and the command read one TOML file, `settings.toml` at the top of the release or the file
`BMO_VOICE_SETTINGS` names; the hosted tool reads the same file through the deploy tree's link (「The hosted
tool」).

| Key | Meaning |
|---|---|
| `listen` | where the service listens |
| `address` | the service's own address, as the command and the plugin call it |
| `token_file` | the file under the data root's `env/` holding the token callers present |
| `database` | the address of the engine's own database (`docs/store.md`) |
| `data_root` | the runtime data root; everything the engine keeps stands under its `voice/` folder |
| `synth` | optional, the folder of the engine's Python side, `synth/` where it is not in the release |
| `[units.node]` | `address` and `token_file?` of the node service |
| `[units.arbiter]` | `address` and `token_file?` of the arbiter's master, which grants every synthesis its card |
| `[units.web]` | `address` and `token_file?` of the account service the script studio signs in against |
| `[units.agency]` | optional, `address` and `token_file?` of agency, whose co-worker pool derives the spoken words; a line with no `spoken` waits, its `waitingOn` saying so, while none is named |
| `[spoken]` | optional, how the spoken words are derived: `role`, the agency role the seat is asked for (`transliterator`), `line`, the runner line its session goes out on (`search`), and `provider?`, the provider the seat is asked on |
| `[studio]` | optional, the script studio's deployment values (`docs/script-studio.md`, 「Access and service」) |

Every token stands in its own file under the data root's `env/`, never in the settings, a log, a command
line or an environment variable.

## The script studio

The script studio has its own server and screen (`docs/script-studio.md`). It is not part of the engine's
upstream API: its screen speaks a language pack the repository holds, its API refuses with its own codes,
and its server calls the engine's synthesis and reading rules through the crate rather than HTTP.

## Versions

- **The repository has one version,** in `VERSION` and the root manifest's workspace, shared by the engine, its
  client, its plugin, its hosted tool and its OpenAPI document.
- **Before its first release it is `0.1.0` and counts revisions:** the first `0.1.0` is the base, and every
  later bump is one revision, written `0.1.0-rev.<n>` with `n` counting from 1. Major, minor and patch never
  move before the first release, a breaking change included.
- **The first release drops the suffix.** From then on the version moves by semver; a removed or renamed
  route or field, or a field that becomes required, is a major version.
