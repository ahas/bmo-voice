import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { parseEnv } from "node:util";

import { createParser } from "eventsource-parser";

import type { HubTarget } from "./node";

// A hub node's hub, asked from here, as the node service's target names it (the node service's `docs/api.md`,
// 「Registry routes」): a command run on the node with its output streamed back as server-sent events, and a whole
// file carried either way. Every call presents the hub's control token, read out of the hub's token file for that
// one request and put nowhere else. This is the scripts' whole client of the hub.

// The token file's path, a leading `~/` read as the account's home.
function tokenPath(tokenFile: string): string {
  return tokenFile.startsWith("~/") ? join(homedir(), tokenFile.slice(2)) : tokenFile;
}

function hubToken(tokenFile: string): { ok: true; token: string } | { ok: false; detail: string } {
  let token: string | undefined;

  try {
    token = parseEnv(readFileSync(tokenPath(tokenFile), "utf8")).JAKE_CONTROL_TOKEN;
  } catch {
    return { ok: false, detail: `the token file ${tokenFile} cannot be read` };
  }

  return token === undefined || token === "" ? { ok: false, detail: `the token file ${tokenFile} holds no JAKE_CONTROL_TOKEN` } : { ok: true, token };
}

export type ExecEnd = { kind: "exit"; code: number | null; stderr: string } | { kind: "error"; detail: string };

// What the hub's status says before any stream is read; null where it is a stream to read.
function refused(answer: Response, agent: string): string | null {
  if (answer.status === 401 || answer.status === 403) return `the hub refused the token (${answer.status})`;
  if (answer.status === 404 || answer.status === 409) return `the agent ${agent} is not attached to its hub`;
  if (answer.status !== 200 || answer.body === null) return `the hub answered ${answer.status}`;

  return null;
}

// An answer's stream read through, each event handed on in the order it came. A stream that ends without the
// blank line after its last event still ends that event.
async function readEvents(answer: Response, frame: (event: string, data: string) => void): Promise<string | null> {
  const decoder = new TextDecoder();
  const events = createParser({
    onEvent(event) {
      if (event.event !== undefined && event.data !== "") frame(event.event, event.data);
    },
  });

  try {
    for await (const chunk of answer.body as unknown as AsyncIterable<Uint8Array>) {
      events.feed(decoder.decode(chunk, { stream: true }));
    }
  } catch (error) {
    return `the hub's stream broke off: ${error instanceof Error ? error.message : String(error)}`;
  }

  events.feed(`${decoder.decode()}\n\n`);

  return null;
}

// One command on the node through its hub, its stdout's lines handed on as they arrive.
export async function hubExec(target: HubTarget, request: { command: string; env: Record<string, string> }, onLine: (line: string) => void): Promise<ExecEnd> {
  const token = hubToken(target.tokenFile);

  if (!token.ok) return { kind: "error", detail: token.detail };

  let answer: Response;

  try {
    answer = await fetch(`${target.hubUrl.replace(/\/+$/, "")}/exec?agent=${encodeURIComponent(target.agent)}`, {
      method: "POST",
      headers: { authorization: `Bearer ${token.token}`, "content-type": "application/json" },
      body: JSON.stringify({ shell: "linux", command: request.command, env: request.env }),
    });
  } catch (error) {
    return { kind: "error", detail: `the hub did not answer: ${error instanceof Error ? error.message : String(error)}` };
  }

  const refusal = refused(answer, target.agent);

  if (refusal !== null) return { kind: "error", detail: refusal };

  const stdout = new TextDecoder();
  let line = "";
  let stderr = "";
  let end: ExecEnd | null = null;
  const lines = (text: string): void => {
    line += text;

    let cut: number;

    while ((cut = line.indexOf("\n")) !== -1) {
      const one = line.slice(0, cut);

      line = line.slice(cut + 1);
      if (one.trim() !== "") onLine(one);
    }
  };
  const broke = await readEvents(answer, (event, data) => {
    let body: { chunk?: { base64?: string }; code?: number | null; message?: string; reason?: string };

    try {
      body = JSON.parse(data) as typeof body;
    } catch {
      return;
    }

    if (event === "stdout" && typeof body.chunk?.base64 === "string") lines(stdout.decode(Buffer.from(body.chunk.base64, "base64"), { stream: true }));
    else if (event === "stderr" && typeof body.chunk?.base64 === "string") stderr = (stderr + Buffer.from(body.chunk.base64, "base64").toString("utf8")).slice(-2_000);
    else if (event === "exit") end = { kind: "exit", code: typeof body.code === "number" ? body.code : null, stderr };
    else if (event === "error") end = { kind: "error", detail: body.message ?? "the hub reported an error" };
    else if (event === "ended" && end === null) end = { kind: "error", detail: `the run ended: ${body.reason ?? "no reason given"}` };
  });

  if (broke !== null) return { kind: "error", detail: broke };

  lines(stdout.decode());
  if (line.trim() !== "") onLine(line);

  return end ?? { kind: "error", detail: "the hub's stream closed without the command's end" };
}

export type TransferEnd = { kind: "completed"; bytes: Uint8Array } | { kind: "error"; detail: string };

// A whole file carried either way. The hub reports `completed` only once the bytes the agent sent match the size
// and checksum it declared, so what is gathered is whole whenever the end says so.
async function transfer(target: HubTarget, way: "upload" | "download", path: string, body: Uint8Array | null): Promise<TransferEnd> {
  const token = hubToken(target.tokenFile);

  if (!token.ok) return { kind: "error", detail: token.detail };

  let answer: Response;

  try {
    answer = await fetch(`${target.hubUrl.replace(/\/+$/, "")}/${way}?path=${encodeURIComponent(path)}&agent=${encodeURIComponent(target.agent)}`, {
      method: "POST",
      headers: { authorization: `Bearer ${token.token}`, "content-type": "application/octet-stream" },
      ...(body === null ? {} : { body: body.slice() }),
    });
  } catch (error) {
    return { kind: "error", detail: `the hub did not answer: ${error instanceof Error ? error.message : String(error)}` };
  }

  const refusal = refused(answer, target.agent);

  if (refusal !== null) return { kind: "error", detail: refusal };

  const chunks: Uint8Array[] = [];
  let end: TransferEnd | null = null;
  const broke = await readEvents(answer, (event, data) => {
    let read: { chunk?: { base64?: string }; message?: string; reason?: string };

    try {
      read = JSON.parse(data) as typeof read;
    } catch {
      return;
    }

    if (event === "chunk" && typeof read.chunk?.base64 === "string") chunks.push(new Uint8Array(Buffer.from(read.chunk.base64, "base64")));
    else if (event === "completed") end = { kind: "completed", bytes: new Uint8Array(Buffer.concat(chunks)) };
    else if (event === "error") end = { kind: "error", detail: read.message ?? "the hub reported an error" };
    else if (event === "ended" && end === null) end = { kind: "error", detail: `the transfer ended: ${read.reason ?? "no reason given"}` };
  });

  if (broke !== null) return { kind: "error", detail: broke };

  return end ?? { kind: "error", detail: "the hub's stream closed without the transfer's end" };
}

// A whole file written on the node at `path`, which is absolute.
export async function hubUpload(target: HubTarget, path: string, bytes: Uint8Array): Promise<TransferEnd> {
  return await transfer(target, "upload", path, bytes);
}

// A whole file read back from the node at `path`, which is absolute.
export async function hubDownload(target: HubTarget, path: string): Promise<TransferEnd> {
  return await transfer(target, "download", path, null);
}
