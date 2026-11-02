import { createHash, randomBytes } from "node:crypto";
import { lstatSync, readdirSync, unlinkSync, writeFileSync } from "node:fs";
import { isAbsolute, join, relative } from "node:path";

import { nodeTarget, reservedAgainst } from "./node";
import { NodeShellError, nodeShell, shellWord } from "./node-shell";
import { locksFolder, nodePlace, voicesFolder } from "./settings";
import { store } from "./store";
import { encodeWav, readyVoiceLock, voiceQueueExit } from "./voice";

import type { NodeShell } from "./node-shell";
import type { VoiceRow } from "./store";

// The scripts' way to the voice servers on the nodes that lend their cards (docs/voice-plan.md, 「The voice servers
// on the voice nodes」). The server listens on the node's loopback alone, so every call goes through the node's own
// shell: the request handed in on standing input, the asker started apart on the node, its answer asked after until
// it is there, the wav carried back as a file and every file of the call removed. The OGG/Opus encode stays on this
// machine.
//
// Which nodes there are is the voice node list, read at every call; how each is reached is the node service's
// answer for it. A line is spoken on one free node of the list (docs/voice-plan.md, 「Which node speaks a line」).

export type VoiceNodeFailure =
  // No connection: the node down, its hub or agent gone, the server not installed or not listening.
  | "unreachable"
  // The server said no: a wrong token, a voice it does not hold.
  | "refused"
  // The server took the line and failed to speak it, or the node failed to start asking.
  | "failed"
  // An answer that is not a wav.
  | "bad-answer"
  // Every node of the list stayed taken or speaking for the whole of the wait for one, or the card of the one taken
  // stayed held by other work.
  | "taken"
  // The voice's files stand outside the voices root, which is the only layout the node mirrors.
  | "elsewhere";

// The message is the kind, a status and the server's own reason where it gave one; never the line, so a log may
// carry it.
export class VoiceNodeError extends Error {
  readonly kind: VoiceNodeFailure;
  readonly status: number | null;

  constructor(kind: VoiceNodeFailure, status: number | null = null, reason: string | null = null) {
    super(`voice node ${kind}${status === null ? "" : ` (${status})`}${reason === null ? "" : `: ${reason}`}`);
    this.name = "VoiceNodeError";
    this.kind = kind;
    this.status = status;
  }
}

// The voice's own name, the one a node's role may reserve the node for and its server declares to the node's card
// arbiter.
export const VOICE_SERVICE = "voice";

// Where everything lands on the node, relative to its home (`weights/checkpoints.toml`), and the port the server
// listens on there. The token file is the node's own, beside the other configuration in its home.
export const VOICE_PLACE = nodePlace;
export const VOICE_PORT = 7432;

// The engine environment's interpreter and the base weights, both relative to the node's place (docs/voice-plan.md,
// 「The environments」, 「Where the weights live」).
export const ENGINE_PYTHON = "fish-speech/.venv/bin/python";
export const BASE_PLACE = "s2-pro";
export const VOICE_TOKEN = `.config/${VOICE_PLACE}/token`;

// The stamp a push leaves on the node, relative to its home: one digest of everything that push brought. The node
// is what a sync compares against, so a push by hand counts and a node that was out of reach at one deployment is
// still due at the next.
export const VOICE_STAMP = `${VOICE_PLACE}/pushed`;

// The one file of the voices directory a push leaves behind: optimiser state for resuming training, with no part
// in speaking.
export const VOICE_PUSH_SKIPS = "resume.pt";

// One file of the voices directory as the stamp sees it: its place under the directory, its size and the
// millisecond it was last written. The weights are hundreds of megabytes, so they are told apart by these rather
// than read.
export interface VoiceFile {
  path: string;
  size: number;
  mtimeMs: number;
}

// What a push brings the node.
export interface VoicePush {
  server: Uint8Array;
  unit: string;
  token: string;
  voices: readonly VoiceFile[];
}

