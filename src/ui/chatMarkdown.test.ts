import { describe, expect, it } from 'vitest';
import { parseChat, plainText, safeHref } from './chatMarkdown';

const text = (value: string) => ({ kind: 'text', text: value });

describe('chat markdown', () => {
  it('leaves plain text alone, including anything that looks like HTML', () => {
    expect(parseChat('hello <b>valley</b> & co')).toEqual([text('hello <b>valley</b> & co')]);
  });

  it('reads bold, italic, strikethrough and code', () => {
    expect(parseChat('**a** *b* _c_ ~~d~~ `e`')).toEqual([
      { kind: 'strong', children: [text('a')] }, text(' '),
      { kind: 'em', children: [text('b')] }, text(' '),
      { kind: 'em', children: [text('c')] }, text(' '),
      { kind: 'del', children: [text('d')] }, text(' '),
      { kind: 'code', text: 'e' },
    ]);
  });

  it('nests spans', () => {
    expect(parseChat('**bold *and italic* too**')).toEqual([
      { kind: 'strong', children: [text('bold '), { kind: 'em', children: [text('and italic')] }, text(' too')] },
    ]);
    expect(parseChat('*it **bold** it*')).toEqual([
      { kind: 'em', children: [text('it '), { kind: 'strong', children: [text('bold')] }, text(' it')] },
    ]);
  });

  it('keeps unclosed or spaced-out markers as they are', () => {
    expect(parseChat('**not bold')).toEqual([text('**not bold')]);
    expect(parseChat('2 * 3 * 4')).toEqual([text('2 * 3 * 4')]);
    expect(parseChat('snake_case_name')).toEqual([text('snake_case_name')]);
    expect(parseChat('a ` b')).toEqual([text('a ` b')]);
  });

  it('does not read markdown inside code', () => {
    expect(parseChat('`**x**` and\n```js\nconst a = *b*;\n```\nafter')).toEqual([
      { kind: 'code', text: '**x**' }, text(' and'), { kind: 'br' },
      { kind: 'codeBlock', text: 'const a = *b*;' },
      text('after'),
    ]);
  });

  it('honours backslash escapes', () => {
    expect(parseChat('\\*not italic\\*')).toEqual([text('*not italic*')]);
  });

  it('turns line breaks into breaks', () => {
    expect(parseChat('one\ntwo')).toEqual([text('one'), { kind: 'br' }, text('two')]);
  });

  it('links http and https only', () => {
    expect(parseChat('[the map](https://example.com/map) and https://example.com/a.')).toEqual([
      { kind: 'link', href: 'https://example.com/map', children: [text('the map')] },
      text(' and '),
      { kind: 'link', href: 'https://example.com/a', children: [text('https://example.com/a')] },
      text('.'),
    ]);
    expect(parseChat('[x](javascript:alert(1))')).toEqual([text('[x](javascript:alert(1))')]);
    expect(parseChat('[x](data:text/html,hi)')).toEqual([text('[x](data:text/html,hi)')]);
    expect(safeHref('javascript:alert(1)')).toBeNull();
    expect(safeHref('/relative')).toBeNull();
  });

  it('does not put links inside link labels', () => {
    expect(parseChat('[see https://a.example](https://b.example)')).toEqual([
      { kind: 'link', href: 'https://b.example/', children: [text('see https://a.example')] },
    ]);
  });

  it('keeps emoji, joined sequences included', () => {
    expect(parseChat('hi 👋🏽 👨‍👩‍👧 **🎉**')).toEqual([
      text('hi 👋🏽 👨‍👩‍👧 '), { kind: 'strong', children: [text('🎉')] },
    ]);
  });

  it('reads as plain text for previews', () => {
    expect(plainText(parseChat('**Hi** [there](https://example.com)\n`x`'))).toBe('Hi there x');
  });
});
