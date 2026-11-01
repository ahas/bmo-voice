# Inventory

What this repository takes from the old single bmo repository, read at commit `3e4c032da`, and what it
leaves to others. Every file of the voice set is listed once, with where it lands or why it is not carried.

## How the set was read

- **The source** is the old repository's git index at commit `3e4c032da`, read with one `git ls-files` over
  these patterns:
  - `bot/bmo-server/src/verbs/voice/**`;
  - `tools/voice/**` (113 files);
  - `bot/bot/voice*` and `bot/bot/agent-voice*`;
  - `archive/archive/voice-store*` and `archive/archive/briefing-store*`;
  - `archive/bmo-archive/src/migration/m20261010_000001_voice_awaits_line.rs`;
  - `lang/bmo-lang/lang/ko/voice*` and `lang/bmo-lang/src/voice*`;
  - `prompts/agents/voice-*` and `docs/voice-*`;
  - the console's briefing files: `client/app/domain/briefing*`, `client/app/components/BriefingPlayer*`,
    `client/app/components/SettingsBriefing.vue`, `client/server/api/briefing/*`, `client/shared/briefings*`,
    `client/server/utils/presence*`, `client/server/utils/briefing-stream.ts`, `client/public/briefing-trial.ogg`;
  - `bot/bmo-server/tests/integration/voice_verbs.rs`, `bot/bmo-server/tests/fixtures/voice-lines.json` and
    `testbed/testkit/voice-language-cases.json`.
- **The set is 175 files.** Line counts are read from the files as they stand at that commit.
- **A step** is one task of this repository's build order, named by its number and title. The order is:
  1. Inventory and ownership;
  2. Repository skeleton;
  3. Canonical documents first;
  4. Voice store;
  5. Script rules;
  6. Sibling clients;
  7. Synthesis and nodes;
  8. Briefing runtime and verbs;
  9. HTTP service;
  10. VoxCPM server and tool entry;
  11. Node and install scripts;
  12. Voice training scripts;
  13. Script studio server;
  14. Script studio client;
  15. Script studio tooling;
  16. Briefing plugin bundle;
  17. Units and deploy declaration;
  18. Final review and build checks.
- **Not carried** means the file stays in the old repository or is already carried by the sibling named.

## Counts

