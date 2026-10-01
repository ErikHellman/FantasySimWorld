import { describe, expect, it } from 'vitest';
import { callsFromMessage, callsFromText, jsonBlocks, looseJson, stripThinking } from './parse';
import type { ToolDef } from './types';

const tools: ToolDef[] = [
  { name: 'go_to', description: '', parameters: { type: 'object', properties: { place: { type: 'string' } } } },
  { name: 'continue', description: '', parameters: { type: 'object', properties: {} } },
];

describe('tolerant parsing', () => {
  it('strips reasoning blocks, including one whose opening tag was lost', () => {
    expect(stripThinking('<think>hmm</think>{"a":1}')).toBe('{"a":1}');
    expect(stripThinking('still thinking...</think> done')).toBe('done');
  });

  it('repairs slightly broken JSON', () => {
    expect(looseJson('{"a": 1,}')).toEqual({ a: 1 });
    expect(looseJson("{'place': 'tavern'}")).toEqual({ place: 'tavern' });
    expect(looseJson({ already: true })).toEqual({ already: true });
    expect(looseJson('not json')).toBeUndefined();
  });

  it('finds balanced JSON blocks in prose', () => {
    expect(jsonBlocks('I will go. {"name":"go_to","arguments":{"place":"a } b"}} ok [1,2]')).toEqual([
      '{"name":"go_to","arguments":{"place":"a } b"}}',
      '[1,2]',
    ]);
  });

  it('reads native tool calls with string or object arguments', () => {
    const message = {
      content: null,
      tool_calls: [
        { type: 'function', function: { name: 'go_to', arguments: '{"place":"tavern"}' } },
        { type: 'function', function: { name: 'continue', arguments: {} } },
        { type: 'function', function: { name: 'unknown_tool', arguments: '{}' } },
      ],
    };
    expect(callsFromMessage(message, tools)).toEqual([
      { name: 'go_to', args: { place: 'tavern' } },
      { name: 'continue', args: {} },
    ]);
  });

  it('falls back to tool calls written as text', () => {
    expect(callsFromMessage({ content: '```json\n{"name": "go_to", "arguments": {"place": "market"}}\n```' }, tools)).toEqual([
      { name: 'go_to', args: { place: 'market' } },
    ]);
    expect(callsFromText('<tool_call>{"tool":"go_to","place":"smithy"}</tool_call>', tools)).toEqual([{ name: 'go_to', args: { place: 'smithy' } }]);
    expect(callsFromText('{"calls":[{"tool":"go_to","args":{"place":"green"}},{"tool":"continue","args":{}}]}', tools)).toEqual([
      { name: 'go_to', args: { place: 'green' } },
      { name: 'continue', args: {} },
    ]);
    expect(callsFromText('{"go_to": {"place": "chapel"}}', tools)).toEqual([{ name: 'go_to', args: { place: 'chapel' } }]);
    expect(callsFromText('I think I will just rest.', tools)).toEqual([]);
  });
});
