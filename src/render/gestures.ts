/** A finger on the screen, in client pixels. */
export interface Finger {
  x: number;
  y: number;
}

export type FingerPair = readonly [Finger, Finger];

/** How a two-finger touch changed between two moments. */
export interface TwoFingerMove {
  /** the new spread of the fingers over the old: above 1 when they move apart */
  scale: number;
  /** how far the line between the fingers turned, in radians; clockwise on screen is positive */
  twist: number;
  /** how far both fingers moved down together, in pixels (negative is up); 0 unless they moved the same way */
  lift: number;
}

/**
 * Reads a two-finger move as pinch, twist and a shared vertical drag, the touch gestures for zoom, turn and tilt.
 * Plain maths so it can be tested without a browser.
 */
export function twoFinger(before: FingerPair, after: FingerPair): TwoFingerMove {
  const [a0, b0] = before;
  const [a1, b1] = after;
  const spread0 = Math.hypot(b0.x - a0.x, b0.y - a0.y);
  const spread1 = Math.hypot(b1.x - a1.x, b1.y - a1.y);
  // Fingers on the same spot have no direction to turn from and no spread to scale.
  if (spread0 < 1 || spread1 < 1) return { scale: 1, twist: 0, lift: 0 };
  let twist = Math.atan2(b1.y - a1.y, b1.x - a1.x) - Math.atan2(b0.y - a0.y, b0.x - a0.x);
  if (twist > Math.PI) twist -= 2 * Math.PI;
  else if (twist < -Math.PI) twist += 2 * Math.PI;
  const dyA = a1.y - a0.y;
  const dyB = b1.y - b0.y;
  // A pinch moves the fingers in opposite directions; only a shared drag tilts.
  const lift = Math.sign(dyA) === Math.sign(dyB) ? (dyA + dyB) / 2 : 0;
  return { scale: spread1 / spread0, twist, lift };
}
