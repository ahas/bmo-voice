---
name: bmo-voice
description: How a chat agent's own voice, another voice and a written sound are made with the hosted voice tools, and how the bmo-voice MCP tools speak a text in a trained voice, transcribe spoken files, read a text into a script for the ear and list the voices, with the arguments each takes and what each error means.
---

# bmo-voice

The voice engine keeps trained voices and speaks text in them on the voice servers of the nodes it is
configured with. A synthesis is answered at once; the sound follows, and the member says when it is done.

## Which tool makes which sound

- **Your own voice in the room you answer** is `bmo/speak`, the receiver's: called with no argument it says your
  round's answer in the neutral voice, `--arg emotion=<a feeling>` only where the person asks for one, and the
  answer goes out as a voice note in your voice. Write that answer as you would say it aloud.
- **A sound that is not you speaking** (another voice, words read in a voice that is not yours, a sample) is
  `voice/generate`: `bmo use voice/generate --json '{"voiceName": "<voice>", "text": "<the words>"}'`, with
  `emotion`, `language` or an absolute `path` where needed. It answers the file it wrote; send that file into the
  room with `bmo/file` and no caption, your words about it in your answer. It is generated media, never you
  speaking: your face does not move for it. A voice it does not know is refused with the voices it has.
- **Which voices the nodes hold loaded** is `voice/servers`, with no arguments.
- **A run that took a line in under its own key writes its sound** with `worker/voice_sound`:
  `{"synthesisId": <id>, "key": "<its key>", "path": "<absolute path>"}`.
- An answer marked `isError` is a refusal: read its words for what to do next.

The MCP tools below reach the engine directly, for a harness that holds the `bmo-voice` server.

## Speaking a line

1. `voice_voices` lists the trained voices: each one's `name`, `language`, `checkpoint`, `engine`, `served` and
   `emotions`. Choose a voice by its name, among those `served`; one that is not is refused `retired-voice`.
2. `voice_synthesize` with `{ "voiceName": "<name>", "text": "<the words>", "emotion": "<one of its emotions>" }`.
   `text` is the written words, what people read of the line. `spoken`, optional, is the words the voice says:
   the same words as a reader of the line's language says them aloud, every foreign name, acronym, product and
   model name in that language's letters, every number, date and unit in words, and no address, path, code or
   markup. Write them with judgement and pass them, or leave them out and the engine derives them. `language`,
   `ko` or `en`, is optional: left out, a line with more English words than Korean is said in English and any
   other in Korean, and a line of neither in the voice's own. `emotion`, `style` and `seed` are optional. `key`, optional, is your own name for the line: asked again with
   the same `key` while its synthesis has not failed, that synthesis answers instead of a new one. The answer is the synthesis: its `id` and its state, `waiting` or
   `speaking`.
3. `voice_synthesis` with `{ "synthesisId": <id> }` until the state is `done` or `failed`. A `done`
   synthesis carries `durationMs` and its `spoken` words. One that has not ended says what it waits on in
   `wait` (its words, its card or its node) and in the sentence `waitingOn`. Keep reading it while you wait for
   it: one its caller has not read for two minutes fails as its caller gone. A `failed` one says why in
   `failure`.
4. The sound is the synthesis's `sound` route of the engine's API, `GET /syntheses/{synthesisId}/sound`, an
   OGG/Opus answer that honours `Range`. The MCP server names no tool for the bytes; read them through the
   engine's client or its address.

A call naming no `key` makes a new synthesis each time, even of a text and a voice already asked for; one naming
a `key` already asked answers that synthesis while it has not failed, so a retry never speaks the line twice. A voice that stands at no node's server answers `not-found` at `voice_synthesize`, and a synthesis
that could not be spoken ends `failed`.

## Taking spoken files down

`voice_transcribe` with `{ "language": "ko", "files": ["/path/one.ogg", "/path/two.ogg"] }`. The files stand
on the engine's own machine, as paths it can read. The answer is the transcription with its `id` and state;
`voice_transcribe` answers at once with state `waiting`, so read it again with `voice_transcription` and
`{ "transcriptionId": <id> }` until the state is `done` or `failed`, then its `text` is what was heard, one file's
words to a line, in the order given.

## Reading a text as it is to be said

`voice_script` with `{ "text": "<the text>", "language": "ko" }` answers the `script`, the text as the
engine's own reading rules make it speakable, and its `sentences` in order. It asks no model and touches no
card.

## When a tool answers an error

A tool error's text is JSON: `status`, `code` and `error`, the engine's English sentence. Match on `code`:

- `not-found`: no voice, synthesis or transcription stands at the name or id.
- `bad-request`: the arguments are not the shape the tool takes; the sentence names what is wrong.
- `unknown-emotion`: the emotion is not one the voice offers; `voice_voices` lists them.
- `unconverted-run`: the `spoken` given holds a word a voice cannot say as written; the sentence names the first
  one. Write that word in the line's language and ask again.
- `unreadable-source`: an encoding's wav is not a file the engine can read.
- `unauthorized`: the token is missing or wrong.
- `failed`: the engine failed; the sentence is the failure.

An error with no `code` means the engine could not be reached at all.

The routes behind each tool are stated in the engine's `docs/boundary-api.md` and `docs/openapi.json`,
whose `x-tools` lists every tool's input and output.
