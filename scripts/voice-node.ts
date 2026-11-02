// The voice servers on the nodes that lend their cards: built, brought up to date, asked how they are and asked for
// one line (docs/voice-plan.md, 「The voice servers on the voice nodes」).
//
//   bun scripts/voice-node.ts nodes   the voice node list
//   bun scripts/voice-node.ts adapter install|uninstall|start|stop|health --node <name> --placed <name,...> | placed
//       the voice's adapter, which the card arbiter runs for every placement of the voice
//   bun scripts/voice-node.ts setup  [--node <name>]   the node's engine environment and the base weights
//   bun scripts/voice-node.ts push   [--node <name>]   the mouth, the voices, the token and the unit; restarts it
//   bun scripts/voice-node.ts sync   [--node <name>]   push, only where the stamp the last one left differs
//   bun scripts/voice-node.ts health [--node <name>]   what the server holds right now
//   bun scripts/voice-node.ts speak  [--node <name>] --text '<words>' --wav <path>
//       [--voice <name> | --checkpoint <folder under the voices root>] [--style '<instruction>'] [--seed 0]
//       [--sentences on|off]
//
// Every verb but speak acts on every node of the voice node list, side by side, unless `--node` names one; speak
// takes a free node the way a synthesis does unless `--node` names one. A node is one the node service lists,
// reached through its own shell as the node service's target gives it. What lands on it lives under one directory
// in its home: the server file, the engine environment, the base weights and a copy of the data root's voices
// directory laid out the same,
// which is what lets a request name a voice by its place under that directory. The unit and the token file are the
// node's own, beside them.

import { createHash, randomBytes } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

import { NodeShellError, nodeHome } from "./lib/node-shell";
import { dataFile, nodeTokenPath, readTokenFile, releaseRoot, voicesFolder } from "./lib/settings";
import { nowSeconds, store } from "./lib/store";
import {
  anyNodeShell,
  BASE_PLACE,
  ENGINE_PYTHON,
  speakOnNode,
  takeVoiceNode,
  VOICE_PLACE,
  VOICE_PORT,
  VOICE_PUSH_SKIPS,
  VOICE_SERVICE,
  VOICE_STAMP,
  VOICE_TOKEN,
  VoiceNodeError,
  voiceNodeHealth,
  voiceNodeNames,
  voiceNodeRequest,
  voiceNodeShell,
  voicePushStamp,
  voicesManifest,
} from "./lib/voice-node";

import type { NodeShell } from "./lib/node-shell";
import type { VoiceNodeRequest } from "./lib/voice-node";

const args = process.argv.slice(2);
const verb = args[0] ?? "";
const argValue = (name: string): string | null => {
  const at = args.indexOf(name);

  return at === -1 || at + 1 >= args.length ? null : (args[at + 1] as string);
};
const serverFile = join(releaseRoot, "synth/voice-fish.py");
const engineFile = join(releaseRoot, "synth/engine.toml");
const usage = "usage: bun scripts/voice-node.ts nodes | adapter <verb> --node <name> --placed <name,...> | setup|push|sync|health|speak [--node <name>]";

const unit = "bmo-voice-server.service";
// How often a freshly restarted server is asked again whether it answers, until it does or its process has ended.
const upPollMs = 1_000;
// How often the node is asked whether building the engine environment, which downloads torch and the engine's
// packages, is done.
const setupPollMs = 5_000;
// The machine's own certificate store. A node may stand behind a proxy that signs what it passes with a certificate
// only that store trusts, so the downloads are made to trust it rather than the bundles uv and Python carry.
const systemCerts = "/etc/ssl/certs/ca-certificates.crt";
// The card arbiter's own installation on a node, relative to its home, where its turn client `turn.py` stands (the
// arbiter's docs/api.md, 「The turn client」). The server finds the client there and waits its turn for the card
// through it; a node without it takes the card lock as it comes.
const arbiterPlace = "bmo-card";

function fail(why: string): never {
  console.error(`voice-node: ${why}`);
  process.exit(1);
}

// What stops the work on one node. The other nodes go on, and the verb fails once they are done.
class NodeRefused extends Error {}

