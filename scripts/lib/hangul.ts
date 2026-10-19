// Where the scripts read Korean text itself: the range of the composed Hangul syllables, the letters a Korean line is
// counted and compared by. It stands here once, written plainly, and every reader builds its pattern from it.
export const syllableRange = "가-힣";

// One composed Hangul syllable, every one in a text.
export function hangulSyllables(text: string): string[] {
  return text.match(new RegExp(`[${syllableRange}]`, "g")) ?? [];
}
