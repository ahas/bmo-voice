import { appendFileSync, closeSync, existsSync, mkdirSync, openSync, readdirSync, readFileSync, statSync, unlinkSync, writeSync } from "node:fs";
import { join } from "node:path";

import { NodeShellError, shellWord } from "./node-shell";
import { VOICE_PLACE, VOICE_SERVICE } from "./voice-node";

import type { NodeShell } from "./node-shell";

// One step of a voice-training experiment placed on a free card, fed what it needs, waited out and its results
// brought into the record (docs/voice-plan.md, 「Placing a step on a card」). The operations machine is the one place
// every node's shell is reached from, so the choosing, the carrying and the waiting all happen here and the node only
// runs the step. Whether a card is free is the node's card arbiter's to say, through its hold (the arbiter's
// docs/api.md, 「The node arbiter's hold」, 「The node arbiter's units」), and the step itself waits its turn and
// holds the card lock through the arbiter's turn client (「The turn client」).

// Where the experiments stand under the account's home on every node.
export const TRAIN_PLACE = "bmo-voice-train";
// The voice server's unit; every other active `bmo-voice-*` unit on a node is a step of training.
const SERVER_UNIT = "bmo-voice-server.service";
// The card arbiter's installation under the account's home, its turn client and its environment file, and the port
// its hold answers on where that file names none.
const ARBITER_PLACE = "bmo-card";
const ARBITER_ENV = ".config/bmo-card/env";
const TURN_PORT = 7439;
// What an experiment's run writes rather than reads, which is never carried out to a node.
const OUTPUTS = ["runs", "out", "steps"];
// Record files that are a run's own output rather than a script, which a node keeps as it wrote them.
const RECORD_KEEPS = /\.(log|out)$/;
// A hub refuses a single upload over 128 MiB, so a carried archive goes up in pieces well below it.
export const CARRY_PIECE = 64 * 1024 * 1024;
const STEP_NAME = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/;
// A path brought back is a plain relative path under the experiment, `*` allowed; the node's shell expands it, and a
// pattern that matches nothing is passed over like a missing path.
const BRING_PATH = /^(?!\/)(?!.*\.\.)[A-Za-z0-9_.*/-]+$/;

export type CardKind = "own" | "shared";

// What the node's card arbiter says of its card, as its units route answers: the card the driver names, or none; the
// pids holding the card lock; the claimant the card waits for, or none; and the tickets in line.
export interface ArbiterState {
  card: unknown;
  lockHeldBy: number[];
  claim: unknown;
  queue: { rest?: boolean }[];
}

// What one look at a node says about its card.
export interface CardProbe {
  // The voice environment is built there.
  env: boolean;
  // The card arbiter's answer, or null where none answered on the node.
  arbiter: ArbiterState | null;
  // The voice server's unit is running there, and the pid it runs as.
  server: boolean;
  serverPid: number | null;
  // Every active `bmo-voice-*` unit but the server: training steps, ours or an older pipeline's.
  units: string[];
}

// The shell text that answers a probe, one `key=value` line each; the arbiter's answer is one line of JSON, asked of
// its hold on the node's loopback with the standard library of the node's Python.
export const probeCommand = [
  `printf 'env=%s\\n' "$([ -x "$HOME/${VOICE_PLACE}/.venv/bin/python" ] && echo yes || echo no)"`,
  `printf 'server=%s\\n' "$(systemctl --user is-active ${SERVER_UNIT} 2>/dev/null)"`,
  `printf 'server_pid=%s\\n' "$(systemctl --user show -p MainPID --value ${SERVER_UNIT} 2>/dev/null)"`,
  `printf 'units=%s\\n' "$(systemctl --user list-units --plain --no-legend --state=active 'bmo-voice-*' 2>/dev/null | awk '{print $1}' | grep -vx '${SERVER_UNIT}' | paste -sd, -)"`,
  `printf 'arbiter=%s\\n' "$(set -a; . "$HOME/${ARBITER_ENV}" 2>/dev/null; set +a; python3 -c 'import json, os, urllib.request; print(json.dumps(json.load(urllib.request.urlopen("http://127.0.0.1:%s/units" % os.environ.get("CARD_TURN_PORT", "${TURN_PORT}")))))' 2>/dev/null)"`,
].join("\n");

