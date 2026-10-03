import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ChatMessage } from '../generated/api';
import { addChat, fromChatWelcome, loadChatSettings, saveChatSettings, setChatView, ui } from './store';

function stubStorage(initial: Record<string, string> = {}): Map<string, string> {
  const store = new Map(Object.entries(initial));
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
  });
  return store;
}

const message = (id: number): ChatMessage => ({ id, at: 0, kind: 'say', author: 'Mossy Wren', text: `message ${id}` });

afterEach(() => vi.unstubAllGlobals());

describe('chat settings', () => {
  it('starts collapsed with no chosen name', () => {
    stubStorage();
    expect(loadChatSettings()).toEqual({ view: 'collapsed' });
  });

  it('keeps the view and the chosen name in this browser', () => {
    const store = stubStorage();
    saveChatSettings({ view: 'hidden', name: 'Old Tom' });
    setChatView('expanded');
    expect(JSON.parse(store.get('fsw.chat')!)).toEqual({ view: 'expanded', name: 'Old Tom' });
    expect(loadChatSettings()).toEqual({ view: 'expanded', name: 'Old Tom' });
  });

  it('ignores settings it cannot read', () => {
    stubStorage({ 'fsw.chat': JSON.stringify({ view: 'sideways', name: 7 }) });
    expect(loadChatSettings()).toEqual({ view: 'collapsed' });
    stubStorage({ 'fsw.chat': '{not json' });
    expect(loadChatSettings()).toEqual({ view: 'collapsed' });
    vi.stubGlobal('localStorage', { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); } });
    expect(loadChatSettings()).toEqual({ view: 'collapsed' });
    expect(() => saveChatSettings({ view: 'hidden' })).not.toThrow();
  });
});

describe('chat history', () => {
  it('skips messages the welcome already held and counts unread ones while collapsed', () => {
    stubStorage();
    setChatView('collapsed');
    ui.chatUnread.value = 0;
    fromChatWelcome({ you: 'Mossy Wren', history: [message(1), message(2)], maxLength: 500 });
    addChat(message(2));
    addChat(message(3));
    expect(ui.chat.value.map((m) => m.id)).toEqual([1, 2, 3]);
    expect(ui.chatUnread.value).toBe(1);
    setChatView('expanded');
    expect(ui.chatUnread.value).toBe(0);
  });

  it('does not count comings and goings as unread', () => {
    stubStorage();
    setChatView('collapsed');
    ui.chatUnread.value = 0;
    fromChatWelcome({ you: 'Mossy Wren', history: [], maxLength: 500 });
    addChat({ id: 1, at: 0, kind: 'join', author: 'Old Tom', text: '' });
    addChat({ id: 2, at: 0, kind: 'leave', author: 'Old Tom', text: '' });
    addChat(message(3));
    expect(ui.chat.value.map((m) => m.kind)).toEqual(['join', 'leave', 'say']);
    expect(ui.chatUnread.value).toBe(1);
  });

  it('keeps the last fifty', () => {
    stubStorage();
    fromChatWelcome({ you: null, history: [], maxLength: 500 });
    for (let id = 1; id <= 60; id++) addChat(message(id));
    expect(ui.chat.value).toHaveLength(50);
    expect(ui.chat.value[0]!.id).toBe(11);
  });
});
