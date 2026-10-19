# Deployment

How a commit of this repository goes live. The testbed deploys it from the repository's own declaration,
`testbed.toml`; nothing here deploys itself.

## What a deployment does

- **The release:** the testbed builds the crate's three binaries into a release's `bin/`, installs the Bun projects'
  packages, generates the script studio's screen and the plugin bundle, and links the settings from the data root's
  `env/voice.toml` as the release's `settings.toml`.
- **The units:** the files under `deploy/`, each kept as a user unit by the testbed: `bmo-voice.service`, the voice
  service, and `bmo-script.service`, the script studio's server, which serves the screen generated into the release.
- **Health:** the deployment waits for the service's `GET /health` to answer and for the studio's API to answer 401
  without a session and its screen 200 with HTML.
- **After health:** `scripts/voice-node.ts sync` brings every voice node up to date, pushing only where a node's stamp
  differs (`docs/voice-plan.md`, 「Building and updating them」).
- **A deployment's own voice and names** are set once by hand, never carried by the repository: the first trained
  voice with `scripts/voice-install.ts --add` (`docs/voice-plan.md`, 「The voices」), and the readings of the names
  the deployment's agents and owner go by with `scripts/voice-names.ts --set`, beside the generic seed names a new
  table is given.
- **The persona the training lines are written in** is the deployment's own file, kept outside every repository and
  handed to `scripts/voice-lines.ts run --persona` (`docs/voice-lines.md`, 「The speech standard」): who the agent is
  and the world it lives in, how it talks and names itself, the people it may name, and a few invented example lines,
  a long briefing among them.
- **The proxy:** `deploy/nginx.conf` holds the script studio's two locations, the bare path redirected to the one with
  the slash and everything under it passed to the studio's port. A deployment never touches the proxy; the path and
  the port are fixed and the locations stand in the host's server block.

## Where things stand

- **The deploy tree** is the testbed's, under the root `testbed.toml` names; the units run from it and never from a
  checkout.
- **The data root** is the one `testbed.toml` names; the voice keeps everything under its `voice/` folder
  (`docs/store.md`).
- **A failed step** stops the deployment before the units are restarted, so what is served has not moved.