// One node being worked on: its name, which opens every line said about it, and its shell.
interface VoiceNode {
  name: string;
  shell: NodeShell;
}

const say = (node: VoiceNode, what: string): void => console.log(`voice-node: ${node.name}: ${what}`);

// The node `--node` names, or every node of the list.
async function nodeNames(): Promise<string[]> {
  const named = argValue("--node");

  return named === null ? await voiceNodeNames() : [named];
}

// The verb done on every node side by side. A node that could not be reached or refused is said and the others
// finish; the verb then fails, naming the ones that did.
async function everyNode(work: (node: VoiceNode) => Promise<void>): Promise<void> {
  const names = await nodeNames();

  if (names.length === 0) {
    console.log("voice-node: the voice node list is empty, so there is nothing to do");

    return;
  }

  const failed: string[] = [];

  await Promise.all(
    names.map(async (name) => {
      try {
        await work({ name, shell: await voiceNodeShell(name) });
      } catch (error) {
        failed.push(name);
        console.error(`voice-node: ${name}: ${error instanceof Error ? error.message : String(error)}`);
      }
    }),
  );

  if (failed.length > 0) {
    fail(`${failed.sort().join(", ")} did not finish`);
  }
}

// One command on the node, which has to succeed; what it printed is passed through.
async function onNode(node: VoiceNode, command: string, input?: string): Promise<string> {
  let ran;

  try {
    ran = await node.shell.run(command, input);
  } catch (error) {
    throw new NodeRefused(error instanceof NodeShellError ? error.message : String(error));
  }

  if (ran.code !== 0) {
    throw new NodeRefused(`the node refused: ${command.split("\n")[0]}\n${ran.stderr.trim().slice(-2_000)}`);
  }

  return ran.stdout;
}

// The token every voice node is asked with, made the first time it is wanted and kept 0600 under the voice's folder.
function token(): string {
  const path = nodeTokenPath();
  const kept = readTokenFile(path);

  if (kept !== null) {
    return kept;
  }

  if (existsSync(path)) {
    fail(`${path} is there but not a usable token file (it must be 0600 and not empty)`);
  }

  const made = randomBytes(32).toString("hex");

  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${made}\n`, { mode: 0o600 });
  console.log(`voice-node: made a token at ${path}`);

  return made;
}

// The engine the voice servers speak with (`synth/engine.toml`): the code checkout their environment is built from and
// the base weights each node carries.
interface Engine {
  code: { repository: string; commit: string; extra: string; python: string; leave_out: string[] };
  base: { repository: string; revision: string; file: { name: string; bytes: number; sha256: string }[] };
}

const engine = Bun.TOML.parse(readFileSync(engineFile, "utf8")) as Engine;

// The least memory a voice node's card holds, in MiB: the server peaks at about 11,100 MiB on a line and 11,800 MiB
// on its longest piece (docs/voice-plan.md, 「The synthesiser」), so only a 12 GB card takes it.
const leastCardMib = 12_000;

// What one command carries of a base weight file to a node; a hub node takes a transfer this size whole.
const sliceBytes = 256 * 1024 * 1024;

// The base weights on this machine: the pinned revision's snapshot in the account's Hugging Face cache.
function baseSnapshot(): string {
  const hub = process.env.HF_HUB_CACHE ?? join(process.env.HF_HOME ?? join(homedir(), ".cache/huggingface"), "hub");

  return join(hub, `models--${engine.base.repository.replace("/", "--")}`, "snapshots", engine.base.revision);
}

async function digestOf(path: string): Promise<string> {
  const hash = createHash("sha256");

  for await (const chunk of Bun.file(path).stream()) {
    hash.update(chunk);
  }

  return hash.digest("hex");
}

// Every base weight file this machine holds as the declaration says, checked once for every node a verb sets up.
let checkedBase: string | null = null;

async function localBase(): Promise<string> {
  if (checkedBase !== null) {
    return checkedBase;
  }

  const snapshot = baseSnapshot();

  for (const file of engine.base.file) {
    const path = join(snapshot, file.name);

    if (!existsSync(path) || statSync(path).size !== file.bytes || (await digestOf(path)) !== file.sha256) {
      fail(`${path} is missing or is not the file synth/engine.toml declares; fetch ${engine.base.repository}@${engine.base.revision.slice(0, 8)} into this machine's Hugging Face cache first`);
    }
  }

  checkedBase = snapshot;

  return snapshot;
}

