import { SAVE_VERSION, type SimState } from '../sim/state';

export interface SaveDocument {
  version: number;
  savedAt: number;
  state: SimState;
}

/**
 * Upgrades, one version at a time. Index n holds the function that turns a
 * version n save into a version n + 1 save. Each must be a pure function of
 * the document so it can be tested against a frozen fixture.
 */
const MIGRATIONS: Record<number, (document: SaveDocument) => SaveDocument> = {};

/** Brings a stored save up to the current version, or returns null if it cannot be used. */
export function migrate(input: unknown): SaveDocument | null {
  if (!input || typeof input !== 'object') return null;
  let document = input as SaveDocument;
  if (typeof document.version !== 'number' || !document.state || typeof document.state !== 'object') return null;
  while (document.version < SAVE_VERSION) {
    const step = MIGRATIONS[document.version];
    if (!step) return null;
    document = step(document);
  }
  if (document.version !== SAVE_VERSION) return null;
  const state = document.state;
  if (typeof state.tick !== 'number' || !state.characters || !Array.isArray(state.order)) return null;
  return document;
}
