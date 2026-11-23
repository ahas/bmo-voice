import { describe, expect, test } from "bun:test";
import { WORDING_KEYS } from "bmo-ui/shared/wording-keys";
import languages from "../i18n/languages.json";

// Every pack the repository carries answers every key the layer reads (docs/script-studio.md, 「Access and service」):
// a key a pack lacks shows on the screen as the key itself. Each holds the very keys the English pack holds, so no
// language says less or more than another.

const locales = new URL("../i18n/locales/", import.meta.url).pathname;

const PACKS = Object.keys(languages).map((code) => `${code}.yaml`);

// The dotted keys of every message a pack holds, in the order it holds them.
function keys(node: unknown, prefix = ""): string[] {
  return Object.entries(node as Record<string, unknown>).flatMap(([part, child]) =>
    child !== null && typeof child === "object" ? keys(child, `${prefix}${part}.`) : [`${prefix}${part}`]
  );
}

async function read(file: string): Promise<unknown> {
  return Bun.YAML.parse(await Bun.file(`${locales}${file}`).text());
}

// The words a pack holds under a dotted key, or undefined where it holds none.
function answer(pack: unknown, key: string): unknown {
  return key.split(".").reduce<unknown>((node, part) => (node !== null && typeof node === "object" ? (node as Record<string, unknown>)[part] : undefined), pack);
}

describe("the language packs", () => {
  for (const file of PACKS) {
    test(`${file} answers every key the layer reads`, async () => {
      const pack = await read(file);
      const missing = WORDING_KEYS.filter((key) => {
        const words = answer(pack, key);

        return typeof words !== "string" || words === "";
      });

      expect(missing).toEqual([]);
    });
  }

  for (const file of PACKS.slice(1)) {
    test(`${file} holds the English pack's keys`, async () => {
      expect(keys(await read(file))).toEqual(keys(await read("en.yaml")));
    });
  }
});
