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

  it('accepts a play script, a bare tool name, or a plain spoken line when that is what the model wrote', () => {
    const conversation: ToolDef = { name: 'conversation', description: '', parameters: { type: 'object', properties: { lines: {}, topic: {} } } };
    const script = 'Kael: Just the usual tug of the tide, Elowen.\n\nElowen: I see. Your answers are as elusive as your fish.\n\n**Kael**: "And yet here I stand."';
    expect(callsFromText(script, [conversation])).toEqual([{
      name: 'conversation',
      args: { topic: '', lines: [
        { speaker: 'Kael', text: 'Just the usual tug of the tide, Elowen.' },
        { speaker: 'Elowen', text: 'I see. Your answers are as elusive as your fish.' },
        { speaker: 'Kael', text: 'And yet here I stand.' },
      ] },
    }]);

    const react: ToolDef[] = [
      { name: 'fight', description: '', parameters: { type: 'object', properties: { say: {}, thought: {} } } },
      { name: 'flee', description: '', parameters: { type: 'object', properties: { say: {}, thought: {} } } },
    ];
    expect(callsFromText('flee\nthought: I am too old for this.\nsay: The wind calls for rest.\nnote: ignored', react)).toEqual([
      { name: 'flee', args: { thought: 'I am too old for this.', say: 'The wind calls for rest.' } },
    ]);

    const reply: ToolDef = { name: 'reply', description: '', parameters: { type: 'object', properties: { text: {}, feeling: {}, end: {} } } };
    expect(callsFromText('You always did have a sharp tongue.', [reply])).toEqual([
      { name: 'reply', args: { text: 'You always did have a sharp tongue.', feeling: 'same', end: false } },
    ]);
    expect(callsFromText('You always did have a sharp tongue.', tools)).toEqual([]);
  });
});
