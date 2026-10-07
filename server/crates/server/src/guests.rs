//! Guests: everyone who watches without the admin's token.
//!
//! A guest takes one of the seats (`MAX_VIEWERS`, read at start; 0 means no cap) and keeps it for as
//! long as they watch. The seat is a pass in an HttpOnly cookie, shared by the browser's tabs, with
//! one live stream at a time: a newer tab takes the stream from an older one. When every seat is
//! taken, newcomers wait in line and ask again once a minute; a seat that comes free goes to the
//! front of the line, not to whoever happens to ask first. Everything here lives in memory: guests
//! are anonymous, and a restart simply empties the seats and the line.
//!
//! Guests watch and chat (their chat key is the pass, so a browser's tabs share one name); only the
//! admin routes are closed to them.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

/// How many guests may watch at once unless `MAX_VIEWERS` says otherwise.
pub const DEFAULT_SEATS: u32 = 25;
/// How long a guest in line waits between asks.
pub const POLL_EVERY: Duration = Duration::from_secs(60);
/// An ask sooner than this after the last one is refused; the gap to [`POLL_EVERY`] allows for
/// timers that fire a little early.
const POLL_SOONEST: Duration = Duration::from_secs(50);
/// A place in line that has not been asked about for this long is given up (the page was closed).
const TICKET_LOST: Duration = Duration::from_secs(3 * 60);
/// A pass with no stream for this long gives up its seat (the tab was closed). Long enough for
/// a reload or a dropped connection.
const GRACE: Duration = Duration::from_secs(60);
/// A new pass must open its first stream within this long; the page loads the valley's models
/// first, which takes a while on a slow phone.
const FIRST_STREAM: Duration = Duration::from_secs(3 * 60);

/// A fresh random token, as hex.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The key a token is kept under: only its hash ever sits in memory.
pub fn hash(token: &str) -> [u8; 32] {
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, token.as_bytes());
    digest.as_ref().try_into().expect("SHA-256 is 32 bytes")
}

/// How many guests may watch at once; 0 is no cap. From `MAX_VIEWERS`; tests shrink it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestLimits {
    pub seats: u32,
}

impl Default for GuestLimits {
    fn default() -> Self {
        Self { seats: DEFAULT_SEATS }
    }
}

/// A guest's pass, as the request guard sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestPass {
    /// the SHA-256 of the cookie's token
    pub id: [u8; 32],
}

/// The answer to a guest who asks for a seat.
#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    /// A seat. `token` is the new pass for the cookie, or `None` when the asker already had one.
    Admitted { token: Option<String> },
    /// Every seat is taken: `position` in line (1 is next), and the ticket to ask again with.
    Queued { ticket: String, position: u32 },
    /// Asked again sooner than allowed; the place in line is kept.
    TooSoon { retry_after: Duration },
}

struct Pass {
    /// the live stream's token (cancelled when a newer stream takes over) and its number
    stream: Option<(u64, CancellationToken)>,
    /// since when the pass has had no stream; `None` while one is open
    idle_since: Option<Instant>,
    /// a stream has been opened on it at least once
    streamed: bool,
}

struct Ticket {
    id: [u8; 32],
    last_ask: Instant,
}

#[derive(Default)]
struct Inner {
    passes: HashMap<[u8; 32], Pass>,
    line: VecDeque<Ticket>,
    streams_opened: u64,
}

impl Inner {
    fn prune(&mut self, now: Instant) {
        self.passes.retain(|_, pass| {
            let grace = if pass.streamed { GRACE } else { FIRST_STREAM };
            pass.idle_since.is_none_or(|idle| now.saturating_duration_since(idle) < grace)
        });
        self.line.retain(|ticket| now.saturating_duration_since(ticket.last_ask) < TICKET_LOST);
    }

    fn admit(&mut self, now: Instant) -> Admission {
        let token = random_token();
        self.passes.insert(hash(&token), Pass { stream: None, idle_since: Some(now), streamed: false });
        Admission::Admitted { token: Some(token) }
    }
}

/// The seats and the line.
pub struct GuestHub {
    limits: GuestLimits,
    inner: Mutex<Inner>,
}

impl GuestHub {
    pub fn new(limits: GuestLimits) -> Self {
        Self { limits, inner: Mutex::default() }
    }

    pub fn limits(&self) -> GuestLimits {
        self.limits
    }