export function parseProbe(text: string): CardProbe {
  const values = new Map<string, string>();

  for (const line of text.split("\n")) {
    const at = line.indexOf("=");

    if (at > 0) values.set(line.slice(0, at).trim(), line.slice(at + 1).trim());
  }

  const list = (key: string): string[] => (values.get(key) ?? "").split(",").map((one) => one.trim()).filter((one) => one !== "");
  const pid = Number(values.get("server_pid") ?? "");
  let arbiter: ArbiterState | null = null;

  try {
    const read = JSON.parse(values.get("arbiter") ?? "") as Partial<ArbiterState>;

    arbiter = {
      card: read.card ?? null,
      lockHeldBy: Array.isArray(read.lockHeldBy) ? read.lockHeldBy.filter((one) => Number.isInteger(one) && one > 0) : [],
      claim: read.claim ?? null,
      queue: Array.isArray(read.queue) ? read.queue : [],
    };
  } catch {
    // No arbiter answered on the node.
  }

  return {
    env: values.get("env") === "yes",
    arbiter,
    server: values.get("server") === "active",
    serverPid: Number.isInteger(pid) && pid > 0 ? pid : null,
    units: list("units"),
  };
}

// A node a step may be put on at all: the environment built, and a card arbiter answering for a card.
export function trainingNode(probe: CardProbe): boolean {
  return probe.env && probe.arbiter !== null && probe.arbiter.card !== null;
}

// Free: no claim on the card, nobody holding its lock but the voice server, nobody in line for it but to rest, and no
// step of training running on the node.
export function cardFree(probe: CardProbe): boolean {
  const arbiter = probe.arbiter;

  return trainingNode(probe)
    && arbiter !== null
    && arbiter.claim === null
    && arbiter.lockHeldBy.every((pid) => pid === probe.serverPid)
    && arbiter.queue.every((ticket) => ticket.rest === true)
    && probe.units.length === 0;
}

// A card the voice server stands on is shared with it, loaded or not, since a synthesis may load it at any moment.
export function cardKind(probe: CardProbe): CardKind {
  return probe.server ? "shared" : "own";
}

// What stands on a card, in a word a person reads.
export function cardState(probe: CardProbe): string {
  if (!probe.env) return "no voice environment";
  if (probe.arbiter === null) return "no card arbiter";
  if (probe.arbiter.card === null) return "no card";
  if (probe.units.length > 0) return `training (${probe.units.join(", ")})`;
  if (probe.arbiter.claim !== null) return `claimed (${JSON.stringify(probe.arbiter.claim)})`;

  const others = probe.arbiter.lockHeldBy.filter((pid) => pid !== probe.serverPid);

  if (others.length > 0) return `in use (pid ${others.join(", ")})`;

  const waiting = probe.arbiter.queue.filter((ticket) => ticket.rest !== true).length;

  return waiting > 0 ? `asked for (${waiting} in line)` : "free";
}

// The first free card, a card of its own before one shared with the voice server, in the order the nodes were given.
export function pickCard<T extends { probe: CardProbe }>(looked: readonly T[]): T | null {
  const free = looked.filter((one) => cardFree(one.probe));

  return free.find((one) => cardKind(one.probe) === "own") ?? free[0] ?? null;
}

// One file as a listing names it: its path under the home, its size and its whole-second mtime.
export interface Listed {
  path: string;
  size: number;
  mtime: number;
}

// The shell text that lists every file under the given paths (under the home), leaving out an experiment's own
// outputs.
export function listCommand(experimentDir: string, paths: readonly string[]): string {
  const prune = OUTPUTS.map((name) => `-path ${shellWord(`${experimentDir}/${name}`)}`).join(" -o ");
  const roots = [experimentDir, ...paths].map(shellWord).join(" ");

  return `cd "$HOME" && find ${roots} \\( ${prune} -o -name __pycache__ \\) -prune -o -type f -printf '%p\\t%s\\t%T@\\n' 2>/dev/null; true`;
}

export function parseListing(text: string): Listed[] {
  const files: Listed[] = [];

  for (const line of text.split("\n")) {
    const [path, size, mtime] = line.split("\t");

    if (path === undefined || path === "" || size === undefined || mtime === undefined) continue;
    files.push({ path, size: Number(size), mtime: Math.floor(Number(mtime)) });
  }

  return files;
}

