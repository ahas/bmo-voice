// The node list is read as the node service pages it (docs/boundary-api.md, 「bmo-node」): `items` and `more`, each
// page after the last name the one before ended on, so the training runner's `nodes` sees every node.

import { expect, test } from "bun:test";

import { pagedNames } from "../scripts/lib/node";

import type { NodePage } from "../scripts/lib/node";

test("every page is read, each after the last name of the one before", async () => {
  const asked: string[] = [];
  const pages: Record<string, NodePage> = {
    "": { items: [{ name: "n2" }, { name: "n3" }], more: true },
    "?after=n3": { items: [{ name: "n4" }], more: false },
  };

  const names = await pagedNames(async (query) => {
    asked.push(query);
    return pages[query] ?? { items: [], more: false };
  });

  expect(names).toEqual(["n2", "n3", "n4"]);
  expect(asked).toEqual(["", "?after=n3"]);
});

test("an empty page ends the list even where it says more", async () => {
  expect(await pagedNames(async () => ({ items: [], more: true }))).toEqual([]);
});
