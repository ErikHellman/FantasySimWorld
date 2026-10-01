import type { LlmSettings } from '../../persist/settings';
import { LlmClient } from './client';
import { LlmError, type ToolDef } from './types';

const PROBE_TOOLS: ToolDef[] = [
  {
    name: 'go_to',
    description: 'Walk to a place.',
    parameters: { type: 'object', properties: { place: { type: 'string', description: 'tavern, market or chapel' } }, required: ['place'], additionalProperties: false },
  },
  { name: 'continue', description: 'Carry on with what you are doing.', parameters: { type: 'object', properties: {}, additionalProperties: false } },
];

export interface ProbeResult {
  ok: boolean;
  message: string;
  latencyMs: number;
  protocol: string;
}

type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

/** Asks the endpoint for one small decision, to check that it is reachable and can call tools. */
export async function probe(settings: LlmSettings, fetchImpl?: FetchLike): Promise<ProbeResult> {
  const client = new LlmClient({ ...settings, enabled: true }, fetchImpl);
  const request = {
    messages: [
      { role: 'system' as const, content: 'You are a villager. Answer only by calling a tool.' },
      { role: 'user' as const, content: 'You are hungry and the tavern serves food. Decide what to do.' },
    ],
    tools: PROBE_TOOLS,
    maxCalls: 1,
    maxTokens: Math.max(200, settings.maxTokens),
  };
  let lastError: unknown = null;
  // Two tries: the first may teach the client a quirk of this endpoint (or switch it to JSON output).
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      const response = await client.complete(request);
      const call = response.calls[0];
      return {
        ok: true,
        latencyMs: response.latencyMs,
        protocol: response.protocol,
        message: `Works. The model chose ${call?.name}(${JSON.stringify(call?.args)}) in ${Math.round(response.latencyMs)} ms using ${response.protocol === 'tools' ? 'native tool calls' : 'JSON output'}.`,
      };
    } catch (error) {
      lastError = error;
      if (!(error instanceof LlmError) || (error.kind !== 'no_call' && error.kind !== 'truncated')) break;
    }
  }
  const message = lastError instanceof Error ? lastError.message : String(lastError);
  return { ok: false, latencyMs: 0, protocol: client.protocol, message: `Failed: ${message}` };
}
