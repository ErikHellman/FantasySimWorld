import { parseHeaders, type LlmSettings } from '../../persist/settings';
import { callsFromMessage, callsFromText, stripThinking } from './parse';
import { LlmError, type LlmRequest, type LlmResponse, type ToolCall, type ToolDef } from './types';

const DEFAULT_TIMEOUT_MS = 45_000;
const WARMUP_TIMEOUT_MS = 150_000;
const MAX_ADAPTATIONS = 4;

type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

/** What we have learned about this endpoint's quirks; each starts optimistic. */
interface Capabilities {
  protocol: 'tools' | 'json';
  toolChoice: boolean;
  maxTokensField: 'max_tokens' | 'max_completion_tokens';
  temperature: boolean;
  jsonSchema: boolean;
  /** the server accepts reasoning_effort */
  reasoningEffort: boolean;
  /** consecutive replies in the tools protocol that contained no usable call */
  toolMisses: number;
  /** multiplier on the output token limit, raised when answers get cut off */
  tokenScale: number;
}

const MAX_TOKEN_SCALE = 6;

function trimSlash(url: string): string {
  return url.replace(/\/+$/, '');
}

/** JSON Schema asking for {"calls": [{"tool": ..., "args": {...}}]} with one branch per tool. */
function callsSchema(tools: readonly ToolDef[], maxCalls: number): Record<string, unknown> {
  return {
    type: 'object',
    properties: {
      calls: {
        type: 'array',
        minItems: 1,
        maxItems: maxCalls,
        items: {
          anyOf: tools.map((tool) => ({
            type: 'object',
            properties: { tool: { type: 'string', enum: [tool.name] }, args: tool.parameters },
            required: ['tool', 'args'],
            additionalProperties: false,
          })),
        },
      },
    },
    required: ['calls'],
    additionalProperties: false,
  };
}

function describeTools(tools: readonly ToolDef[], maxCalls: number): string {
  const lines = tools.map((tool) => `- ${tool.name}: ${tool.description} Arguments (JSON Schema): ${JSON.stringify(tool.parameters)}`);
  return [
    'Reply with JSON only, no prose, in exactly this shape:',
    `{"calls": [{"tool": "<tool name>", "args": { ... }}]}  (1 to ${maxCalls} calls, in the order they should happen)`,
    'Tools:',
    ...lines,
  ].join('\n');
}

/**
 * Talks to an OpenAI-compatible /chat/completions endpoint. Tool calling is
 * the contract; it is carried either as native tool calls or, for servers
 * where that is unreliable, as JSON-schema-constrained output. The client
 * adapts to each server's quirks as it meets them.
 */
export class LlmClient {
  private readonly capabilities: Capabilities;
  private warmedUp = false;

  constructor(private readonly settings: LlmSettings, private readonly fetchImpl: FetchLike = (input, init) => fetch(input, init)) {
    this.capabilities = {
      protocol: settings.protocol === 'json' ? 'json' : 'tools',
      toolChoice: true,
      maxTokensField: 'max_tokens',
      temperature: true,
      jsonSchema: true,
      reasoningEffort: true,
      toolMisses: 0,
      tokenScale: 1,
    };
  }

  get protocol(): 'tools' | 'json' {
    return this.capabilities.protocol;
  }

  private headers(): Record<string, string> {
    const headers: Record<string, string> = { 'Content-Type': 'application/json', ...parseHeaders(this.settings.headers) };
    if (this.settings.apiKey) headers.Authorization = `Bearer ${this.settings.apiKey}`;
    return headers;
  }

  async listModels(): Promise<string[]> {
    const response = await this.fetchImpl(`${trimSlash(this.settings.baseUrl)}/models`, { headers: this.headers() });
    if (!response.ok) throw new LlmError('http', `The server answered ${response.status} to the model list request.`, response.status);
    const body = (await response.json()) as { data?: Array<{ id?: string }>; models?: Array<{ id?: string; name?: string }> };
    const entries = body.data ?? body.models ?? [];
    return entries.map((entry) => entry.id ?? (entry as { name?: string }).name ?? '').filter(Boolean).sort();
  }

  private body(request: LlmRequest): Record<string, unknown> {
    const caps = this.capabilities;
    const body: Record<string, unknown> = { model: this.settings.model, stream: false };
    body[caps.maxTokensField] = Math.round((request.maxTokens ?? this.settings.maxTokens) * caps.tokenScale);
    if (caps.temperature) body.temperature = request.temperature ?? this.settings.temperature;
    // Models that reason before answering are asked not to: a villager's next step does not need it.
    if (caps.reasoningEffort && this.settings.reasoning !== 'default') body.reasoning_effort = this.settings.reasoning === 'off' ? 'none' : 'low';

    if (caps.protocol === 'tools') {
      body.messages = request.messages;
      body.tools = request.tools.map((tool) => ({ type: 'function', function: { name: tool.name, description: tool.description, parameters: tool.parameters } }));
      if (caps.toolChoice) body.tool_choice = 'required';
      return body;
    }

    // JSON protocol: the tools are described in the prompt and the output is constrained to calls.
    // The description goes in the system message, ahead of anything that varies, so servers can cache it.
    const messages = request.messages.map((message) => ({ ...message }));
    const instruction = describeTools(request.tools, request.maxCalls);
    const system = messages.find((message) => message.role === 'system');
    if (system) system.content = `${system.content}\n\n${instruction}`;
    else messages.unshift({ role: 'system', content: instruction });
    body.messages = messages;
    body.response_format = caps.jsonSchema
      ? { type: 'json_schema', json_schema: { name: 'tool_calls', strict: true, schema: callsSchema(request.tools, request.maxCalls) } }
      : { type: 'json_object' };
    return body;
  }

