import { describe, expect, it } from 'vitest';
import { LINE_HEIGHT, layoutLines, type Measure } from './billboardText';

/** Every character is half the font size wide. */
const measure: Measure = (text, size) => text.length * size * 0.5;

describe('billboard text', () => {
  it('sets a short slogan large, on one line', () => {
    const layout = layoutLines('Warning: the river is wet.', measure, { width: 1000, height: 400 }, { max: 72, min: 24 });
    expect(layout).toEqual({ lines: ['Warning: the river is wet.'], size: 72 });
  });

  it('wraps and shrinks a long one until it fits both ways', () => {
    const text = 'New boots? Your feet deserve it. Your purse disagrees.';
    const box = { width: 600, height: 300 };
    const layout = layoutLines(text, measure, box, { max: 96, min: 24 });
    expect(layout.size).toBeLessThan(96);
    expect(layout.lines.join(' ')).toBe(text);
    for (const line of layout.lines) expect(measure(line, layout.size)).toBeLessThanOrEqual(box.width);
    expect(layout.lines.length * layout.size * LINE_HEIGHT).toBeLessThanOrEqual(box.height);
  });

  it('breaks a word too long for any line', () => {
    const layout = layoutLines('Supercalifragilistic', measure, { width: 100, height: 400 }, { max: 20, min: 20 });
    expect(layout.lines.join('')).toBe('Supercalifragilistic');
    for (const line of layout.lines) expect(measure(line, 20)).toBeLessThanOrEqual(100);
  });

  it('ends with an ellipsis when even the smallest size is too big', () => {
    const layout = layoutLines('one two three four five six seven eight nine ten', measure, { width: 100, height: 50 }, { max: 20, min: 20 });
    expect(layout.lines.length).toBe(2);
    expect(layout.lines[1]!.endsWith('…')).toBe(true);
  });
});
