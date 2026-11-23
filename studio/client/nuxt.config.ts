import { resolve } from "node:path";
import languages from "./i18n/languages.json";

// The packs the repository carries, each language named in its own script (docs/script-studio.md, 「Access and
// service」); each pack stands in `i18n/locales/` under its language's code.
const PACKED: Record<string, string> = languages;

// The language the screen speaks, and the pack it reads it from: the pack a deployment hands the generate step, else
// the packed one of the language named, else English.
const pack = process.env.BMO_VOICE_STUDIO_PACK;
const named = process.env.BMO_VOICE_STUDIO_LANGUAGE ?? "en";
const handed = pack !== undefined && pack !== "";
const language = handed || named in PACKED ? named : "en";
const locales = [{ code: "en", language: "en", name: PACKED.en, file: "en.yaml" }];

if (language !== "en") {
  locales.push({ code: language, language, name: PACKED[language] ?? language, file: handed ? resolve(pack) : `${language}.yaml` });
}

export default defineNuxtConfig({
  // bmo-ui brings the component library, its stylesheet, fonts and overrides, and the skeleton, island and frame
  // components the page stands on.
  extends: ["bmo-ui"],
  modules: ["@nuxtjs/i18n"],
  compatibilityDate: "2025-09-01",
  devtools: { enabled: false },
  // The studio is a single page generated into static files; its own server hands them out and answers every
  // other path under the prefix with the page (docs/script-studio.md).
  ssr: false,
  app: {
    baseURL: "/voice-studio/",
    head: {
      htmlAttrs: { lang: language },
      meta: [{ name: "theme-color", content: "#18181b" }]
    }
  },
  appConfig: {
    // The library's words, and the zone and date words every time it shows is read in. The deployment names
    // the zone in BMO_VOICE_STUDIO_ZONE; the runtime's own zone stands where it names none.
    bmoUi: {
      locale: language,
      zone: process.env.BMO_VOICE_STUDIO_ZONE ?? "Asia/Seoul",
      dateLocale: language
    }
  },
  // A generated page has no server to ask for an icon, so every icon the studio draws is bundled.
  icon: {
    provider: "none",
    clientBundle: { scan: { globInclude: ["app/**/*.{vue,ts}"] } }
  },
  nitro: {
    // A deployment generates into a release directory of its own and names it here.
    output: { dir: process.env.BMO_VOICE_STUDIO_OUTPUT_DIR ?? ".output" }
  },
  // The development bench passes what the studio's server answers through to it; the Host header is kept, since
  // the server matches a write's Origin against it (docs/development.md).
  $development: {
    nitro: {
      devProxy: Object.fromEntries(
        ["api", "media", "subtitle"].map((part) => [
          `/voice-studio/${part}`,
          { target: `${process.env.BMO_VOICE_STUDIO_DEV_API ?? "http://127.0.0.1:3215"}/voice-studio/${part}`, changeOrigin: false }
        ])
      )
    }
  },
  // Every word the studio shows is a key into the pack of the language it speaks.
  i18n: {
    strategy: "no_prefix",
    defaultLocale: language,
    detectBrowserLanguage: false,
    locales,
    // An emotion tag is written `<tag>` in a line, and the placeholder showing it is a message of the pack; the
    // studio renders no message as markup, so the compiler is not asked to refuse what looks like it.
    compilation: { strictMessage: false }
  }
});
