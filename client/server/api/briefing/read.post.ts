import { readLine } from "../../utils/receiver";

// A line's full reading, asked for from its line (the voice's `docs/voice-briefing.md`, 「A full reading」). The
// receiver makes it, or answers that it is already made or being made; a refusal, the whole briefing switched off
// among them, travels back with its code.
export default defineEventHandler(async (event): Promise<{ ok: true; state: "making" | "ready" }> => {
  await requireUserSession(event);

  const body = await readBody<{ chat?: unknown; message?: unknown }>(event);

  if (!Number.isSafeInteger(body?.chat) || !Number.isSafeInteger(body?.message)) {
    throw createError({ statusCode: 400, message: "a reading names the line's chat and message", data: { code: "which" } });
  }

  return await readLine(event, body.chat as number, body.message as number);
});