// What the node lacks or holds differently from the home, by size and whole-second mtime, which is what an archive
// keeps of a file.
export function carryList(home: readonly Listed[], there: readonly Listed[]): string[] {
  const held = new Map(there.map((one) => [one.path, one]));

  return home
    .filter((one) => {
      const other = held.get(one.path);

      return other === undefined || other.size !== one.size || other.mtime !== one.mtime;
    })
    .map((one) => one.path)
    .sort();
}

// A byte string cut into the pieces a hub carries.
export function pieces(bytes: Uint8Array, most = CARRY_PIECE): Uint8Array[] {
  const out: Uint8Array[] = [];

  for (let at = 0; at < bytes.length; at += most) out.push(bytes.subarray(at, Math.min(bytes.length, at + most)));

  return out.length > 0 ? out : [bytes];
}

// The files of a record an experiment's scripts are: its top-level files, but a run's logs. A record not made yet
// has none.
export function recordScripts(record: string): string[] {
  if (!existsSync(record)) return [];

  return readdirSync(record)
    .filter((name) => !RECORD_KEEPS.test(name) && statSync(join(record, name)).isFile())
    .sort();
}

export function stepUnit(experiment: string, step: string): string {
  return `bmo-voice-train-${experiment}-${step}`;
}

export function checkStepNames(experiment: string, step: string, bring: readonly string[] = []): string | null {
  if (!STEP_NAME.test(experiment)) return `the experiment name ${experiment} is not a plain folder name`;
  if (!STEP_NAME.test(step)) return `the step name ${step} may hold only letters, digits, '-' and '_'`;

  const odd = bring.find((path) => !BRING_PATH.test(path));

  return odd === undefined ? null : `${odd} is not a plain path under the experiment (letters, digits, '.', '_', '-', '/' and '*')`;
}

// One line of the record's steps.log.
export function stepsLogLine(row: { step: string; node: string; card: CardKind; started: Date; ended: Date; code: number | null }): string {
  return [row.started.toISOString(), row.ended.toISOString(), row.step, row.node, row.card, row.code === null ? "stopped" : String(row.code)].join("\t");
}

// --- The step itself ---------------------------------------------------------------------------------

export interface TrainNode {
  name: string;
  shell: NodeShell;
}

export interface StepPlan {
  experiment: string;
  step: string;
  // The shell text run in the experiment's folder on the node.
  command: string;
  // Further paths under the home the step reads, carried from the home node.
  carry: string[];
  // Paths under the experiment's folder brought into the record when the step ends.
  bring: string[];
  // The record's folder on the operations machine.
  record: string;
  home: TrainNode;
  candidates: TrainNode[];
}

export interface StepDeps {
  sleep(ms: number): Promise<void>;
  now(): Date;
  say(line: string): void;
  // Runs `work` while no other runner on this machine is choosing a card.
  lock<T>(work: () => Promise<T>): Promise<T>;
  // A local archive of `files` under `dir`, and one unpacked into `dir`.
  pack(dir: string, files: string[]): Promise<Uint8Array>;
  unpack(dir: string, bytes: Uint8Array): Promise<void>;
  pollMs?: number;
  waitMs?: number;
}

export interface StepEnd {
  node: string;
  card: CardKind;
  code: number | null;
}

const POLL_MS = 30_000;
const WAIT_MS = 60_000;

function experimentDir(plan: Pick<StepPlan, "experiment">): string {
  return `${TRAIN_PLACE}/experiments/${plan.experiment}`;
}

async function must(shell: TrainNode, command: string, input?: string): Promise<string> {
  const ran = await shell.shell.run(command, input);

  if (ran.code !== 0) {
    throw new NodeShellError(`${shell.name} refused: ${command.split("\n")[0]}\n${ran.stderr.trim().slice(-1_000)}`);
  }

  return ran.stdout;
}

async function probe(node: TrainNode): Promise<CardProbe | null> {
  try {
    return parseProbe(await must(node, probeCommand));
  } catch {
    return null;
  }
}

export async function lookAll(nodes: readonly TrainNode[]): Promise<{ node: TrainNode; probe: CardProbe | null }[]> {
  return await Promise.all(nodes.map(async (node) => ({ node, probe: await probe(node) })));
}

