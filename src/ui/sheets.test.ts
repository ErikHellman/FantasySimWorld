import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChatMessage } from '../generated/api';
import { addChat, closeSheet, fromChatWelcome, setChatView, togglePanel, toggleSheet, ui } from './store';

const message = (id: number): ChatMessage => ({ id, at: 0, kind: 'say', author: 'Mossy Wren', text: `message ${id}` });

beforeEach(() => {
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {} });
  ui.mobile.value = true;
  ui.sheet.value = 'none';
  ui.panel.value = 'none';
  ui.chatUnread.value = 0;
  fromChatWelcome({ you: 'Old Tom', history: [], maxLength: 500 });
});

afterEach(() => {
  ui.mobile.value = false;
  vi.unstubAllGlobals();
});

describe('phone panes', () => {
  it('opens one sheet at a time and closes it when tapped again', () => {
    toggleSheet('villagers');
    expect(ui.sheet.value).toBe('villagers');
    toggleSheet('feed');
    expect(ui.sheet.value).toBe('feed');
    toggleSheet('feed');
    expect(ui.sheet.value).toBe('none');
  });

  it('closes the sheet when a window opens, and the window when a sheet opens', () => {
    toggleSheet('menu');
    togglePanel('settings');
    expect(ui.panel.value).toBe('settings');
    expect(ui.sheet.value).toBe('none');
    toggleSheet('chat');
    expect(ui.panel.value).toBe('none');
    expect(ui.sheet.value).toBe('chat');
    closeSheet();
    expect(ui.sheet.value).toBe('none');
  });

  it('counts chat messages as unread unless the chat sheet is open, whatever the desktop strip is set to', () => {
    setChatView('expanded');
    addChat(message(1));
    expect(ui.chatUnread.value).toBe(1);
    toggleSheet('chat');
    expect(ui.chatUnread.value).toBe(0);
    addChat(message(2));
    expect(ui.chatUnread.value).toBe(0);
  });

  it('leaves the desktop chat strip in charge of unread messages on a wide screen', () => {
    ui.mobile.value = false;
    ui.sheet.value = 'chat';
    setChatView('collapsed');
    addChat(message(1));
    expect(ui.chatUnread.value).toBe(1);
    setChatView('expanded');
    addChat(message(2));
    expect(ui.chatUnread.value).toBe(0);
  });
});