// The base weights carried to the node: each file only where the node's copy is missing or its digest differs, sent a
// slice a command, joined there and checked by its digest before it takes the file's name.
async function placeBase(node: VoiceNode): Promise<void> {
  const place = `${VOICE_PLACE}/${BASE_PLACE}`;
  const home = await nodeHome(node.shell);

  await onNode(node, `mkdir -p ~/${place}/.incoming`);

  for (const file of engine.base.file) {
    const standing = (await onNode(node, `sha256sum ~/${place}/${file.name} 2>/dev/null | cut -c1-64 || true`)).trim();

    if (standing === file.sha256) {
      continue;
    }

    const source = Bun.file(join(await localBase(), file.name));
    const slices = Math.max(1, Math.ceil(file.bytes / sliceBytes));

    say(node, `carrying ${file.name} (${(file.bytes / 2 ** 20).toFixed(0)} MiB, ${slices} slices)`);

    for (let at = 0; at < slices; at++) {
      const bytes = new Uint8Array(await source.slice(at * sliceBytes, Math.min(file.bytes, (at + 1) * sliceBytes)).arrayBuffer());

      try {
        await node.shell.put(`${home}/${place}/.incoming/${file.name}.${String(at).padStart(4, "0")}`, bytes);
      } catch (error) {
        throw new NodeRefused(error instanceof Error ? error.message : String(error));
      }
    }

    const joined = (await onNode(node, [
      `cd ~/${place}/.incoming`,
      `cat ${file.name}.[0-9][0-9][0-9][0-9] > ${file.name}`,
      `rm -f ${file.name}.[0-9][0-9][0-9][0-9]`,
      `sha256sum ${file.name} | cut -c1-64`,
    ].join(" && "))).trim();

    if (joined !== file.sha256) {
      await onNode(node, `rm -f ~/${place}/.incoming/${file.name}`);
      throw new NodeRefused(`${file.name} arrived with another digest than synth/engine.toml declares`);
    }

    await onNode(node, `mv ~/${place}/.incoming/${file.name} ~/${place}/${file.name}`);
  }

  say(node, `the base weights ${engine.base.repository}@${engine.base.revision.slice(0, 8)} stand under ~/${place}, every file checked`);
}

function unitText(): string {
  return [
    "[Unit]",
    "Description=Voice server (docs/voice-plan.md in the voice repository)",
    "After=network-online.target",
    "Wants=network-online.target",
    "",
    "[Service]",
    `WorkingDirectory=%h/${VOICE_PLACE}`,
    `ExecStart=%h/${VOICE_PLACE}/${ENGINE_PYTHON} %h/${VOICE_PLACE}/voice-fish.py --serve --voices %h/${VOICE_PLACE}/voices --base %h/${VOICE_PLACE}/${BASE_PLACE} --token-file %h/${VOICE_TOKEN} --host 127.0.0.1 --port ${VOICE_PORT}`,
    // Under WSL the driver's libraries and its nvidia-smi, which the server asks for the card's free memory, are
    // kept here; a directory that is not there is passed over.
    "Environment=LD_LIBRARY_PATH=/usr/lib/wsl/lib",
    "Environment=PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/lib/wsl/lib",
    // The card arbiter's turn client, imported from the arbiter's own installation.
    `Environment=PYTHONPATH=%h/${arbiterPlace}`,
    // Declared to the node's card arbiter, which on a node reserved for another service ends a holder of the card
    // lock that names another.
    `Environment=BMO_CARD_SERVICE=${VOICE_SERVICE}`,
    "Environment=PYTHONUNBUFFERED=1",
    // The model's progress bars say nothing a journal needs and bury the lines that do.
    "Environment=TQDM_DISABLE=1",
    // What the mouth compiles is kept on the node's home disk, so a restart reads it back rather than compiling
    // again (docs/voice-plan.md, 「How the model is loaded」).
    `Environment=TORCHINDUCTOR_CACHE_DIR=%h/${VOICE_PLACE}/compiled`,
    "Restart=always",
    "RestartSec=5",
    // The server ends on the stop signal itself, so a stop waits for it and never kills it after a span.
    "TimeoutStopSec=infinity",
    "",
    "[Install]",
    "WantedBy=default.target",
    "",
  ].join("\n");
}

