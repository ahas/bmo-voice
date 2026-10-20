import { resolve } from "node:path";

// The language the screen speaks: English from its own words, or the pack a deployment hands the generate step
// (docs/script-studio.md, 「Access and service」).
const pack = process.env.BMO_SCRIPT_PACK;
const language = pack === undefined || pack === "" ? "en" : (process.env.BMO_SCRIPT_LANGUAGE ?? "en");
const locales = [{ code: "en", language: "en", name: "English", file: "en.yaml" }];

if (language !== "en" && pack !== undefined) {
  locales.push({ code: language, language, name: language, file: resolve(pack) });
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
    baseURL: "/bmo-script/",
    head: {
      htmlAttrs: { lang: language },
      meta: [{ name: "theme-color", content: "#18181b" }]
    }
  },
  appConfig: {
    // The library's words, and the zone and date words every time it shows is read in. The deployment names
    // the zone in BMO_SCRIPT_ZONE; the runtime's own zone stands where it names none.
    bmoUi: {
      locale: language,
      zone: process.env.BMO_SCRIPT_ZONE ?? "Asia/Seoul",
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
    output: { dir: process.env.BMO_SCRIPT_OUTPUT_DIR ?? ".output" }
  },
  // The development bench passes what the studio's server answers through to it; the Host header is kept, since
  // the server matches a write's Origin against it (docs/development.md).
  $development: {
    nitro: {
      devProxy: Object.fromEntries(
        ["api", "media", "subtitle"].map((part) => [
          `/bmo-script/${part}`,
          { target: `${process.env.BMO_SCRIPT_DEV_API ?? "http://127.0.0.1:3215"}/bmo-script/${part}`, changeOrigin: false }
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