  /** Learns from a 400: which parameter did the server not like? Returns true if a retry is worthwhile. */
  private adapt(errorText: string): boolean {
    const caps = this.capabilities;
    const text = errorText.toLowerCase();
    if (caps.toolChoice && text.includes('tool_choice')) {
      caps.toolChoice = false;
      return true;
    }
    if (caps.maxTokensField === 'max_tokens' && text.includes('max_tokens') && text.includes('max_completion_tokens')) {
      caps.maxTokensField = 'max_completion_tokens';
      return true;
    }
    if (caps.reasoningEffort && this.settings.reasoning !== 'default' && (text.includes('reasoning_effort') || text.includes('reasoning'))) {
      caps.reasoningEffort = false;
      return true;
    }
    if (caps.temperature && text.includes('temperature')) {
      caps.temperature = false;
      return true;
    }
    if (caps.protocol === 'json' && caps.jsonSchema && (text.includes('response_format') || text.includes('json_schema'))) {
      caps.jsonSchema = false;
      return true;
    }
    if (caps.protocol === 'tools' && this.settings.protocol === 'auto' && (text.includes('tools') || text.includes('function')) && text.includes('support')) {
      caps.protocol = 'json';
      return true;
    }
    return false;
  }

  async complete(request: LlmRequest): Promise<LlmResponse> {
    const started = performance.now();
    const timeoutMs = request.timeoutMs ?? (this.warmedUp ? DEFAULT_TIMEOUT_MS : WARMUP_TIMEOUT_MS);
    const controller = new AbortController();
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      controller.abort();
    }, timeoutMs);
    const onAbort = (): void => controller.abort();
    request.signal?.addEventListener('abort', onAbort);

    try {
      for (let attempt = 0; ; attempt++) {
        let response: Response;
        try {
          response = await this.fetchImpl(`${trimSlash(this.settings.baseUrl)}/chat/completions`, {
            method: 'POST',
            headers: this.headers(),
            body: JSON.stringify(this.body(request)),
            signal: controller.signal,
          });
        } catch (error) {
          if (timedOut) throw new LlmError('timeout', `No answer within ${Math.round(timeoutMs / 1000)} s.`);
          if (request.signal?.aborted) throw new LlmError('aborted', 'The request was cancelled.');
          throw new LlmError('network', `Could not reach ${this.settings.baseUrl}: ${String((error as Error)?.message ?? error)}. If the server is running, it may not allow requests from this page (CORS).`);
        }

        if (!response.ok) {
          const text = await response.text().catch(() => '');
          if (response.status === 400 && attempt < MAX_ADAPTATIONS && this.adapt(text)) continue;
          const retryAfter = Number(response.headers.get('retry-after'));
          throw new LlmError('http', `The server answered ${response.status}: ${text.slice(0, 300)}`, response.status, Number.isFinite(retryAfter) && retryAfter > 0 ? retryAfter * 1000 : undefined, text);
        }

        const payload = (await response.json()) as {
          choices?: Array<{ message?: Record<string, unknown>; finish_reason?: string }>;
          usage?: { prompt_tokens?: number; completion_tokens?: number };
        };
        this.warmedUp = true;
        const choice = payload.choices?.[0];
        const message = choice?.message ?? {};
        const content = typeof message.content === 'string' ? message.content : '';
        // Some servers put the whole constrained answer in the reasoning channel and leave content empty.
        const reasoning = typeof message.reasoning_content === 'string' ? message.reasoning_content : typeof message.reasoning === 'string' ? message.reasoning : '';
        const protocol = this.capabilities.protocol;
        let calls: ToolCall[] = protocol === 'tools' ? callsFromMessage(message, request.tools) : callsFromText(content, request.tools);
        if (calls.length === 0 && reasoning) calls = callsFromText(reasoning, request.tools);
        calls = calls.slice(0, request.maxCalls);
        const raw = JSON.stringify(message.tool_calls && (message.tool_calls as unknown[]).length > 0
          ? { content: message.content ?? null, tool_calls: message.tool_calls }
          : content || reasoning.slice(-600));

        if (calls.length === 0) {
          const truncated = choice?.finish_reason === 'length';
          if (truncated) {
            // Cut off (often by a model thinking aloud first): give it more room and try once more.
            if (this.capabilities.tokenScale < MAX_TOKEN_SCALE) {
              this.capabilities.tokenScale = Math.min(MAX_TOKEN_SCALE, this.capabilities.tokenScale * 2);
              if (attempt < MAX_ADAPTATIONS) continue;
            }
            throw new LlmError('truncated', 'The answer was cut off before it finished (raise the output token limit).', undefined, undefined, raw);
          }
          // A model that answers in prose despite its tools is switched to constrained JSON, which
          // servers enforce with a grammar (measured on a local Qwen: 85% usable natively, 100% as JSON).
          if (protocol === 'tools' && this.settings.protocol === 'auto' && ++this.capabilities.toolMisses >= 1) {
            this.capabilities.protocol = 'json';
            if (attempt < MAX_ADAPTATIONS) continue;
          }
          throw new LlmError('no_call', 'The model answered without calling a tool.', undefined, undefined, raw);
        }
        this.capabilities.toolMisses = 0;
        return {
          calls,
          text: stripThinking(content),
          raw,
          promptTokens: payload.usage?.prompt_tokens ?? 0,
          completionTokens: payload.usage?.completion_tokens ?? 0,
          latencyMs: performance.now() - started,
          protocol,
        };
      }
    } finally {
      clearTimeout(timer);
      request.signal?.removeEventListener('abort', onAbort);
    }
  }
}
