**English** | [한국어](README.ko.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

# bmo-voice

The voice engine, as one service of its own:

- **the voices:** trained voices kept by name, each a Fish S2-Pro adapter that speaks with no reference clip;
  the retired VoxCPM2 voices kept as data and never served;
- **synthesis:** a text spoken in a trained voice on one of the voice nodes, the sound following the answer
  at once;
- **transcription and encodings:** spoken files taken down in one language, and a reference recording
  written out as a sound;
- **script readings:** a text made speakable by the engine's own reading rules;
- **the voice servers:** a Fish S2-Pro server, the voice's adapter folded into the weights it loads, on every
  voice node, taking its
  card turn through the card arbiter's own client, installed and updated by this repository's node scripts;
- **voice training:** the lines a voice is trained on, written in the agent's manner, training stages placed
  on a card through the card arbiter's queue, a trained voice checked and installed;
- **the script studio:** the screen and server where the lines a voice is trained on are recorded from the
  original material, with its Korean language pack in this repository;
- **briefings:** a line spoken for a room and kept with its room, its line and its moment, told on the event
  stream for its listeners to play; its caller decides when one is spoken and what it says;
- **the harness plugin:** the `bmo-voice mcp` MCP server and the `bmo-voice` skill;
- **the hosted tool:** `voice/servers` on the tool host, for a session kept off the engine's port, answering
  which voice nodes are listed and whether their servers answer.

Every other unit is reached through standard APIs alone (`docs/boundary-api.md`), and of other repositories
only the base libraries `bmo-chat`, `bmo-chat-client`, `bmo-ui` and `bmo-text` may be depended on.

## Models

The repository holds no weights. A voice node needs:

| What | Where it comes from | Licence |
|---|---|---|
| Fish Audio S2-Pro, the base weights | [`fishaudio/s2-pro`](https://huggingface.co/fishaudio/s2-pro) at the revision `synth/engine.toml` pins; `setup` carries it to each node | Fish Audio Research License: research and non-commercial use only |
| fish-speech, the engine's code | [`fishaudio/fish-speech`](https://github.com/fishaudio/fish-speech) at the commit `synth/engine.toml` pins | its own |
| Whisper, for the transcriber and for measuring a voice | [`openai/whisper-large-v3-turbo`](https://huggingface.co/openai/whisper-large-v3-turbo) | MIT |
| A trained voice | yours, trained by the scripts here on your agent's recorded lines; or BMO's own, [`ahas-io/bmo-voice`](https://huggingface.co/ahas-io/bmo-voice), copied under the voices folder | as its base: Fish Audio Research License |

**Built with Fish Audio.** The code here is MIT; the S2-Pro weights and every voice trained on them are not, and a
voice spoken for a paying service, a business's own operations or a hosted API needs Fish Audio's own licence
first (`docs/voice-plan.md`).

## Folders

| Folder | What it holds |
|---|---|
| `data/` | Language material the code reads by language, `data/<language>/`; every Korean line of it is in `korean-ledger.json` |
| `deploy/` | The units the testbed's deployment keeps, the voice service and the script studio |
| `docs/` | The documents, which come before the code: start with `docs/layout.md` |
| `plugin/` | The harness plugin's skill, `plugin/skills/bmo-voice/` |
| `policy/` | The engine's own policy values, `policy/voice.toml` |
| `prompts/` | What the engine's own model runs are told: the training lines' writer, in English |
| `scripts/` | The node and training scripts, a Bun project of its own: the voice node install, push, sync, health, speak and card adapter, a voice installed and checked, the voice names, training steps placed on a card, the training lines written |
| `src/` | The crate: the settings, the store, the event log, the client, the MCP server, the OpenAPI document, the script rules, the voice nodes, the routes, a document's narration, the script studio's server, and the `voice-server`, `bmo-voice` and `script-studio` binaries |
| `studio/` | The script studio's media tools, a Bun project with Python scripts beside it (`studio/lib/`, `studio/scripts/`, `studio/fixtures/`), and its screen, `studio/client/`, a Nuxt project on bmo-ui |
| `synth/` | What speaks: the Fish S2-Pro mouth and voice server and the engine's pinned install (`engine.toml`), the encoder, the transcriber, the intelligibility measure, the tool entries and their schemas, and the frozen package list of the voice environment |
| `tests/` | Every test of the repository's crate and of its node scripts, whatever its language |
| `tools/` | The voice's hosted tools, each a crate of the workspace with its manifest, schema and tests: `tools/servers/`, the `voice/servers` entry a session reads the voice servers through |
| `weights/` | The declarations of the voice's weights: the voice server's card declaration and where the voices and the base model stand; never the weights themselves |

## Tests

Tests live in `tests/`, never beside the code, a hosted tool's in its own folder's `tests/` and the studio screen's in
`studio/client/tests/`, which its project's own check runs after its type check. The node scripts' tests,
`tests/*.test.ts`, are run by `bun test` from the repository's root and type-checked with the scripts. The testbed's `personal-data` check holds the personal-data rule
(`docs/PERSONAL-DATA.md`): no tracked file holds a mail address, a home path, a
telephone number, a token or a private network address. `docs/inventory.md` records the behaviour the tests are
to keep.

The Python tests (`tests/*.py`) run on an interpreter of their own, `tests/.venv`, which the declaration's setup
makes with `uv` from `tests/python-requirements.txt`, the voice environment's own versions of the packages they read,
so they run on whatever machine the suite is placed on; `BMO_VOICE_PYTHON` names another interpreter instead.

## Personal data

No file of the repository holds personal data. A value belongs to the installation's settings, an example uses
a neutral name, and the testbed's `personal-data` check holds the rule `docs/PERSONAL-DATA.md` states.

## Checking a change

- `cargo check --workspace --all-targets`: the static check, of the crate and the hosted tools.
- `bmo-testbed check personal-data`: the personal-data check.
- `bunx tsc --noEmit` in `scripts/`: the scripts' static check.
- `bun run check` in `studio/client/`: the studio screen's static check, then its tests.
- `bunx tsc --noEmit` in `studio/` and `python3 -m py_compile studio/scripts/*.py`: the studio tools' static
  checks.
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`: the review.

## Version

The version stands in `VERSION` and in the root manifest's workspace, which hold the same number.
