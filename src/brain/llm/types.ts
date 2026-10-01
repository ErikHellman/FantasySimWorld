export interface ChatMessage {
  role: 'system' | 'user' | 'assistant';
  content: string;
}

/** A tool the model may call. `parameters` is a JSON Schema object. */
export interface ToolDef {
  name: string;
  description: string;
  parameters: {
    type: 'object';
    properties: Record<string, unknown>;
    required?: string[];
    additionalProperties?: boolean;
  };
}

export interface ToolCall {
  name: string;
  args: Record<string, unknown>;
}

export interface LlmRequest {
  messages: ChatMessage[];
  tools: ToolDef[];
  /** most tool calls that will be used from one response */
  maxCalls: number;
  maxTokens?: number;
  temperature?: number;
  signal?: AbortSignal;
  timeoutMs?: number;
}

export interface LlmResponse {
  calls: ToolCall[];
  /** assistant text with any reasoning blocks removed */
  text: string;
  /** the message as received, for the decision log */
  raw: string;
  promptTokens: number;
  completionTokens: number;
  latencyMs: number;
  protocol: 'tools' | 'json';
}

export type LlmErrorKind = 'network' | 'http' | 'timeout' | 'aborted' | 'no_call' | 'truncated';

export class LlmError extends Error {
  constructor(
    readonly kind: LlmErrorKind,
    message: string,
    readonly status?: number,
    readonly retryAfterMs?: number,
    readonly raw?: string,
  ) {
    super(message);
    this.name = 'LlmError';
  }
}
