//! The viewers' chat: who is connected under which name, the last messages, and the filter
//! every message passes before anyone sees it.
//!
//! Chat lives beside the world, not in it: nothing here touches the sim, and the history
//! survives a new world (but not a restart). Names are unique among the people connected
//! right now; a name is freed when its owner's last stream closes.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use fsw_core::jsmath::utf16_len;
use parking_lot::Mutex;
use tokio::sync::broadcast;

use crate::api::wire::{ChatMessage, ChatWelcome, ChatYou};
use crate::auth::Viewer;
use crate::auth::rate_limit::RateLimiter;
use crate::store::now_ms;

/// Messages kept for, and sent to, each new connection.
pub const HISTORY_KEPT: usize = 50;
/// Longest message, in UTF-16 code units (what the browser's `maxlength` counts).
pub const MAX_TEXT: usize = 500;
/// Shortest and longest name, in UTF-16 code units.
pub const NAME_MIN: usize = 2;
pub const NAME_MAX: usize = 24;
/// Messages and renames one person may send in a burst, and the sustained rate after it.
const BURST: f64 = 5.0;
const PER_SECOND: f64 = 1.0;
/// Chat is slow next to ticks; a stream this far behind just gets the history again.
const CHANNEL: usize = 256;
/// More blank lines in a row than this are dropped.
const BLANK_LINES_KEPT: usize = 2;

const ADJECTIVES: [&str; 40] = [
    "Amber",
    "Ashen",
    "Brave",
    "Bright",
    "Brisk",
    "Copper",
    "Dappled",
    "Dusky",
    "Early",
    "Fabled",
    "Fern",
    "Gentle",
    "Gilded",
    "Hazel",
    "Hidden",
    "Hollow",
    "Honest",
    "Ivory",
    "Jolly",
    "Lucky",
    "Merry",
    "Misty",
    "Mossy",
    "Nimble",
    "Old",
    "Quiet",
    "Rowan",
    "Russet",
    "Silver",
    "Sleepy",
    "Stout",
    "Sunny",
    "Swift",
    "Tawny",
    "Thistle",
    "Velvet",
    "Wandering",
    "Willow",
    "Windy",
    "Wise",
];
const NOUNS: [&str; 40] = [
    "Badger",
    "Bard",
    "Beacon",
    "Bramble",
    "Candle",
    "Cobbler",
    "Crow",
    "Drake",
    "Falcon",
    "Fox",
    "Gnome",
    "Griffin",
    "Hare",
    "Hearth",
    "Heron",
    "Kettle",
    "Knight",
    "Lantern",
    "Lark",
    "Mage",
    "Miller",
    "Moth",
    "Otter",
    "Owl",
    "Pilgrim",
    "Pine",
    "Raven",
    "Rook",
    "Sage",
    "Shepherd",
    "Sparrow",
    "Squire",
    "Stag",
    "Thrush",
    "Tinker",
    "Toad",
    "Traveller",
    "Weaver",
    "Wolf",
    "Wren",
];

/// One chat participant: a signed-in account (all its tabs share a name) or, when everyone
/// shares one identity (token or sign-in off), a single tab.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ChatKey {
    User(i64),
    Tab(String),
}

impl ChatKey {
    /// `None` when there is nothing to tell this person apart by (no account, no tab id):
    /// they can read the chat but not take part.
    pub fn of(viewer: &Viewer, tab: &str) -> Option<Self> {
        match viewer.user_id {
            Some(id) => Some(Self::User(id)),
            None if !tab.is_empty() && tab.len() <= 64 => Some(Self::Tab(tab.to_owned())),
            None => None,
        }
    }
}

/// What the hub tells every open stream.
#[derive(Clone, Debug)]
pub enum ChatOut {
    /// a new message (`ChatMessage` JSON); `id` lets a stream skip what its welcome already held
    Message { id: u64, json: Arc<str> },
    /// `key` has a new name (`ChatYou` JSON), for that person's own streams
    Renamed { key: ChatKey, json: Arc<str> },
}

/// Why a message or a rename was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatRefusal {
    /// this person has no open stream, so has no name yet
    NotConnected,
    TooFast,
    Empty,
    TooLong,
    NameTaken,
    BadName(&'static str),
    /// refused by [`filter`], with the reason to show
    Filtered(String),
}

/// What [`filter`] decided about a message before it goes out.
pub enum Verdict {
    /// send it, with this (possibly rewritten) text
    Allow(String),
    /// do not send it; the reason is shown to the author
    Refuse(String),
}

/// Every chat message passes through here before anyone sees it. It allows everything for
/// now; moderation (word lists, a model check) goes here later.
pub fn filter(_author: &str, text: String) -> Verdict {
    Verdict::Allow(text)
}

