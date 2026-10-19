import { FetchError } from "ofetch";

import type { H3Event } from "h3";

// The voice's own routes as the bundle's server calls them (`docs/layout.md`, 「The console plugin bundle」): at the
// address and with the token the host's runtime configuration gives under `voice`, read for each call. A refusal of
// the voice's is handed on with its status and its code, the English sentence as its message; a voice that does not
// answer is a 502.

interface Unit {
  address: string;
  token: string;
}

function voiceUnit(event: H3Event): Unit {
  const { voice } = useRuntimeConfig(event) as unknown as { voice: Unit };

  if (voice.address === "") {
    throw createError({ statusCode: 503, message: "the console names no voice", data: { code: "no_voice_address" } });
  }

  return voice;
}

// One call answered with JSON, or with nothing.
export async function askVoice<T>(event: H3Event, path: string, options: { method?: "GET" | "POST"; body?: Record<string, unknown> } = {}): Promise<T> {
  const voice = voiceUnit(event);

  try {
    const answer: unknown = await $fetch(path, {
      baseURL: voice.address,
      method: options.method ?? "GET",
      body: options.body,
      headers: { authorization: `Bearer ${voice.token}` }
    });

    return answer as T;
  } catch (cause) {
    if (cause instanceof FetchError && cause.statusCode !== undefined) {
      const refusal = cause.data as { error?: unknown; code?: unknown } | undefined;

      throw createError({
        statusCode: cause.statusCode,
        message: typeof refusal?.error === "string" ? refusal.error : `the voice answered ${cause.statusCode}`,
        data: { code: typeof refusal?.code === "string" ? refusal.code : "voice_refused" }
      });
    }

    throw createError({ statusCode: 502, message: "the voice does not answer", data: { code: "voice_unreachable" } });
  }
}

// The voice's listener stream, opened from the frame after `after` where the browser resumes, closed when `signal`
// aborts.
export async function voiceStream(event: H3Event, after: string | null, signal: AbortSignal): Promise<ReadableStream<Uint8Array>> {
  const voice = voiceUnit(event);
  let answer: Response;

  try {
    answer = await fetch(new URL("/briefings/events", voice.address), {
      headers: { authorization: `Bearer ${voice.token}`, accept: "text/event-stream", ...(after === null ? {} : { "last-event-id": after }) },
      signal
    });
  } catch {
    throw createError({ statusCode: 502, message: "the voice does not answer", data: { code: "voice_unreachable" } });
  }

  if (!answer.ok || answer.body === null) {
    throw createError({ statusCode: answer.status, message: `the voice answered ${answer.status}`, data: { code: "voice_refused" } });
  }

  return answer.body;
}
