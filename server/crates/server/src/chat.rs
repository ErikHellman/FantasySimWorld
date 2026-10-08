//! The viewers' chat: who is connected under which name, the last messages, and the filter
//! every message passes before anyone sees it.
//!
//! Chat lives beside the world, not in it: nothing here touches the sim, and the history
//! survives a new world (but not a restart). Names are unique among the people in the room.
//!
//! Comings and goings go into the history as lines of their own. A reload, or a tab that was
//! hidden for a while, closes a stream and opens another, so someone is only said to have
//! left once they have been gone for [`AWAY`], and someone back within that time was never
//! said to have gone. Until then they are still in the room as far as anyone can tell, so
//! their name stays theirs: nobody else can take it, and a reload gets it back.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fsw_core::jsmath::utf16_len;
use parking_lot::Mutex;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::api::wire::{ChatKind, ChatMessage, ChatWelcome, ChatYou};
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
/// How long someone must be gone before they are said to have left; back sooner, and
/// neither the leaving nor the coming back is announced.
pub const AWAY: Duration = Duration::from_secs(5 * 60);
/// How often [`spawn_sweeper`] looks for people who have now been gone for [`AWAY`].
const SWEEP_EVERY: Duration = Duration::from_secs(15);

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

/// One chat participant: a guest's pass (a browser's tabs share the cookie, so they share a
/// name) or, for admins, who all hold the same token, a single tab.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ChatKey {
    Guest([u8; 32]),
    Tab(String),
}

