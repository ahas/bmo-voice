# voice/generate

A sound that is not you speaking: another voice, words read in a voice that is not yours, a sample to listen to.
Your own voice in a room is `bmo/speak`, never this tool.

    bmo use voice/generate --json '{"voiceName": "<voice>", "text": "<the words>"}'

- `voiceName` (required): a served voice. A name no list holds is refused with the served voices named;
  `voice/servers` says which voices each node holds loaded.
- `text` (required): the words as written; the engine derives how the voice says them, so write names and numbers
  as you would write them.
- `emotion`: one of the voice's emotions (default: the voice's own).
- `language`: `ko` or `en` (default: the language the words are in).
- `path`: an absolute path to write the file at (default: a file under the temporary folder).

The answer names the OGG/Opus file it wrote. Send it into the room with `bmo/file` and no caption, and put your
words about it in your answer. It is generated media: it drives no face and plays nowhere by itself. An answer
marked `isError` is a refusal: read its words for what to do next.
