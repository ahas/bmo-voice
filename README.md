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
| `docs/` | The documents, which come before the code: start with `docs/layout.md` |
| `src/` | The crate: the settings, and the `voice-server`, `bmo-voice` and `script-studio` binaries |
| `weights/` | The declarations of the voice's weights: the voice server's card declaration and where the voices and the base model stand; never the weights themselves |

## Tests

Tests are deferred. This repository was moved from the old single repository with its code only; its tests are
written and run on bmo-next once the minimal services are up there, from the old tests `docs/inventory.md` lists
as the record of the behaviour to keep. Until then a build check is the only verification.

## Checking a change

- `cargo check --all-targets`: the static check.
- `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings`: the review.

## Version

The version stands in `VERSION` and in the crate's manifest, which hold the same number.
