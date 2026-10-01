import type { Vec2 } from '../core/math2d';
import { TICK_SECONDS } from '../core/time';
import type { SimEvent } from '../sim/events';
import type { Id, SimState, VoiceProfile } from '../sim/state';

/** One spoken line, with everything a sink needs to show it or say it aloud. */
export interface Utterance {
  id: number;
  tick: number;
  speakerId: Id;
  speakerName: string;
  toId: Id | null;
  toName: string | null;
  text: string;
  /** shouted rather than said */
  loud: boolean;
  /** how long the sim allows for the line, at 1x speed */
  durationMs: number;
  voice: VoiceProfile;
  /** where the speaker stands, for positional audio */
  position: Vec2;
}

export interface SpeechSink {
  speak(utterance: Utterance): void;
  /** called when the sim is paused or the speed changes, so audio can follow */
  setRate?(rate: number): void;
}

/**
 * Every line a character says passes through here. Today's sinks draw speech
 * bubbles and keep a transcript. Speech synthesis is a third sink: implement
 * SpeechSink, turn `text` into audio with `voice` (pitch, rate, timbre), play
 * it at `position`, and keep to `durationMs` (the sim paces conversations by
 * it: about 0.055 s per character, close to a natural speaking rate).
 */
export class SpeechBus {
  private readonly sinks: SpeechSink[] = [];
  private nextId = 1;

  add(sink: SpeechSink): void {
    this.sinks.push(sink);
  }

  /** Feeds the bus from a tick's events. */
  handle(events: readonly SimEvent[], state: SimState): void {
    for (const event of events) {
      if (event.type !== 'speech') continue;
      const speaker = state.characters[event.speakerId];
      if (!speaker) continue;
      const listener = event.toId ? state.characters[event.toId] : undefined;
      const utterance: Utterance = {
        id: this.nextId++,
        tick: state.tick,
        speakerId: speaker.id,
        speakerName: speaker.profile.name.split(' ')[0] as string,
        toId: listener?.id ?? null,
        toName: listener ? (listener.profile.name.split(' ')[0] as string) : null,
        text: event.text,
        loud: event.loud,
        durationMs: event.durationTicks * TICK_SECONDS * 1000,
        voice: speaker.profile.voice,
        position: { ...speaker.pos },
      };
      for (const sink of this.sinks) sink.speak(utterance);
    }
  }

  setRate(rate: number): void {
    for (const sink of this.sinks) sink.setRate?.(rate);
  }
}