// Building takes longer than a hub node lets one command run, so it runs apart on the node, its output in
// `setup.log` and its exit in `setup.code`, while the base weights are carried beside it; the node is then asked after
// until the code is there.
async function setup(node: VoiceNode): Promise<void> {
  const code = engine.code;
  const steps = [
    // uv into the account where it is missing, nothing written to the account's shell profile.
    "{ test -x ~/.local/bin/uv || curl -LsSf https://astral.sh/uv/install.sh | env UV_NO_MODIFY_PATH=1 sh; }",
    // The engine's torch is a CUDA 12.9 build, which an older driver does not run, and the server needs a 12 GB card:
    // such a node is refused here rather than failing on its first line. WSL keeps the driver's tools apart.
    "{ PATH=$PATH:/usr/lib/wsl/lib nvidia-smi | grep -Eq \"CUDA Version: (12[.]9|1[3-9][.]|[2-9][0-9][.])\" || { echo the driver runs no CUDA 12.9; false; }; }",
    `{ test "$(PATH=$PATH:/usr/lib/wsl/lib nvidia-smi --id=0 --query-gpu=memory.total --format=csv,noheader,nounits)" -ge ${leastCardMib} || { echo the card holds less than ${leastCardMib} MiB; false; }; }`,
    `{ test -d fish-speech/.git || git clone --quiet ${code.repository} fish-speech; }`,
    `{ git -C fish-speech -c advice.detachedHead=false checkout --quiet ${code.commit} 2>/dev/null || { git -C fish-speech fetch --quiet origin && git -C fish-speech -c advice.detachedHead=false checkout --quiet ${code.commit}; }; }`,
    // The checkout's own lockfile, as it stands.
    // In a subshell of its own, so the exit code is written beside the log rather than in the checkout.
    `(cd fish-speech && UV_SYSTEM_CERTS=1 SSL_CERT_FILE=${systemCerts} ~/.local/bin/uv sync --frozen --python ${code.python} --extra ${code.extra} ${code.leave_out.map((name) => `--no-install-package ${name}`).join(" ")})`,
  ].join(" && ");

  // The build holds setup.lock from before this returns until it has written its exit, so a lock let go with no exit
  // written is the build gone without one; a second setup waits on the lock for the first to end.
  await onNode(node, `mkdir -p ~/${VOICE_PLACE} && cd ~/${VOICE_PLACE} && exec 9> setup.lock && flock 9 && rm -f setup.code && (setsid sh -c '${steps}; echo $? > setup.code' > setup.log 2>&1 < /dev/null &) && exec 9>&-`);
  say(node, `building on the node; its output is ~/${VOICE_PLACE}/setup.log there`);
  await placeBase(node);

  for (;;) {
    const done = (await onNode(node, `cd ~/${VOICE_PLACE} && if [ -f setup.code ]; then cat setup.code; elif flock -n -s setup.lock true 2>/dev/null; then [ -f setup.code ] && cat setup.code || echo gone; fi`)).trim();

    if (done === "gone") {
      throw new NodeRefused(`building ended without writing its exit; ~/${VOICE_PLACE}/setup.log on the node says how far it got`);
    }

    if (done !== "") {
      const tail = await onNode(node, `tail -n 15 ~/${VOICE_PLACE}/setup.log`);

      console.log(tail);

      if (done !== "0") {
        throw new NodeRefused(`building ended with ${done}`);
      }

      say(node, `the engine environment of fish-speech@${code.commit.slice(0, 8)} is on the node`);

      return;
    }

    await Bun.sleep(setupPollMs);
  }
}

