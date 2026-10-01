import { describe, expect, it } from 'vitest';
import { DEFAULT_SETTINGS, type LlmSettings } from '../../persist/settings';
import { LlmClient } from './client';
import { JobQueue } from './queue';
import { LlmError, type LlmRequest, type ToolDef } from './types';

const tools: ToolDef[] = [
  { name: 'go_to', description: 'Walk to a place.', parameters: { type: 'object', properties: { place: { type: 'string' } }, required: ['place'] } },
];
const request: LlmRequest = { messages: [{ role: 'system', content: 's' }, { role: 'user', content: 'u' }], tools, maxCalls: 2 };
const settings = (patch: Partial<LlmSettings> = {}): LlmSettings => ({ ...DEFAULT_SETTINGS.llm, enabled: true, baseUrl: 'http://llm.test/v1/', model: 'm', ...patch });

function json(body: unknown, status = 200, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json', ...headers } });
}

const toolReply = { choices: [{ message: { content: null, tool_calls: [{ type: 'function', function: { name: 'go_to', arguments: '{"place":"tavern"}' } }] } }], usage: { prompt_tokens: 12, completion_tokens: 5 } };

describe('LlmClient', () => {
  it('sends native tools and reads the call back', async () => {
    const seen: Array<{ url: string; body: Record<string, unknown>; headers: Record<string, string> }> = [];
    const client = new LlmClient(settings({ apiKey: 'k', headers: 'X-Title: Sim' }), async (url, init) => {
      seen.push({ url, body: JSON.parse(String(init?.body)), headers: init?.headers as Record<string, string> });
      return json(toolReply);
    });
    const response = await client.complete(request);
    expect(response.calls).toEqual([{ name: 'go_to', args: { place: 'tavern' } }]);
    expect(response.promptTokens).toBe(12);
    expect(seen[0]?.url).toBe('http://llm.test/v1/chat/completions');
    expect(seen[0]?.body.tool_choice).toBe('required');
    expect((seen[0]?.body.tools as unknown[]).length).toBe(1);
    expect(seen[0]?.headers.Authorization).toBe('Bearer k');
    expect(seen[0]?.headers['X-Title']).toBe('Sim');
  });

  it('omits the Authorization header when there is no key', async () => {
    let headers: Record<string, string> = {};
    const client = new LlmClient(settings(), async (_url, init) => {
      headers = init?.headers as Record<string, string>;
      return json(toolReply);
    });
    await client.complete(request);
    expect(headers.Authorization).toBeUndefined();
  });

  it('adapts when the server rejects tool_choice, max_tokens or temperature', async () => {
    const bodies: Array<Record<string, unknown>> = [];
    const client = new LlmClient(settings(), async (_url, init) => {
      const body = JSON.parse(String(init?.body)) as Record<string, unknown>;
      bodies.push(body);
      if ('tool_choice' in body) return json({ error: { message: 'tool_choice is not supported' } }, 400);
      if ('max_tokens' in body) return json({ error: { message: "Unsupported parameter: 'max_tokens'. Use 'max_completion_tokens' instead." } }, 400);
      if ('temperature' in body) return json({ error: { message: 'temperature does not support 0.8 with this model' } }, 400);
      return json(toolReply);
    });
    const response = await client.complete(request);
    expect(response.calls.length).toBe(1);
    const last = bodies[bodies.length - 1]!;
    expect(last.tool_choice).toBeUndefined();
    expect(last.max_completion_tokens).toBeDefined();
    expect(last.temperature).toBeUndefined();
    // The lessons stick for the next request.
    bodies.length = 0;
    await client.complete(request);
    expect(bodies.length).toBe(1);
  });

  it('switches to JSON output when a model ignores its tools', async () => {
    const bodies: Array<Record<string, unknown>> = [];
    const client = new LlmClient(settings({ protocol: 'auto' }), async (_url, init) => {
      const body = JSON.parse(String(init?.body)) as Record<string, unknown>;
      bodies.push(body);
      if (body.tools) return json({ choices: [{ message: { content: 'I shall wander to the tavern.' } }] });
      return json({ choices: [{ message: { content: '{"calls":[{"tool":"go_to","args":{"place":"tavern"}}]}' } }] });
    });
    const response = await client.complete(request);
    expect(response.protocol).toBe('json');
    expect(bodies.length).toBe(2);
    expect(response.calls).toEqual([{ name: 'go_to', args: { place: 'tavern' } }]);
    const last = bodies[bodies.length - 1]!;
    expect(last.tools).toBeUndefined();
    expect((last.response_format as { type: string }).type).toBe('json_schema');
    expect(JSON.stringify(last.messages)).toContain('Reply with JSON only');
  });

  it('asks reasoning models not to think, and stops asking if the server objects', async () => {
    const bodies: Array<Record<string, unknown>> = [];
    const client = new LlmClient(settings(), async (_url, init) => {
      const body = JSON.parse(String(init?.body)) as Record<string, unknown>;
      bodies.push(body);
      if ('reasoning_effort' in body) return json({ error: { message: "Unrecognized request argument supplied: reasoning_effort" } }, 400);
      return json(toolReply);
    });
    await client.complete(request);
    expect(bodies[0]!.reasoning_effort).toBe('none');
    expect(bodies[1]!.reasoning_effort).toBeUndefined();
    const untouched = new LlmClient(settings({ reasoning: 'default' }), async (_url, init) => {
      bodies.push(JSON.parse(String(init?.body)) as Record<string, unknown>);
      return json(toolReply);
    });
    await untouched.complete(request);
    expect(bodies[bodies.length - 1]!.reasoning_effort).toBeUndefined();
  });

  it('finds the answer when a server puts it in the reasoning channel', async () => {
    const client = new LlmClient(settings({ protocol: 'json' }), async () => json({
      choices: [{ finish_reason: 'stop', message: { content: '', reasoning_content: '{"calls": [{"tool": "go_to", "args": {"place": "tavern"}}]}', tool_calls: [] } }],
    }));
    expect((await client.complete(request)).calls).toEqual([{ name: 'go_to', args: { place: 'tavern' } }]);
  });

  it('gives a cut-off answer more room and tries again, without changing protocol', async () => {
    const limits: number[] = [];
    const client = new LlmClient(settings({ protocol: 'auto', maxTokens: 300 }), async (_url, init) => {
      const body = JSON.parse(String(init?.body)) as Record<string, number>;
      limits.push(body.max_tokens!);
      if (body.max_tokens! < 1000) return json({ choices: [{ finish_reason: 'length', message: { content: '', reasoning_content: 'Thinking Process: 1. Analyse' } }] });
      return json(toolReply);
    });
    const response = await client.complete(request);
    expect(limits).toEqual([300, 600, 1200]);
    expect(response.protocol).toBe('tools');
    // The larger limit is remembered.
    await client.complete(request);
    expect(limits[limits.length - 1]).toBe(1200);
  });

  it('reports HTTP errors with Retry-After and network failures clearly', async () => {
    const limited = new LlmClient(settings(), async () => json({ error: 'slow down' }, 429, { 'Retry-After': '7' }));
    await expect(limited.complete(request)).rejects.toMatchObject({ kind: 'http', status: 429, retryAfterMs: 7000 });
    const down = new LlmClient(settings(), async () => {
      throw new TypeError('Failed to fetch');
    });
    await expect(down.complete(request)).rejects.toMatchObject({ kind: 'network' });
  });

  it('times out', async () => {
    const client = new LlmClient(settings(), (_url, init) => new Promise((_resolve, reject) => {
      init?.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')));
    }));
    await expect(client.complete({ ...request, timeoutMs: 20 })).rejects.toMatchObject({ kind: 'timeout' });
  });
});

