---
name: bmo-voice
description: Turn text into speech in a trained voice, transcribe spoken files, read a text into a script for the ear, or list the voices and voice servers a voice engine holds, through the bmo-voice MCP server. Use when a harness needs to speak a line in one of the engine's voices, take spoken material down, or prepare text as it is to be said.
---

# bmo-voice

The voice engine keeps trained voices and speaks text in them on the voice servers of the nodes it is
configured with. A synthesis is answered at once; the sound follows, and the member says when it is done.
The `bmo-voice` MCP server reaches the engine, one tool for each route, so a harness can speak a line with
nothing else running.

## Starting the server

Register the server with the harness as a stdio MCP server:

```
bmo-voice mcp
```

It calls the engine at the address in the voice settings, with the engine's token from its token file.
`--address <url>` and `--token <token>` name them instead.

## Speaking a line

1. `voice_voices` lists the trained voices: each one's `name`, `language`, `checkpoint`, `engine`, `served` and
   `emotions`. Choose a voice by its name, among those `served`; one that is not is refused `retired-voice`.
2. `voice_synthesize` with `{ "voiceName": "<name>", "text": "<the words>", "emotion": "<one of its emotions>" }`.
   `text` is the written words, what people read of the line. `spoken`, optional, is the words the voice says:
   the same words as a reader of the voice's language says them aloud, every foreign name, acronym, product and
   model name in that language's letters, every number, date and unit in words, and no address, path, code or
   markup. Write them with judgement and pass them, or leave them out and the engine derives them. `emotion`,
   `style` and `seed` are optional. The answer is the synthesis: its `id` and its state, `waiting` or
   `speaking`.
3. `voice_synthesis` with `{ "synthesisId": <id> }` until the state is `done` or `failed`. A `done`
   synthesis carries `durationMs` and its `spoken` words. One that has not ended says what it waits on in
   `wait` (its words, its card or its node) and in the sentence `waitingOn`. Keep reading it while you wait for
   it: one its caller has not read for two minutes fails as its caller gone. A `failed` one says why in
   `failure`.
4. The sound is the synthesis's `sound` route of the engine's API, `GET /syntheses/{synthesisId}/sound`, an
   OGG/Opus answer that honours `Range`. The MCP server names no tool for the bytes; read them through the
   engine's client or its address.

A synthesis of a text and a voice already asked for is answered with a new member each time; nothing is
cached. A voice that stands at no node's server answers `not-found` at `voice_synthesize`, and a synthesis
that could not be spoken ends `failed`.

## Taking spoken files down

`voice_transcribe` with `{ "language": "ko", "files": ["/path/one.ogg", "/path/two.ogg"] }`. The files stand
on the engine's own machine, as paths it can read. The answer is the transcription with its `id` and state;
`voice_transcribe` answers at once with state `waiting`, so read it again through `GET
/transcriptions/{transcriptionId}` until the state is `done` or `failed`, then its `text` is what was heard,
one file's words to a line, in the order given. There is no MCP tool for reading one transcription; the
engine's client has `transcription(id)`.

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
  one. Write that word in the voice's language and ask again.
- `unreadable-source`: an encoding's wav is not a file the engine can read.
- `unauthorized`: the token is missing or wrong.
- `failed`: the engine failed; the sentence is the failure.

An error with no `code` means the engine could not be reached at all.

The routes behind each tool are stated in the engine's `docs/boundary-api.md` and `docs/openapi.json`,
whose `x-tools` lists every tool's input and output.
