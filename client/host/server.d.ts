// What the host's server owes this layer's routes (`docs/layout.md`, 「The console plugin bundle」): its sign-in,
// `nuxt-auth-utils`'s `requireUserSession`, which refuses a request nobody signed and answers the signed-in user. It
// is declared here by what the routes read of it; the host's own module answers it at build, under its own types.
import type { H3Event } from "h3";

declare global {
  function requireUserSession(event: H3Event): Promise<{ user: { name: string } }>;
}

export {};
