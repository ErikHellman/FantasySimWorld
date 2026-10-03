import * as THREE from 'three/webgpu';
import type { QualitySettings } from './quality';

export type BackendChoice = 'auto' | 'webgl' | 'webgpu';

export interface RendererHandle {
  renderer: THREE.WebGPURenderer;
  /** which backend is actually in use */
  backend: 'webgpu' | 'webgl';
}

/** `?renderer=webgl` or `?renderer=webgpu` in the URL overrides auto-detection. */
export function backendFromUrl(): BackendChoice {
  const value = new URLSearchParams(location.search).get('renderer');
  return value === 'webgl' || value === 'webgpu' ? value : 'auto';
}

export async function createRenderer(canvas: HTMLCanvasElement, choice: BackendChoice, quality: QualitySettings): Promise<RendererHandle> {
  const renderer = new THREE.WebGPURenderer({ canvas, antialias: quality.antialias, forceWebGL: choice === 'webgl' });
  await renderer.init();
  renderer.setPixelRatio(quality.pixelRatio);
  renderer.shadowMap.enabled = true;
  renderer.toneMapping = THREE.NoToneMapping;
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  const isWebGPU = (renderer.backend as { isWebGPUBackend?: boolean }).isWebGPUBackend === true;
  return { renderer, backend: isWebGPU ? 'webgpu' : 'webgl' };
}

/**
 * Keeps the drawing buffer matched to the canvas' CSS size. Call every frame;
 * returns false while the canvas has no area (hidden tab or pane), when
 * rendering must be skipped.
 */
export function fitRenderer(renderer: THREE.WebGPURenderer, camera: { resize(width: number, height: number): void }): boolean {
  const canvas = renderer.domElement;
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  if (width === 0 || height === 0) return false;
  const size = renderer.getSize(scratch);
  if (size.x !== width || size.y !== height) {
    renderer.setSize(width, height, false);
    camera.resize(width, height);
  }
  return true;
}

const scratch = new THREE.Vector2();