// Bytes written to a file on the node, in pieces a hub carries, joined there.
async function putLarge(node: TrainNode, path: string, bytes: Uint8Array): Promise<void> {
  const parts = pieces(bytes);

  if (parts.length === 1) {
    await node.shell.put(path, bytes);

    return;
  }

  const names = parts.map((_, at) => `${path}.part${String(at).padStart(3, "0")}`);

  for (const [at, part] of parts.entries()) await node.shell.put(names[at] as string, part);
  await must(node, `cat ${names.map(shellWord).join(" ")} > ${shellWord(path)} && rm -f ${names.map(shellWord).join(" ")}`);
}

// The home's copy of what the step reads, then the record's scripts over it.
async function carry(plan: StepPlan, node: TrainNode, deps: StepDeps): Promise<void> {
  const dir = experimentDir(plan);
  const tag = `${plan.experiment}-${plan.step}-${process.pid}`;

  if (node.name !== plan.home.name) {
    const listing = listCommand(dir, plan.carry);
    const wanted = carryList(parseListing(await must(plan.home, listing)), parseListing(await must(node, listing)));

    if (wanted.length > 0) {
      const list = `/tmp/bmo-voice-carry-${tag}.list`;
      const archive = `/tmp/bmo-voice-carry-${tag}.tgz`;

      await plan.home.shell.put(list, new TextEncoder().encode(`${wanted.join("\n")}\n`));
      await must(plan.home, `cd "$HOME" && tar czf ${archive} -T ${list}; code=$?; rm -f ${list}; exit $code`);

      try {
        const bytes = await plan.home.shell.get(archive);

        deps.say(`${node.name}: carrying ${wanted.length} files (${Math.round(bytes.length / 2 ** 20)} MiB) from ${plan.home.name}`);
        await putLarge(node, archive, bytes);
      } finally {
        await plan.home.shell.run(`rm -f ${archive}`);
      }

      await must(node, `cd "$HOME" && tar xzf ${archive}; code=$?; rm -f ${archive}; exit $code`);
    }
  }

  const scripts = recordScripts(plan.record);

  if (scripts.length > 0) {
    const archive = `/tmp/bmo-voice-record-${tag}.tgz`;

    await putLarge(node, archive, await deps.pack(plan.record, scripts));
    await must(node, `mkdir -p "$HOME"/${dir} && tar xzf ${archive} -C "$HOME"/${dir}; code=$?; rm -f ${archive}; exit $code`);
  }
}

async function start(plan: StepPlan, node: TrainNode, card: CardKind): Promise<void> {
  const dir = `"$HOME"/${experimentDir(plan)}`;
  const unit = stepUnit(plan.experiment, plan.step);
  const steps = `steps/${plan.step}`;
  // The arbiter's own interpreter where its installation carries one, the system's otherwise.
  const turn = [
    `p="$HOME/${ARBITER_PLACE}/.venv/bin/python"; [ -x "$p" ] || p=python3`,
    `"$p" "$HOME/${ARBITER_PLACE}/turn.py" --service ${VOICE_SERVICE} -- bash ${steps}.cmd`,
  ].join("\n");

  // The unit runs a file rather than a command line, since systemd expands `$` and `%` in the line it is given. The
  // step's command runs inside the turn client, which waits for the voice's turn on the card, holds the card lock
  // around it and exits with its code, or 3 where the voice has no place on the node.
  await must(node, `mkdir -p ${dir}/steps && cat > ${dir}/${steps}.cmd`, `${plan.command}\n`);
  await must(node, `cat > ${dir}/${steps}.run`, `${turn}\necho $? > ${steps}.code\n`);
  await must(
    node,
    [
      `cd ${dir} && rm -f ${steps}.code`,
      `systemd-run --user --collect --unit=${unit} --working-directory="$PWD" --setenv=BMO_VOICE_CARD=${card} --setenv=BMO_CARD_SERVICE=${VOICE_SERVICE}`
        + ` -p StandardOutput=append:"$PWD"/${steps}.log -p StandardError=append:"$PWD"/${steps}.log`
        + ` /bin/bash "$PWD"/${steps}.run`,
    ].join("\n"),
  );
}

