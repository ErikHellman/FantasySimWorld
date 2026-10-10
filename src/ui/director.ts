import type { Hotspot, HotspotKind } from '../generated/api';

/** How long the viewer leaves the camera alone before the Auto camera takes it. */
export const IDLE_SECONDS = 30;
/** The Auto camera stays at least this long on what it chose, unless that is over. */
const MIN_DWELL_SECONDS = 20;
/** After this long on one thing it moves on to another nearly as good, so a quiet stretch still shows the village. */
const MAX_DWELL_SECONDS = 60;
/** How much better something new must be to cut away before the dwell is up. */
const CUT_MARGIN = 20;
/** "Nearly as good", for moving on after the longest dwell. */
const ROTATE_MARGIN = 15;

/** What earns a "Happening now" card. */
const CARD_KINDS: ReadonlySet<HotspotKind> = new Set<HotspotKind>(['dragon', 'boss', 'fire', 'raid', 'ceremony', 'brawl', 'battle']);
/** A kind that was just carded is not carded again this soon, so a fight that changes hands is one card. */
const CARD_COOLDOWN_SECONDS = 90;
/** How long a card stays up. */
export const CARD_SECONDS = 12;

/** One hotspot as the client tells it apart: its subject, or its kind and place when it has none. */
export function hotspotKey(spot: Hotspot): string {
  return spot.subject ?? `${spot.kind}@${Math.round(spot.at.x)},${Math.round(spot.at.z)}`;
}

/** What the Auto camera is on, and since when (in seconds). */
export interface Shot {
  key: string;
  since: number;
}

/**
 * The hotspot the Auto camera should be on now, given what it is on: it keeps a shot for a while, cuts at once
 * when that is over or something much better starts, and moves on after a long dwell when there is an alternative.
 * `followable` says whether a subject can be followed on this page (it may not have reached it yet).
 */
export function chooseShot(hotspots: readonly Hotspot[], current: Shot | null, now: number, followable: (id: string) => boolean): Hotspot | null {
  const usable = hotspots.filter((spot) => spot.subject === null || followable(spot.subject));
  const best = usable[0];
  if (!best) return null;
  const kept = current ? usable.find((spot) => hotspotKey(spot) === current.key) : undefined;
  if (!kept || !current) return best;
  const dwell = now - current.since;
  if (dwell < MIN_DWELL_SECONDS) return kept;
  if (best.score >= kept.score + CUT_MARGIN) return best;
  if (dwell >= MAX_DWELL_SECONDS) {
    const other = usable.find((spot) => hotspotKey(spot) !== current.key && spot.score >= kept.score - ROTATE_MARGIN);
    if (other) return other;
  }
  return kept;
}

/** Remembers which hotspots were already announced, and says which new ones earn a card. */
export class CardDealer {
  private known = new Set<string>();
  private lastCarded = new Map<HotspotKind, number>();
  private primed = false;

  /** The hotspots in `hotspots` that deserve a card now. The first call only learns what is already going on. */
  deal(hotspots: readonly Hotspot[], now: number): Hotspot[] {
    const fresh = hotspots.filter((spot) => !this.known.has(hotspotKey(spot)));
    this.known = new Set(hotspots.map(hotspotKey));
    if (!this.primed) {
      this.primed = true;
      return [];
    }
    const cards: Hotspot[] = [];
    for (const spot of fresh) {
      if (!CARD_KINDS.has(spot.kind)) continue;
      const last = this.lastCarded.get(spot.kind);
      if (last !== undefined && now - last < CARD_COOLDOWN_SECONDS) continue;
      this.lastCarded.set(spot.kind, now);
      cards.push(spot);
    }
    return cards;
  }
}

/** The emoji a card or the Auto chip wears for a kind of hotspot. */
export function hotspotIcon(kind: HotspotKind): string {
  switch (kind) {
    case 'dragon': return '🐉';
    case 'boss': return '💀';
    case 'fire': return '🔥';
    case 'raid': return '🪓';
    case 'ceremony': return '🔔';
    case 'battle': return '⚔️';
    case 'brawl': return '👊';
    case 'dead': return '🧟';
    case 'ghost': return '👻';
    case 'quarrel': return '💢';
    case 'music': return '🎵';
    case 'romance': return '💕';
    case 'daily': return '👀';
  }
}