// What a push would bring a node now, as the stamp it leaves there.
function stampNow(): string {
  return voicePushStamp({
    server: readFileSync(serverFile),
    unit: unitText(),
    token: token(),
    voices: voicesManifest(voicesFolder()),
  });
}

// The weights and the clips a voice is conditioned on, and nothing else under the directory, packed once for every
// node a verb pushes to.
let packed: Uint8Array | null = null;

function voicesPack(): Uint8Array {
  if (packed !== null) {
    return packed;
  }

  const voices = Bun.spawnSync(["tar", "-C", voicesFolder(), "-cf", "-", `--exclude=${VOICE_PUSH_SKIPS}`, "."]);

  if (voices.exitCode !== 0) {
    fail(`the voices directory could not be packed: ${voices.stderr.toString()}`);
  }

  packed = voices.stdout;

  return packed;
}

async function push(node: VoiceNode): Promise<void> {
  const secret = token();
  // Taken before the pack, so a file written while it is packed makes the stamp older than the node rather than
  // newer, and the next sync brings it.
  const stamp = stampNow();
  const voices = voicesPack();
  let home: string;

  try {
    home = await nodeHome(node.shell);
    await onNode(node, `mkdir -p ~/${VOICE_PLACE}/voices ~/.config/${VOICE_PLACE} ~/.config/systemd/user`);
    await node.shell.put(`${home}/${VOICE_PLACE}/voice-fish.py`, readFileSync(serverFile));
    await node.shell.put(`${home}/${VOICE_PLACE}/voices.tar`, voices);
  } catch (error) {
    throw new NodeRefused(error instanceof Error ? error.message : String(error));
  }

  await onNode(node, `cd ~/${VOICE_PLACE} && tar -C voices -xf voices.tar && rm -f voices.tar`);
  await onNode(node, `umask 077 && cat > ~/${VOICE_TOKEN} && chmod 600 ~/${VOICE_TOKEN}`, `${secret}\n`);
  await onNode(node, `cat > ~/.config/systemd/user/${unit}`, unitText());
  await onNode(node, `systemctl --user daemon-reload && systemctl --user enable --quiet ${unit} && systemctl --user restart ${unit}`);
  say(node, `pushed and restarted ${unit} under ${home}`);
  await health(node, true);
  // Only a server that answered is stamped, so a push that fell over short of it is still due.
  await onNode(node, `cat > ~/${VOICE_STAMP}`, `${stamp}\n`);
}

async function sync(node: VoiceNode): Promise<void> {
  const standing = (await onNode(node, `cat ~/${VOICE_STAMP} 2>/dev/null || true`)).trim();

  if (standing === stampNow()) {
    say(node, "the node already holds what a push would bring");

    return;
  }

  say(node, `${standing === "" ? "the node carries no stamp" : "the node holds something else"}; pushing`);
  await push(node);
}

// The server's health, said. Waiting, as after a restart, it is asked again until it answers, however long that
// takes, and only the server's process ending ends the wait: its unit failed or stopped, or restarting it after a
// crash. Otherwise one answer that is not there throws.
async function health(node: VoiceNode, waiting = false): Promise<void> {
  let restarts: string | null = null;

  for (;;) {
    try {
      say(node, JSON.stringify(await voiceNodeHealth(node.shell)));

      return;
    } catch (error) {
      const why = error instanceof Error ? error.message : String(error);

      if (!waiting) {
        throw new NodeRefused(`the server did not answer: ${why}`);
      }

      const shown = await onNode(node, `systemctl --user show -p ActiveState -p SubState -p NRestarts ${unit} || true`);
      const state = new Map(shown.split("\n").filter((line) => line.includes("=")).map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1).trim()]));
      const active = state.get("ActiveState") ?? "";
      const crashed = state.get("SubState") === "auto-restart" || (restarts !== null && state.get("NRestarts") !== restarts);

      if (active === "failed" || active === "inactive" || crashed) {
        throw new NodeRefused(`the server's unit is ${crashed ? "restarting it after it ended" : active} and the server did not answer: ${why}`);
      }

      restarts ??= state.get("NRestarts") ?? null;
    }

    await Bun.sleep(upPollMs);
  }
}

