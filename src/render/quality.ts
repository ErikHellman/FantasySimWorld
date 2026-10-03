/** How much the picture may cost: `low` is for phones and other small machines. */
export type Quality = 'high' | 'low';

export interface QualitySettings {
  quality: Quality;
  /** device pixels drawn per CSS pixel, at most */
  pixelRatio: number;
  /** multisampled edges; at two or more device pixels per CSS pixel the steps are too small to see */
  antialias: boolean;
  /** the sun's shadow map, in texels along a side */
  shadowMapSize: number;
  /** how far from the camera's target shadows reach, in metres */
  shadowRange: number;
  /** point lights shared out among the lamps nearest the camera */
  lamps: number;
  /** the shortest time between two drawn frames, in seconds; 0 draws every frame the browser offers */
  frameSeconds: number;
  /** grass, flowers and the like: one in this many is drawn */
  groundCoverStep: number;
  /** canvas pixels per metre of billboard face */
  billboardPixels: number;
  /** lower the pixel ratio when frames keep coming late */
  adaptive: boolean;
  /** flames drawn on a burning house */
  fireFlames: number;
  /** puffs of smoke rising from it */
  fireSmoke: number;
}

/** What `pickQuality` looks at; passed in so the choice can be tested without a browser. */
export interface Device {
  /** the page's query string */
  search: string;
  /** the main pointer is a finger */
  coarsePointer: boolean;
  /** `navigator.deviceMemory` in gigabytes, where the browser tells */
  memoryGb?: number;
}

/** `?quality=low` or `?quality=high` in the URL decides; otherwise a touch device or one short of memory gets `low`. */
export function pickQuality(device: Device): Quality {
  const asked = new URLSearchParams(device.search).get('quality');
  if (asked === 'low' || asked === 'high') return asked;
  if (device.coarsePointer) return 'low';
  if (device.memoryGb !== undefined && device.memoryGb <= 4) return 'low';
  return 'high';
}

export function thisDevice(): Device {
  return {
    search: location.search,
    coarsePointer: matchMedia('(pointer: coarse)').matches,
    memoryGb: (navigator as { deviceMemory?: number }).deviceMemory,
  };
}

/** The settings are read once at start: several of them are compiled into the shaders. */
export function qualitySettings(quality: Quality, devicePixelRatio: number): QualitySettings {
  if (quality === 'high') {
    return {
      quality, pixelRatio: Math.min(devicePixelRatio, 2), antialias: true, shadowMapSize: 2048, shadowRange: 70, lamps: 6,
      frameSeconds: 0, groundCoverStep: 1, billboardPixels: 300, adaptive: false, fireFlames: 16, fireSmoke: 8,
    };
  }
  return {
    quality, pixelRatio: Math.min(devicePixelRatio, 1.5), antialias: devicePixelRatio < 2, shadowMapSize: 1024, shadowRange: 45, lamps: 3,
    frameSeconds: 1 / 30, groundCoverStep: 2, billboardPixels: 150, adaptive: true, fireFlames: 7, fireSmoke: 0,
  };
}
