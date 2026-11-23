// A training step asks the card arbiter's queue for its turn whoever holds the card, rather than waiting for the card
// to stand free (docs/voice-plan.md, 「Placing a step on a card」): which card it goes to, and a step started at once
// on a card another occupant holds, run here against a node's shell made in the test.

import { afterEach, beforeEach, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { cardState, parseProbe, pickCard, probeCommand, runStep, trainingNode } from "../scripts/lib/voice-train";

import type { ArbiterState, CardProbe, TrainNode } from "../scripts/lib/voice-train";
import type { ShellRun } from "../scripts/lib/node-shell";

const SERVER_PID = 100;
// A holder of the lock that is not the voice server, as an image engine holding the card from its start.
const OTHER_HOLDER = 4242;

function arbiter(state: Partial<ArbiterState> = {}): ArbiterState {
  return { card: { name: "card" }, lockHeldBy: [], claim: null, queue: [], ...state };
}

function probe(state: Partial<CardProbe> = {}): CardProbe {
  return { env: true, arbiter: arbiter(), server: false, serverPid: null, units: [], ...state };
}

const idleOwn = probe();
const idleShared = probe({ server: true, serverPid: SERVER_PID, arbiter: arbiter({ lockHeldBy: [SERVER_PID] }) });
const heldOwn = probe({ arbiter: arbiter({ lockHeldBy: [OTHER_HOLDER] }) });
const heldShared = probe({ server: true, serverPid: SERVER_PID, arbiter: arbiter({ lockHeldBy: [OTHER_HOLDER] }) });

// The node arbiter's card read as it answers (bmo-arbiter's docs/api.md, 「The card」), the driver's card under
// `device`, its lock held by another occupant's turn client.
function cardRead(fields: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    version: "0.1.0",
    desktop: false,
    occupants: ["joy", "comfyui", "voice"],
    ranks: [["joy"], ["comfyui", "voice"]],
    tiers: { joy: 0, comfyui: 2, voice: 2 },
    claim: null,
    lockHeldBy: [OTHER_HOLDER],
    busy: true,
    queue: [],
    places: [],
    typicalHoldMs: {},
    units: { items: [], more: false },
    device: { name: "card", totalMiB: 12282, usedMiB: 858, freeMiB: 11424, utilizationPercent: 0, processes: [] },
    stage: { waiting: false, waitingMs: 0, graphs: 0, waits: 0, waitedMs: 0 },
    ram: { dumpsMiB: 0, availableMiB: 59436, maxMiB: null, floorMiB: 0, dump: "disk" },
    rules: { loadedAt: 1, error: null },
    ...fields,
  };
}

function probeAnswer(read: Record<string, unknown>): string {
  return `env=yes\nserver=active\nserver_pid=${SERVER_PID}\nunits=\narbiter=${JSON.stringify(read)}\n`;
}

test("the probe reads the node arbiter's card read and takes the card the driver names", () => {
  expect(probeCommand).toContain("/card\"");

  const held = parseProbe(probeAnswer(cardRead()));

  expect(trainingNode(held)).toBe(true);
  expect(held.arbiter?.lockHeldBy).toEqual([OTHER_HOLDER]);
  expect(cardState(held)).toBe(`in use (pid ${OTHER_HOLDER})`);
  expect(cardState(parseProbe(probeAnswer(cardRead({ device: undefined }))))).toBe("no card");
});

function named(entries: [string, CardProbe][]): { name: string; probe: CardProbe }[] {
  return entries.map(([name, one]) => ({ name, probe: one }));
}

test("a card another occupant holds takes the step where it is the only one", () => {
  for (const held of [heldOwn, heldShared, probe({ arbiter: arbiter({ claim: { claimantName: "runner" } }) }), probe({ arbiter: arbiter({ queue: [{}] }) })]) {
    expect(pickCard(named([["n2", held]]))?.name).toBe("n2");
  }
});

test("an idle card goes before a busy one, and a card of its own before the voice server's", () => {
  const all = named([["held-shared", heldShared], ["held-own", heldOwn], ["idle-shared", idleShared], ["idle-own", idleOwn]]);

  expect(pickCard(all)?.name).toBe("idle-own");
  expect(pickCard(all.filter((one) => one.name !== "idle-own"))?.name).toBe("idle-shared");
  expect(pickCard(all.filter((one) => one.name.startsWith("held")))?.name).toBe("held-own");
  expect(pickCard(named([["first", heldOwn], ["second", heldOwn]]))?.name).toBe("first");
});

test("a node running a training step, or lacking the environment, an arbiter or a card, takes none", () => {
  const none = named([
    ["training", probe({ units: ["bmo-voice-train-run-a"] })],
    ["no-env", probe({ env: false })],
    ["no-arbiter", probe({ arbiter: null })],
    ["no-card", probe({ arbiter: arbiter({ card: null }) })],
  ]);

  expect(pickCard(none)).toBeNull();
  expect(pickCard([...none, ...named([["held", heldShared]])])?.name).toBe("held");
});

let record: string;

beforeEach(() => {
  record = mkdtempSync(join(tmpdir(), "voice-train-card-"));
});

afterEach(() => {
  rmSync(record, { recursive: true, force: true });
});

// A node whose card the arbiter says another occupant holds, that runs a step to exit 0 once started.
function heldNode(): { node: TrainNode; written: Map<string, string> } {
  const written = new Map<string, string>();
  const answer = (stdout = ""): ShellRun => ({ code: 0, stdout, stderr: "" });
  const node: TrainNode = {
    name: "n2",
    shell: {
      run: async (command, input) => {
        if (command === probeCommand) return answer(probeAnswer(cardRead()));

        const file = /cat > (\S+)$/.exec(command)?.[1];

        if (file !== undefined) written.set(file.split("/").pop() as string, String(input ?? ""));
        if (command.startsWith("cat ") && command.includes("steps/train.code")) return answer("0\nunit=inactive\n");

        return answer();
      },
      put: async () => {},
      get: async () => new Uint8Array(),
    },
  };

  return { node, written };
}

test("a step on a card another occupant holds starts at once inside the turn client, as the voice", async () => {
  const { node, written } = heldNode();
  const slept: number[] = [];
  const said: string[] = [];

  const end = await runStep(
    { experiment: "run-a", step: "train", command: "python train.py", carry: [], bring: [], record, home: node, candidates: [node] },
    {
      sleep: async (ms) => {
        slept.push(ms);
      },
      now: () => new Date(0),
      say: (line) => said.push(line),
      lock: (work) => work(),
      pack: async () => new Uint8Array(),
      unpack: async () => {},
      pollMs: 1,
      waitMs: 60_000,
    },
  );

  expect(end).toEqual({ node: "n2", card: "shared", code: 0 });
  expect(slept).not.toContain(60_000);
  expect(written.get("train.cmd")).toBe("python train.py\n");
  expect(written.get("train.run")).toContain(`/bmo-card/turn.py" --service voice -- bash steps/train.cmd`);
  expect(said[0]).toContain("waiting there for its turn");
  expect(readFileSync(join(record, "steps.log"), "utf8")).toContain("\ttrain\tn2\tshared\t0\n");
});