// The voice node list, shown. Which nodes it holds is the card arbiter's to change, through the adapter below.
async function nodes(): Promise<void> {
  const names = await voiceNodeNames();

  console.log(names.length === 0 ? "voice-node: the list is empty; a trained voice is spoken on this machine" : names.join("\n"));
}

// One line in a voice, written as the wav the node answered. The voice is a row of the voices table by name, the
// current row when none is named, or a voice folder named outright under the voices root, which is how a measurement
// draws an adapter no row carries.
async function speakRequest(text: string): Promise<VoiceNodeRequest> {
  const named = argValue("--voice");
  const checkpoint = argValue("--checkpoint");
  let request: VoiceNodeRequest;

  if (checkpoint !== null) {
    request = { text, checkpoint, seed: 0, style: "", sentences: true };
  } else {
    const row = named === null ? await store().currentVoice() : await store().voice(named);

    if (row === null) {
      fail(named === null ? "no voice is current; name one with --voice" : `there is no voice called ${named}`);
    }

    request = voiceNodeRequest(row, text, voicesFolder());
  }

  const seed = argValue("--seed");
  const style = argValue("--style");
  const sentences = argValue("--sentences");

  if (sentences !== null && sentences !== "on" && sentences !== "off") {
    fail("--sentences is on or off");
  }

  return {
    ...request,
    ...(seed === null ? {} : { seed: Number.parseInt(seed, 10) }),
    ...(style === null ? {} : { style }),
    ...(sentences === null ? {} : { sentences: sentences === "on" }),
  };
}

// Spoken on the node `--node` names, or on a free node of the list taken the way a synthesis takes one and let go
// once the line is back.
async function speak(): Promise<void> {
  const text = argValue("--text");
  const wav = argValue("--wav");

  if (text === null || wav === null) {
    fail("speak needs --text and --wav");
  }

  const request = await speakRequest(text);
  const named = argValue("--node");
  let release = (): void | Promise<unknown> => undefined;

  try {
    let node: VoiceNode;

    if (named !== null) {
      node = { name: named, shell: await voiceNodeShell(named) };
    } else {
      const taken = await takeVoiceNode({
        names: await voiceNodeNames(),
        checkpoint: request.checkpoint,
        shellOf: (name) => voiceNodeShell(name),
      });

      release = taken.release;
      node = { name: taken.name, shell: taken.shell };
    }

    const spoken = await speakOnNode(request, { shell: node.shell });

    writeFileSync(wav, spoken.wav);
    say(node, `${spoken.seconds.toFixed(2)}s of ${spoken.checkpoint} spoken in ${spoken.speakSeconds.toFixed(2)}s (weights ${spoken.loadSeconds.toFixed(2)}s)`);
  } catch (error) {
    await release();
    fail(String(error));
  }

  await release();
}

// The line a node is tried on once the server is installed there (`data/ko/voice-node.toml`).
function moveCheckLine(): string {
  return (Bun.TOML.parse(readFileSync(dataFile("ko", "voice-node.toml"), "utf8")) as { check_line: string }).check_line;
}

// What the voice's adapter answers the card arbiter, the same shape for every verb (the arbiter's
// docs/placement.md, 「An adapter's verbs」).
interface AdapterAnswer {
  ok: boolean;
  node: string | null;
  state: "running" | "stopped" | "absent" | "failing" | null;
  detail: string;
  nodes?: string[];
}

// The node list brought in step with the card arbiter's placed nodes: the nodes it no longer names taken off, the
// ones it names and the list lacks put last, in its order.
async function listAs(placed: readonly string[]): Promise<void> {
  const standing = await voiceNodeNames();

  for (const name of standing.filter((one) => !placed.includes(one))) {
    await store().removeVoiceNode(name);
  }

  for (const name of placed.filter((one) => !standing.includes(one))) {
    await store().addVoiceNode(name, nowSeconds());
  }
}