// Every file under the voices directory a push packs. A file anywhere under it named as the skipped one is left
// out, as the pack leaves it out; a directory that is not there is no files at all.
export function voicesManifest(root: string): VoiceFile[] {
  const found: VoiceFile[] = [];

  const walk = (dir: string): void => {
    let names: string[];

    try {
      names = readdirSync(dir);
    } catch {
      return;
    }

    for (const name of names) {
      if (name === VOICE_PUSH_SKIPS) continue;

      const path = join(dir, name);
      const stat = lstatSync(path);

      if (stat.isDirectory()) {
        walk(path);
      } else if (stat.isFile()) {
        found.push({ path: relative(root, path), size: stat.size, mtimeMs: Math.floor(stat.mtimeMs) });
      }
    }
  };

  walk(root);

  return found;
}

// The stamp of one push, as the hex of a digest. The token goes in only as its own digest, so the stamp standing on
// the node says nothing about it beyond whether it moved.
export function voicePushStamp(push: VoicePush): string {
  const digest = (data: Uint8Array | string): string => createHash("sha256").update(data).digest("hex");

  return digest(JSON.stringify({
    server: digest(push.server),
    unit: push.unit,
    token: digest(push.token),
    voices: [...push.voices].sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0)),
  }));
}

// The node's interpreter running the server file, as a line of the node's shell, which is what every verb that
// asks the server starts from.
export const voiceOnNode = `"$HOME/${VOICE_PLACE}/${ENGINE_PYTHON}" "$HOME/${VOICE_PLACE}/voice-fish.py"`;

// The voice node list, in the order a free node is taken; empty where the trained voice is spoken on this machine.
export async function voiceNodeNames(): Promise<string[]> {
  return await store().voiceNodes();
}

// The shell of a node by its name, whatever its role reserves it for: what is stopped there is the voice's. A name
// the node service does not list, or a node that cannot be reached as it stands, is the node out of reach.
export async function anyNodeShell(name: string): Promise<NodeShell> {
  try {
    return await nodeShell(name, await nodeTarget(name));
  } catch (error) {
    throw new VoiceNodeError("unreachable", null, `there is no reachable node called ${name}: ${error instanceof Error ? error.message : String(error)}`);
  }
}

// The shell of a node the voice may speak on: one its role does not reserve for other services.
export async function voiceNodeShell(name: string): Promise<NodeShell> {
  let target;

  try {
    target = await nodeTarget(name);
  } catch (error) {
    throw new VoiceNodeError("unreachable", null, `there is no reachable node called ${name}: ${error instanceof Error ? error.message : String(error)}`);
  }

  const reserved = reservedAgainst(target, VOICE_SERVICE);

  if (reserved !== null) {
    throw new VoiceNodeError("unreachable", null, reserved);
  }

  try {
    return await nodeShell(name, target);
  } catch (error) {
    throw new VoiceNodeError("unreachable", null, error instanceof Error ? error.message : String(error));
  }
}

export interface VoiceNodeRequest {
  text: string;
  // The voice's folder as a name under the voices root: the node holds a copy of that directory, laid out the same.
  checkpoint: string;
  seed: number;
  // The delivery instruction as plain words; the node's mouth puts it where the model reads it.
  style: string;
  // Whether the line is spoken a sentence at a time rather than in passes.
  sentences: boolean;
}

function under(root: string, path: string): string | null {
  const name = relative(root, path);

  return name === "" || name.startsWith("..") || isAbsolute(name) ? null : name;
}

// What the node is asked for one line in one voice (docs/voice-plan.md, 「What it answers」). The row names its folder
// by where it stands on this machine; the node is sent the part under the voices root, which is all its copy shares.
export function voiceNodeRequest(voice: VoiceRow, text: string, root: string = voicesFolder()): VoiceNodeRequest {
  const checkpoint = under(root, voice.checkpoint);

  if (checkpoint === null) {
    throw new VoiceNodeError("elsewhere", null, `${voice.name} is not under ${root}`);
  }

  return { text, checkpoint, seed: voice.seed, style: voice.style, sentences: voice.bySentence };
}

