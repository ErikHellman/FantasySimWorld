import type { Vec2 } from '../core/math2d';
import type { ChronicleEntry, Feeling, Id, TalkTone } from './state';

export type Emote = 'heart' | 'anger' | 'alert' | 'thinking' | 'sleep' | 'grief' | 'laugh' | 'broken_heart' | 'gift';

/**
 * One-shot happenings of a tick, for the renderer, UI, audio, speech and the
 * brain. Continuous things (who is where, doing what) are read from the state.
 */
export type SimEvent =
  | { type: 'speech'; speakerId: Id; toId: Id | null; text: string; durationTicks: number; loud: boolean }
  | { type: 'emote'; id: Id; emote: Emote }
  | { type: 'attack'; attackerId: Id; targetId: Id; ranged: 'arrow' | 'bolt' | 'magic' | null; from: Vec2; to: Vec2 }
  | { type: 'hit'; attackerId: Id; targetId: Id; damage: number; pos: Vec2; fatal: boolean }
  | { type: 'death'; id: Id; name: string; pos: Vec2; monster: boolean }
  | { type: 'monster_spawn'; id: Id; pos: Vec2 }
  | { type: 'chronicle'; entry: ChronicleEntry }
  | { type: 'arrival'; id: Id }
  | { type: 'new_day'; day: number }
  | { type: 'conversation_started'; convoId: Id; a: Id; b: Id; tone: TalkTone }
  | { type: 'conversation_waiting'; convoId: Id }
  | { type: 'conversation_ended'; convoId: Id; a: Id; b: Id; feelings: Record<Id, Feeling> }
  | { type: 'mercy_request'; attackerId: Id; victimId: Id }
  | { type: 'woke'; id: Id }
  | { type: 'save_point'; reason: string };