/// Tidies a message: one kind of line ending, no control or text-direction characters, no
/// trailing spaces, at most two blank lines in a row. Fails when nothing is left or the
/// result is longer than [`MAX_TEXT`].
pub fn clean_text(raw: &str) -> Result<String, ChatRefusal> {
    let text = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<String> = Vec::new();
    let mut blanks = 0;
    for line in text.split('\n') {
        let line: String = line
            .chars()
            .map(|c| if c == '\t' { ' ' } else { c })
            .filter(|&c| !c.is_control() && !is_direction_control(c))
            .collect();
        let line = line.trim_end();
        if line.is_empty() {
            blanks += 1;
            if blanks > BLANK_LINES_KEPT {
                continue;
            }
        } else {
            blanks = 0;
        }
        lines.push(line.to_owned());
    }
    let text = lines.join("\n").trim().to_owned();
    if text.is_empty() {
        return Err(ChatRefusal::Empty);
    }
    if utf16_len(&text) > MAX_TEXT {
        return Err(ChatRefusal::TooLong);
    }
    Ok(text)
}

/// Embedding, override and isolate marks, which can make a line read backwards (or pass for
/// someone else's name).
fn is_direction_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Checks a wanted name and returns it tidied (trimmed, single spaces).
pub fn validate_name(raw: &str) -> Result<String, ChatRefusal> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if !name.chars().all(|c| c.is_alphanumeric() || " -_'.".contains(c)) {
        return Err(ChatRefusal::BadName("use letters, digits, spaces and - _ ' . only"));
    }
    let len = utf16_len(&name);
    if len < NAME_MIN {
        return Err(ChatRefusal::BadName("a name needs at least 2 characters"));
    }
    if len > NAME_MAX {
        return Err(ChatRefusal::BadName("a name has at most 24 characters"));
    }
    Ok(name)
}

struct Member {
    name: String,
    /// open streams under this key; the name is freed at zero
    streams: u32,
}

#[derive(Default)]
struct Inner {
    /// oldest first
    history: VecDeque<ChatMessage>,
    next_id: u64,
    members: HashMap<ChatKey, Member>,
}

impl Inner {
    /// Whether someone other than `key` is called `name` (ignoring case).
    fn taken(&self, key: Option<&ChatKey>, name: &str) -> bool {
        let wanted = name.to_lowercase();
        self.members.iter().any(|(k, m)| Some(k) != key && m.name.to_lowercase() == wanted)
    }

    fn fresh_name(&self) -> String {
        for _ in 0..8 {
            let [a, b] = random_pair();
            let name = format!("{} {}", ADJECTIVES[a % ADJECTIVES.len()], NOUNS[b % NOUNS.len()]);
            if !self.taken(None, &name) {
                return name;
            }
        }
        // A crowded room: number a random name until it is free.
        let [a, b] = random_pair();
        let base = format!("{} {}", ADJECTIVES[a % ADJECTIVES.len()], NOUNS[b % NOUNS.len()]);
        (2..).map(|n| format!("{base} {n}")).find(|name| !self.taken(None, name)).expect("some number is free")
    }

    fn welcome(&self, key: Option<&ChatKey>) -> ChatWelcome {
        ChatWelcome {
            you: key.and_then(|key| self.members.get(key)).map(|m| m.name.clone()),
            history: self.history.iter().cloned().collect(),
            max_length: MAX_TEXT as u32,
        }
    }
}

fn random_pair() -> [usize; 2] {
    let mut bytes = [0u8; 2];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    [bytes[0] as usize, bytes[1] as usize]
}

/// The chat room shared by every stream and chat route.
pub struct ChatHub {
    inner: Mutex<Inner>,
    pub out: broadcast::Sender<ChatOut>,
    limiter: RateLimiter<ChatKey>,
}

impl Default for ChatHub {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            out: broadcast::channel(CHANNEL).0,
            limiter: RateLimiter::new(BURST, PER_SECOND),
        }
    }
}

impl ChatHub {
    /// Counts a stream under `key` (giving the person a name if this is their first) and
    /// returns what the stream starts with. The name lasts as long as the guard.
    pub fn join(self: &Arc<Self>, key: Option<ChatKey>) -> (ChatGuard, ChatWelcome) {
        let mut inner = self.inner.lock();
        if let Some(key) = &key {
            if let Some(member) = inner.members.get_mut(key) {
                member.streams += 1;
            } else {
                let name = inner.fresh_name();
                inner.members.insert(key.clone(), Member { name, streams: 1 });
            }
        }
        let welcome = inner.welcome(key.as_ref());
        (ChatGuard { hub: self.clone(), key }, welcome)
    }