impl ChatKey {
    /// `None` when there is nothing to tell this person apart by (an admin without a tab id):
    /// they can read the chat but not take part.
    pub fn of(viewer: &Viewer, tab: &str) -> Option<Self> {
        match &viewer.guest {
            Some(pass) => Some(Self::Guest(pass.id)),
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
    /// open streams under this key; at zero they are [`Gone`]
    streams: u32,
}

/// Someone whose last stream closed, not yet said to have left. Their name is kept for them
/// until they are.
struct Gone {
    /// their name when they went
    name: String,
    at: Instant,
}

#[derive(Default)]
struct Inner {
    /// oldest first
    history: VecDeque<ChatMessage>,
    next_id: u64,
    members: HashMap<ChatKey, Member>,
    gone: HashMap<ChatKey, Gone>,
}

impl Inner {
    /// Whether someone other than `key`, here or not yet said to have left, is called `name`
    /// (ignoring case).
    fn taken(&self, key: Option<&ChatKey>, name: &str) -> bool {
        let wanted = name.to_lowercase();
        let here = self.members.iter().map(|(k, m)| (k, &m.name));
        let away = self.gone.iter().map(|(k, g)| (k, &g.name));
        here.chain(away).any(|(k, name)| Some(k) != key && name.to_lowercase() == wanted)
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

    /// `wanted` if it is a valid name nobody else has, otherwise a fresh one.
    fn name_for(&self, key: &ChatKey, wanted: Option<&str>) -> String {
        wanted
            .and_then(|wanted| validate_name(wanted).ok())
            .filter(|name| !self.taken(Some(key), name))
            .unwrap_or_else(|| self.fresh_name())
    }

    /// Adds a line to the history and sends it to every stream. Callers hold the lock, so
    /// streams see ids in order.
    fn record(
        &mut self,
        out: &broadcast::Sender<ChatOut>,
        kind: ChatKind,
        author: String,
        text: String,
    ) -> ChatMessage {
        self.next_id += 1;
        let message = ChatMessage { id: self.next_id, at: now_ms().max(0) as u64, kind, author, text };
        self.history.push_back(message.clone());
        while self.history.len() > HISTORY_KEPT {
            self.history.pop_front();
        }
        let json = serde_json::to_string(&message).expect("messages serialise");
        let _ = out.send(ChatOut::Message { id: message.id, json: Arc::from(json) });
        message
    }

    /// Says goodbye to everyone gone for [`AWAY`] by `now`, longest gone first.
    fn sweep(&mut self, out: &broadcast::Sender<ChatOut>, now: Instant) {
        let mut left: Vec<(ChatKey, Instant)> = self
            .gone
            .iter()
            .filter(|(_, gone)| now.saturating_duration_since(gone.at) >= AWAY)
            .map(|(key, gone)| (key.clone(), gone.at))
            .collect();
        left.sort_by_key(|(_, at)| *at);
        for (key, _) in left {
            let gone = self.gone.remove(&key).expect("listed above");
            self.record(out, ChatKind::Leave, gone.name, String::new());
        }
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
    /// Counts a stream under `key` and returns what the stream starts with. The name lasts as
    /// long as the guard.
    ///
    /// On a person's first stream they are named `wanted` (the name their browser remembers)
    /// if it is valid and free, or else given a fresh name, and the room is told they joined,
    /// unless they were here less than [`AWAY`] ago.
    pub fn join(
        self: &Arc<Self>,
        key: Option<ChatKey>,
        wanted: Option<&str>,
        now: Instant,
    ) -> (ChatGuard, ChatWelcome) {
        let mut inner = self.inner.lock();
        inner.sweep(&self.out, now);
        if let Some(key) = &key {
            if inner.members.contains_key(key) {
                // Another tab of theirs, or a reload that beat the old stream's close: they keep
                // their place, and take the name they asked for if it is free.
                let renamed = wanted
                    .and_then(|wanted| validate_name(wanted).ok())
                    .filter(|name| inner.members[key].name != *name && !inner.taken(Some(key), name));
                let member = inner.members.get_mut(key).expect("checked above");
                member.streams += 1;
                if let Some(name) = renamed {
                    member.name = name.clone();
                    self.renamed(key, name);
                }
            } else {
                let back = inner.gone.remove(key);
                let name = inner.name_for(key, wanted.or(back.as_ref().map(|gone| gone.name.as_str())));
                inner.members.insert(key.clone(), Member { name: name.clone(), streams: 1 });
                if back.is_none() {
                    inner.record(&self.out, ChatKind::Join, name, String::new());
                }
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
        self.renamed(key, name.clone());
        Ok(name)
    }

    /// Tells `key`'s streams their new name.
    fn renamed(&self, key: &ChatKey, name: String) {
        let json = serde_json::to_string(&ChatYou { name }).expect("names serialise");
        let _ = self.out.send(ChatOut::Renamed { key: key.clone(), json: Arc::from(json) });
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
        Ok(inner.record(&self.out, ChatKind::Say, author, text))
    }

    /// The name `key` goes by, while they have a stream open.
    pub fn name_of(&self, key: &ChatKey) -> Option<String> {
        self.inner.lock().members.get(key).map(|member| member.name.clone())
    }

    /// Everyone with a stream open, by name.
    pub fn present(&self) -> Vec<(ChatKey, String)> {
        let inner = self.inner.lock();
        let mut present: Vec<_> =
            inner.members.iter().map(|(key, member)| (key.clone(), member.name.clone())).collect();
        present.sort_by_key(|(_, name)| name.to_lowercase());
        present
    }

    /// Announces everyone who has now been gone for [`AWAY`].
    pub fn sweep(&self, now: Instant) {
        self.inner.lock().sweep(&self.out, now);
    }
}

/// Runs [`ChatHub::sweep`] every few seconds, so a leave is announced even when nothing else
/// happens in the chat.
pub fn spawn_sweeper(hub: Arc<ChatHub>, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(SWEEP_EVERY);
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                _ = every.tick() => hub.sweep(Instant::now()),
            }
        }
    })
}

/// Keeps a stream's person in the room. When their last guard drops they are [`Gone`], and
/// [`ChatHub::sweep`] later says they left and frees their name.
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
                let name = inner.members.remove(key).expect("found above").name;
                inner.gone.insert(key.clone(), Gone { name, at: Instant::now() });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::api::wire::Role;
    use crate::guests::GuestPass;
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
        let (_guard, _) = hub.join(tab("a"), None, Instant::now());
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
    fn a_guests_tabs_share_a_name_until_the_last_one_closes() {
        let hub = Arc::new(ChatHub::default());
        let key = ChatKey::Guest([7; 32]);
        let (first, welcome) = hub.join(Some(key.clone()), None, Instant::now());
        let (second, again) = hub.join(Some(key.clone()), None, Instant::now());
        assert_eq!(welcome.you, again.you);
        drop(first);
        assert_eq!(name(&hub, &key), welcome.you);
        drop(second);
        assert_eq!(name(&hub, &key), None);
    }

    #[test]
    fn readers_without_a_key_get_the_history_but_no_name() {
        let hub = Arc::new(ChatHub::default());
        let (guard, welcome) = hub.join(None, None, Instant::now());
        assert_eq!(welcome.you, None);
        assert_eq!(welcome.max_length, MAX_TEXT as u32);
        assert!(guard.key().is_none());
    }

    #[test]
    fn guests_are_known_by_their_pass_and_admins_by_their_tab() {
        let admin = Viewer { role: Role::Admin, guest: None };
        let guest = Viewer { role: Role::Guest, guest: Some(GuestPass { id: [3; 32] }) };
        assert_eq!(ChatKey::of(&admin, "tab"), Some(ChatKey::Tab("tab".into())));
        assert_eq!(ChatKey::of(&admin, ""), None);
        assert_eq!(ChatKey::of(&guest, "tab"), Some(ChatKey::Guest([3; 32])));
        assert_eq!(ChatKey::of(&guest, ""), Some(ChatKey::Guest([3; 32])));
    }

    #[test]
    fn auto_names_never_collide_with_someone_connected() {
        let hub = Arc::new(ChatHub::default());
        // More people than adjective-noun pairs tried before numbering kicks in.
        let guards: Vec<_> = (0..300).map(|i| hub.join(tab(&format!("t{i}")), None, Instant::now())).collect();
        let mut names: Vec<_> = guards.iter().map(|(_, w)| w.you.clone().unwrap().to_lowercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 300);
    }

    #[test]
    fn renames_are_unique_among_connected_people_ignoring_case() {
        let hub = Arc::new(ChatHub::default());
        let (a, b) = (ChatKey::Tab("a".into()), ChatKey::Tab("b".into()));
        let (_ga, _) = hub.join(Some(a.clone()), None, Instant::now());
        let (gb, _) = hub.join(Some(b.clone()), None, Instant::now());
        let now = Instant::now();
        assert_eq!(hub.rename(&b, "  Old   Tom ", now), Ok("Old Tom".into()));
        assert_eq!(hub.rename(&a, "old tom", now), Err(ChatRefusal::NameTaken));
        assert_eq!(hub.rename(&b, "OLD TOM", now), Ok("OLD TOM".into()), "re-casing your own name is fine");
        drop(gb);
        assert_eq!(
            hub.rename(&a, "Old Tom", now),
            Err(ChatRefusal::NameTaken),
            "still theirs while they might be back"
        );
        hub.sweep(Instant::now() + AWAY * 2);
        assert_eq!(hub.rename(&a, "Old Tom", now), Ok("Old Tom".into()), "freed once they are said to have left");
    }

    #[test]
    fn a_rename_frees_the_old_name_and_tells_the_owners_streams() {
        let hub = Arc::new(ChatHub::default());
        let mut out = hub.out.subscribe();
        let (a, b) = (ChatKey::Tab("a".into()), ChatKey::Tab("b".into()));
        let (_ga, _) = hub.join(Some(a.clone()), None, Instant::now());
        let (_gb, _) = hub.join(Some(b.clone()), None, Instant::now());
        let now = Instant::now();
        let old = name(&hub, &a).unwrap();
        hub.rename(&a, "Bramblewick", now).unwrap();
        assert_eq!(hub.rename(&b, &old, now), Ok(old));
        // Past the two join lines.
        assert!(matches!(out.try_recv().unwrap(), ChatOut::Message { .. }));
        assert!(matches!(out.try_recv().unwrap(), ChatOut::Message { .. }));
        match out.try_recv().unwrap() {
            ChatOut::Renamed { key, json } => {
                assert_eq!(key, a);
                assert_eq!(&*json, r#"{"name":"Bramblewick"}"#);
            }
            other => panic!("expected a rename, got {other:?}"),
        }
    }

    /// The kind and name of each line in the history, oldest first.
    fn lines(hub: &ChatHub) -> Vec<(ChatKind, String)> {
        hub.welcome(None).history.into_iter().map(|m| (m.kind, m.author)).collect()
    }

    #[test]
    fn a_newcomer_is_announced_under_the_name_they_asked_for() {
        let hub = Arc::new(ChatHub::default());
        let mut out = hub.out.subscribe();
        let (_guard, welcome) = hub.join(tab("a"), Some(" Old  Tom "), Instant::now());
        assert_eq!(welcome.you.as_deref(), Some("Old Tom"));
        assert_eq!(lines(&hub), [(ChatKind::Join, "Old Tom".to_owned())]);
        let line = &welcome.history[0];
        assert!(line.text.is_empty());
        match out.try_recv().unwrap() {
            ChatOut::Message { id, json } => {
                assert_eq!(id, line.id);
                assert!(json.contains(r#""kind":"join""#), "{json}");
            }
            other => panic!("expected the join line, got {other:?}"),
        }
    }

    #[test]
    fn a_wanted_name_that_is_taken_or_invalid_gets_a_fresh_one() {
        let hub = Arc::new(ChatHub::default());
        let (_a, _) = hub.join(tab("a"), Some("Old Tom"), Instant::now());
        let (_b, taken) = hub.join(tab("b"), Some("old tom"), Instant::now());
        let (_c, invalid) = hub.join(tab("c"), Some("<b>"), Instant::now());
        for welcome in [taken, invalid] {
            let name = welcome.you.unwrap();
            assert!(!name.eq_ignore_ascii_case("old tom") && name != "<b>", "{name}");
        }
    }

    #[test]
    fn more_tabs_of_one_guest_are_not_announced_again() {
        let hub = Arc::new(ChatHub::default());
        let key = Some(ChatKey::Guest([7; 32]));
        let (_first, _) = hub.join(key.clone(), None, Instant::now());
        let (_second, _) = hub.join(key, Some("Someone Else"), Instant::now());
        assert_eq!(lines(&hub).len(), 1);
    }

    #[test]
    fn a_stream_that_arrives_before_the_old_one_closes_still_takes_the_name_it_asks_for() {
        let hub = Arc::new(ChatHub::default());
        let mut out = hub.out.subscribe();
        let key = ChatKey::Tab("a".into());
        let (_old, _) = hub.join(Some(key.clone()), None, Instant::now());
        let (_new, welcome) = hub.join(Some(key.clone()), Some("Old Tom"), Instant::now());
        assert_eq!(welcome.you.as_deref(), Some("Old Tom"));
        assert!(matches!(out.try_recv().unwrap(), ChatOut::Message { .. }), "the first join");
        match out.try_recv().unwrap() {
            ChatOut::Renamed { key: renamed, json } => {
                assert_eq!(renamed, key);
                assert_eq!(&*json, r#"{"name":"Old Tom"}"#);
            }
            other => panic!("expected a rename for the older stream, got {other:?}"),
        }
        assert_eq!(lines(&hub).len(), 1, "a rename is not a join");
    }

    #[test]
    fn readers_without_a_key_come_and_go_unannounced() {
        let hub = Arc::new(ChatHub::default());
        drop(hub.join(None, None, Instant::now()));
        hub.sweep(Instant::now() + AWAY * 2);
        assert!(lines(&hub).is_empty());
    }

    #[test]
    fn a_quick_return_is_quiet_and_keeps_the_name() {
        let hub = Arc::new(ChatHub::default());
        let (guard, welcome) = hub.join(tab("a"), None, Instant::now());
        drop(guard);
        let soon = Instant::now() + AWAY / 2;
        hub.sweep(soon);
        let (_guard, again) = hub.join(tab("a"), None, soon);
        assert_eq!(again.you, welcome.you, "back under the name they left with");
        assert_eq!(lines(&hub).len(), 1, "only the first join");
        hub.sweep(soon + AWAY * 2);
        assert_eq!(lines(&hub).len(), 1, "still here, so never said to have left");
    }

    #[test]
    fn someone_gone_for_a_while_is_said_to_have_left_and_announced_when_back() {
        let hub = Arc::new(ChatHub::default());
        let (guard, _) = hub.join(tab("a"), Some("Old Tom"), Instant::now());
        let (_other, _) = hub.join(tab("b"), Some("Bramblewick"), Instant::now());
        drop(guard);
        let later = Instant::now() + AWAY + Duration::from_secs(1);
        hub.sweep(later);
        let (_back, _) = hub.join(tab("a"), Some("Old Tom"), later);
        assert_eq!(
            lines(&hub),
            [
                (ChatKind::Join, "Old Tom".to_owned()),
                (ChatKind::Join, "Bramblewick".to_owned()),
                (ChatKind::Leave, "Old Tom".to_owned()),
                (ChatKind::Join, "Old Tom".to_owned()),
            ]
        );
    }

    #[test]
    fn nobody_else_can_take_the_name_of_someone_briefly_away() {
        let hub = Arc::new(ChatHub::default());
        drop(hub.join(tab("a"), Some("Old Tom"), Instant::now()));
        let (_b, welcome) = hub.join(tab("b"), Some("Old Tom"), Instant::now());
        assert_ne!(welcome.you.as_deref(), Some("Old Tom"));
        let (_back, welcome) = hub.join(tab("a"), Some("Old Tom"), Instant::now());
        assert_eq!(welcome.you.as_deref(), Some("Old Tom"));
    }

    #[test]
    fn a_late_sweep_says_goodbye_before_the_same_person_comes_back() {
        let hub = Arc::new(ChatHub::default());
        drop(hub.join(tab("a"), Some("Old Tom"), Instant::now()));
        // No sweep ran while they were away; their return runs it first.
        let (_back, _) = hub.join(tab("a"), Some("Old Tom"), Instant::now() + AWAY * 2);
        let kinds: Vec<_> = lines(&hub).into_iter().map(|(kind, _)| kind).collect();
        assert_eq!(kinds, [ChatKind::Join, ChatKind::Leave, ChatKind::Join]);
    }

    #[test]
    fn a_leave_names_whoever_they_were_when_they_went() {
        let hub = Arc::new(ChatHub::default());
        let key = ChatKey::Tab("a".into());
        let (guard, _) = hub.join(Some(key.clone()), None, Instant::now());
        hub.rename(&key, "Bramblewick", Instant::now()).unwrap();
        drop(guard);
        hub.sweep(Instant::now() + AWAY * 2);
        assert_eq!(lines(&hub).last(), Some(&(ChatKind::Leave, "Bramblewick".to_owned())));
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
        let (_guard, _) = hub.join(Some(key.clone()), None, Instant::now());
        for _ in 0..BURST as usize {
            hub.post(&key, "hello", now).unwrap();
        }
        assert_eq!(hub.post(&key, "hello", now), Err(ChatRefusal::TooFast));
        assert!(hub.post(&key, "hello", now + Duration::from_secs(1)).is_ok());
    }
}
