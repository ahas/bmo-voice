# bmo-voice

The voice engine, as one service of its own:

- **the voices:** trained voices kept by name, with the model, reference clip and continuation prompts each
  speaks with;
- **synthesis:** a text spoken in a trained voice on one of the voice nodes, the sound following the answer
  at once;
- **transcription and encodings:** spoken files taken down in one language, and a reference recording
  written out as a sound;
- **script readings:** a text made speakable by the engine's own reading rules;
- **the voice servers:** a VoxCPM2 server with each voice's LoRA weights on every voice node, taking its
  card turn through the card arbiter's own client, installed and updated by this repository's node scripts;
- **voice training:** the lines a voice is trained on, written in the agent's manner, training stages placed
  on a free card, a trained voice checked and installed;
- **the script studio:** the screen and server where the lines a voice is trained on are recorded from the
  original material, with its Korean language pack in this repository;
- **briefings:** a line spoken for a room and kept with its room, its line and its moment, told on the event
  stream for its listeners to play; its caller decides when one is spoken and what it says;
- **the harness plugin:** the `bmo-voice mcp` MCP server and the `bmo-voice` skill;
- **the hosted tool:** `voice/servers` on the tool host, for a session kept off the engine's port, answering
  which voice nodes are listed and whether their servers answer.

Every other unit is reached through standard APIs alone (`docs/boundary-api.md`), and of other repositories
only the base libraries `bmo-chat`, `bmo-chat-client`, `bmo-ui` and `bmo-text` may be depended on.

## Folders

| Folder | What it holds |
|---|---|
| `data/` | Language material the code reads by language, `data/<language>/`; every Korean line of it is in `korean-ledger.json` |
| `deploy/` | The units the testbed's deployment keeps, the voice service and the script studio, and the studio's proxy locations |
| `docs/` | The documents, which come before the code: start with `docs/layout.md` |
| `.graph/` | The code dependency graph: one node per feature with its files and the features it needs, and the APIs this repository exposes and calls, written only by `bmo graph` |
| `plugin/` | The harness plugin's skill, `plugin/skills/bmo-voice/` |
| `policy/` | The engine's own policy values, `policy/voice.toml` |
| `prompts/` | What the engine's own model runs are told: the training lines' writer, in English |
| `scripts/` | The node and training scripts, a Bun project of its own: the voice node install, push, sync, health, speak and card adapter, a voice installed and checked, the voice names, training steps placed on a card, the training lines written |
| `src/` | The crate: the settings, the store, the event log, the client, the MCP server, the OpenAPI document, the script rules, the voice nodes, the routes, the script studio's server, and the `voice-server`, `bmo-voice` and `script-studio` binaries |
| `studio/` | The script studio's media tools, a Bun project with Python scripts beside it (`studio/lib/`, `studio/scripts/`, `studio/fixtures/`), and its screen, `studio/client/`, a Nuxt project on bmo-ui |
| `synth/` | What speaks: the VoxCPM2 mouth and voice server, the encoder, the transcriber, the intelligibility measure, the tool entries and their schemas, and the frozen package list of the voice environment |
| `tests/` | Every test of the repository's crate, whatever its language: the repository-wide personal-data check is one (`docs/PERSONAL-DATA.md`) |
| `tools/` | The voice's hosted tools, each a crate of the workspace with its manifest, schema and tests: `tools/servers/`, the `voice/servers` entry a session reads the voice servers through |
| `weights/` | The declarations of the voice's weights: the voice server's card declaration and where the voices and the base model stand; never the weights themselves |

## Tests

Tests live in `tests/`, never beside the code, and a hosted tool's in its own folder's `tests/`. The repository-wide personal-data check stands there
(`tests/personal_data.rs`, `docs/PERSONAL-DATA.md`): no tracked file holds a mail address, a home path, a
telephone number, a token or a private network address. `docs/inventory.md` records the behaviour the tests are
to keep.

## Personal data

No file of the repository holds personal data. A value belongs to the installation's settings, an example uses
a neutral name, and `tests/personal_data.rs` holds the rule `docs/PERSONAL-DATA.md` states.

## Checking a change

- `cargo check --workspace --all-targets`: the static check, of the crate and the hosted tools.
- `cargo test --test personal_data`: the repository-wide personal-data check.
- `bunx tsc --noEmit` in `scripts/`: the scripts' static check.
- `bunx nuxi typecheck` in `studio/client/`: the studio screen's static check.
- `bunx tsc --noEmit` in `studio/` and `python3 -m py_compile studio/scripts/*.py`: the studio tools' static
  checks.
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`: the review.

## Version

The version stands in `VERSION` and in the root manifest's workspace, which hold the same number.