    /// The history and `key`'s name now, for a stream that fell behind.
    pub fn welcome(&self, key: Option<&ChatKey>) -> ChatWelcome {
        self.inner.lock().welcome(key)
    }

    /// Gives `key` a new name, if it is valid and nobody else connected has it.
    pub fn rename(&self, key: &ChatKey, wanted: &str, now: Instant) -> Result<String, ChatRefusal> {
        let name = validate_name(wanted)?;
        let mut inner = self.inner.lock();
        let current = &inner.members.get(key).ok_or(ChatRefusal::NotConnected)?.name;
        if *current == name {
            return Ok(name);
        }
        if inner.taken(Some(key), &name) {
            return Err(ChatRefusal::NameTaken);
        }
        if !self.limiter.allow(key.clone(), now) {
            return Err(ChatRefusal::TooFast);
        }
        inner.members.get_mut(key).expect("checked above").name = name.clone();
        let json = serde_json::to_string(&ChatYou { name: name.clone() }).expect("names serialise");
        let _ = self.out.send(ChatOut::Renamed { key: key.clone(), json: Arc::from(json) });
        Ok(name)
    }

    /// Cleans, filters, records and sends a message from `key`.
    pub fn post(&self, key: &ChatKey, raw: &str, now: Instant) -> Result<ChatMessage, ChatRefusal> {
        let author = self.inner.lock().members.get(key).ok_or(ChatRefusal::NotConnected)?.name.clone();
        let text = clean_text(raw)?;
        let text = match filter(&author, text) {
            Verdict::Allow(text) => text,
            Verdict::Refuse(reason) => return Err(ChatRefusal::Filtered(reason)),
        };
        if !self.limiter.allow(key.clone(), now) {
            return Err(ChatRefusal::TooFast);
        }
        let mut inner = self.inner.lock();
        // The name now, in case it changed while the filter ran.
        let author = inner.members.get(key).ok_or(ChatRefusal::NotConnected)?.name.clone();
        inner.next_id += 1;
        let message = ChatMessage { id: inner.next_id, at: now_ms().max(0) as u64, author, text };
        inner.history.push_back(message.clone());
        while inner.history.len() > HISTORY_KEPT {
            inner.history.pop_front();
        }
        // Sent under the lock, so streams see ids in order.
        let json = serde_json::to_string(&message).expect("messages serialise");
        let _ = self.out.send(ChatOut::Message { id: message.id, json: Arc::from(json) });
        Ok(message)
    }
}

/// Keeps a stream's person in the room; their name is freed when their last guard drops.
pub struct ChatGuard {
    hub: Arc<ChatHub>,
    key: Option<ChatKey>,
}

impl ChatGuard {
    pub fn key(&self) -> Option<&ChatKey> {
        self.key.as_ref()
    }
}

