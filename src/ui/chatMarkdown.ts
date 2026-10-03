/**
 * The small piece of markdown the chat understands, parsed into a tree the UI
 * turns into Preact elements. Nothing here produces HTML: chat text is written
 * by strangers, so it only ever reaches the page as text nodes and a few fixed
 * elements, and links only when they are plain http(s).
 *
 * Supported: **bold**, *italic* or _italic_, ~~strikethrough~~, `code`,
 * fenced ``` code blocks ```, [text](https://…) and bare https://… links,
 * line breaks, and a backslash before any of the marker characters.
 */

export type ChatNode =
  | { kind: 'text'; text: string }
  | { kind: 'br' }
  | { kind: 'code'; text: string }
  | { kind: 'codeBlock'; text: string }
  | { kind: 'strong' | 'em' | 'del'; children: ChatNode[] }
  | { kind: 'link'; href: string; children: ChatNode[] };

const ESCAPABLE = '\\`*_~[]()';
const FENCE = /```(?:[\w+-]*\n)?([\s\S]*?)```/g;
const BARE_LINK = /^https?:\/\/[^\s<>]+/;
/** Trailing characters that usually end a sentence rather than a URL. */
const LINK_TAIL = /[.,!?:;'")\]]+$/;

/** The href when `raw` is an absolute http or https URL, else null. */
export function safeHref(raw: string): string | null {
  try {
    const url = new URL(raw);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null;
  } catch {
    return null;
  }
}

export function parseChat(source: string): ChatNode[] {
  const nodes: ChatNode[] = [];
  let at = 0;
  for (const match of source.matchAll(FENCE)) {
    nodes.push(...inline(source.slice(at, match.index), true));
    nodes.push({ kind: 'codeBlock', text: (match[1] ?? '').replace(/\n$/, '') });
    at = match.index + match[0].length;
    // The line break after a closing fence belongs to the block.
    if (source[at] === '\n') at++;
  }
  nodes.push(...inline(source.slice(at), true));
  return merge(nodes);
}

/** The text a message reads as, without its markup (for one-line previews). */
export function plainText(nodes: readonly ChatNode[]): string {
  return nodes
    .map((node) => {
      switch (node.kind) {
        case 'text':
        case 'code':
        case 'codeBlock':
          return node.text;
        case 'br':
          return ' ';
        default:
          return plainText(node.children);
      }
    })
    .join('');
}

const isWordChar = (ch: string | undefined): boolean => ch !== undefined && /[\p{L}\p{N}]/u.test(ch);

/** Where the `marker` closing a span opened just before `from` is, or -1. */
function closing(source: string, from: number, marker: string): number {
  for (let i = from; i < source.length; i++) {
    if (source[i] === '\\') {
      i++;
      continue;
    }
    if (source[i] === '`') {
      // Markers inside a code span do not count.
      const end = source.indexOf('`', i + 1);
      if (end > 0) i = end;
      continue;
    }
    if (!source.startsWith(marker, i)) continue;
    if (marker.length === 1) {
      // A single * or _ next to another is part of a ** or __, not this span's end.
      if (source[i + 1] === marker || source[i - 1] === marker) {
        while (source[i + 1] === marker) i++;
        continue;
      }
      if (marker === '_' && isWordChar(source[i + 1])) continue;
    }
    if (i === from || /\s/.test(source[i - 1] ?? '')) continue;
    return i;
  }
  return -1;
}

function inline(source: string, links: boolean): ChatNode[] {
  const nodes: ChatNode[] = [];
  let text = '';
  const flush = (): void => {
    if (text) nodes.push({ kind: 'text', text });
    text = '';
  };
  const span = (kind: 'strong' | 'em' | 'del', start: number, marker: string): number => {
    const end = closing(source, start, marker);
    if (end < 0 || /\s/.test(source[start] ?? '')) return -1;
    flush();
    nodes.push({ kind, children: inline(source.slice(start, end), links) });
    return end + marker.length;
  };

  let i = 0;
  while (i < source.length) {
    const ch = source[i]!;
    const next = source[i + 1];
    if (ch === '\\' && next !== undefined && ESCAPABLE.includes(next)) {
      text += next;
      i += 2;
      continue;
    }
    if (ch === '\n') {
      flush();
      nodes.push({ kind: 'br' });
      i++;
      continue;
    }
    if (ch === '`') {
      const end = source.indexOf('`', i + 1);
      if (end > i + 1) {
        flush();
        nodes.push({ kind: 'code', text: source.slice(i + 1, end) });
        i = end + 1;
        continue;
      }
    }
    if ((ch === '*' && next === '*') || (ch === '~' && next === '~')) {
      const after = span(ch === '*' ? 'strong' : 'del', i + 2, ch + ch);
      if (after > 0) {
        i = after;
        continue;
      }
      text += ch + ch;
      i += 2;
      continue;
    }
    if ((ch === '*' || (ch === '_' && !isWordChar(source[i - 1]))) && next !== ch) {
      const after = span('em', i + 1, ch);
      if (after > 0) {
        i = after;
        continue;
      }
    }
    if (links && ch === '[') {
      const link = labelledLink(source, i);
      if (link) {
        flush();
        nodes.push({ kind: 'link', href: link.href, children: inline(link.label, false) });
        i = link.end;
        continue;
      }
    }
    if (links && ch === 'h' && !isWordChar(source[i - 1])) {
      const found = BARE_LINK.exec(source.slice(i))?.[0].replace(LINK_TAIL, '');
      const href = found ? safeHref(found) : null;
      if (found && href) {
        flush();
        nodes.push({ kind: 'link', href, children: [{ kind: 'text', text: found }] });
        i += found.length;
        continue;
      }
    }
    text += ch;
    i++;
  }
  flush();
  return nodes;
}

/** `[label](href)` starting at `start`, when the href is safe. */
function labelledLink(source: string, start: number): { label: string; href: string; end: number } | null {
  const close = source.indexOf('](', start + 1);
  if (close < 0 || source.slice(start + 1, close).includes('\n')) return null;
  const end = source.indexOf(')', close + 2);
  if (end < 0) return null;
  const label = source.slice(start + 1, close);
  const href = safeHref(source.slice(close + 2, end).trim());
  if (!label || !href) return null;
  return { label, href, end: end + 1 };
}

/** Joins neighbouring text nodes (left apart by escapes and failed markers). */
function merge(nodes: ChatNode[]): ChatNode[] {
  const out: ChatNode[] = [];
  for (const node of nodes) {
    const last = out.at(-1);
    if (node.kind === 'text' && last?.kind === 'text') last.text += node.text;
    else out.push(node.kind === 'strong' || node.kind === 'em' || node.kind === 'del' || node.kind === 'link'
      ? { ...node, children: merge(node.children) }
      : node);
  }
  return out;
}
