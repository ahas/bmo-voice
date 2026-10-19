# Layout

How the repository is laid out, what each part is, and where it runs. Read this first.

## The folders

| Folder | What it holds |
|---|---|
| `client/` | The console plugin bundle: the briefing player and its settings section at `/briefing`, a Nuxt layer on bmo-ui and a Bun project of its own |
| `data/` | Language material the code reads by language, `data/<language>/`: the reading tables a script is spoken from, the voice names' seed readings, the material the training lines' prompt cannot do without. Every line of it is in `korean-ledger.json` |
| `deploy/` | The units the testbed's deployment keeps on the operations machine, and the proxy locations of the script studio |
| `docs/` | The documents, which come before the code |
| `policy/` | The voice's own policy values, `policy/voice.toml`, served at `GET /policy` |
| `prompts/` | What the voice's own model runs are told: the training lines' writer. English only |
| `scripts/` | The node and training scripts, a Bun project of its own: the voice node install, push, sync, health, speak and card adapter, a voice installed and checked, the voice names, training stages placed on a card, the training lines written |
| `src/` | The crate: the settings, the store, the script rules, the clients of the other units, synthesis and the voice nodes, the briefing runtime, the routes, the script studio's server, and the `voice-server`, `bmo-voice` and `script-studio` binaries |
| `studio/` | The script studio's screen (`studio/client/`, a Nuxt project) and its media tools (`studio/lib/` and `studio/scripts/`, Bun and Python), with their records' shared fixtures |
| `synth/` | What speaks: the VoxCPM2 mouth and voice server, the encoder, the transcriber, the intelligibility measure, the tool entries and their schemas, and the frozen package list of the voice environment |
| `tests/` | Every test of the repository, whatever its language |
| `weights/` | The declarations of the voice's weights: the voice server's card declaration and where the voices and the base model stand; never the weights themselves |

A folder stands only once something is in it. The README's folder table names every top-level folder that
stands, and no other.

## The rules the layout keeps

- **One manifest at the root,** `Cargo.toml`, for the one crate. A Bun or Python project keeps its own manifest
  and lockfile in its own folder, and is a project of its own to the testbed.
- **One crate,** split into modules. A part gets a crate of its own only where it truly has to stand apart.
- **No folder is named after its parent.** A folder's name says what it holds.
- **Tests live in `tests/`,** never beside the code they test.
- **A client of another service is one module named for that service:** `src/units/<service>.rs` in the crate,
  `scripts/lib/<service>.ts` in the scripts. Such a client is temporary, standing until that service publishes its
  own, so nothing of it is spread over the modules that use it.
- **The build output is kept small:** the root manifest's profiles keep debug information down to line tables, no
  incremental state, and a release stripped of debug information.

## The three binaries

- **`voice-server`:** the service. It reads the settings, runs the store's migrations and serves the routes of
  `docs/boundary-api.md`.
- **`bmo-voice`:** the unit command `bmo` hands the voice's words to: `voice say`, `voice brief` and `runner voice`.
  A briefing's runner is this binary, started by agency in its agent's host.
- **`script-studio`:** the script studio's server (`docs/script-studio.md`).

## Settings

- **One TOML file,** `settings.toml` at the top of the release the running binary stands in, or the file
  `BMO_VOICE_SETTINGS` names. The deployment links it from the data root's `env/voice.toml`.
- **What it holds** is `docs/boundary-api.md`, 「Settings」: where the service listens and is reached, its token
  file, the database, the data root, the agent that speaks where nothing names another, and the address and token
  file of each unit it calls.
- **Every token** stands in its own file under the data root's `env/`, never in the settings, a log, a command
  line or an environment variable.
- **The node and training scripts** read the same file, and the Python under `synth/` is handed what it needs on
  its command line or standard input.

## Where things run

- **The operations machine** runs the service, the unit command, the script studio's server and the node and
  training scripts, from the testbed's deploy tree (`docs/deploy.md`).
- **Each voice node** runs a voice server, `synth/voice-vox.py` on the voice environment, as the user unit
  `bmo-voice.service` the node scripts write there (`docs/voice-plan.md`, 「The voice nodes」).
- **A training node** runs a training stage as a transient user unit the training scripts start there
  (`docs/voice-plan.md`, 「Where the voice is trained」).

## Tests

Tests are deferred: the repository was moved with its code only, and its tests are written and run once the
minimal services stand on bmo-next. `docs/inventory.md` lists the old tests as the record of the behaviour to keep.
