import { describe, expect, it } from 'vitest';
import { pickQuality, qualitySettings } from '../src/render/quality';

describe('pickQuality', () => {
  it('gives a desktop the full picture', () => {
    expect(pickQuality({ search: '', coarsePointer: false, memoryGb: 8 })).toBe('high');
    expect(pickQuality({ search: '', coarsePointer: false })).toBe('high');
  });

  it('gives touch devices and machines short of memory the light one', () => {
    expect(pickQuality({ search: '', coarsePointer: true, memoryGb: 8 })).toBe('low');
    expect(pickQuality({ search: '', coarsePointer: false, memoryGb: 4 })).toBe('low');
  });

  it('lets the URL decide either way', () => {
    expect(pickQuality({ search: '?quality=high', coarsePointer: true, memoryGb: 2 })).toBe('high');
    expect(pickQuality({ search: '?renderer=webgl&quality=low', coarsePointer: false, memoryGb: 16 })).toBe('low');
    expect(pickQuality({ search: '?quality=ultra', coarsePointer: false })).toBe('high');
  });
});

describe('qualitySettings', () => {
  it('keeps the high tier as the picture has always been', () => {
    expect(qualitySettings('high', 3)).toMatchObject({ pixelRatio: 2, antialias: true, shadowMapSize: 2048, lamps: 6, frameSeconds: 0, groundCoverStep: 1 });
  });

  it('draws fewer pixels on the low tier, and skips multisampling where pixels are already small', () => {
    expect(qualitySettings('low', 3)).toMatchObject({ pixelRatio: 1.5, antialias: false, adaptive: true });
    expect(qualitySettings('low', 1)).toMatchObject({ pixelRatio: 1, antialias: true });
  });
});