// One verb of the voice's adapter for the card arbiter (docs/voice-plan.md, 「Building and updating them」).
async function adapterAnswer(asked: string, node: string | null, placed: readonly string[]): Promise<AdapterAnswer> {
  if (asked === "placed") {
    return { ok: true, node: null, state: null, detail: "", nodes: await voiceNodeNames() };
  }

  if (node === null) {
    return { ok: false, node, state: null, detail: `${asked} needs --node` };
  }

  const listed = (await voiceNodeNames()).includes(node);

  switch (asked) {
    case "install": {
      await listAs(placed.includes(node) ? placed : [...placed, node]);
      const at: VoiceNode = { name: node, shell: await voiceNodeShell(node) };

      await setup(at);
      await push(at);
      const started = Date.now();
      // Another occupant holding the card lock is never made to yield for the check line, which would wait for it
      // however long it held the card: the server answering its health on the card's device stands for the line,
      // and the node's first granted line is its first spoken one. So does a 503, the node's arbiter giving the
      // voice no place on the card.
      const standing = await voiceNodeHealth(at.shell);
      const onDevice = (why: string): AdapterAnswer => {
        const device = String(standing.device ?? "cpu");

        if (device === "cpu") {
          throw new NodeRefused(`the server answers with no card to speak on, and the check line waited: ${why}`);
        }

        const waited = `the check line waited: ${why}; the server answers on ${device}`;

        say(at, waited);

        return { ok: true, node, state: "running", detail: waited };
      };

      if (standing.cardTaken === true) {
        return onDevice("the card is another's in use");
      }

      let spoken;

      try {
        spoken = await speakOnNode(await speakRequest(moveCheckLine()), { shell: at.shell });
      } catch (error) {
        if (!(error instanceof VoiceNodeError) || error.kind !== "taken") {
          throw error;
        }

        return onDevice(error.message);
      }

      const tried = `${spoken.seconds.toFixed(2)}s of ${spoken.checkpoint} spoken in ${((Date.now() - started) / 1000).toFixed(1)}s`;

      say(at, `tried: ${tried}`);

      return { ok: true, node, state: "running", detail: tried };
    }
    case "uninstall": {
      await listAs(placed.filter((one) => one !== node));
      await onNode({ name: node, shell: await anyNodeShell(node) }, `systemctl --user disable --now ${unit}`);
      console.log(`voice-node: ${node}: off the list, ${unit} stopped and kept from starting; its files stay`);

      return { ok: true, node, state: "stopped", detail: "" };
    }
    case "start":
    case "stop": {
      await onNode({ name: node, shell: await anyNodeShell(node) }, `systemctl --user ${asked} ${unit}`);

      return { ok: true, node, state: asked === "start" ? "running" : "stopped", detail: "" };
    }
    case "health": {
      if (!listed) {
        return { ok: true, node, state: "absent", detail: `${node} is not on the voice node list` };
      }

      try {
        return { ok: true, node, state: "running", detail: JSON.stringify(await voiceNodeHealth(await voiceNodeShell(node))) };
      } catch (error) {
        return { ok: true, node, state: "failing", detail: error instanceof Error ? error.message : String(error) };
      }
    }
    default:
      return { ok: false, node, state: null, detail: `the adapter's verbs are install, uninstall, start, stop, health and placed, not ${asked}` };
  }
}

// The voice's adapter: the verb read, done, and the answer written as the last line.
async function adapter(): Promise<void> {
  const node = argValue("--node");
  const placed = (argValue("--placed") ?? "").split(",").filter((one) => one !== "");
  let answer: AdapterAnswer;

  try {
    answer = await adapterAnswer(args[1] ?? "", node, placed);
  } catch (error) {
    answer = { ok: false, node, state: null, detail: error instanceof Error ? error.message : String(error) };
  }

  console.log(JSON.stringify(answer));
  process.exit(answer.ok ? 0 : 1);
}

switch (verb) {
  case "nodes":
    await nodes();
    break;
  case "adapter":
    await adapter();
    break;
  case "setup":
    await everyNode(setup);
    break;
  case "push":
    await everyNode(push);
    break;
  case "sync":
    await everyNode(sync);
    break;
  case "health":
    await everyNode((node) => health(node));
    break;
  case "speak":
    await speak();
    break;
  default:
    fail(usage);
}

process.exit(0);
