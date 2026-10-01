import type { ToolCall, ToolDef } from './types';

/** Removes reasoning blocks some models put in front of their answer. */
export function stripThinking(text: string): string {
  return text
    .replace(/<think(?:ing)?>[\s\S]*?<\/think(?:ing)?>/gi, '')
    .replace(/^[\s\S]*?<\/think(?:ing)?>/i, '') // an unopened block (the opening tag was cut off)
    .trim();
}

/** Parses JSON that may be a string, already an object, or slightly malformed. */
export function looseJson(value: unknown): unknown {
  if (value === null || value === undefined) return undefined;
  if (typeof value !== 'string') return value;
  const text = value.trim();
  if (!text) return undefined;
  try {
    return JSON.parse(text);
  } catch {
    // Trailing commas and single quotes are the usual offenders.
    try {
      return JSON.parse(text.replace(/,\s*([}\]])/g, '$1').replace(/([{,]\s*)'([^']+)'\s*:/g, '$1"$2":').replace(/:\s*'([^']*)'/g, ': "$1"'));
    } catch {
      return undefined;
    }
  }
}

/** Every balanced top-level {...} or [...] block in a text, in order. */
export function jsonBlocks(text: string): string[] {
  const blocks: string[] = [];
  let depth = 0;
  let start = -1;
  let inString = false;
  let escaped = false;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (inString) {
      if (escaped) escaped = false;
      else if (ch === '\\') escaped = true;
      else if (ch === '"') inString = false;
      continue;
    }
    if (ch === '"') inString = true;
    else if (ch === '{' || ch === '[') {
      if (depth === 0) start = i;
      depth++;
    } else if (ch === '}' || ch === ']') {
      if (depth > 0 && --depth === 0 && start >= 0) {
        blocks.push(text.slice(start, i + 1));
        start = -1;
      }
    }
  }
  return blocks;
}

function asObject(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function nameMatch(name: unknown, tools: readonly ToolDef[]): string | null {
  if (typeof name !== 'string') return null;
  const wanted = name.trim().toLowerCase().replace(/^functions?[.:]/, '');
  return tools.find((tool) => tool.name.toLowerCase() === wanted)?.name ?? null;
}

/** Recognises the shapes models use for a tool call when they write it as plain JSON. */
function callFrom(value: unknown, tools: readonly ToolDef[]): ToolCall | null {
  const object = asObject(value);
  if (!object) return null;
  const fn = asObject(object.function);
  const name = nameMatch(object.name ?? object.tool ?? object.tool_name ?? object.action ?? fn?.name, tools);
  if (name) {
    const args = asObject(looseJson(object.arguments ?? object.args ?? object.parameters ?? object.input ?? fn?.arguments)) ?? {};
    // {"tool": "go_to", "place": "tavern"}: arguments given inline.
    if (Object.keys(args).length === 0) {
      const inline: Record<string, unknown> = {};
      for (const key of Object.keys(object)) {
        if (!['name', 'tool', 'tool_name', 'action', 'function', 'arguments', 'args', 'parameters', 'input', 'type', 'id'].includes(key)) inline[key] = object[key];
      }
      return { name, args: inline };
    }
    return { name, args };
  }
  // {"go_to": {"place": "tavern"}}
  const keys = Object.keys(object);
  if (keys.length === 1) {
    const only = nameMatch(keys[0], tools);
    const args = asObject(object[keys[0] as string]);
    if (only && args) return { name: only, args };
  }
  return null;
}

/** Finds tool calls written into the text of a reply. */
export function callsFromText(text: string, tools: readonly ToolDef[]): ToolCall[] {
  const calls: ToolCall[] = [];
  const visit = (value: unknown): void => {
    if (Array.isArray(value)) {
      value.forEach(visit);
      return;
    }
    const object = asObject(value);
    if (!object) return;
    const wrapped = object.calls ?? object.tool_calls ?? object.actions ?? object.steps;
    if (Array.isArray(wrapped)) {
      wrapped.forEach(visit);
      return;
    }
    const call = callFrom(object, tools);
    if (call) calls.push(call);
  };
  for (const block of jsonBlocks(stripThinking(text))) visit(looseJson(block));
  return calls;
}

/** Tool calls from an OpenAI-style assistant message, falling back to its text. */
export function callsFromMessage(message: Record<string, unknown>, tools: readonly ToolDef[]): ToolCall[] {
  const calls: ToolCall[] = [];
  const native = message.tool_calls;
  if (Array.isArray(native)) {
    for (const entry of native) {
      const fn = asObject(asObject(entry)?.function);
      const name = nameMatch(fn?.name, tools);
      if (!name) continue;
      calls.push({ name, args: asObject(looseJson(fn?.arguments)) ?? {} });
    }
  }
  if (calls.length > 0) return calls;
  const legacy = asObject(message.function_call);
  const legacyName = nameMatch(legacy?.name, tools);
  if (legacyName) return [{ name: legacyName, args: asObject(looseJson(legacy?.arguments)) ?? {} }];
  return typeof message.content === 'string' ? callsFromText(message.content, tools) : [];
}
