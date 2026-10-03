# Viewers' chat — design

## Context

People watching the shared village had no way to talk to each other. This adds a chat for everyone who is signed in:

- Each person gets an auto-assigned name, which they can change in Settings as long as nobody connected at the same time has it.
- The server keeps the last 50 messages and sends them to each new connection first.
- Messages take a safe subset of markdown and emoji, with no attachments and a cap of 500 characters.
- A server-side filter sees every message before it goes out. For now it allows everything.
- The chat is a strip along the bottom of the screen that can be expanded, collapsed or hidden.

Decisions made with the owner:

| Topic | Decision |
|---|---|
| Chosen name | Remembered in the browser (`fsw.chat` in local storage) and claimed again after each connect; no database change |
| Layout | A full-width strip along the bottom; the Chronicle and Inspector move up by its height |
| Markdown and emoji | A small hand-written subset rendered to Preact elements (never `innerHTML`), plus a curated emoji picker; no new dependencies |
| History | In memory only: it survives a new world, not a restart |

## Who counts as one person

`ChatKey` decides who counts as one person:

- With Google or Apple sign-in, the key is `User(user_id)`, so all of an account's tabs share one name.
- With a shared token or sign-in off, everyone is the same account, so the key is `Tab(?viewer=)`.
- A connection with neither can read but not post.

Joins are counted per key. A name is freed when that key's last stream closes, so "taken" means taken by someone connected right now. `Viewer` is fixed when a stream opens, so the current name is kept in the hub, not in `Viewer`.

## Server (`fsw-server` only; the sim is untouched)

**`chat.rs`: `ChatHub`, held in `AppState.chat`.** It holds:
- the history, the newest 50 messages oldest first;
- the members, mapping each key to its name and open stream count;
- a broadcast channel of `ChatOut`;
- a `RateLimiter<ChatKey>` with a burst of 5 and one message per second, shared by messages and renames.

`RateLimiter` was made generic over its key; `/auth` keeps its own limits.

**Messages:**
- **`clean_text`** settles line endings and removes control and text-direction characters. It trims trailing spaces, keeps at most two blank lines in a row, and refuses an empty message or one over 500 UTF-16 units.
- **`filter(author, text) -> Verdict`** is the hook for moderation. It returns `Allow(text)`, which may carry rewritten text, or `Refuse(reason)`, which becomes a 422. Today it always allows.

**Names:**
- Auto names pair an adjective with a noun, 40 of each. After eight collisions a number is added.
- **`validate_name`** collapses spaces and allows 2–24 UTF-16 units of letters, digits, spaces and `-_'.`.
- Names are unique ignoring case.

**Stream:**
- A stream subscribes to the chat channel before it joins.
- It sends `chat_welcome` (`you`, `history`, `maxLength`) after the opening events.
- Then it sends `chat` for each message whose id is newer than its welcome, and `chat_you` when its own key is renamed.
- If it falls behind on chat, it sends a fresh `chat_welcome` instead of dropping the viewer.

**Routes**, behind the CSRF and session layers, open to every role:
- `POST /api/chat` (`ChatPost`) answers:
  - 204 when the message went out
  - 409 with no open stream
  - 422 for an empty or too-long message, or a refusal from the filter
  - 429 when sending too fast
- `POST /api/chat/name` (`ChatRename`) answers:
  - 200 with `ChatYou`
  - 409 when the name is taken, or with no open stream
  - 422 for an invalid name
  - 429 when renaming too fast

The new wire types are `ChatMessage`, `ChatWelcome`, `ChatYou`, `ChatPost` and `ChatRename`. They only add to the protocol, so `PROTOCOL_VERSION` stays at 1.

## Client

- **`net/stream.ts`** handles the three new events; **`net/api.ts`** adds `chat` and `chatName`.
- **`ui/store.ts`** holds the chat signals: messages, name, maximum length, view and unread count. It also loads and saves `fsw.chat`.
- **`main.ts`** claims the stored name after each welcome.
- **`ui/chatMarkdown.ts`** parses `**bold**`, `*italic*`/`_italic_`, `~~strike~~`, `` `code` ``, fenced blocks, `[text](url)`, bare `https://` links and backslash escapes into a tree. Links survive only when they are http(s). It also gives a plain-text preview.
- **`ui/Chat.tsx`** renders that tree with Preact. Links open in a new tab with `rel="noopener noreferrer nofollow ugc"`.
- **`ChatPanel`** has three views:
  - expanded: the history (it follows new messages unless you have scrolled up) and the input
  - collapsed: one header line with the latest message and the unread count
  - hidden: a 💬 button with the unread count
- **The input**:
  - Enter sends; Shift+Enter adds a new line; Escape collapses.
  - `maxlength` follows the server's cap, and a counter appears near it.
  - Files dropped or pasted in are refused.
- **Height**: the panel writes its height to `--chat-h`, so the Chronicle and Inspector sit above it, on phones too.
- **Settings** has a chat name field.

## Tests

- **`chat.rs` unit tests:**
  - history keeps the newest 50
  - shared names per account
  - auto names never collide
  - rename uniqueness, case and freeing
  - name and text rules, with the cap counted in UTF-16
  - the filter allows everything
  - the rate limit
- **`tests/http.rs`:**
  - welcome and history
  - a message reaching another tab and a later connection
  - 409, 422, 400 and 415 refusals
  - a taken name, a rename and `chat_you`
  - a name freed when its owner leaves
- **`tests/auth.rs`:** separate names for separate Google users, one shared name across an account's tabs, and a viewer-role user may post.
- **`chatMarkdown.test.ts`** covers the parser, including `javascript:` and `data:` links staying text.
- **`chatSettings.test.ts`** covers stored settings (including blocked storage), skipping duplicate ids, unread counts and the 50 cap.