export interface VoiceNodeSpoken {
  wav: Uint8Array;
  // How long the line lasts, what loading or swapping the weights took on the node, what speaking it took, and the
  // checkpoint that spoke: the node's own account, from its answer.
  seconds: number;
  loadSeconds: number;
  speakSeconds: number;
  checkpoint: string;
}

export interface VoiceNodeDeps {
  shell: NodeShell;
  // The longest one wait on the node for the asker to end lasts, well inside what a hub node lets a command run; the
  // wait is asked again until the asker lets go.
  askWaitSeconds?: number;
}

// The start of a call's command exits with this where the server is not installed on the node.
const NOT_INSTALLED = 4;
// The longest one wait on the node lasts.
const ASK_WAIT_SECONDS = 10;

// What the asker on the node wrote once it had its answer.
interface Asked {
  status: number | null;
  error: string | null;
  seconds?: number;
  loadSeconds?: number;
  speakSeconds?: number;
  checkpoint?: string;
}

// Starts the asker apart from the call and says where the call's files stand. What the command reads on its
// standing input is the request, written where only the account can read it. The call's lock is taken before any of
// its files is written, and the asker holds it from before this returns until it has written its end, so the moment
// it lets go is the moment its line is there.
function startCommand(id: string): string {
  return [
    `d="$HOME/${VOICE_PLACE}/spoken"`,
    `mkdir -p "$d" && chmod 700 "$d" || exit 3`,
    // What a call left behind once its asker let go and nothing of it was written for an hour, and any hour-old file
    // with no lock beside it. A call whose asker still holds its lock is never swept, however long it runs, and a
    // fresh call is passed over before its lock is even tried.
    `for l in "$d"/*.lock; do [ -e "$l" ] || continue; c="\${l%.lock}"; [ -z "$(find "$d" -maxdepth 1 -name "\${c##*/}.*" -mmin -60 -print -quit)" ] && flock -n "$l" true 2>/dev/null && rm -f "$c".*; done`,
    `find "$d" -maxdepth 1 -type f ! -name "*.lock" -mmin +60 | while IFS= read -r f; do [ -e "\${f%.*}.lock" ] || rm -f "$f"; done`,
    `[ -x "$HOME/${VOICE_PLACE}/.venv/bin/python" ] || { echo "the voice server is not installed on this node" >&2; exit ${NOT_INSTALLED}; }`,
    `umask 077`,
    `exec 9> "$d/${id}.lock" && flock -n 9 || exit 3`,
    `cat > "$d/${id}.in" || exit 3`,
    `setsid sh -c '${voiceOnNode} --ask --port ${VOICE_PORT} --token-file "$HOME/${VOICE_TOKEN}" --request "$1/$2.in" --wav "$1/$2.wav" > "$1/$2.out" 2> "$1/$2.err"; echo $? > "$1/$2.code"' sh "$d" ${shellWord(id)} < /dev/null > /dev/null 2>&1 &`,
    // The asker keeps the lock on its own copy; this shell's goes now, whether or not it ends with the call.
    `exec 9>&-`,
    `printf '%s\\n' "$d"`,
  ].join("\n");
}

// Waits on the node, `seconds` at most, for the asker to let go of the call's lock, then says whether it has ended
// and what it wrote: `waiting`, `gone` where it ended without writing its end, or `done <code>` followed by its
// answer and the tail of what it said on its error stream folded onto one line. The lock is read free before the
// end is looked for, since the end is written before it frees.
function awaitCommand(dir: string, id: string, seconds: number): string {
  const at = `${shellWord(dir)}/${shellWord(id)}`;

  return [
    `[ -f ${at}.code ] || flock -w ${seconds} -s ${at}.lock true 2>/dev/null`,
    `if flock -n -s ${at}.lock true 2>/dev/null; then free=1; else free=0; fi`,
    `if [ -f ${at}.code ]; then echo "done $(cat ${at}.code)"; cat ${at}.out; echo; tail -c 1000 ${at}.err | tr '\\n' ' '; elif [ $free = 1 ]; then echo gone; else echo waiting; fi`,
  ].join("\n");
}

