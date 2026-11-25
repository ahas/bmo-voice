# Working in this repository

## Principles

(a) each service runs on its own, with no context bleeding in from other repositories: it builds, tests and runs
from its own repository, data and settings, and never reads another repository's code, files, data folders or
tables; (b) services talk to each other only through documented APIs (HTTP routes, events, a documented command),
never by importing another service's code; (c) references run one way: lower code uses the base, and the base
never knows what is below it (no per-provider branch such as `if telegram`, no list of the units below, no
dependency on any service); where two repositories must know the same policy value, the lowest service in the
dependency direction owns it and the ones above read it through that service's API; and each repository holds
only what it uses itself, since the same thing kept in several places is the wrong structure (archive document
#301, 「원칙」).

This repository is `bmo-voice`: the voice engine, which keeps trained voices, speaks a text in one of them on the
voice servers of the voice nodes, transcribes spoken files, encodes recordings and reads a text into a script for
the ear; the scripts that install, push and train voices; the script studio where the lines a voice is trained on
are recorded; the briefings, each a line spoken for a room and kept with its room and its line, and the events
their listeners play them from; and the harness plugin that offers the engine to a coding harness. When a briefing
is spoken, and what it says, is its caller's to decide.

## The documents come first

- `docs/` is the canonical statement of what the voice does; the code is one realisation of it. Before changing
  code, read the document covering that area, `docs/layout.md` first. `docs/boundary-api.md` is every route,
  event and command the voice serves and every one it calls; `docs/inventory.md` is what came from the old
  repository.
- A change that would make a document wrong or incomplete repairs the document first, stating what is true now,
  and the code is written to match. A change to the boundary moves no version: the version is the set's, read
  off the git tags at build time, as `docs/boundary-api.md` 「Versions」 says.
- A document states what is true now: no change history, no rationale told through an earlier design, no pasted
  source, signature or config fragment, and no file-and-line pins.

## Boundaries

- **Other units are reached only through their published routes and commands,** as `docs/boundary-api.md`
  states. Nothing here imports another unit's code or reads its files, its paths or its database.
- **Dependencies:** crates and packages from their registries, pinned in their manifests, and of other
  repositories only the base libraries `bmo-chat`, `bmo-chat-client`, `bmo-ui` and `bmo-text`.
- **No agent is known by name.** The code handles a registered agent by its id; its name, label and voice come
  from the registration and the voice's own tables.
- **A variation belongs to its member,** never to a central table or a branch on a name.
- **No hand-written parser for a general format:** TOML, YAML, JSON, CSV, subtitles, command lines and their kin
  are read by a maintained library; where none fits, the reason is written beside the code.

## Language

- Everything here is English: code, comments, strings, documents and commit messages. The voice answers in
  English sentences and codes; words a person reads in another language are a console's language pack.
- Korean stands only where the code handles Korean text itself: the reading tables a script is spoken from, the
  voice names' spoken forms, the material a prompt cannot do without, and the subtitles and line records the
  script studio reads. It stands in data files loaded by language under `data/`, or written plainly where the
  code reads Korean text, and every such line is recorded in `korean-ledger.json`. It is never written as escape
  sequences or character codes.
- Every model prompt is English; the text it is handed stays in its own language.
- No personal data: no user names, home directories, machine names, domains, network addresses or names of any
  agent. What the voice needs at runtime comes from its settings and the other units' answers.

## Weights

- Trained voices, the Fish S2-Pro base weights and the retired VoxCPM2 voices' clips are runtime data and are never
  committed. `weights/checkpoints.toml` declares where they stand, `synth/engine.toml` pins the engine's code and
  base weights, and `weights/voice.card.toml` declares the voice
  server to the card arbiter.

## Checking a change

- `cargo check --all-targets`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all --check` for the
  crate; `bunx tsc --noEmit` in each Bun project; `python3 -m py_compile` for the Python under `synth/`.
- Tests stand in `tests/`, never beside the code. The testbed's `personal-data` check holds the
  personal-data rule (`docs/PERSONAL-DATA.md`).
