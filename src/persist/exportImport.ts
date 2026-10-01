import { SAVE_VERSION, type SimState } from '../sim/state';
import { migrate, type SaveDocument } from './migrations';

/** Offers the world as a JSON file. Settings (and so the API key) are not part of it. */
export function downloadSave(state: SimState): void {
  const document_: SaveDocument = { version: SAVE_VERSION, savedAt: Date.now(), state };
  const blob = new Blob([JSON.stringify(document_)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = `fantasy-sim-world-day${Math.floor(state.tick / 28800) + 1}.json`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export async function readSaveFile(file: File): Promise<SaveDocument | null> {
  try {
    return migrate(JSON.parse(await file.text()));
  } catch {
    return null;
  }
}
