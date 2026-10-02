// Server-sent event framing and the replay buffer.

/** One event in the text/event-stream format. JSON never contains a raw newline, so one `data:` line is enough. */
export function sseEvent(name: string, data: unknown, id?: string): string {
  return `${id === undefined ? '' : `id: ${id}\n`}event: ${name}\ndata: ${JSON.stringify(data)}\n\n`;
}

export const eventId = (bootId: string, tick: number): string => `${bootId}:${tick}`;

/** The last `size` encoded tick events, so a viewer that lost its connection briefly can catch up without a snapshot. */
export class TickRing {
  private readonly ticks: number[] = [];
  private readonly texts: string[] = [];

  constructor(private readonly size = 100) {}

  push(tick: number, text: string): void {
    this.ticks.push(tick);
    this.texts.push(text);
    if (this.ticks.length > this.size) {
      this.ticks.shift();
      this.texts.shift();
    }
  }

  clear(): void {
    this.ticks.length = 0;
    this.texts.length = 0;
  }

  /**
   * The events after `lastTick`, or null when the gap cannot be filled (the
   * viewer is further behind than the buffer reaches, or ahead of the world).
   */
  since(lastTick: number, currentTick: number): string[] | null {
    if (lastTick > currentTick) return null;
    if (lastTick === currentTick) return [];
    const first = this.ticks[0];
    if (first === undefined || lastTick < first - 1) return null;
    return this.texts.filter((_, index) => (this.ticks[index] as number) > lastTick);
  }
}

/** Parses a `Last-Event-ID` of the form "<bootId>:<tick>". */
export function parseEventId(value: string | undefined | null): { bootId: string; tick: number } | null {
  if (!value) return null;
  const index = value.lastIndexOf(':');
  if (index <= 0) return null;
  const tick = Number(value.slice(index + 1));
  return Number.isInteger(tick) && tick >= 0 ? { bootId: value.slice(0, index), tick } : null;
}
