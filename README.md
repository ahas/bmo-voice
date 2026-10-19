# bmo-voice

The voice, as one service of its own:

- **briefings:** a script read into speakable words through the gate's reading and polishing stages, spoken in
  an agent's trained voice on a voice node, and sent to a room as a voice message and to the console's listeners;
  a direct voice message, `voice say`; a line's full reading; a document's narration;
- **the voice servers:** a VoxCPM2 server with each voice's LoRA weights, on every voice node, placed by the card
  arbiter and installed, updated and checked by this repository's node scripts;
- **voice training:** the lines a voice is trained on, written in the agent's manner, training stages placed on a
  free card, a trained voice checked and installed;
- **the script studio:** the screen and server where the lines a voice is trained on are recorded from the
  original material;
- **the briefing player:** the console's player as a plugin bundle.

Every other unit is reached through standard APIs alone (`docs/boundary-api.md`), and of other repositories only
the base libraries `bmo-chat`, `bmo-chat-client`, `bmo-ui` and `bmo-markup` may be depended on.

## Folders

| Folder | What it holds |
|---|---|
| `client/` | The console plugin bundle: the briefing player and its settings section at `/briefing`, a Nuxt layer on bmo-ui (`docs/layout.md`, 「The console plugin bundle」) |
| `data/` | Language material the code reads by language, `data/<language>/`; every Korean line of it is in `korean-ledger.json` |
| `deploy/` | The units the testbed's deployment keeps, the voice service and the script studio, and the studio's proxy locations |
| `docs/` | The documents, which come before the code: start with `docs/layout.md` |
| `policy/` | The voice's own policy values, `policy/voice.toml` |
| `prompts/` | What the voice's own model runs are told: the training lines' writer, in English |
| `scripts/` | The node, install and training scripts, a Bun project of its own: the voice node install, push, sync, health, speak and card adapter, a voice installed and checked, the voice names, training steps placed on a card, the training lines written |
| `src/` | The crate: the settings, the store, the script rules, the clients of the other units, synthesis and the voice nodes, the briefing runtime, the routes, the script studio's server, and the `voice-server`, `bmo-voice` and `script-studio` binaries |
| `studio/` | The script studio's media tools, a Bun project with Python scripts beside it (`studio/lib/`, `studio/scripts/`, `studio/fixtures/`), and its screen, `studio/client/`, a Nuxt project on bmo-ui |
| `synth/` | What speaks: the VoxCPM2 mouth and voice server, the encoder, the transcriber, the intelligibility measure, the tool entries and their schemas, and the frozen package list of the voice environment |
| `weights/` | The declarations of the voice's weights: the voice server's card declaration and where the voices and the base model stand; never the weights themselves |

## Tests

Tests are deferred. This repository was moved from the old single repository with its code only; its tests are
written and run on bmo-next once the minimal services are up there, from the old tests `docs/inventory.md` lists
as the record of the behaviour to keep. Until then a build check is the only verification.

## Checking a change

- `cargo check --all-targets`: the static check.
- `bunx tsc --noEmit` in `scripts/`: the scripts' static check.
- `bunx nuxi typecheck` in `studio/client/` and in `client/`: the studio screen's and the plugin bundle's static checks.
- `bunx tsc --noEmit` in `studio/` and `python3 -m py_compile studio/scripts/*.py`: the studio tools' static checks.
- `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings`: the review.

## Version

The version stands in `VERSION` and in the crate's manifest, which hold the same number.
