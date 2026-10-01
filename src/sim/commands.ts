import type { ConversationLine, Feeling, Id, Intent, PlanBlock, Tuning } from './state';

/**
 * Everything the outside world (the LLM brain, the UI) may ask of the sim.
 * Commands are queued and applied at the start of the next tick, so the sim
 * stays deterministic for a given command stream.
 */
export type Command =
  | { type: 'set_plan'; id: Id; blocks: PlanBlock[]; thought?: string }
  /** Replaces what the character is doing; ignored if their situation changed since `epoch`. */
  | { type: 'set_intents'; id: Id; epoch: number; intents: Intent[] }
  | { type: 'conversation_lines'; convoId: Id; lines: ConversationLine[]; topic?: string; feelings?: Record<Id, Feeling>; closed: boolean }
  /** Stop waiting for LLM lines and finish the conversation with local ones. */
  | { type: 'conversation_local'; convoId: Id }
  | { type: 'mercy'; attackerId: Id; spare: boolean; say?: string }
  | { type: 'reflection'; id: Id; summary: string; insights: string[]; relationships: Array<{ id: Id; summary: string }> }
  | { type: 'thought'; id: Id; text: string }
  | { type: 'tuning'; tuning: Partial<Tuning> };
