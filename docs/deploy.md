# Deployment

How a commit of this repository goes live. The testbed deploys it from the repository's own declaration,
`testbed.toml`; nothing here deploys itself.

## What a deployment does

- **The release:** the testbed builds the crate's three binaries and the hosted tool's into a release's `bin/`,
  installs the Bun projects' packages (`scripts/`, `studio/` and `studio/client/`, the declaration's setup), links
  the settings from the data root's `env/voice.toml` as the release's `settings.toml`, and, before the flip,
  generates the script studio's screen into `studio/client/.output/public`, where the studio's server serves it
  from. The screen is generated in Korean, from the pack this repository holds, `studio/client/i18n/locales/ko.yaml`.
- **The hosted tool:** before the flip, the release's `voice-servers-tool`, the manifest and the schema of
  `tools/servers/` are copied into its `.placed/` and put into the tool host's registry for this service with
  `room.tools` allowed (`docs/boundary-api.md`, 「The hosted tool」). The tool host reads the registry's copy, so
  a session sees the new tool with no restart.
- **The units:** the files under `deploy/`, each kept as a user unit by the testbed and marked for it:
  `bmo-voice.service`, the voice service, the one whose health the deployment waits on; and `bmo-script.service`, the
  script studio's server on port 3214, restarted by every deployment just before the voice service. Each starts only
  where the data root holds `env/voice.toml`.
- **Health:** the deployment waits until `bmo-voice health` answers 0 (`docs/boundary-api.md`, 「The command
  line」) and the studio's API answers 401 without a session and its screen HTML.
- **After health:** `scripts/voice-node.ts sync` brings every voice node up to date, pushing only where a node's stamp
  differs (`docs/voice-plan.md`, 「Building and updating them」); a failure is a warning, never a rollback.
- **A deployment's own voice and names** are set once by hand, never carried by the repository: the first trained
  voice with `scripts/voice-install.ts --add` (`docs/voice-plan.md`, 「The voices」), and the readings of the names
  the deployment's agents and owner go by with `scripts/voice-names.ts --set`, beside the generic seed names a new
  table is given.
- **The persona the training lines are written in** is the deployment's own file, kept outside every repository and
  handed to `scripts/voice-lines.ts run --persona` (`docs/voice-lines.md`, 「The speech standard」): who the agent is
  and the world it lives in, how it talks and names itself, the people it may name, and a few invented example lines,
  a long line among them.
- **The proxy:** `deploy/nginx.conf` holds the script studio's two locations, the bare path redirected to the one with
  the slash and everything under it passed to the studio's port, with the request's host and scheme handed on, which
  the studio's origin check and its secure cookie read. A deployment never touches the proxy; the path and the port
  are fixed and the locations stand in the host's server block.

## Where things stand

- **The deploy tree** is the testbed's, `~/.local/share/bmo-voice/deploy`, under the root `testbed.toml` names; the
  units run from it and never from a checkout.
- **The data root** is the one `testbed.toml` names; the voice keeps everything under its `voice/` folder
  (`docs/store.md`).
- **A failed step** stops the deployment before the units are restarted, so what is served has not moved.
