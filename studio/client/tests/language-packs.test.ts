import { describe, expect, test } from "bun:test";
import { WORDING_KEYS } from "bmo-ui/shared/wording-keys";

// Both packs the repository carries answer every key the layer reads (docs/script-studio.md, 「Access and service」):
// a key a pack lacks shows on the screen as the key itself.

const locales = new URL("../i18n/locales/", import.meta.url).pathname;

// The words a pack holds under a dotted key, or undefined where it holds none.
function answer(pack: unknown, key: string): unknown {
  return key.split(".").reduce<unknown>((node, part) => (node !== null && typeof node === "object" ? (node as Record<string, unknown>)[part] : undefined), pack);
}

describe("the language packs", () => {
  for (const file of ["en.yaml", "ko.yaml"]) {
    test(`${file} answers every key the layer reads`, async () => {
      const pack = Bun.YAML.parse(await Bun.file(`${locales}${file}`).text());
      const missing = WORDING_KEYS.filter((key) => {
        const words = answer(pack, key);

        return typeof words !== "string" || words === "";
      });

      expect(missing).toEqual([]);
    });
  }
});
