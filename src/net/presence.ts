/** A tab counts as being watched while it is on screen and has the focus. */
export const tabIsWatched = (): boolean => document.visibilityState === 'visible' && document.hasFocus();

export interface HeartbeatOptions {
  /** seconds between reports, as the server asks (`CreditsState.beatSeconds`) */
  seconds: number;
  /** whether the viewer is watching right now */
  watched: () => boolean;
  /** tells the server; resolves to the balance it answers with */
  report: () => Promise<number>;
  balance: (balance: number) => void;
  /** runs `tick` every `ms` and returns how to stop it; `setInterval` unless a test passes its own */
  every?: (tick: () => void, ms: number) => () => void;
}

/**
 * Tells the server, at a steady beat, that this tab is being watched, which is what earns credits.
 * Nothing is sent while the tab is hidden or out of focus. The server decides what each report is
 * worth, so sending more of them would earn nothing.
 */
export class Heartbeat {
  private stop: (() => void) | null = null;

  constructor(private readonly options: HeartbeatOptions) {}

  start(): void {
    if (this.stop) return;
    const every = this.options.every ?? ((tick, ms) => {
      const timer = setInterval(tick, ms);
      return () => clearInterval(timer);
    });
    this.stop = every(() => this.beat(), this.options.seconds * 1000);
    // The first report only starts the run on the server; send it now so the next one counts.
    this.beat();
  }

  end(): void {
    this.stop?.();
    this.stop = null;
  }

  private beat(): void {
    if (!this.options.watched()) return;
    // A report that fails is a report the server never counted; the next one starts again.
    this.options.report().then(this.options.balance, () => {});
  }
}
