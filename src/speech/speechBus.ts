import type { Utterance as WireUtterance, WireEvent } from '../generated/api';

/** One spoken line, with everything a sink needs to show it or say it aloud (the server's `Utterance` plus a local id). */
export interface Utterance extends WireUtterance {
  id: number;
}

export interface SpeechSink {
  speak(utterance: Utterance): void;
  /** called when the world is paused or the speed changes, so audio can follow */
  setRate?(rate: number): void;
}

/**
 * Every line a character says passes through here. Today's sinks draw speech
 * bubbles and keep a transcript. Speech synthesis is a third sink: implement
 * SpeechSink, turn `text` into audio with `voice` (pitch, rate, timbre), play
 * it at `position`, and keep to `durationMs` (the sim paces conversations by
 * it: about 0.055 s per character, close to a natural speaking rate).
 *
 * The server sends each line complete, so the bus needs no world state.
 */
export class SpeechBus {
  private readonly sinks: SpeechSink[] = [];
  private nextId = 1;

  add(sink: SpeechSink): void {
    this.sinks.push(sink);
  }

  /** Feeds the bus from a tick's events. */
  handle(events: readonly WireEvent[]): void {
    for (const event of events) {
      if (event.type !== 'speech') continue;
      const { type: _type, ...line } = event;
      const utterance: Utterance = { ...line, id: this.nextId++ };
      for (const sink of this.sinks) sink.speak(utterance);
    }
  }

  setRate(rate: number): void {
    for (const sink of this.sinks) sink.setRate?.(rate);
  }
}
