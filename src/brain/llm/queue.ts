import { LlmError } from './types';

export interface Job {
  /** lower runs first */
  priority: number;
  /** jobs with the same key replace each other while queued */
  key: string;
  /** dropped (onDrop is called) if not started by this time (performance.now()) */
  expiresAt: number;
  run(signal: AbortSignal): Promise<void>;
  onDrop?(reason: 'expired' | 'replaced' | 'circuit' | 'cancelled'): void;
}

interface Running {
  job: Job;
  controller: AbortController;
}

const CIRCUIT_FAILURES = 4;
const CIRCUIT_OPEN_MS = 30_000;

/**
 * Runs LLM jobs with a concurrency limit, strict priority order, an optional
 * requests-per-minute cap and a circuit breaker, so a slow or broken endpoint
 * never piles up work: late jobs are dropped and their callers fall back.
 */
export class JobQueue {
  private readonly queued: Job[] = [];
  private readonly running = new Map<string, Running>();
  private readonly startTimes: number[] = [];
  private failures = 0;
  private circuitOpenUntil = 0;
  private pausedUntil = 0;
  private paused = false;
  /** exponential moving average of how long a job takes, in ms */
  latencyMs = 0;
  completed = 0;
  failed = 0;
  onChange: (() => void) | null = null;

  constructor(public concurrency: number, public requestsPerMinute: number) {}

  get inFlight(): number {
    return this.running.size;
  }

  get waiting(): number {
    return this.queued.length;
  }

  get circuitOpen(): boolean {
    return performance.now() < this.circuitOpenUntil;
  }

  /** Requests per second this endpoint can absorb at the current latency. */
  get capacity(): number {
    return this.concurrency / Math.max(0.3, (this.latencyMs || 4000) / 1000);
  }

  has(key: string): boolean {
    return this.running.has(key) || this.queued.some((job) => job.key === key);
  }

  setPaused(paused: boolean): void {
    this.paused = paused;
    if (!paused) this.pump();
  }

  enqueue(job: Job): void {
    if (this.circuitOpen) {
      job.onDrop?.('circuit');
      return;
    }
    const existing = this.queued.findIndex((other) => other.key === job.key);
    if (existing >= 0) {
      this.queued[existing]?.onDrop?.('replaced');
      this.queued.splice(existing, 1);
    }
    this.queued.push(job);
    this.queued.sort((a, b) => a.priority - b.priority || a.expiresAt - b.expiresAt);
    this.pump();
    this.onChange?.();
  }

  /** Cancels queued and running jobs whose key starts with `prefix`. */
  cancel(prefix: string): void {
    for (let i = this.queued.length - 1; i >= 0; i--) {
      const job = this.queued[i] as Job;
      if (job.key.startsWith(prefix)) {
        this.queued.splice(i, 1);
        job.onDrop?.('cancelled');
      }
    }
    for (const [key, running] of this.running) if (key.startsWith(prefix)) running.controller.abort();
    this.onChange?.();
  }

  clear(): void {
    this.cancel('');
  }

  private underRateLimit(now: number): boolean {
    if (this.requestsPerMinute <= 0) return true;
    while (this.startTimes.length > 0 && now - (this.startTimes[0] as number) > 60_000) this.startTimes.shift();
    return this.startTimes.length < this.requestsPerMinute;
  }

  /** Starts as many queued jobs as the limits allow. Call regularly: expiry and rate limits are time-based. */
  pump(): void {
    const now = performance.now();
    for (let i = this.queued.length - 1; i >= 0; i--) {
      const job = this.queued[i] as Job;
      if (job.expiresAt <= now) {
        this.queued.splice(i, 1);
        job.onDrop?.('expired');
      }
    }
    if (this.paused || now < this.pausedUntil) return;
    while (this.running.size < this.concurrency && this.queued.length > 0 && this.underRateLimit(now)) {
      const job = this.queued.shift() as Job;
      if (this.running.has(job.key)) {
        job.onDrop?.('replaced');
        continue;
      }
      const controller = new AbortController();
      this.running.set(job.key, { job, controller });
      this.startTimes.push(now);
      const started = performance.now();
      job.run(controller.signal).then(
        () => {
          this.failures = 0;
          this.completed++;
          const elapsed = performance.now() - started;
          this.latencyMs = this.latencyMs === 0 ? elapsed : this.latencyMs * 0.75 + elapsed * 0.25;
        },
        (error: unknown) => {
          this.failed++;
          const llmError = error instanceof LlmError ? error : null;
          if (llmError?.kind === 'aborted') return;
          if (llmError?.retryAfterMs) this.pausedUntil = performance.now() + Math.min(llmError.retryAfterMs, 60_000);
          // A model that answered but called no tool is a prompt problem, not an outage.
          if (llmError?.kind !== 'no_call' && llmError?.kind !== 'truncated' && ++this.failures >= CIRCUIT_FAILURES) {
            this.circuitOpenUntil = performance.now() + CIRCUIT_OPEN_MS;
            this.failures = 0;
            for (const waiting of this.queued.splice(0)) waiting.onDrop?.('circuit');
          }
        },
      ).finally(() => {
        this.running.delete(job.key);
        this.pump();
        this.onChange?.();
      });
    }
  }
}