impl Drop for ChatGuard {
    fn drop(&mut self) {
        let Some(key) = &self.key else { return };
        let mut inner = self.hub.inner.lock();
        if let Some(member) = inner.members.get_mut(key) {
            member.streams -= 1;
            if member.streams == 0 {
                inner.members.remove(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn tab(id: &str) -> Option<ChatKey> {
        Some(ChatKey::Tab(id.into()))
    }

    fn name(hub: &ChatHub, key: &ChatKey) -> Option<String> {
        hub.welcome(Some(key)).you
    }

    #[test]
    fn keeps_the_newest_fifty_messages_oldest_first() {
        let hub = Arc::new(ChatHub::default());
        let (_guard, _) = hub.join(tab("a"));
        let key = ChatKey::Tab("a".into());
        let start = Instant::now();
        for i in 0..60 {
            hub.post(&key, &format!("message {i}"), start + Duration::from_secs(i)).unwrap();
        }
        let history = hub.welcome(None).history;
        assert_eq!(history.len(), HISTORY_KEPT);
        assert_eq!(history[0].text, "message 10");
        assert_eq!(history[49].text, "message 59");
        assert!(history.windows(2).all(|w| w[0].id < w[1].id));
    }

    #[test]
    fn an_accounts_tabs_share_a_name_until_the_last_one_closes() {
        let hub = Arc::new(ChatHub::default());
        let key = ChatKey::User(7);
        let (first, welcome) = hub.join(Some(key.clone()));
        let (second, again) = hub.join(Some(key.clone()));
        assert_eq!(welcome.you, again.you);
        drop(first);
        assert_eq!(name(&hub, &key), welcome.you);
        drop(second);
        assert_eq!(name(&hub, &key), None);
    }

    #[test]
    fn readers_without_a_key_get_the_history_but_no_name() {
        let hub = Arc::new(ChatHub::default());
        let (guard, welcome) = hub.join(None);
        assert_eq!(welcome.you, None);
        assert_eq!(welcome.max_length, MAX_TEXT as u32);
        assert!(guard.key().is_none());
    }

    #[test]
    fn auto_names_never_collide_with_someone_connected() {
        let hub = Arc::new(ChatHub::default());
        // More people than adjective-noun pairs tried before numbering kicks in.
        let guards: Vec<_> = (0..300).map(|i| hub.join(tab(&format!("t{i}")))).collect();
        let mut names: Vec<_> = guards.iter().map(|(_, w)| w.you.clone().unwrap().to_lowercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 300);
    }

    #[test]
    fn renames_are_unique_among_connected_people_ignoring_case() {
        let hub = Arc::new(ChatHub::default());
        let (a, b) = (ChatKey::Tab("a".into()), ChatKey::Tab("b".into()));
        let (_ga, _) = hub.join(Some(a.clone()));
        let (gb, _) = hub.join(Some(b.clone()));
        let now = Instant::now();
        assert_eq!(hub.rename(&b, "  Old   Tom ", now), Ok("Old Tom".into()));
        assert_eq!(hub.rename(&a, "old tom", now), Err(ChatRefusal::NameTaken));
        assert_eq!(hub.rename(&b, "OLD TOM", now), Ok("OLD TOM".into()), "re-casing your own name is fine");
        drop(gb);
        assert_eq!(hub.rename(&a, "Old Tom", now), Ok("Old Tom".into()), "a name is freed when its owner leaves");
    }

    #[test]
    fn a_rename_frees_the_old_name_and_tells_the_owners_streams() {
        let hub = Arc::new(ChatHub::default());
        let mut out = hub.out.subscribe();
        let (a, b) = (ChatKey::Tab("a".into()), ChatKey::Tab("b".into()));
        let (_ga, _) = hub.join(Some(a.clone()));
        let (_gb, _) = hub.join(Some(b.clone()));
        let now = Instant::now();
        let old = name(&hub, &a).unwrap();
        hub.rename(&a, "Bramblewick", now).unwrap();
        assert_eq!(hub.rename(&b, &old, now), Ok(old));
        match out.try_recv().unwrap() {
            ChatOut::Renamed { key, json } => {
                assert_eq!(key, a);
                assert_eq!(&*json, r#"{"name":"Bramblewick"}"#);
            }
            other => panic!("expected a rename, got {other:?}"),
        }
    }

    #[test]
    fn names_must_be_tidy() {
        assert_eq!(validate_name("Åsa-Lill O'Hara"), Ok("Åsa-Lill O'Hara".into()));
        assert_eq!(validate_name("x"), Err(ChatRefusal::BadName("a name needs at least 2 characters")));
        assert!(validate_name(&"a".repeat(25)).is_err());
        assert!(validate_name("<script>").is_err());
        assert!(validate_name("Tom\u{202E}moT").is_err());
        assert!(validate_name("Tom 😀").is_err());
    }

    #[test]
    fn messages_are_tidied_and_capped_in_utf16_units() {
        assert_eq!(clean_text("  hi\r\nthere\t!  \r"), Ok("hi\nthere !".into()));
        assert_eq!(clean_text("a\n\n\n\n\nb"), Ok("a\n\n\nb".into()));
        assert_eq!(clean_text("bell\u{7}\u{202E}"), Ok("bell".into()));
        assert_eq!(clean_text("👨‍👩‍👧"), Ok("👨‍👩‍👧".into()), "joiners in emoji survive");
        assert_eq!(clean_text(" \n\t "), Err(ChatRefusal::Empty));
        assert!(clean_text(&"x".repeat(MAX_TEXT)).is_ok());
        assert_eq!(clean_text(&"😀".repeat(MAX_TEXT / 2 + 1)), Err(ChatRefusal::TooLong));
    }

    #[test]
    fn the_filter_allows_everything_for_now() {
        assert!(
            matches!(filter("Mossy Wren", "anything at all".into()), Verdict::Allow(text) if text == "anything at all")
        );
    }

    #[test]
    fn posting_needs_a_stream_and_is_rate_limited() {
        let hub = Arc::new(ChatHub::default());
        let key = ChatKey::Tab("a".into());
        let now = Instant::now();
        assert_eq!(hub.post(&key, "hello", now), Err(ChatRefusal::NotConnected));
        let (_guard, _) = hub.join(Some(key.clone()));
        for _ in 0..BURST as usize {
            hub.post(&key, "hello", now).unwrap();
        }
        assert_eq!(hub.post(&key, "hello", now), Err(ChatRefusal::TooFast));
        assert!(hub.post(&key, "hello", now + Duration::from_secs(1)).is_ok());
    }
}
