import { hashString } from '../core/rng';
import { LAYOUT_ROWS, SCATTER_SEED } from './layout';
import { places } from './places';
import { scatter } from './scatter';

/**
 * A fingerprint of the static world, sent by the server in `hello` so a client
 * can tell when its own copy of the layout differs. Integers and ids only, so
 * any implementation reproduces it exactly: FNV-1a (UTF-16 code units) of
 *
 *   rows joined by "/" | scatter seed | "id:slots:gravePlots" per place, joined by "," | scatter model count
 *
 * as 8 lowercase hex digits.
 */
export function worldHash(): string {
  const text = [
    LAYOUT_ROWS.join('/'),
    String(SCATTER_SEED),
    places.map((place) => `${place.id}:${place.slots.length}:${place.gravePlots.length}`).join(','),
    String(scatter.models.length),
  ].join('|');
  return hashString(text).toString(16).padStart(8, '0');
}