function failureOf(asked: Asked): VoiceNodeError {
  const error = asked.error ?? null;

  if (asked.status === null) {
    if (error?.startsWith("token") === true) return new VoiceNodeError("refused", null, error);

    return new VoiceNodeError("unreachable", null, error);
  }

  // The server's 503 is the node's card arbiter giving the voice no place on the card: the line did not get its turn
  // there.
  if (asked.status === 503) return new VoiceNodeError("taken", asked.status, error);

  return new VoiceNodeError(asked.status >= 500 ? "failed" : "refused", asked.status, error);
}

// Waits on the node, a piece at a time, until the asker has ended: what it wrote, or how it ended without it.
async function askAfter(shell: NodeShell, dir: string, id: string, waitSeconds: number): Promise<Asked> {
  for (;;) {
    const polled = await shell.run(awaitCommand(dir, id, waitSeconds));
    const lines = polled.stdout.split("\n");
    const done = /^done (\d+)$/.exec(lines[0]?.trim() ?? "");

    if (lines[0]?.trim() === "gone") {
      throw new VoiceNodeError("failed", null, "the asker on the node ended without writing its end");
    }

    if (done !== null) {
      const answer = lines.slice(1).find((line) => line.trim().startsWith("{"));

      if (answer !== undefined) {
        try {
          return JSON.parse(answer) as Asked;
        } catch {
          // Said below as the asker's own failure.
        }
      }

      const said = lines.slice(1).filter((line) => !line.trim().startsWith("{")).join(" ").trim().slice(-300);

      throw new VoiceNodeError("failed", null, `the asker on the node ended ${done[1]} without an answer${said === "" ? "" : `: ${said}`}`);
    }
  }
}

// One line spoken on the node, as the wav it answered, waited for until the asker on the node has ended.
export async function speakOnNode(request: VoiceNodeRequest, deps: VoiceNodeDeps): Promise<VoiceNodeSpoken> {
  const { shell } = deps;
  const id = `${Date.now().toString(36)}-${randomBytes(4).toString("hex")}`;
  let dir: string | null = null;

  try {
    const started = await shell.run(startCommand(id), JSON.stringify(request));

    if (started.code === NOT_INSTALLED) {
      throw new VoiceNodeError("unreachable", null, started.stderr.trim() || "the voice server is not installed on this node");
    }

    dir = started.stdout.trim().split("\n").pop()?.trim() ?? "";

    if (started.code !== 0 || !dir.startsWith("/")) {
      throw new VoiceNodeError("failed", null, `the node did not start asking: ${started.stderr.trim().slice(-300)}`);
    }

    const asked = await askAfter(shell, dir, id, deps.askWaitSeconds ?? ASK_WAIT_SECONDS);

    if (asked.status !== 200) {
      throw failureOf(asked);
    }

    const wav = await shell.get(`${dir}/${id}.wav`);

    if (wav.byteLength < 44 || new TextDecoder().decode(wav.subarray(0, 4)) !== "RIFF") {
      throw new VoiceNodeError("bad-answer", asked.status, "the answer is not a wav");
    }

    return {
      wav,
      seconds: asked.seconds ?? 0,
      loadSeconds: asked.loadSeconds ?? 0,
      speakSeconds: asked.speakSeconds ?? 0,
      checkpoint: asked.checkpoint ?? "",
    };
  } catch (error) {
    if (error instanceof NodeShellError) {
      throw new VoiceNodeError("unreachable", null, error.message);
    }

    throw error;
  } finally {
    if (dir !== null && dir.startsWith("/")) {
      // The line and its sound leave the node with the call; a node gone meanwhile keeps them until the next call's
      // sweep.
      await shell.run(`rm -f ${shellWord(dir)}/${shellWord(id)}.*`).catch(async () => undefined);
    }
  }
}

export interface NodeSpeech {
  voice: VoiceRow;
  text: string;
  out: string;
  shell: NodeShell;
  askWaitSeconds?: number;
}

