import { settings, unitToken } from "./settings";

// bmo-node, as its `docs/api.md` states it (「Agency's calls」, 「Node reads」, 「Registry routes」): the node list, how
// a node is reached by its name, and the arguments of a batch SSH login to it. Bodies are camelCase, and a refusal is an answer of its own.
// This is the scripts' whole client of the node service, standing until that service publishes its own.

// An ssh node by its host, port, account, key and shell, or a hub node through its hub by the hub's address, its
// token file and the agent the node attaches as; with the services its role reserves it for, joined by `+`.
export type NodeTarget = (
  | { kind: "ssh"; host: string; port: number; user: string; keyPath?: string | null; shell?: string | null; sshArgs?: string[] | null }
  | { kind: "hub"; hubAddress: string; hubTokenFile: string; agentName: string }
) & { role?: string | null };

export type HubTarget = Extract<NodeTarget, { kind: "hub" }>;

// The node service refused, or did not answer.
export class NodeServiceError extends Error {
  constructor(said: string) {
    super(said);
    this.name = "NodeServiceError";
  }
}

async function ask<T>(method: "GET" | "POST", path: string, body?: unknown): Promise<T> {
  const unit = settings().units.node;
  const token = unitToken(unit);
  let answer: Response;

  try {
    answer = await fetch(`${unit.address.replace(/\/+$/, "")}${path}`, {
      method,
      headers: {
        ...(token === null ? {} : { authorization: `Bearer ${token}` }),
        ...(body === undefined ? {} : { "content-type": "application/json" }),
      },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
  } catch (error) {
    throw new NodeServiceError(`the node service did not answer: ${error instanceof Error ? error.message : String(error)}`);
  }

  const read = (await answer.json().catch(() => null)) as (Record<string, unknown> & { answered?: boolean; refusal?: { message?: string }; error?: string }) | null;

  if (!answer.ok || read === null) {
    throw new NodeServiceError(`the node service answered ${answer.status}${read?.error === undefined ? "" : `: ${read.error}`}`);
  }

  if (read.answered === false) {
    throw new NodeServiceError(`the node service refused: ${read.refusal?.message ?? "no reason given"}`);
  }

  return read as T;
}

// Every node the service lists, by name, in the list's order: its pages read one after another, each after the last
// name the one before ended on.
export async function nodeNames(): Promise<string[]> {
  return await pagedNames(async (query) => await ask<NodePage>("GET", `/nodes${query}`));
}

// A page of the node list, as the node service answers one (its `docs/api.md`, 「Node reads」).
export interface NodePage {
  items?: { name: string }[];
  more?: boolean;
}

// The names of every page `read` answers, the first asked with no query and each after it after the last name read.
export async function pagedNames(read: (query: string) => Promise<NodePage>): Promise<string[]> {
  const names: string[] = [];

  for (;;) {
    const query = names.length === 0 ? "" : `?after=${encodeURIComponent(names.at(-1) as string)}`;
    const page = await read(query);
    const items = page.items ?? [];

    names.push(...items.map((node) => node.name));

    if (page.more !== true || items.length === 0) return names;
  }
}

// How a node is reached by its name.
export async function nodeTarget(name: string): Promise<NodeTarget> {
  return await ask<NodeTarget>("GET", `/nodes/${encodeURIComponent(name)}/target`);
}

// The arguments that open a batch SSH login to a node, ahead of the command, as the node's target lays them out;
// null where the node has no SSH route (a hub node).
export async function nodeLogin(name: string): Promise<string[] | null> {
  const target = await nodeTarget(name);

  return target.kind === "ssh" ? (target.sshArgs ?? null) : null;
}

// Why a node's role keeps a service off it, or null where the node carries it: a node with no role carries every
// service, and one with a role only those it names.
export function reservedAgainst(target: NodeTarget, service: string): string | null {
  const role = target.role ?? null;

  return role === null || role.split("+").includes(service) ? null : `the node is reserved for ${role} and carries no ${service}`;
}
