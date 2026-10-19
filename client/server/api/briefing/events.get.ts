import { voiceStream } from "../../utils/voice";

// A listener's stream: the voice's own, its briefings, the stops of every listener and the agent's presence, with
// its keep frames, handed on as it comes (the voice's `docs/boundary-api.md`, 「The listener stream」). The browser
// resumes it with the last id it read.
export default defineEventHandler(async (event) => {
  await requireUserSession(event);

  const aborted = new AbortController();
  const query = getQuery(event);
  const after = getRequestHeader(event, "last-event-id") ?? (typeof query.after === "string" ? query.after : null);

  event.node.res.on("close", () => aborted.abort());

  const stream = await voiceStream(event, after, aborted.signal);

  setResponseHeaders(event, {
    "content-type": "text/event-stream",
    "cache-control": "no-cache",
    connection: "keep-alive",
    "x-accel-buffering": "no"
  });

  return sendStream(event, stream);
});
