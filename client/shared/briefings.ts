/**
 * The voices that belong to a line already standing in its room, its summary and its full reading, read as a part of
 * that line rather than as rows of their own (the voice's `docs/voice-briefing.md`, 「Two voices of a line」), for a
 * host that shows a line's voices on it.
 *
 * The voice names the line a voice belongs to. A voice message sent before that was written down is read as the full
 * reading of the line that stands right before it when it says that line's words, since that is the only thing that
 * ever sent one there.
 */

// Where a line's full reading stands.
export type BriefingState = "idle" | "making" | "ready" | "failed";

// The archive's mark at the head of a voice message, with its length where that was known: Korean text the code
// matches, as the room's record writes it.
export const VOICE_MARK = /^\[음성(?: \d+:\d{2})?\]/;

// The words a voice message says, as its archived line holds them after the mark; null for a line that is not a voice
// message.
export function spokenWords(text: string): string | null {
  const found = VOICE_MARK.exec(text);

  return found === null ? null : text.slice(found[0].length).trim();
}

// Whether a voice message says the words of a line, which is how one sent before the voice named the line it spoke
// is matched to the line right before it.
export function speaksLine(voiceText: string, lineText: string): boolean {
  const words = spokenWords(voiceText);

  return words !== null && words !== "" && words === lineText.trim();
}

// How the run that makes a line's full reading ended.
export interface BriefingRun {
  finishedAt: number | null;
  outcome: string | null;
}

// Where a line's full reading stands, off its run and whether it has a sound. A reading the switch refused before it
// was made, and a run that ended well without a sound to show for it, leave the line as though nothing had been asked
// of it: ready to be pressed again.
export function briefingState(run: BriefingRun | null, sounded: boolean): BriefingState {
  if (sounded) {
    return "ready";
  }

  if (run === null) {
    return "idle";
  }

  if (run.finishedAt === null) {
    return "making";
  }

  return run.outcome === "failed" ? "failed" : "idle";
}
