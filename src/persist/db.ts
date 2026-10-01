import { openDB, type IDBPDatabase } from 'idb';
import { SAVE_VERSION, type SimState } from '../sim/state';
import { migrate, type SaveDocument } from './migrations';
import { normalizeSettings, type Settings } from './settings';

const DB_NAME = 'fantasy-sim-world';
const SAVES = 'saves';
const SETTINGS = 'settings';
const BACKUP_EVERY = 10;

let database: Promise<IDBPDatabase> | null = null;
let savesWritten = 0;

function db(): Promise<IDBPDatabase> {
  database ??= openDB(DB_NAME, 1, {
    upgrade(upgradeDb) {
      upgradeDb.createObjectStore(SAVES);
      upgradeDb.createObjectStore(SETTINGS);
    },
  });
  return database;
}

/** The latest usable save: the current one, or the backup if that is missing or unreadable. */
export async function loadSave(): Promise<SaveDocument | null> {
  const store = await db();
  for (const key of ['current', 'backup']) {
    try {
      const document = migrate(await store.get(SAVES, key));
      if (document) return document;
    } catch (error) {
      console.warn(`Could not read the ${key} save`, error);
    }
  }
  return null;
}

/** Writes the whole world in one put, so a save is never half-written. */
export async function writeSave(state: SimState): Promise<void> {
  const store = await db();
  const document: SaveDocument = { version: SAVE_VERSION, savedAt: Date.now(), state };
  await store.put(SAVES, document, 'current');
  if (++savesWritten % BACKUP_EVERY === 1) await store.put(SAVES, document, 'backup');
}

export async function clearSaves(): Promise<void> {
  const store = await db();
  await store.clear(SAVES);
}

export async function loadSettings(): Promise<Settings> {
  try {
    return normalizeSettings(await (await db()).get(SETTINGS, 'settings'));
  } catch (error) {
    console.warn('Could not read settings', error);
    return normalizeSettings(null);
  }
}

export async function saveSettings(settings: Settings): Promise<void> {
  await (await db()).put(SETTINGS, JSON.parse(JSON.stringify(settings)), 'settings');
}
