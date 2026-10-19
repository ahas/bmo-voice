import { voiceMessageSound } from "../../utils/receiver";

// A briefing's sound, the voice message it went out as, handed on as the receiver answers it, ranges and all, so the
// player seeks and resumes as with any sound.
export default defineEventHandler(async (event) => {
  await requireUserSession(event);

  const query = getQuery(event);
  const chat = Number(query.chat);
  const message = Number(query.message);

  if (!Number.isSafeInteger(chat) || !Number.isSafeInteger(message)) {
    throw createError({ statusCode: 400, message: "a sound names the briefing's chat and message", data: { code: "which" } });
  }

  const answer = await voiceMessageSound(event, chat, message, getRequestHeader(event, "range"));

  setResponseStatus(event, answer.status);

  for (const name of ["content-type", "content-length", "content-range", "accept-ranges"]) {
    const value = answer.headers.get(name);

    if (value !== null) {
      setResponseHeader(event, name, value);
    }
  }

  return answer.body === null ? null : sendStream(event, answer.body);
});
