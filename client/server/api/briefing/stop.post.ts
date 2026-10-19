import { askVoice } from "../../utils/voice";

// Stop-listening, pressed on any listener: the voice files the stop on its stream, and every listener playing that
// briefing stops (the voice's `docs/voice-briefing.md`, 「What a listener hears」).
export default defineEventHandler(async (event): Promise<{ ok: true }> => {
  await requireUserSession(event);

  const body = await readBody<{ chat?: unknown; message?: unknown }>(event);

  if (!Number.isSafeInteger(body?.chat) || !Number.isSafeInteger(body?.message)) {
    throw createError({ statusCode: 400, message: "a stop names the briefing's chat and message", data: { code: "which" } });
  }

  await askVoice(event, "/briefings/stop", { method: "POST", body: { chat: body.chat, message: body.message } });

  return { ok: true };
});
