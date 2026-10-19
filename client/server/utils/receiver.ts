import { FetchError } from "ofetch";

import type { H3Event } from "h3";

// The receiver's routes the bundle's server calls, as the receiver's `docs/boundary-api.md` states them: a line's
// full reading, and the bytes of a voice message's sound. This is the bundle's whole client of the receiver, standing
// until that service publishes its own. It calls at the address and with the token the host's runtime configuration
// gives under `receiver`; a refusal is handed on with its status and its code, and a receiver that does not answer is
// a 502.

interface Unit {
  address: string;
  token: string;
}

function receiverUnit(event: H3Event): Unit {
  const { receiver } = useRuntimeConfig(event) as unknown as { receiver: Unit };

  if (receiver.address === "") {
    throw createError({ statusCode: 503, message: "the console names no receiver", data: { code: "no_receiver_address" } });
  }

  return receiver;
}

const unreachable = () => createError({ statusCode: 502, message: "the receiver does not answer", data: { code: "receiver_unreachable" } });

// A line's full reading asked for: made now, or already made or being made.
export async function readLine(event: H3Event, chat: number, message: number): Promise<{ ok: true; state: "making" | "ready" }> {
  const receiver = receiverUnit(event);

  try {
    return await $fetch<{ ok: true; state: "making" | "ready" }>("/voice-briefing/line", {
      baseURL: receiver.address,
      method: "POST",
      body: { chatId: chat, messageId: message },
      headers: { authorization: `Bearer ${receiver.token}` }
    });
  } catch (cause) {
    if (cause instanceof FetchError && cause.statusCode !== undefined) {
      const refusal = cause.data as { error?: unknown; code?: unknown } | undefined;

      throw createError({
        statusCode: cause.statusCode,
        message: typeof refusal?.error === "string" ? refusal.error : `the receiver answered ${cause.statusCode}`,
        data: { code: typeof refusal?.code === "string" ? refusal.code : "receiver_refused" }
      });
    }

    throw unreachable();
  }
}

// The voice message's sound, its one file, answered as a file is over HTTP, with the range the browser asked for.
export async function voiceMessageSound(event: H3Event, chat: number, message: number, range: string | undefined): Promise<Response> {
  const receiver = receiverUnit(event);

  try {
    return await fetch(new URL(`/console/rooms/${chat}/messages/${message}/files/0`, receiver.address), {
      headers: { authorization: `Bearer ${receiver.token}`, ...(range === undefined ? {} : { range }) }
    });
  } catch {
    throw unreachable();
  }
}