    /// A guest asks for a seat, with the pass from their cookie and the ticket from their last
    /// ask, if any.
    pub fn ask(&self, pass: Option<&str>, ticket: Option<&str>, now: Instant) -> Admission {
        let mut inner = self.inner.lock();
        inner.prune(now);
        if pass.is_some_and(|token| inner.passes.contains_key(&hash(token))) {
            return Admission::Admitted { token: None };
        }
        // No cap: everyone is seated at once.
        if self.limits.seats == 0 {
            return inner.admit(now);
        }
        let free = (self.limits.seats as usize).saturating_sub(inner.passes.len());
        let place = ticket.map(hash).and_then(|id| inner.line.iter().position(|t| t.id == id));
        match place {
            Some(i) => {
                let since = now.saturating_duration_since(inner.line[i].last_ask);
                if since < POLL_SOONEST {
                    return Admission::TooSoon { retry_after: POLL_EVERY.saturating_sub(since) };
                }
                if i < free {
                    inner.line.remove(i);
                    return inner.admit(now);
                }
                inner.line[i].last_ask = now;
                let ticket = ticket.expect("a place in line comes from a ticket").to_owned();
                Admission::Queued { ticket, position: i as u32 + 1 }
            }
            None if inner.line.len() < free => inner.admit(now),
            None => {
                let ticket = random_token();
                inner.line.push_back(Ticket { id: hash(&ticket), last_ask: now });
                Admission::Queued { ticket, position: inner.line.len() as u32 }
            }
        }
    }

    /// The pass behind a cookie's token, while it holds a seat.
    pub fn pass(&self, token: &str, now: Instant) -> Option<GuestPass> {
        let mut inner = self.inner.lock();
        inner.prune(now);
        let id = hash(token);
        inner.passes.contains_key(&id).then_some(GuestPass { id })
    }

    /// Whether a pass still holds its seat.
    pub fn holds(&self, id: [u8; 32], now: Instant) -> bool {
        let mut inner = self.inner.lock();
        inner.prune(now);
        inner.passes.contains_key(&id)
    }

    /// A stream opens on a pass. Any older stream on it is told to stop (its token is cancelled).
    /// `None` when the pass has gone in the meantime.
    pub fn open_stream(self: &Arc<Self>, id: [u8; 32], now: Instant) -> Option<GuestStream> {
        let mut inner = self.inner.lock();
        inner.prune(now);
        inner.streams_opened += 1;
        let number = inner.streams_opened;
        let pass = inner.passes.get_mut(&id)?;
        let token = CancellationToken::new();
        if let Some((_, older)) = pass.stream.replace((number, token.clone())) {
            older.cancel();
        }
        pass.idle_since = None;
        pass.streamed = true;
        Some(GuestStream { hub: self.clone(), id, number, superseded: token })
    }

    /// A stream closed: if it was the pass's live one, the seat's grace period starts.
    fn close_stream(&self, id: [u8; 32], number: u64, now: Instant) {
        let mut inner = self.inner.lock();
        if let Some(pass) = inner.passes.get_mut(&id)
            && pass.stream.as_ref().is_some_and(|(live, _)| *live == number)
        {
            pass.stream = None;
            pass.idle_since = Some(now);
        }
    }

    /// Ends a pass at once (the guest left), stopping its stream.
    pub fn end(&self, id: [u8; 32]) {
        if let Some(pass) = self.inner.lock().passes.remove(&id)
            && let Some((_, token)) = pass.stream
        {
            token.cancel();
        }
    }

    /// Seats in use and people in line, for the logs.
    pub fn counts(&self, now: Instant) -> (usize, usize) {
        let mut inner = self.inner.lock();
        inner.prune(now);
        (inner.passes.len(), inner.line.len())
    }
}

/// A guest's open stream. Dropping it starts the seat's grace period.
pub struct GuestStream {
    hub: Arc<GuestHub>,
    pub id: [u8; 32],
    number: u64,
    /// cancelled when a newer stream takes over the pass, or the pass ends
    pub superseded: CancellationToken,
}

