// A push lays the data root's voices directory on a node as the data root holds it (docs/voice-plan.md, 「Building
// and updating them」): the pack and the node's line run here against a node's home made in a temporary folder.

import { afterEach, beforeEach, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import { voicesLaying, voicesPack } from "../scripts/lib/voice-node";

const place = "voice-place";

let scratch: string;
let dataVoices: string;
let nodeHome: string;
let nodeVoices: string;

// Files written under a folder, each by its place under it.
function write(root: string, files: Record<string, string>): void {
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), text);
  }
}

// Every file under a folder by its place under it, with what it holds.
function holding(root: string): Record<string, string> {
  const found: Record<string, string> = {};

  for (const entry of readdirSync(root, { recursive: true, withFileTypes: true })) {
    if (entry.isFile()) {
      const path = join(entry.parentPath, entry.name);

      found[path.slice(root.length + 1)] = readFileSync(path, "utf8");
    }
  }

  return found;
}

// One push's voices: the data root's packed, left on the node and laid there by the node's line.
function push(): { code: number; stderr: string } {
  writeFileSync(join(nodeHome, place, "voices.tar"), voicesPack(dataVoices));

  const ran = Bun.spawnSync(["sh", "-c", voicesLaying(place)], { env: { ...process.env, HOME: nodeHome } });

  return { code: ran.exitCode, stderr: ran.stderr.toString() };
}

beforeEach(() => {
  scratch = mkdtempSync(join(tmpdir(), "voice-node-push-"));
  dataVoices = join(scratch, "data/voices");
  nodeHome = join(scratch, "home");
  nodeVoices = join(nodeHome, place, "voices");
  write(dataVoices, {
    "voice-a/model.pt": "weights a",
    "voice-a/reference.wav": "clip a",
    "voice-b/model.pt": "weights b",
  });
  // What stands beside the voices directory on the node, which a push leaves as it is.
  write(join(nodeHome, place), { "s2-pro/base.pt": "base", "spoken/call.json": "call" });
  write(nodeHome, { "other/file": "other" });
});

afterEach(() => {
  rmSync(scratch, { recursive: true, force: true });
});

const besides = (): Record<string, string> => ({
  ...holding(join(nodeHome, place, "s2-pro")),
  ...holding(join(nodeHome, place, "spoken")),
  ...holding(join(nodeHome, "other")),
});

test("a voice folder the data root no longer holds is gone from the node, and nothing beside the voices is touched", () => {
  expect(push().code).toBe(0);
  write(nodeVoices, { "bench-scratch/model.pt": "scratch weights" });
  const before = besides();

  expect(push()).toEqual({ code: 0, stderr: "" });
  expect(holding(nodeVoices)).toEqual(holding(dataVoices));
  expect(readdirSync(join(nodeHome, place)).sort()).toEqual(["s2-pro", "spoken", "voices"]);
  expect(besides()).toEqual(before);
});

test("a voice folder the data root still holds stands as the data root has it, without its resume state", () => {
  expect(push().code).toBe(0);
  write(dataVoices, { "voice-a/model.pt": "weights a, trained on", "voice-a/resume.pt": "optimiser" });
  rmSync(join(dataVoices, "voice-a/reference.wav"));

  expect(push().code).toBe(0);
  expect(holding(nodeVoices)).toEqual({ "voice-a/model.pt": "weights a, trained on", "voice-b/model.pt": "weights b" });
});

test("a push of what the node already holds leaves it as it was", () => {
  expect(push().code).toBe(0);
  const before = holding(nodeVoices);

  expect(push().code).toBe(0);
  expect(holding(nodeVoices)).toEqual(before);
  expect(before).toEqual(holding(dataVoices));
});

test("a pack that does not unpack leaves the node's voices directory as it stood", () => {
  expect(push().code).toBe(0);
  const before = holding(nodeVoices);
  writeFileSync(join(nodeHome, place, "voices.tar"), "not a tar");

  const ran = Bun.spawnSync(["sh", "-c", voicesLaying(place)], { env: { ...process.env, HOME: nodeHome } });

  expect(ran.exitCode).not.toBe(0);
  expect(holding(nodeVoices)).toEqual(before);
});
