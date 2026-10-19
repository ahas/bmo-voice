// The voice briefing as a layer: its player, the composable that hears briefings, its settings page at /briefing
// and the server routes that call the voice and the receiver, on top of bmo-ui. A host extends it as it extends
// bmo-ui, and brings its i18n module, its language pack, its sign-in and the placing of the player
// (`docs/layout.md`, 「The console plugin bundle」).
export default defineNuxtConfig({
  extends: ["bmo-ui"],
  // Where the voice and the receiver answer and the tokens they are called with; the host sets them from
  // NUXT_VOICE_ADDRESS, NUXT_VOICE_TOKEN, NUXT_RECEIVER_ADDRESS and NUXT_RECEIVER_TOKEN, and none reaches the
  // browser.
  runtimeConfig: {
    voice: { address: "", token: "" },
    receiver: { address: "", token: "" }
  },
  // Where a press on the player leads: the voice message on its line, by the host's own room route.
  appConfig: {
    briefing: { line: "/rooms/{chat}?at={message}" }
  },
  compatibilityDate: "2025-09-01",
  devtools: { enabled: false }
});
