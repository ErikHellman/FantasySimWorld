import type { ComponentChildren } from 'preact';
import { useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { ChatMessage } from '../generated/api';
import { HttpError } from '../net/api';
import { parseChat, plainText, type ChatNode } from './chatMarkdown';
import { actions, setChatView, ui } from './store';

// ️ asks for the colour emoji form; without it some of these fall back to plain text glyphs.
const EMOJI = [
  '😀', '😂', '😊', '😍', '🤔', '😮', '😢', '😡', '😴', '🥳', '👍', '👎', '👏', '🙌', '🙏', '💪', '👋', '🎉', '❤️', '🔥',
  '✨', '⭐', '☀️', '🌙', '🌧️', '❄️', '🌲', '🌸', '🍄', '🐺', '🐉', '🦉', '🐸', '🍺', '🍞', '🧀', '⚔️', '🛡️', '🏹', '💀',
];

/** Room left under the cap, cut at a whole character (emoji count as two units, like the server counts them). */
function fit(text: string, room: number): string {
  let out = '';
  for (const ch of text) {
    if (out.length + ch.length > room) break;
    out += ch;
  }
  return out;
}

/** A chat refusal as a sentence for the person who sent it. */
export function chatError(error: unknown): string {
  if (error instanceof HttpError) {
    if (error.status === 429) return 'Slow down a little, then try again.';
    if (error.status === 409 && error.message === 'that name is taken') return 'Someone connected is already called that.';
    if (error.status === 409) return 'Not connected to the chat yet. Try again in a moment.';
    const message = error.message.charAt(0).toUpperCase() + error.message.slice(1);
    return message.endsWith('.') ? message : `${message}.`;
  }
  return 'Could not reach the server.';
}

function render(nodes: readonly ChatNode[]): ComponentChildren {
  return nodes.map((node) => {
    switch (node.kind) {
      case 'text':
        return node.text;
      case 'br':
        return <br />;
      case 'code':
        return <code>{node.text}</code>;
      case 'codeBlock':
        return <pre><code>{node.text}</code></pre>;
      case 'strong':
        return <strong>{render(node.children)}</strong>;
      case 'em':
        return <em>{render(node.children)}</em>;
      case 'del':
        return <del>{render(node.children)}</del>;
      case 'link':
        return <a href={node.href} target="_blank" rel="noopener noreferrer nofollow ugc">{render(node.children)}</a>;
    }
  });
}

/** A message's markdown as elements; text only ever goes in as text. */
export function ChatText({ text }: { text: string }) {
  return <>{render(parseChat(text))}</>;
}

const timeOf = (at: number): string => new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

function History({ messages, me }: { messages: readonly ChatMessage[]; me: string | null }) {
  const list = useRef<HTMLOListElement>(null);
  // Follow new messages unless the reader has scrolled up to read older ones.
  const pinned = useRef(true);
  useLayoutEffect(() => {
    const element = list.current;
    if (element && pinned.current) element.scrollTop = element.scrollHeight;
  }, [messages]);
  const onScroll = (): void => {
    const element = list.current;
    if (element) pinned.current = element.scrollHeight - element.scrollTop - element.clientHeight < 24;
  };

  if (messages.length === 0) return <p class="muted chat-empty">Nobody has said anything yet. Say hello!</p>;
  return (
    <ol class="chat-history" ref={list} onScroll={onScroll}>
      {messages.map((message) => (
        <li key={message.id} class={message.author === me ? 'chat-line mine' : 'chat-line'}>
          <span class="chat-time">{timeOf(message.at)}</span>
          <span class="chat-who">{message.author}</span>
          <span class="chat-text"><ChatText text={message.text} /></span>
        </li>
      ))}
    </ol>
  );
}

function Composer() {
  const [draft, setDraft] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [picking, setPicking] = useState(false);
  const input = useRef<HTMLTextAreaElement>(null);
  const max = ui.chatMax.value;
  const name = ui.chatName.value;

  const grow = (): void => {
    const element = input.current;
    if (!element) return;
    element.style.height = 'auto';
    element.style.height = `${Math.min(element.scrollHeight, 110)}px`;
  };
  useLayoutEffect(grow, [draft]);

  /** Puts `text` where the caret is, as far as the cap allows. */
  const insert = (text: string): void => {
    const element = input.current;
    const start = element?.selectionStart ?? draft.length;
    const end = element?.selectionEnd ?? draft.length;
    const added = fit(text, max - (draft.length - (end - start)));
    setDraft(draft.slice(0, start) + added + draft.slice(end));
    requestAnimationFrame(() => {
      element?.focus();
      element?.setSelectionRange(start + added.length, start + added.length);
    });
  };

  const send = async (): Promise<void> => {
    const text = draft.trim();
    if (!text || sending) return;
    setSending(true);
    setError(null);
    try {
      await actions.sendChat(text);
      setDraft('');
      setPicking(false);
    } catch (failure) {
      setError(chatError(failure));
    } finally {
      setSending(false);
      input.current?.focus();
    }
  };

  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      void send();
    } else if (event.key === 'Escape') {
      input.current?.blur();
      setChatView('collapsed');
    }
  };

  // Text only: files dropped or pasted in are turned away (a paste keeps any text it also carries).
  const onDrop = (event: DragEvent): void => {
    if (event.dataTransfer?.files.length) event.preventDefault();
  };
  const onPaste = (event: ClipboardEvent): void => {
    if (!event.clipboardData?.files.length) return;
    event.preventDefault();
    const text = event.clipboardData.getData('text/plain');
    if (text) insert(text);
  };

  return (
    <form class="chat-compose" onSubmit={(event) => { event.preventDefault(); void send(); }}>
      {picking && (
        <div class="emoji-grid" role="listbox" aria-label="Emoji">
          {EMOJI.map((emoji) => (
            <button type="button" key={emoji} onMouseDown={(event) => event.preventDefault()} onClick={() => insert(emoji)}>{emoji}</button>
          ))}
        </div>
      )}
      <div class="chat-row">
        <textarea
          ref={input}
          rows={1}
          maxLength={max}
          value={draft}
          disabled={name === null}
          placeholder={name === null ? 'Joining the chat…' : `Say something as ${name}… (**bold**, *italic*, \`code\`)`}
          aria-label="Chat message"
          onInput={(event) => setDraft(event.currentTarget.value)}
          onKeyDown={onKeyDown}
          onDrop={onDrop}
          onPaste={onPaste}
        />
        <button type="button" class={picking ? 'on' : ''} title="Emoji" aria-label="Emoji" disabled={name === null} onClick={() => setPicking(!picking)}>😀</button>
        <button type="submit" class="primary" disabled={name === null || sending || !draft.trim()}>Send</button>
      </div>
      {(error || draft.length > max - 100) && (
        <div class="chat-status">
          <span class="bad-text">{error}</span>
          {draft.length > max - 100 && <span class="muted">{draft.length}/{max}</span>}
        </div>
      )}
    </form>
  );
}