// The whole of speaking through the node: the line spoken there, then encoded here into `out` by the voice's
// encoder, the same OGG/Opus this machine's card writes. A failure of the node throws a VoiceNodeError; a failure
// of the encode step, which is this machine's own, throws a plain Error.
export async function speakThroughNode(speech: NodeSpeech): Promise<VoiceNodeSpoken> {
  const spoken = await speakOnNode(voiceNodeRequest(speech.voice, speech.text), {
    shell: speech.shell,
    ...(speech.askWaitSeconds === undefined ? {} : { askWaitSeconds: speech.askWaitSeconds }),
  });
  const wav = `${speech.out}.node.wav`;

  writeFileSync(wav, spoken.wav, { mode: 0o600 });

  try {
    await encodeWav(wav, speech.out).catch((error: unknown) => {
      throw new Error(`the encode step failed: ${error instanceof Error ? error.message : String(error)}`);
    });
  } finally {
    try {
      unlinkSync(wav);
    } catch {
      // Already gone: nothing to clear.
    }
  }

  return spoken;
}

// ------------------------------------------------------------------- which node speaks a line

// What a node's server says of itself: the checkpoint it holds loaded, if any, whether it is speaking a line this
// moment, whether its card has room to speak one (false where other work holds the card's lock, or leaves too
// little of it for the model to load) and whether that other work holds the lock this moment, which is what
// waiting in the node's own line can wait out.
export interface VoiceNodeHealth {
  loaded: string | null;
  busy: boolean;
  room?: boolean;
  cardTaken?: boolean;
  [more: string]: unknown;
}

// The node's server asked what it holds, through the node's shell, its answer waited for. A node that cannot be
// asked, or a server that is not listening, throws as out of reach.
export async function voiceNodeHealth(shell: NodeShell): Promise<VoiceNodeHealth> {
  let ran;

  try {
    ran = await shell.run(`${voiceOnNode} --ask --port ${VOICE_PORT} --token-file "$HOME/${VOICE_TOKEN}"`);
  } catch (error) {
    throw new VoiceNodeError("unreachable", null, error instanceof Error ? error.message : String(error));
  }

  const last = ran.stdout.trim().split("\n").pop() ?? "";
  let answer: { status?: number | null; error?: string | null; health?: VoiceNodeHealth };

  try {
    answer = JSON.parse(last) as typeof answer;
  } catch {
    throw new VoiceNodeError("unreachable", null, (ran.stderr.trim() || "the server's asker said nothing").slice(-300));
  }

  if (answer.status !== 200 || answer.health === undefined) {
    throw new VoiceNodeError("unreachable", answer.status ?? null, answer.error ?? null);
  }

  return answer.health;
}

// The lock file on this machine a run holds a node through while it speaks there.
export function voiceNodeLockPath(name: string): string {
  return join(locksFolder(), `voice-node-${name}.lock`);
}

// Takes the node's lock without waiting: a letting-go where it was taken, null where another run on this machine
// holds it. The lock is held by a small process reading this one's pipe, so a run that dies lets go with it. The
// letting-go settles once that process has gone, which is when the lock is free for the next run to take.
export async function holdVoiceNode(path: string): Promise<(() => Promise<void>) | null> {
  await readyVoiceLock(path);

  const holder = Bun.spawn(
    ["flock", "--nonblock", "--conflict-exit-code", String(voiceQueueExit), path, "sh", "-c", "echo held; exec cat > /dev/null"],
    { stdin: "pipe", stdout: "pipe", stderr: "ignore" },
  );
  const first = await holder.stdout.getReader().read();

  if (first.done || !new TextDecoder().decode(first.value).startsWith("held")) {
    await holder.exited;

    return null;
  }

  // The pipe's end is what lets go. flock keeps the lock open in the reader it started as well as in itself and
  // waits for that reader, so it exits only once the reader has read to the end and the lock is free; a signal to
  // flock would end it while the reader still holds the lock.
  return () => {
    try {
      holder.stdin.end();
    } catch {
      // Already closed: the holder is on its way out.
    }

    return holder.exited.then(async () => undefined);
  };
}

export interface VoiceNodeTaken {
  name: string;
  shell: NodeShell;
  // How long the run stood waiting for a free node.
  waitedMs: number;
  // Whether other work held the node's card as it was taken, so the line waits its turn there.
  cardHeld?: boolean;
  release(): void | Promise<unknown>;
}

