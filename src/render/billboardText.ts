/** Measures `text` set at `size` pixels, in pixels (a canvas's measureText in the game, a stand-in in tests). */
export type Measure = (text: string, size: number) => number;

export interface TextLayout {
  lines: string[];
  /** font size in pixels */
  size: number;
}

/** Line height as a multiple of the font size. */
export const LINE_HEIGHT = 1.18;

/** Greedy word wrap at one size; a word too long for a line on its own is broken between letters. */
function wrap(text: string, size: number, measure: Measure, width: number): string[] {
  const lines: string[] = [];
  let line = '';
  for (const word of text.split(/\s+/).filter(Boolean)) {
    const joined = line ? `${line} ${word}` : word;
    if (measure(joined, size) <= width) {
      line = joined;
      continue;
    }
    if (line) lines.push(line);
    line = '';
    let rest = word;
    while (measure(rest, size) > width) {
      let cut = rest.length - 1;
      while (cut > 1 && measure(rest.slice(0, cut), size) > width) cut--;
      lines.push(rest.slice(0, cut));
      rest = rest.slice(cut);
    }
    line = rest;
  }
  if (line) lines.push(line);
  return lines;
}

/**
 * Lays a billboard's text out as large as it fits in a box: the biggest size from `max` down to `min` at which
 * the wrapped lines fit both ways. At `min` it gives up shrinking and keeps the lines that fit, the last one
 * ending in an ellipsis.
 */
export function layoutLines(text: string, measure: Measure, box: { width: number; height: number }, sizes: { max: number; min: number }): TextLayout {
  for (let size = sizes.max; size > sizes.min; size -= 4) {
    const lines = wrap(text, size, measure, box.width);
    if (lines.length * size * LINE_HEIGHT <= box.height) return { lines, size };
  }
  const size = sizes.min;
  const lines = wrap(text, size, measure, box.width);
  const room = Math.max(1, Math.floor(box.height / (size * LINE_HEIGHT)));
  if (lines.length <= room) return { lines, size };
  const kept = lines.slice(0, room);
  let last = `${kept[room - 1]}…`;
  while (last.length > 1 && measure(last, size) > box.width) last = `${last.slice(0, -2)}…`;
  kept[room - 1] = last;
  return { lines: kept, size };
}