// The step's exit, once it has one; null where the unit ended without writing it. A node out of reach is looked at
// again at the same pace, however long it stays so, since the step runs on there.
async function waitOut(plan: StepPlan, node: TrainNode, deps: StepDeps): Promise<number | null> {
  const dir = `"$HOME"/${experimentDir(plan)}`;
  const look = `cat ${dir}/steps/${plan.step}.code 2>/dev/null; printf 'unit=%s\\n' "$(systemctl --user is-active ${stepUnit(plan.experiment, plan.step)})"`;

  for (;;) {
    await deps.sleep(deps.pollMs ?? POLL_MS);

    let text: string;

    try {
      text = (await node.shell.run(look)).stdout;
    } catch {
      continue;
    }

    const code = /^(\d+)$/m.exec(text)?.[1];

    if (code !== undefined) return Number(code);
    if (!/^unit=(active|activating)$/m.test(text)) return null;
  }
}

async function bring(plan: StepPlan, node: TrainNode, deps: StepDeps): Promise<void> {
  const dir = `"$HOME"/${experimentDir(plan)}`;
  const archive = `/tmp/bmo-voice-bring-${plan.experiment}-${plan.step}-${process.pid}.tgz`;
  const paths = [...plan.bring, `steps/${plan.step}.log`, `steps/${plan.step}.cmd`].join(" ");

  await must(node, `cd ${dir} && set -- ${paths} && tar czf ${archive} --ignore-failed-read --exclude=resume.pt "$@" 2>/dev/null; test -f ${archive}`);

  try {
    await deps.unpack(plan.record, await node.shell.get(archive));
  } finally {
    await node.shell.run(`rm -f ${archive}`);
  }
}

// One step from start to end: a free card chosen and fed, the step run apart and waited out, and what it made
// brought into the record.
export async function runStep(plan: StepPlan, deps: StepDeps): Promise<StepEnd> {
  const named = checkStepNames(plan.experiment, plan.step, plan.bring);

  if (named !== null) throw new Error(named);

  let placed: { node: TrainNode; card: CardKind; started: Date } | null = null;
  let told = false;

  while (placed === null) {
    placed = await deps.lock(async () => {
      const looked = (await lookAll(plan.candidates)).flatMap((one) => (one.probe === null ? [] : [{ node: one.node, probe: one.probe }]));
      const chosen = pickCard(looked);

      if (chosen === null) return null;

      await carry(plan, chosen.node, deps);

      const again = await probe(chosen.node);

      if (again === null || !cardFree(again)) return null;

      const card = cardKind(again);

      await start(plan, chosen.node, card);
      deps.say(`${plan.step}: running on ${chosen.node.name} (${card} card) as ${stepUnit(plan.experiment, plan.step)}`);

      return { node: chosen.node, card, started: deps.now() };
    });

    if (placed === null) {
      if (!told) deps.say(`${plan.step}: every card it may use is taken; asking again every minute`);
      told = true;
      await deps.sleep(deps.waitMs ?? WAIT_MS);
    }
  }

  const code = await waitOut(plan, placed.node, deps);

  await bring(plan, placed.node, deps);
  mkdirSync(plan.record, { recursive: true });
  appendFileSync(join(plan.record, "steps.log"), `${stepsLogLine({ step: plan.step, node: placed.node.name, card: placed.card, started: placed.started, ended: deps.now(), code })}\n`);
  deps.say(`${plan.step}: ended on ${placed.node.name} with ${code === null ? "no exit (stopped)" : `exit ${code}`}`);

  return { node: placed.node.name, card: placed.card, code };
}

// --- The lock on this machine --------------------------------------------------------------------------

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);

    return true;
  } catch {
    return false;
  }
}

// Runs `work` holding a lock file that names this process; a lock whose holder is gone is taken over.
export async function withLockFile<T>(path: string, work: () => Promise<T>, sleep: (ms: number) => Promise<void> = Bun.sleep): Promise<T> {
  for (;;) {
    let fd: number;

    try {
      fd = openSync(path, "wx");
    } catch {
      const holder = existsSync(path) ? Number(readFileSync(path, "utf8").trim()) : NaN;

      if (Number.isInteger(holder) && holder > 0 && alive(holder)) {
        await sleep(2_000);
      } else {
        try {
          unlinkSync(path);
        } catch {
          // Another runner took it over first.
        }
      }

      continue;
    }

    writeSync(fd, `${process.pid}\n`);
    closeSync(fd);

    try {
      return await work();
    } finally {
      unlinkSync(path);
    }
  }
}
