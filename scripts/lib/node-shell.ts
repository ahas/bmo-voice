import { hubDownload, hubExec, hubUpload } from "./hub";
import { nodeLogin } from "./node";

import type { HubTarget, NodeTarget } from "./node";

// A node's own shell, the one way the voice server on a node is built, fed and asked: a command run there as bash
// runs it, with bytes on its standing input, and a whole file carried either way. An ssh node answers over the
// batch login the node service gives for it, a hub node through its hub's exec and file transfer, so what stands
// on a node never asks which kind it is on. A node whose login answers in PowerShell runs no bash command and is
// out of reach.

export interface ShellRun {
  // The command's own exit; null where it was stopped rather than ended.
  code: number | null;
  stdout: string;
  stderr: string;
}

// The node could not be reached, or would not carry what it was handed: its shell never ran the command, or the
// file never arrived. A command that ran and failed is a ShellRun, not this.
export class NodeShellError extends Error {
  constructor(detail: string) {
    super(detail);
    this.name = "NodeShellError";
  }
}

export interface NodeShell {
  run(command: string, input?: Uint8Array | string): Promise<ShellRun>;
  // Both take an absolute path on the node.
  put(path: string, bytes: Uint8Array): Promise<void>;
  get(path: string): Promise<Uint8Array>;
}

// A word as one word of a POSIX shell line: wrapped in single quotes, each single quote it holds closed, escaped
// and reopened.
export function shellWord(text: string): string {
  return `'${text.replaceAll("'", `'\\''`)}'`;
}

function bytesOf(input: Uint8Array | string): Uint8Array {
  return typeof input === "string" ? new TextEncoder().encode(input) : input;
}

// ssh reports its own failure to connect, log in or keep the channel as 255; every other code is the command's.
const SSH_FAILED = 255;

interface SshRun {
  code: number | null;
  stdout: Uint8Array;
  stderr: string;
}

class SshShell implements NodeShell {
  // `login` is the node service's batch login, every argument ahead of the command.
  constructor(private readonly login: readonly string[], private readonly host: string) {}

  // One batch login running `command` as bash reads it, whatever shell the account logs into, with `input` on its
  // standing input.
  private async spawn(command: string, input?: Uint8Array): Promise<SshRun> {
    let run: ReturnType<typeof Bun.spawn>;

    try {
      run = Bun.spawn(["ssh", ...this.login, `bash -c ${shellWord(command)}`], {
        stdin: input === undefined ? "ignore" : Buffer.from(input),
        stdout: "pipe",
        stderr: "pipe",
      });
    } catch (error) {
      throw new NodeShellError(`ssh could not be started: ${error instanceof Error ? error.message : String(error)}`);
    }

    const [stdout, stderr, code] = await Promise.all([
      new Response(run.stdout as ReadableStream).arrayBuffer().then((buffer) => new Uint8Array(buffer)),
      new Response(run.stderr as ReadableStream).text(),
      run.exited,
    ]);

    if (code === SSH_FAILED) {
      throw new NodeShellError(`ssh to ${this.host} failed: ${stderr.trim().slice(-300)}`);
    }

    return { code: run.signalCode === null ? code : null, stdout, stderr };
  }

  async run(command: string, input?: Uint8Array | string): Promise<ShellRun> {
    const ran = await this.spawn(command, input === undefined ? undefined : bytesOf(input));

    return { code: ran.code, stdout: new TextDecoder().decode(ran.stdout), stderr: ran.stderr };
  }

  async put(path: string, bytes: Uint8Array): Promise<void> {
    const ran = await this.spawn(`cat > ${shellWord(path)}`, bytes);

    if (ran.code !== 0) {
      throw new NodeShellError(`${path} could not be written on ${this.host}: ${ran.stderr.trim().slice(-300)}`);
    }
  }

  async get(path: string): Promise<Uint8Array> {
    const ran = await this.spawn(`cat ${shellWord(path)}`);

    if (ran.code !== 0) {
      throw new NodeShellError(`${path} could not be read on ${this.host}: ${ran.stderr.trim().slice(-300)}`);
    }

    return ran.stdout;
  }
}

// A hub exec carries no standing input, so what a command reads there comes in the environment overlay and is
// decoded in front of it. Linux holds one variable to 128 KiB; what is larger goes up as a file.
const INPUT_MOST = 96 * 1024;

class HubShell implements NodeShell {
  constructor(private readonly target: HubTarget) {}

  async run(command: string, input?: Uint8Array | string): Promise<ShellRun> {
    const env: Record<string, string> = {};
    let text = `{\n${command}\n} < /dev/null`;

    if (input !== undefined) {
      const encoded = Buffer.from(bytesOf(input)).toString("base64");

      if (encoded.length > INPUT_MOST) {
        throw new NodeShellError(`${encoded.length} bytes of standing input is more than a hub node's command carries; send it as a file`);
      }

      env.BMO_NODE_INPUT = encoded;
      text = `printf '%s' "$BMO_NODE_INPUT" | base64 -d | {\n${command}\n}`;
    }

    const lines: string[] = [];
    const end = await hubExec(this.target, { command: text, env }, (line) => void lines.push(line));

    if (end.kind === "error") {
      throw new NodeShellError(end.detail);
    }

    return { code: end.code, stdout: lines.join("\n"), stderr: end.stderr };
  }

  async put(path: string, bytes: Uint8Array): Promise<void> {
    const end = await hubUpload(this.target, path, bytes);

    if (end.kind === "error") {
      throw new NodeShellError(`${path} could not be written on ${this.target.agent}: ${end.detail}`);
    }
  }

  async get(path: string): Promise<Uint8Array> {
    const end = await hubDownload(this.target, path);

    if (end.kind === "error") {
      throw new NodeShellError(`${path} could not be read on ${this.target.agent}: ${end.detail}`);
    }

    return end.bytes;
  }
}

// A node whose login answers in PowerShell: every verb is refused as the node out of reach.
class PowerShellNode implements NodeShell {
  constructor(private readonly address: string) {}

  private refusal(): NodeShellError {
    return new NodeShellError(`${this.address} answers in Windows PowerShell and runs no bash command`);
  }

  run(): Promise<ShellRun> {
    return Promise.reject(this.refusal());
  }

  put(): Promise<void> {
    return Promise.reject(this.refusal());
  }

  get(): Promise<Uint8Array> {
    return Promise.reject(this.refusal());
  }
}

// The shell of the node a name and its target name. An ssh node is reached over the batch login the node service
// gives for it; one it gives none for is out of reach.
export async function nodeShell(name: string, target: NodeTarget): Promise<NodeShell> {
  if (target.kind === "jake") return new HubShell(target);
  if (target.shell === "powershell") return new PowerShellNode(`${target.host}:${target.port}`);

  const login = await nodeLogin(name);

  if (login === null) {
    throw new NodeShellError(`${name} has no SSH route`);
  }

  return new SshShell(login, target.host);
}

// The account's home on the node, which every absolute path a file is carried to is built on.
export async function nodeHome(shell: NodeShell): Promise<string> {
  const ran = await shell.run(`printf '%s\\n' "$HOME"`);
  const home = ran.stdout.trim();

  if (ran.code !== 0 || !home.startsWith("/")) {
    throw new NodeShellError(`the node did not say where its home is: ${ran.stderr.trim().slice(-300)}`);
  }

  return home;
}
