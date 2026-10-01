import { TICKS_PER_DAY } from '../../core/time';
import type { Character, Id, Memory } from '../../sim/state';

const STOP_WORDS = new Set(['the', 'and', 'for', 'with', 'that', 'this', 'was', 'were', 'have', 'has', 'had', 'you', 'your', 'are', 'not', 'but', 'from', 'they', 'them', 'his', 'her', 'she', 'him', 'about', 'into', 'near', 'just', 'what', 'when']);

function words(text: string): Set<string> {
  const result = new Set<string>();
  for (const word of text.toLowerCase().split(/[^a-z]+/)) {
    if (word.length > 3 && !STOP_WORDS.has(word)) result.add(word);
  }
  return result;
}

export interface Cue {
  /** people the situation involves */
  subjects: readonly Id[];
  /** free text describing the situation (stimuli, place, activity) */
  text: string;
}

/**
 * Picks the memories most worth recalling: recent, important, and about the
 * people or things at hand. No embeddings: subject ids and shared words stand
 * in for relevance. Returned in chronological order.
 */
export function retrieve(c: Character, tick: number, cue: Cue, count: number): Memory[] {
  const cueWords = words(cue.text);
  const scored = c.memories.map((memory) => {
    const ageDays = Math.max(0, tick - memory.tick) / TICKS_PER_DAY;
    const recency = Math.pow(0.5, ageDays / 1.5);
    let relevance = 0;
    for (const id of memory.subjects) if (cue.subjects.includes(id)) relevance += 1;
    if (cueWords.size > 0) {
      let shared = 0;
      for (const word of words(memory.text)) if (cueWords.has(word)) shared++;
      relevance += Math.min(1, shared * 0.34);
    }
    const score = recency * 1.0 + (memory.importance / 10) * 1.1 + Math.min(2, relevance) * 0.9 + (memory.kind === 'reflection' ? 0.35 : 0);
    return { memory, score };
  });
  scored.sort((a, b) => b.score - a.score);
  return scored.slice(0, count).map((entry) => entry.memory).sort((a, b) => a.tick - b.tick);
}

/** Memories formed on or after a tick, most important first. */
export function since(c: Character, tick: number, count: number): Memory[] {
  return c.memories
    .filter((memory) => memory.tick >= tick && memory.kind !== 'reflection')
    .sort((a, b) => b.importance - a.importance || a.tick - b.tick)
    .slice(0, count)
    .sort((a, b) => a.tick - b.tick);
}