impl Drop for GuestStream {
    fn drop(&mut self) {
        self.hub.close_stream(self.id, self.number, Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    fn hub(seats: u32) -> Arc<GuestHub> {
        Arc::new(GuestHub::new(GuestLimits { seats }))
    }

    fn admitted(answer: Admission) -> String {
        match answer {
            Admission::Admitted { token: Some(token) } => token,
            other => panic!("expected a new pass, got {other:?}"),
        }
    }

    /// Takes a seat and keeps it with an open stream.
    fn seat(hub: &Arc<GuestHub>, now: Instant) -> GuestStream {
        let token = admitted(hub.ask(None, None, now));
        hub.open_stream(hash(&token), now).expect("a fresh pass")
    }

    fn queued(answer: Admission) -> (String, u32) {
        match answer {
            Admission::Queued { ticket, position } => (ticket, position),
            other => panic!("expected a place in line, got {other:?}"),
        }
    }

    #[test]
    fn tokens_are_hex_and_hash_to_their_key() {
        let token = random_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(token, random_token());
        assert_eq!(hash(&token), hash(&token));
        assert_ne!(hash(&token), hash("other"));
    }

    #[test]
    fn no_cap_seats_everyone() {
        let hub = hub(0);
        let now = Instant::now();
        let mut streams = Vec::new();
        for _ in 0..40 {
            streams.push(seat(&hub, now));
        }
        assert_eq!(hub.counts(now), (40, 0));
    }

    #[test]
    fn guests_are_seated_until_the_seats_run_out_then_wait_in_line() {
        let hub = hub(2);
        let now = Instant::now();
        admitted(hub.ask(None, None, now));
        admitted(hub.ask(None, None, now));
        assert_eq!(queued(hub.ask(None, None, now)).1, 1);
        assert_eq!(queued(hub.ask(None, None, now)).1, 2);
        assert_eq!(hub.counts(now), (2, 2));
    }

    #[test]
    fn asking_again_with_a_pass_changes_nothing() {
        let hub = hub(1);
        let now = Instant::now();
        let token = admitted(hub.ask(None, None, now));
        let again = hub.ask(Some(&token), None, now + MINUTE / 2);
        assert_eq!(again, Admission::Admitted { token: None });
        assert_eq!(hub.counts(now), (1, 0));
    }

    #[test]
    fn a_free_seat_goes_to_the_front_of_the_line_not_to_a_newcomer() {
        let hub = hub(1);
        let now = Instant::now();
        let first = admitted(hub.ask(None, None, now));
        let (ticket, _) = queued(hub.ask(None, None, now));
        hub.end(hash(&first));
        // A newcomer finds the seat kept for the one in line.
        let later = now + MINUTE / 2;
        assert_eq!(queued(hub.ask(None, None, later)).1, 2);
        admitted(hub.ask(None, Some(&ticket), now + MINUTE));
    }

    #[test]
    fn asking_too_soon_is_refused_and_keeps_the_place() {
        let hub = hub(1);
        let now = Instant::now();
        let _seated = seat(&hub, now);
        let (ticket, _) = queued(hub.ask(None, None, now));
        let (_second, _) = queued(hub.ask(None, None, now));
        assert_eq!(
            hub.ask(None, Some(&ticket), now + Duration::from_secs(10)),
            Admission::TooSoon { retry_after: Duration::from_secs(50) }
        );
        assert_eq!(queued(hub.ask(None, Some(&ticket), now + MINUTE)), (ticket, 1));
    }

    #[test]
    fn a_place_nobody_asks_about_is_given_up() {
        let hub = hub(1);
        let now = Instant::now();
        let _seated = seat(&hub, now);
        let (lost, _) = queued(hub.ask(None, None, now));
        let (kept, _) = queued(hub.ask(None, None, now));
        for minute in 1..=3 {
            queued(hub.ask(None, Some(&kept), now + MINUTE * minute));
        }
        // The first in line stopped asking; the second moved up, and the first starts again at the back.
        assert_eq!(queued(hub.ask(None, Some(&kept), now + MINUTE * 4)).1, 1);
        assert_eq!(queued(hub.ask(None, Some(&lost), now + MINUTE * 4)).1, 2);
    }

    #[test]
    fn a_pass_with_a_stream_keeps_its_seat_however_long_it_watches() {
        let hub = hub(1);
        let now = Instant::now();
        let token = admitted(hub.ask(None, None, now));
        let id = hash(&token);
        // A reconnect a minute for an hour, each taking over from the last.
        let mut streams = Vec::new();
        for minute in 0..60 {
            streams.push(hub.open_stream(id, now + MINUTE * minute).expect("the pass is valid"));
        }
        assert!(hub.pass(&token, now + MINUTE * 60 * 24).is_some());
        assert!(hub.holds(id, now + MINUTE * 60 * 24));
    }

    #[test]
    fn a_pass_without_a_stream_gives_up_its_seat_after_the_grace() {
        let hub = hub(1);
        let now = Instant::now();
        let token = admitted(hub.ask(None, None, now));
        // Never opened a stream: the page gets longer for its first one.
        assert!(hub.holds(hash(&token), now + FIRST_STREAM - Duration::from_secs(1)));
        assert!(!hub.holds(hash(&token), now + FIRST_STREAM));

        let start = now + FIRST_STREAM;
        let token = admitted(hub.ask(None, None, start));
        let id = hash(&token);
        let stream = hub.open_stream(id, start).unwrap();
        // An open stream holds the seat however long it runs.
        assert!(hub.holds(id, start + GRACE * 5));
        let closed = start + GRACE * 5;
        hub.close_stream(id, stream.number, closed);
        assert!(hub.holds(id, closed + GRACE - Duration::from_secs(1)));
        assert!(!hub.holds(id, closed + GRACE));
    }

    #[test]
    fn a_newer_stream_takes_over_from_an_older_one() {
        let hub = hub(1);
        let now = Instant::now();
        let id = hash(&admitted(hub.ask(None, None, now)));
        let first = hub.open_stream(id, now).unwrap();
        let second = hub.open_stream(id, now).unwrap();
        assert!(first.superseded.is_cancelled());
        assert!(!second.superseded.is_cancelled());
        // The older stream closing does not start the grace while the newer one is open.
        drop(first);
        assert!(hub.holds(id, now + GRACE * 3));
    }

    #[test]
    fn leaving_frees_the_seat_and_stops_the_stream() {
        let hub = hub(1);
        let now = Instant::now();
        let id = hash(&admitted(hub.ask(None, None, now)));
        let stream = hub.open_stream(id, now).unwrap();
        hub.end(id);
        assert!(stream.superseded.is_cancelled());
        assert!(!hub.holds(id, now));
        admitted(hub.ask(None, None, now));
    }
}