| Where | Files |
|---|---|
| step 2, Repository skeleton (read for content, replaced by this repository's own) | 4 |
| step 3, Canonical documents first | 6 |
| step 4, Voice store | 3 |
| step 5, Script rules | 7 |
| step 7, Synthesis and nodes | 3 |
| step 8, Briefing runtime and verbs | 6 |
| step 10, VoxCPM server and tool entry | 13 |
| step 11, Node and install scripts | 10 |
| step 12, Voice training scripts | 4 |
| step 13, Script studio server | 10 |
| step 14, Script studio client | 33 |
| step 15, Script studio tooling | 22 |
| step 16, Briefing plugin bundle | 11 |
| step 17, Units and deploy declaration | 2 |
| carried or replaced, all steps | 134 |
| not carried: bmo-gate already holds it | 2 |
| deferred tests and test data | 39 |
| **all** | **175** |

Steps 6 and 9 carry no old file: they are new code (the clients of the siblings' routes, and the service's own
routes). Step 1 is this document and step 18 the review.

## Every file

| File | Lines | Where it lands |
|---|---|---|
| `archive/archive/briefing-store.test.ts` | 121 | deferred test or test data |
| `archive/archive/briefing-store.ts` | 172 | step 4, Voice store |
| `archive/archive/voice-store.test.ts` | 160 | deferred test or test data |
| `archive/archive/voice-store.ts` | 228 | step 11, Node and install scripts |
| `archive/bmo-archive/src/migration/m20261010_000001_voice_awaits_line.rs` | 34 | step 4, Voice store |
| `bot/bmo-server/src/verbs/voice/brief.rs` | 194 | step 8, Briefing runtime and verbs |
| `bot/bmo-server/src/verbs/voice/clock.rs` | 40 | step 7, Synthesis and nodes |
| `bot/bmo-server/src/verbs/voice/hand.rs` | 201 | step 8, Briefing runtime and verbs |
| `bot/bmo-server/src/verbs/voice/mod.rs` | 216 | step 8, Briefing runtime and verbs |
| `bot/bmo-server/src/verbs/voice/node.rs` | 973 | step 7, Synthesis and nodes |
| `bot/bmo-server/src/verbs/voice/runner.rs` | 912 | step 8, Briefing runtime and verbs |
| `bot/bmo-server/src/verbs/voice/script/cases/figures.json` | 139 | deferred test or test data |
| `bot/bmo-server/src/verbs/voice/script/cases/polish.json` | 93 | deferred test or test data |
| `bot/bmo-server/src/verbs/voice/script/cases/reading.json` | 184 | deferred test or test data |
| `bot/bmo-server/src/verbs/voice/script/cases/spoken.json` | 111 | deferred test or test data |
| `bot/bmo-server/src/verbs/voice/script/figures.rs` | 778 | step 5, Script rules |
| `bot/bmo-server/src/verbs/voice/script/mod.rs` | 9 | step 5, Script rules |
| `bot/bmo-server/src/verbs/voice/script/polish.rs` | 460 | step 5, Script rules |
| `bot/bmo-server/src/verbs/voice/script/reading.rs` | 1188 | step 5, Script rules |
| `bot/bmo-server/src/verbs/voice/script/spoken.rs` | 920 | step 5, Script rules |
| `bot/bmo-server/src/verbs/voice/synth.rs` | 387 | step 7, Synthesis and nodes |
| `bot/bmo-server/src/verbs/voice/voices.rs` | 251 | step 4, Voice store |
| `bot/bmo-server/tests/fixtures/voice-lines.json` | 10 | deferred test or test data |
| `bot/bmo-server/tests/integration/voice_verbs.rs` | 1423 | deferred test or test data |
| `bot/bot/agent-voice.test.ts` | 65 | deferred test or test data |
| `bot/bot/voice-node.test.ts` | 502 | deferred test or test data |
| `bot/bot/voice-node.ts` | 696 | step 11, Node and install scripts |
| `bot/bot/voice-prompt-check.test.ts` | 70 | deferred test or test data |
| `bot/bot/voice-prompt-check.ts` | 213 | step 11, Node and install scripts |
| `bot/bot/voice-train.test.ts` | 303 | deferred test or test data |
| `bot/bot/voice-train.ts` | 477 | step 12, Voice training scripts |
| `bot/bot/voice-transcript.test.ts` | 28 | deferred test or test data |
| `bot/bot/voice.test.ts` | 187 | deferred test or test data |
| `bot/bot/voice.ts` | 203 | step 11, Node and install scripts |
| `client/app/components/BriefingPlayer.test.ts` | 77 | deferred test or test data |
| `client/app/components/BriefingPlayer.vue` | 417 | step 16, Briefing plugin bundle |
| `client/app/components/SettingsBriefing.vue` | 75 | step 16, Briefing plugin bundle |
| `client/app/domain/briefing-position.test.ts` | 48 | deferred test or test data |
| `client/app/domain/briefing-position.ts` | 103 | step 16, Briefing plugin bundle |
| `client/app/domain/briefing.test.ts` | 588 | deferred test or test data |
| `client/app/domain/briefing.ts` | 690 | step 16, Briefing plugin bundle |
| `client/public/briefing-trial.ogg` | 776 | step 16, Briefing plugin bundle |
| `client/server/api/briefing/events.get.ts` | 31 | step 16, Briefing plugin bundle |
| `client/server/api/briefing/read.post.ts` | 20 | step 16, Briefing plugin bundle |
| `client/server/api/briefing/stop.post.test.ts` | 88 | deferred test or test data |
| `client/server/api/briefing/stop.post.ts` | 18 | step 16, Briefing plugin bundle |
| `client/server/utils/briefing-stream.ts` | 38 | step 16, Briefing plugin bundle |
| `client/server/utils/presence.test.ts` | 156 | deferred test or test data |
| `client/server/utils/presence.ts` | 142 | step 16, Briefing plugin bundle |
| `client/shared/briefings.test.ts` | 49 | deferred test or test data |
| `client/shared/briefings.ts` | 59 | step 16, Briefing plugin bundle |
| `docs/voice-briefing.md` | 609 | step 3, Canonical documents first |
| `docs/voice-lines.md` | 124 | step 3, Canonical documents first |
| `docs/voice-plan.md` | 2018 | step 3, Canonical documents first |
| `lang/bmo-lang/lang/ko/voice-script.yaml` | 212 | step 5, Script rules |
| `lang/bmo-lang/lang/ko/voice.yaml` | 50 | step 8, Briefing runtime and verbs |
| `lang/bmo-lang/src/voice.rs` | 40 | step 8, Briefing runtime and verbs |
| `lang/bmo-lang/src/voice_script.rs` | 129 | step 5, Script rules |
| `prompts/agents/voice-lines/instructions.md` | 125 | step 12, Voice training scripts |
| `prompts/agents/voice-polish/instructions.md` | 36 | not carried: bmo-gate already holds it (its prompts/) |
| `prompts/agents/voice-reading/instructions.md` | 40 | not carried: bmo-gate already holds it (its prompts/) |
| `testbed/testkit/voice-language-cases.json` | 58 | deferred test or test data |
| `tools/voice/.gitignore` | 10 | step 2, Repository skeleton |
| `tools/voice/AGENTS.md` | 91 | step 2, Repository skeleton |
| `tools/voice/CLAUDE.md` | 1 | step 2, Repository skeleton |
| `tools/voice/README.md` | 8 | step 2, Repository skeleton |
| `tools/voice/bmo/bunfig.toml` | 5 | step 11, Node and install scripts |
| `tools/voice/bmo/package.json` | 27 | step 11, Node and install scripts |
| `tools/voice/bmo/voice-cer-sentences.json` | 47 | step 10, VoxCPM server and tool entry |
| `tools/voice/bmo/voice-cer.py` | 235 | step 10, VoxCPM server and tool entry |
| `tools/voice/bmo/voice-encode.py` | 73 | step 10, VoxCPM server and tool entry |
| `tools/voice/bmo/voice-install-script.test.ts` | 334 | deferred test or test data |
| `tools/voice/bmo/voice-install.ts` | 377 | step 11, Node and install scripts |
| `tools/voice/bmo/voice-lines.test.ts` | 125 | deferred test or test data |
| `tools/voice/bmo/voice-lines.ts` | 716 | step 12, Voice training scripts |
| `tools/voice/bmo/voice-names-script.test.ts` | 54 | deferred test or test data |
| `tools/voice/bmo/voice-names.ts` | 73 | step 11, Node and install scripts |
| `tools/voice/bmo/voice-node-requirements.txt` | 170 | step 10, VoxCPM server and tool entry |
| `tools/voice/bmo/voice-node-script.test.ts` | 53 | deferred test or test data |
| `tools/voice/bmo/voice-node.ts` | 580 | step 11, Node and install scripts |
| `tools/voice/bmo/voice-prompt-check-lines.json` | 13 | step 11, Node and install scripts |
| `tools/voice/bmo/voice-script.test.ts` | 349 | deferred test or test data |
| `tools/voice/bmo/voice-tool.py` | 137 | step 10, VoxCPM server and tool entry |
| `tools/voice/bmo/voice-tool.test.ts` | 183 | deferred test or test data |
| `tools/voice/bmo/voice-train.ts` | 142 | step 12, Voice training scripts |
| `tools/voice/bmo/voice-transcribe.py` | 56 | step 10, VoxCPM server and tool entry |
| `tools/voice/bmo/voice-vox-server.test.ts` | 568 | deferred test or test data |
| `tools/voice/bmo/voice-vox.py` | 1404 | step 10, VoxCPM server and tool entry |
| `tools/voice/briefing.json` | 59 | step 10, VoxCPM server and tool entry |
| `tools/voice/bun.lock` | 62 | step 15, Script studio tooling |
| `tools/voice/client/app/app.config.ts` | 107 | step 14, Script studio client |
| `tools/voice/client/app/app.vue` | 18 | step 14, Script studio client |
| `tools/voice/client/app/assets/css/main.css` | 118 | step 14, Script studio client |
| `tools/voice/client/app/assets/fonts/CaskaydiaCoveNFM-Regular.subset.woff2` | 128 | step 14, Script studio client |
| `tools/voice/client/app/assets/fonts/LICENSE-CaskaydiaCove.txt` | 94 | step 14, Script studio client |
| `tools/voice/client/app/assets/fonts/LICENSE-Pretendard.txt` | 94 | step 14, Script studio client |
| `tools/voice/client/app/assets/fonts/Pretendard-Bold.subset.woff2` | 1042 | step 14, Script studio client |
| `tools/voice/client/app/assets/fonts/Pretendard-Medium.subset.woff2` | 1013 | step 14, Script studio client |
| `tools/voice/client/app/assets/fonts/Pretendard-Regular.subset.woff2` | 1047 | step 14, Script studio client |
| `tools/voice/client/app/assets/fonts/Pretendard-SemiBold.subset.woff2` | 1051 | step 14, Script studio client |
| `tools/voice/client/app/components/ContentSkeleton.vue` | 31 | step 14, Script studio client |
| `tools/voice/client/app/components/MessageIsland.vue` | 259 | step 14, Script studio client |
| `tools/voice/client/app/components/ScreenFrame.vue` | 15 | step 14, Script studio client |
| `tools/voice/client/app/components/ScriptDialogueList.vue` | 10 | step 14, Script studio client |
| `tools/voice/client/app/components/ShellClose.vue` | 12 | step 14, Script studio client |
| `tools/voice/client/app/components/SkeletonBar.vue` | 15 | step 14, Script studio client |
| `tools/voice/client/app/components/SkeletonLine.vue` | 17 | step 14, Script studio client |
| `tools/voice/client/app/components/SkeletonStatus.vue` | 20 | step 14, Script studio client |
| `tools/voice/client/app/components/SkeletonText.vue` | 16 | step 14, Script studio client |
| `tools/voice/client/app/domain/island.test.ts` | 89 | deferred test or test data |
| `tools/voice/client/app/domain/island.ts` | 222 | step 14, Script studio client |
| `tools/voice/client/app/domain/layers.ts` | 299 | step 14, Script studio client |
| `tools/voice/client/app/domain/loading.ts` | 13 | step 14, Script studio client |
| `tools/voice/client/app/domain/numbers.test.ts` | 18 | deferred test or test data |
| `tools/voice/client/app/domain/numbers.ts` | 15 | step 14, Script studio client |
| `tools/voice/client/app/domain/overlays.ts` | 254 | step 14, Script studio client |
| `tools/voice/client/app/domain/screen.ts` | 80 | step 14, Script studio client |
| `tools/voice/client/app/pages/index.vue` | 178 | step 14, Script studio client |
| `tools/voice/client/app/plugins/overlays.client.ts` | 7 | step 14, Script studio client |
| `tools/voice/client/app/plugins/painted.client.ts` | 6 | step 14, Script studio client |
| `tools/voice/client/bun.lock` | 2129 | step 14, Script studio client |
| `tools/voice/client/i18n/locales/ko.yaml` | 141 | step 14, Script studio client |
| `tools/voice/client/nuxt.config.ts` | 55 | step 14, Script studio client |
| `tools/voice/client/package.json` | 26 | step 14, Script studio client |
| `tools/voice/client/tsconfig.json` | 9 | step 14, Script studio client |
| `tools/voice/docs/deploy.md` | 69 | step 3, Canonical documents first |
| `tools/voice/docs/development.md` | 35 | step 3, Canonical documents first |
| `tools/voice/docs/script-studio.md` | 170 | step 3, Canonical documents first |
| `tools/voice/encoder.json` | 25 | step 10, VoxCPM server and tool entry |
| `tools/voice/fixtures/reviews.json` | 27 | step 15, Script studio tooling |
| `tools/voice/mouth.json` | 95 | step 10, VoxCPM server and tool entry |
| `tools/voice/package.json` | 16 | step 15, Script studio tooling |
| `tools/voice/scripts/asr-requirements.txt` | 3 | step 15, Script studio tooling |
| `tools/voice/scripts/bmo-script-download.py` | 74 | step 15, Script studio tooling |
| `tools/voice/scripts/bmo-script-import.ts` | 19 | step 15, Script studio tooling |
| `tools/voice/scripts/bmo-script-match.py` | 55 | step 15, Script studio tooling |
| `tools/voice/scripts/bmo-script-transcribe.py` | 36 | step 15, Script studio tooling |
| `tools/voice/scripts/bmo_script_audio.py` | 27 | step 15, Script studio tooling |
| `tools/voice/scripts/cargo.ts` | 33 | step 15, Script studio tooling |
| `tools/voice/scripts/deploy.ts` | 171 | step 15, Script studio tooling |
| `tools/voice/scripts/dev-data.ts` | 11 | step 15, Script studio tooling |
| `tools/voice/scripts/dev.ts` | 48 | step 15, Script studio tooling |
| `tools/voice/scripts/fixtures.ts` | 16 | step 15, Script studio tooling |
| `tools/voice/server/Cargo.lock` | 2698 | step 13, Script studio server |
| `tools/voice/server/Cargo.toml` | 60 | step 13, Script studio server |
| `tools/voice/server/src/accounts.rs` | 80 | step 13, Script studio server |
| `tools/voice/server/src/csv.rs` | 127 | step 13, Script studio server |
| `tools/voice/server/src/handler.rs` | 596 | step 13, Script studio server |
| `tools/voice/server/src/lib.rs` | 8 | step 13, Script studio server |
| `tools/voice/server/src/main.rs` | 80 | step 13, Script studio server |
| `tools/voice/server/src/review.rs` | 192 | step 13, Script studio server |
| `tools/voice/server/src/studio.rs` | 397 | step 13, Script studio server |
| `tools/voice/server/src/subtitles.rs` | 377 | step 13, Script studio server |
| `tools/voice/server/tests/contract.rs` | 516 | deferred test or test data |
| `tools/voice/services/bmo-script.service` | 16 | step 17, Units and deploy declaration |
| `tools/voice/services/nginx.conf` | 6 | step 17, Units and deploy declaration |
| `tools/voice/src/cargo-place.test.ts` | 28 | deferred test or test data |
| `tools/voice/src/cargo-place.ts` | 38 | step 15, Script studio tooling |
| `tools/voice/src/deploy-plan.test.ts` | 108 | deferred test or test data |
| `tools/voice/src/deploy-plan.ts` | 156 | step 15, Script studio tooling |
| `tools/voice/src/dev-data.test.ts` | 56 | deferred test or test data |
| `tools/voice/src/dev-data.ts` | 67 | step 15, Script studio tooling |
| `tools/voice/src/fixtures.test.ts` | 26 | deferred test or test data |
| `tools/voice/src/fixtures.ts` | 63 | step 15, Script studio tooling |
| `tools/voice/src/scratch.ts` | 16 | step 15, Script studio tooling |
| `tools/voice/src/script-download.test.ts` | 42 | deferred test or test data |
| `tools/voice/src/script-import.test.ts` | 13 | deferred test or test data |
| `tools/voice/src/script-import.ts` | 18 | step 15, Script studio tooling |
| `tools/voice/src/script-match.test.ts` | 16 | deferred test or test data |
| `tools/voice/src/script-studio.test.ts` | 37 | deferred test or test data |
| `tools/voice/src/script-studio.ts` | 66 | step 15, Script studio tooling |
| `tools/voice/tool.toml` | 66 | step 10, VoxCPM server and tool entry |
| `tools/voice/transcribed.json` | 27 | step 10, VoxCPM server and tool entry |
| `tools/voice/transcriber.json` | 26 | step 10, VoxCPM server and tool entry |
| `tools/voice/tsconfig.json` | 21 | step 15, Script studio tooling |

## Replaced rather than copied

Files whose work this repository does another way, each named with what does it now:

| Old file | What does its work here |
|---|---|
| `bot/bmo-server/src/verbs/voice/voices.rs` | the store's `voices` and `briefings` modules, on the voice's own database; its read of a document's title from the archive is the artifact unit's `GET /api/archive/{id}` |
| `archive/bmo-archive/src/migration/m20261010_000001_voice_awaits_line.rs` | the store's one migration, which makes `voice_briefings` with `awaits_line` among its columns |
| `archive/archive/briefing-store.ts` | the store's `briefings` module: the TypeScript reading of `voice_briefings` served the old archive facade alone |
| `lang/bmo-lang/src/voice_script.rs` | the script's `words` module, reading `data/ko/voice-script.yaml` by language |
| `lang/bmo-lang/lang/ko/voice-script.yaml` | `data/ko/voice-script.yaml` keeps its reading tables; its gate, reading and polish sentences are the script's English faults, each with a code |
| `bot/bmo-server/src/verbs/voice/script/{mod,reading,spoken,polish,figures}.rs` | `src/script/`, their colocated tests left out as deferred; a gate run is the script's own request handed to the gate's route with the stage's own instructions, and the prompts name the agent rather than an agent's name |
| `bot/bmo-server/src/verbs/voice/{synth,node}.rs` | `src/speech/`, their colocated tests left out as deferred; the mouth and the encoder run through the voice's own tool entry under `synth/`, a node is reached as the node service's target and login say, a hub node through its hub's exec route, the locks stand under the voice's folder and are taken with the standard library's file locks |
| `bot/bmo-server/src/verbs/voice/clock.rs` | `src/speech/clock.rs`, holding a speaking run to agency's clock for it rather than asking past its cap itself |
| `bot/bmo-server/src/verbs/voice/{mod,brief,runner,hand}.rs` | `src/briefing/` and the `bmo-voice` unit command, their colocated tests left out as deferred: the run in agency's ledger, its runner launched there, sends through the receiver, the sanitiser and the gates through the gate, the face through the tool host, the frame on the voice's own listener stream, and a full reading's or a narration's sound kept under the voice's folder |
| `lang/bmo-lang/src/voice.rs`, `lang/bmo-lang/lang/ko/voice.yaml` | English refusals with codes in `src/briefing/`, English run notes in the runner, and the unit command's own usage; the switch-change sentence stays with the receiver, whose switch it is |
| `tools/voice/tool.toml` | `synth/tool.toml`, named `synth` after its folder as the tool host requires, its entries still of the `voice` namespace; its `[card]` table is `weights/voice.card.toml`, and its after-deploy sync of the voice nodes is a step of the deployment (`docs/deploy.md`) |
| the old tool SDK's Python helper the build copied beside the tool | `synth/tool_protocol.py`, the voice's own answer of the tool host's documented call protocol 1, so the voice depends on no other repository's code |
| `archive/archive/voice-store.ts` | `scripts/lib/store.ts`, the same reads and writes of the voice's own tables on its Postgres database through Bun's SQL client |
| `bot/bot/voice.ts` | `scripts/lib/voice.ts`: the cap read from `policy/voice.toml`, the voice's entries run as `synth/voice-tool.py` directly rather than through the tool host, a sound's length read by `music-metadata`; the gate stage names, the transcript re-export and the voice environment's path, which no script reads, are left out, and the seed names are `data/ko/voice-names.toml` |
| `bot/bot/voice-node.ts` | `scripts/lib/voice-node.ts`: a node's target and login are the node service's (`scripts/lib/node.ts`), a hub node is reached through its hub (`scripts/lib/hub.ts`, `scripts/lib/node-shell.ts`), and a call is given up a grace past its time, since no script holds a run's clock; what only the old briefing runner used (a line taken back, stand-in node takers) is left out |
| `bot/bot/voice-prompt-check.ts` | `scripts/lib/voice-prompt-check.ts`, its lines read from `data/<language>/voice-prompt-check-lines.json` |
| `tools/voice/bmo/voice-node.ts` | `scripts/voice-node.ts`: paths name this repository's layout, the token is the voice folder's `voice.token`, the base model is read from the current voice's config, and the turn client is no longer pushed: the unit puts the arbiter's own installation on the server's import path; the node move check line is `data/ko/voice-node.toml` |
| `tools/voice/bmo/voice-install.ts` | `scripts/voice-install.ts`: the bootstrap of two named checkpoints from one experiment directory is `--add <name> --checkpoint <dir> --reference <clip.wav>`, the first voice of an empty table made current; `--use` moves the current mark alone, since agency states no route to write an agent's voice (`docs/boundary-api.md`, 「Contract gaps」) |
| `tools/voice/bmo/voice-names.ts` | `scripts/voice-names.ts`, its usage and listing in English |
| `tools/voice/bmo/voice-prompt-check-lines.json` | `data/ko/voice-prompt-check-lines.json` and `data/en/voice-prompt-check-lines.json`, the first Korean line no longer addressed to a person |
| `tools/voice/bmo/package.json`, `tools/voice/bmo/bunfig.toml` | `scripts/package.json` and `scripts/tsconfig.json`; the old bunfig held only test preloads, and the tests are deferred |
| `bot/bot/voice-train.ts` | `scripts/lib/voice-train.ts`: whether a card is free is the node card arbiter's answer rather than the driver's process list, a node without an arbiter takes no step, and the step runs inside the arbiter's turn client, holding the voice's turn and the card lock |
| `tools/voice/bmo/voice-train.ts` | `scripts/voice-train.ts`: every node is the node service's list, the record stands under the script studio's data root as `docs/script-studio.md` names it, and the choosing lock under the voice folder's `locks/` |
| `tools/voice/bmo/voice-lines.ts` | `scripts/lib/voice-lines.ts` and the command `scripts/voice-lines.ts`: the emotions are `policy/voice.toml`'s, the names the voice name table's, the kinds, bands, meta words and banned words the language material's, the Codex model, effort and id prefix the run's own flags, and its messages English |
| `tools/voice/server/Cargo.toml`, `tools/voice/server/Cargo.lock` | the root `Cargo.toml` and `Cargo.lock`, the studio's server a module of the one crate and `script-studio` its binary; the password hashing it no longer needs is gone, and the archive drivers with it |
| `tools/voice/server/src/{lib,main}.rs` | `src/studio/mod.rs` and `src/bin/script-studio.rs`: the data root's default is the voice folder's `studio/`, the generated screen's the release's `studio/client/.output/public`, and the deployment's own values the settings' `[studio]` section |
| `tools/voice/server/src/accounts.rs` | `src/units/web.rs`: a sign-in is checked by the web unit's sign-in route rather than read as a password hash out of the archive's `web_users` table |
| `tools/voice/server/src/csv.rs` | `src/studio/export.rs`, the rows written by the `csv` crate with the same columns, quoting and CRLF records |
| `tools/voice/server/src/studio.rs` | `src/studio/records.rs`: the folder the manifest's paths stand under is the settings' `source_root` rather than a fixed folder name |
| `tools/voice/server/src/review.rs` | `src/studio/review.rs`: a review that does not read is refused with a code and an English sentence |
| `tools/voice/server/src/subtitles.rs` | `src/studio/subtitles.rs`: the speaker label matching drops is the settings' `speakers` rather than a fixed name; the tracks' labels and the notes a match leaves stay Korean, for the reviewers |
| `tools/voice/server/src/handler.rs` | `src/studio/handler.rs`: every refusal answers a code and an English sentence, and an export names the settings' `source_url` as its source |
| `tools/voice/client/app/components/{ContentSkeleton,MessageIsland,ScreenFrame,ShellClose,SkeletonBar,SkeletonLine,SkeletonStatus,SkeletonText}.vue` | bmo-ui's components of the same names, which the studio's screen takes as a host of the layer |
| `tools/voice/client/app/domain/{island,layers,loading,numbers,overlays,screen}.ts`, `tools/voice/client/app/plugins/{overlays,painted}.client.ts` | bmo-ui's domain modules and page plugins of the same names, imported by the package's path |
| `tools/voice/client/app/app.config.ts`, `tools/voice/client/app/assets/css/main.css` and the six font and licence files under `tools/voice/client/app/assets/fonts/` | bmo-ui's app configuration, stylesheet and fonts, which the layer brings |
| `tools/voice/client/app/{app.vue,pages/index.vue,components/ScriptDialogueList.vue}` | `studio/client/app/`: the library's words in the language the screen speaks, a refusal shown as the pack's words for its code, the tags read from `data/ko/studio-tags.json`, and no character's or show's name in a key or a placeholder |
| `tools/voice/client/{nuxt.config.ts,package.json,tsconfig.json,bun.lock}` | `studio/client/`, extending bmo-ui from a packed copy under `vendor/` with its patch under `patches/`, the language chosen at generate time |
| `tools/voice/client/i18n/locales/ko.yaml` | `studio/client/i18n/locales/en.yaml`, its words in English and bmo-ui's keys beside them; the Korean pack is bmo-web's to hold and hand the deployment (`docs/boundary-api.md`, 「Contract gaps」) |
| `tools/voice/{package.json,tsconfig.json,bun.lock}` | `studio/package.json`, `studio/tsconfig.json` and `studio/bun.lock`, the studio's media tools a Bun project of their own beside its screen |
| `tools/voice/src/script-studio.ts` | `studio/lib/script-studio.ts`: a review that does not read is refused with the server's codes and English sentences, the folder the manifest's paths stand under is the settings' `source_root`, and the tags' defaults are `data/ko/studio-tags.json`'s |
| `tools/voice/src/script-import.ts` | `studio/lib/script-import.ts`, its evidence sentence from `data/ko/studio-notes.json` |
| `tools/voice/src/fixtures.ts`, `tools/voice/fixtures/reviews.json` | `studio/lib/fixtures.ts` and `studio/fixtures/reviews.json`: each refusal stands with its code and sentence, the tag values are the language material's, and no line or file is named for a character |
| `tools/voice/src/dev-data.ts`, `tools/voice/scripts/{dev-data,dev}.ts` | `studio/lib/dev-data.ts` and `studio/scripts/`: the copy is made from the studio's data root to a workbench beside it, and the bench starts the server from the repository's one crate, signing in against the web unit the settings name rather than an archive |
| `tools/voice/scripts/{bmo-script-import,fixtures}.ts` | `studio/scripts/`, their data root the studio's own |
| `tools/voice/scripts/bmo-script-download.py` | `studio/scripts/bmo-script-download.py`: the cloud it downloads from and the folder its player names videos below are the settings' `cloud` and `cloud_home`, the source folder the settings' `source_root`, and a player page's source and track addresses read with the standard library's HTML parser |
| `tools/voice/scripts/{bmo-script-match,bmo-script-transcribe}.py` | `studio/scripts/`: the speaker an automatic line's id and notes name is the settings' first speaker spelling, the notes `data/ko/studio-notes.json`'s, and the source folder the settings' `source_root`; `studio/scripts/bmo_script_settings.py` reads the settings for every Python tool |
| `tools/voice/scripts/{bmo_script_audio.py,asr-requirements.txt}` | `studio/scripts/`, unchanged |
| `tools/voice/scripts/deploy.ts`, `tools/voice/src/deploy-plan.ts` | the testbed's deployment of `testbed.toml` (`docs/deploy.md`): the deploy tree, the release, the generated screen, the unit and its health are the testbed's steps, and no script of the repository deploys |
| `tools/voice/scripts/cargo.ts`, `tools/voice/src/cargo-place.ts` | the root crate's own Cargo, which the testbed and the bench run; the studio's server is no separate Cargo project to place a target directory for |
| `tools/voice/src/scratch.ts` | not carried: a test helper alone, deferred with the tests |
| `client/app/domain/briefing.ts` | `client/app/domain/briefing.ts`: briefings are heard on the voice's listener stream through the browser's own event source rather than on the console's bot stream, the sound and the stop go through the bundle's own routes, a press leads by the host's room route, and bmo-ui serves the island, the player rule and the sound tap |
| `client/app/domain/briefing-position.ts`, `client/app/components/{BriefingPlayer,SettingsBriefing}.vue` | `client/app/`, the settings section's words under `briefing.*` keys and its trial leading back to the page it was tried on; the position is kept under the voice's own storage key |
| `client/public/briefing-trial.ogg` | `client/public/briefing-trial.ogg`, unchanged |
| `client/server/api/briefing/{events.get,stop.post,read.post}.ts` | `client/server/api/briefing/`: the stream relays the voice's `GET /briefings/events`, the stop asks the voice's `POST /briefings/stop`, and a line's reading asks the receiver's `POST /voice-briefing/line`, each behind the host's sign-in with English refusals and codes; a new `sound.get.ts` relays a voice message's sound from the receiver |
| `client/server/utils/briefing-stream.ts`, `client/server/utils/presence.ts` | the voice's own listener stream (`src/service/`), which files only the frames a listener hears and works out the presence itself |
| `client/shared/briefings.ts` | `client/shared/briefings.ts`, for a host showing a line's voices, its state type its own |
| `tools/voice/services/bmo-script.service` | `deploy/bmo-script.service`: the `script-studio` binary out of the voice's deploy tree, the screen the deployment generates into the release served by default, marked to restart with every deployment |
| `tools/voice/services/nginx.conf` | `deploy/nginx.conf`: the same two locations, naming no host and no snippet of a host's, the headers the studio reads written out |
| `prompts/agents/voice-lines/instructions.md` | `prompts/voice-lines.md`, in English, naming no agent and no person; its Korean material is `data/ko/voice-lines.toml`, with the Korean entries of the old wording glossary its rows were checked against, and its sections on who the agent is, how it talks and the people it names are the deployment's persona file (`docs/deploy.md`) |

The voice's Python files keep their work as it was; their paths name this repository's layout, their log lines
are English, and the names of machines, agents and people are gone from their comments. Their Korean that stays is
data: the spoken spellings the intelligibility measure normalises to, the measure's sentences, the line a freshly
loaded model speaks and drops, the Hangul range a sentence's language is read by, and the emotions the briefing
entry's schema allows.

The launch a briefing goes through stood in the old `bot/bmo-server/src/chat/voice.rs`, a file the receiver
carries: its launch, its switch and voice checks and its runner command are the voice's and stand in
`src/briefing/`, while the receiver's own half of that file, the briefings a round ties and lets go, stays the
receiver's and reaches the voice through `docs/boundary-api.md`.

## Deferred tests and test data

The 39 files marked deferred above are 35 tests and 4 test-data files (the script cases
`bot/bmo-server/src/verbs/voice/script/cases/{figures,polish,reading,spoken}.json`). This phase moves the code
only: no test is ported, written or run, and the only check is the build check. The tests are written and run on
bmo once the minimal services are up there, from these files as the record of what the old behaviour was.

## What siblings hold, outside the set

These files touch the voice but belong to other units. None is carried here, and nothing in this repository
duplicates them.

| Files | Owner | Why |
|---|---|---|
| `bot/bmo-server/src/control/voice.rs`, `bot/bmo-server/src/chat/voice.rs`, `bot/bmo-server/src/queue/voiced.rs`, `bot/bmo-server/tests/integration/receiver/voice.rs` (4) | bmo-receiver | The voice briefing's switch, the briefings a round launches and lets go, and voiced notices are the receiver's; it carries them and calls this repository on `POST /briefings`, `GET /briefings` and `POST /briefings/{briefingId}/line`; the engine keeps the briefings in `voice_briefings` |
| `prompts/agents/voice-polish/instructions.md`, `prompts/agents/voice-reading/instructions.md` (2, inside the set above) | bmo-gate | The reading and polishing gates are the gate's stages; their instructions stand in the gate's `prompts/`, and this repository runs them through `POST /stages/{stage}/runs` |
| `bot/bmo-server/src/verbs/benches/gate_eval/gates/{voice_polish,voice_reading}.{rs,toml}` (4) | bmo-gate | The gate's evaluation members; this repository serves them a line's script reading (`docs/boundary-api.md`) |
| `client/server/api/voice-briefing.get.ts`, `voice-briefing.post.ts`, `voice-briefing.post.test.ts` (3) | bmo-web | The switch on the console's agents screen, read and moved through the receiver |
| `chat/chat/kakao/voice-video.ts`, `voice-video.test.ts` (2) | bmo-kakaotalk | A provider's voice and video message handling |
| `tools/song/scripts/song/voice_host.py`, `voice_key.py`, `tools/song/src/song-voice.test.ts`, `song-voice-key.test.ts` (4) | bmo-song | The song bench's own voice parts |
| `android/**` (32), `client/server/api/app/android.apk.get.ts`, `repo/repo/android-release.ts` and its test, `scripts/build/android.ts` and its test (5) | dropped | The native briefing app is dropped by archive document #301 (「앱: bmo-app」) |

## The news brief: not carried

- **The files (18):** `bot/bmo-server/src/verbs/news_brief.rs`, `bot/bmo-server/src/verbs/news_brief_runner/{mod,brief,compose,indices,outside,page}.rs`, `bot/bmo-server/src/schedules/news_brief.rs`, `bot/bmo-server/tests/integration/news_brief_runner.rs`, `bot/bmo-server/tests/integration/news_brief_verb.rs`, `archive/archive/news-brief-store.ts`, `archive/archive/news-brief-store.test.ts`, `docs/news-brief.md`, `lang/bmo-lang/lang/ko/news-brief.yaml`, `lang/bmo-lang/src/news_brief.rs`, `prompts/agents/chat/procedures/news-brief.md`, `prompts/agents/search/news-brief-request.md`, `prompts/agents/search/news-brief.md`. Its table `news_briefs` and its data folder `news-brief/` go with them.
- **Why not here:** it is a written morning news page, made by a search-line writer run, the `stock_index` site
  tool and article pictures, and sent to a room as a link. It speaks nothing. Archive document #301 has each
  repository hold only what it uses (「각 저장소는 자기가 쓰는 것만 가진다」) and a folder's name say what it does.
- **Contract gap:** the bmo-artifact, bmo-receiver and bmo-cli inventories assign it to bmo-voice. That
  assignment is not taken up here. Row #1863 is raised to decide its owner and to correct those three inventories.

## Tables

| Table | Owner | Where it goes |
|---|---|---|
| `voices` | bmo-voice | this repository's own database (step 4) |
| `voice_names` | bmo-voice | this repository's own database (step 4) |
| `voice_nodes` | bmo-voice | this repository's own database (step 4) |
| `voice_briefings` | bmo-voice | this repository's own database (step 4), with the line a briefing awaits |
| `room_voices` | bmo-receiver | not carried: the per-room voice of the avatar mode is the receiver's |
| `news_briefs` | undecided, row #1863 | not carried |

The tables are taken from the old archive schema (`archive/bmo-archive/src/migration/archive.sql`) and the
`voice_awaits_line` migration. Their rows are moved at the switch-over, not by this phase.

## Policy

| Old key | Readers in the old repository | Here |
|---|---|---|
| `chat-round.toml` `[voice]` `run_cap_ms` | the briefing runner, `voice say`, the TypeScript voice library, and the receiver's briefing launch | owned by this repository in its own policy file (step 8). The receiver's copy is a contract gap the receiver takes up: this repository is lower in the dependency direction, so the receiver reads the value through this repository's API (`docs/boundary-api.md`) |
| `chat-round.toml` `[voice]` `failed_kind`, `emotions` | the briefing runner and launch; the receiver | as above |
| `chat-round.toml` `[voice]` `none`, `launch`, `waiting`, `untitled`, `titled`, `switch_intent`, `switch_kind`, `switch_ttl_ms` | the receiver alone | not carried: the receiver's |
| `ssh.toml` (the batch login options) | node reaching | not carried: read through bmo-node's login route |

## Units

| Old unit | Here |
|---|---|
| `tools/voice/services/bmo-script.service` (the script studio) | `deploy/` (step 17) |
| the per-node voice server unit written by `tools/voice/bmo/voice-node.ts` | its template stays with the node scripts (step 11) |
| none in the old repository: the briefing runtime ran inside the bot's own process | a `bmo-voice.service` of this repository's own (step 17) |

## Data folders

| Old place under the data root | Here, under the data root's `voice/` folder |
|---|---|
| `voices/` (the trained voices: LoRA weights and settings, reference clips, continuation prompt clips; about 448 MB) | `voice/voices/`, declared in `weights/`, never committed |
| `locks/voice-synth.lock`, `locks/voice-node-<node>.lock` | `voice/locks/` |
| `env/voice.token` (the bearer token voice node servers check) | `voice/voice.token` |
| `news-brief/` | not carried (row #1863) |

The Fish S2-Pro base weights stand on each voice node, installed by the node scripts, and are declared in `synth/engine.toml`
as well.
