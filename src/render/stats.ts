import type * as THREE from 'three/webgpu';

const WINDOW = 120;

export interface FrameReport {
  /** mean time between drawn frames, in milliseconds */
  frameMs: number;
  /** the 95th percentile of the same */
  p95Ms: number;
  /** mean time this page spent building and submitting a frame */
  cpuMs: number;
  /** draw calls of the last frame, shadow pass included */
  drawCalls: number;
  triangles: number;
  /** render passes of the last frame (the picture and each shadow map) */
  passes: number;
  geometries: number;
  textures: number;
  /** what the renderer has on the GPU, in megabytes, as far as it keeps count */
  memoryMb: number;
}

/**
 * What a frame costs: timings over the last couple of seconds and the
 * renderer's own counts. Read it as `__debug.stats()`.
 */
export class FrameStats {
  private readonly intervals = new Float32Array(WINDOW);
  private readonly cpu = new Float32Array(WINDOW);
  private readonly sorted = new Float32Array(WINDOW);
  private count = 0;
  private next = 0;
  private drawCalls = 0;
  private triangles = 0;
  private passes = 0;
  private geometries = 0;
  private textures = 0;
  private memoryBytes = 0;

  /**
   * Call after each drawn frame. The renderer's counts are copied here because
   * it clears them on every animation frame, drawn or skipped.
   */
  record(intervalMs: number, cpuMs: number, renderer: THREE.WebGPURenderer): void {
    this.intervals[this.next] = intervalMs;
    this.cpu[this.next] = cpuMs;
    this.next = (this.next + 1) % WINDOW;
    this.count = Math.min(WINDOW, this.count + 1);
    const info = renderer.info;
    this.drawCalls = info.render.drawCalls;
    this.triangles = info.render.triangles;
    this.passes = info.render.frameCalls;
    this.geometries = info.memory.geometries;
    this.textures = info.memory.textures;
    this.memoryBytes = (info.memory as { total?: number }).total ?? 0;
  }

  /** True once a full window of frames has been seen since the last `restart`. */
  get settled(): boolean {
    return this.count === WINDOW;
  }

  /** Forgets the timings, for a fresh window after a setting changed. */
  restart(): void {
    this.count = 0;
    this.next = 0;
  }

  /** The 95th percentile of the time between frames, in milliseconds. */
  p95(): number {
    if (this.count === 0) return 0;
    const sorted = this.sorted.subarray(0, this.count);
    sorted.set(this.intervals.subarray(0, this.count));
    sorted.sort();
    return sorted[Math.min(this.count - 1, Math.floor(this.count * 0.95))] as number;
  }

  report(): FrameReport {
    let interval = 0;
    let cpu = 0;
    for (let i = 0; i < this.count; i++) {
      interval += this.intervals[i] as number;
      cpu += this.cpu[i] as number;
    }
    const n = Math.max(1, this.count);
    const round = (value: number): number => Math.round(value * 10) / 10;
    return {
      frameMs: round(interval / n),
      p95Ms: round(this.p95()),
      cpuMs: round(cpu / n),
      drawCalls: this.drawCalls,
      triangles: this.triangles,
      passes: this.passes,
      geometries: this.geometries,
      textures: this.textures,
      memoryMb: round(this.memoryBytes / (1024 * 1024)),
    };
  }
}