export interface VoiceNodeChoice {
  names: readonly string[];
  // The checkpoint the line is spoken in, by its name under the voices root: a node that holds it loaded is taken
  // first.
  checkpoint: string;
  pollMs?: number;
  shellOf(name: string): Promise<NodeShell>;
}

// One node of the list, held for this run until it lets go (docs/voice-plan.md, 「Which node speaks a line」). A
// free one is not held by another run here, answers its health without speaking and has room on its card; among
// the free, a warm one goes first and the list's order decides the rest. With none free, a node lacking room only
// because other work holds its card's lock is taken the same way, so the line waits its turn in that node's own
// line. Every node out of reach throws at once; while every node is taken or busy they are asked again until one is
// free, however long that takes.
export async function takeVoiceNode(choice: VoiceNodeChoice): Promise<VoiceNodeTaken> {
  const started = Date.now();
  const warm = choice.checkpoint.split("/").filter(Boolean).pop() ?? "";

  for (;;) {
    const held: { name: string; release(): void | Promise<void> }[] = [];
    let takenHere = false;

    for (const name of choice.names) {
      const release = await holdVoiceNode(voiceNodeLockPath(name));

      if (release === null) {
        takenHere = true;
      } else {
        held.push({ name, release });
      }
    }

    const asked = await Promise.all(
      held.map(async (one) => {
        try {
          const shell = await choice.shellOf(one.name);

          return { ...one, shell, state: await voiceNodeHealth(shell), fault: null };
        } catch (error) {
          return { ...one, shell: null, state: null, fault: error instanceof Error ? error.message : String(error) };
        }
      }),
    );
    const answering = asked.filter((one) => one.state !== null && !one.state.busy);
    const free = answering.filter((one) => one.state?.room !== false);
    const cardHeld = answering.filter((one) => one.state?.room === false && one.state.cardTaken === true);
    const among = free.length > 0 ? free : cardHeld;
    const chosen = among.find((one) => one.state?.loaded === warm) ?? among[0];

    // Let go of the rest before asking again, so the next round does not find its own locks held.
    await Promise.all(asked.filter((one) => one !== chosen).map((one) => one.release()));

    if (chosen !== undefined && chosen.shell !== null) {
      return { name: chosen.name, shell: chosen.shell, waitedMs: Date.now() - started, cardHeld: free.length === 0, release: chosen.release };
    }

    if (!takenHere && asked.every((one) => one.state === null)) {
      const faults = asked.map((one) => `${one.name}: ${one.fault ?? "?"}`).join("; ");

      throw new VoiceNodeError("unreachable", null, faults === "" ? "the voice node list is empty" : faults);
    }

    await Bun.sleep(choice.pollMs ?? 2_000);
  }
}

export interface FreeNodeSpeech {
  voice: VoiceRow;
  text: string;
  out: string;
}

// The line spoken on one node of the list and encoded here, with which node it was, how long the run waited for
// it, and whether it waited its turn on a card other work held. A node whose arbiter gives the voice no place on its
// card is let go and the run asks again; the node is let go however the call ends.
export async function speakThroughFreeNode(speech: FreeNodeSpeech): Promise<VoiceNodeSpoken & { node: string; waitedMs: number; cardHeld: boolean }> {
  const started = Date.now();
  let cardHeld = false;

  for (;;) {
    const taken = await takeVoiceNode({
      names: await voiceNodeNames(),
      checkpoint: voiceNodeRequest(speech.voice, speech.text).checkpoint,
      shellOf: (name) => voiceNodeShell(name),
    });
    const waitedMs = Date.now() - started;

    cardHeld ||= taken.cardHeld === true;

    try {
      const spoken = await speakThroughNode({
        voice: speech.voice,
        text: speech.text,
        out: speech.out,
        shell: taken.shell,
      });

      return { ...spoken, node: taken.name, waitedMs, cardHeld };
    } catch (error) {
      if (!(error instanceof VoiceNodeError && error.kind === "taken")) {
        throw error;
      }
    } finally {
      await taken.release();
    }
  }
}