/** The viewers' chat: a strip along the bottom of the screen that can be expanded, collapsed or hidden. */
export function ChatPanel() {
  const view = ui.chatView.value;
  const messages = ui.chat.value;
  const unread = ui.chatUnread.value;
  const panel = useRef<HTMLElement>(null);

  // Tell the other panels how much of the bottom edge the chat takes, so they sit above it.
  useLayoutEffect(() => {
    const root = document.documentElement;
    const element = panel.current;
    if (!element) {
      root.style.setProperty('--chat-h', '0px');
      return;
    }
    const publish = (): void => root.style.setProperty('--chat-h', `${element.offsetHeight + 10}px`);
    publish();
    const observer = new ResizeObserver(publish);
    observer.observe(element);
    return () => {
      observer.disconnect();
      root.style.setProperty('--chat-h', '0px');
    };
  }, [view]);

  if (view === 'hidden') {
    return (
      <button class="chat-reopen" title="Show the chat" onClick={() => setChatView('collapsed')}>
        💬{unread > 0 && <span class="chat-unread">{unread}</span>}
      </button>
    );
  }

  const last = messages.at(-1);
  const expanded = view === 'expanded';
  return (
    <section class={`panel chat ${view}`} ref={panel} aria-label="Chat">
      <header class="chat-head" onClick={() => setChatView(expanded ? 'collapsed' : 'expanded')}>
        <h2>Chat</h2>
        {!expanded && (
          <span class="chat-preview">
            {last ? <><b>{last.author}:</b> {plainText(parseChat(last.text))}</> : <span class="muted">Nobody has said anything yet.</span>}
          </span>
        )}
        {expanded && ui.chatName.value && <span class="chat-preview muted">You are {ui.chatName.value}</span>}
        {unread > 0 && <span class="chat-unread">{unread}</span>}
        <button
          class="chat-toggle" title={expanded ? 'Collapse the chat' : 'Expand the chat'}
          onClick={(event) => { event.stopPropagation(); setChatView(expanded ? 'collapsed' : 'expanded'); }}
        >{expanded ? '▾' : '▴'}</button>
        <button class="chat-toggle" title="Hide the chat" onClick={(event) => { event.stopPropagation(); setChatView('hidden'); }}>✕</button>
      </header>
      {expanded && (
        <>
          <History messages={messages} me={ui.chatName.value} />
          <Composer />
        </>
      )}
    </section>
  );
}