describe('JobQueue', () => {
  const wait = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

  it('respects concurrency and priority, and replaces queued jobs with the same key', async () => {
    const queue = new JobQueue(1, 0);
    const order: string[] = [];
    const dropped: string[] = [];
    const job = (key: string, priority: number) => ({
      key, priority, expiresAt: performance.now() + 5000,
      run: async () => {
        order.push(key);
        await wait(5);
      },
      onDrop: (reason: string) => dropped.push(`${key}:${reason}`),
    });
    queue.enqueue(job('first', 5));
    queue.enqueue(job('low', 9));
    queue.enqueue(job('high', 1));
    queue.enqueue(job('low', 9));
    expect(queue.inFlight).toBe(1);
    await wait(40);
    expect(order).toEqual(['first', 'high', 'low']);
    expect(dropped).toEqual(['low:replaced']);
    expect(queue.completed).toBe(3);
  });

  it('drops expired jobs and opens the circuit after repeated failures', async () => {
    const queue = new JobQueue(1, 0);
    const dropped: string[] = [];
    queue.enqueue({ key: 'slow', priority: 1, expiresAt: performance.now() + 5000, run: () => wait(30) });
    queue.enqueue({ key: 'late', priority: 1, expiresAt: performance.now() + 5, run: async () => {}, onDrop: (reason) => dropped.push(reason) });
    await wait(50);
    expect(dropped).toEqual(['expired']);

    for (let i = 0; i < 4; i++) {
      queue.enqueue({ key: `bad${i}`, priority: 1, expiresAt: performance.now() + 5000, run: async () => { throw new LlmError('network', 'down'); } });
      await wait(5);
    }
    expect(queue.circuitOpen).toBe(true);
    queue.enqueue({ key: 'after', priority: 1, expiresAt: performance.now() + 5000, run: async () => {}, onDrop: (reason) => dropped.push(reason) });
    expect(dropped).toEqual(['expired', 'circuit']);
  });
});
