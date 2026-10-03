//! Viewers' credits: earned by watching with the tab in front, spent on things that happen
//! in the world, and topped up when the villagers pray to the viewer.
//!
//! The server is the only judge. A browser reports that it is being watched every
//! [`BEAT_SECONDS`]; a report counts only if it comes about that long after the last one that
//! counted, so sending them faster, or from several tabs, earns nothing more. An account's
//! balance lives in the database; where nobody has an account (token or sign-in off) a tab's
//! balance lives here, in memory, and is forgotten after an hour away or a restart.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fsw_core::time::game_time;
use fsw_core::vec2;
use fsw_sim::SimEvent;
use fsw_sim::commands::ViewerEvent;
use fsw_sim::state::{ItemId, MonsterKind};
use parking_lot::Mutex;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::AppState;
use crate::api::wire::{BuyRequest, CreditGroup, CreditOffer, CreditTarget};
use crate::auth::rate_limit::RateLimiter;
use crate::chat::ChatKey;
use crate::store::{Store, StoreError};

/// How often a browser reports that it is being watched.
pub const BEAT_SECONDS: u32 = 30;
/// A report sooner than this after the last one that counted is ignored.
const EARLY: Duration = Duration::from_secs(25);
/// A report later than this after the last one starts a new run and earns nothing itself.
const LATE: Duration = Duration::from_secs(45);
/// A tab's balance is forgotten after this long without a report or a purchase.
const TAB_IDLE: Duration = Duration::from_secs(3600);
/// This many different villagers praying to a viewer in one game day earns [`PRAYER_BONUS`], once that day.
pub const PRAYERS_NEEDED: usize = 3;
pub const PRAYER_BONUS: u32 = 15;
/// Purchases one viewer may make in a burst, and the sustained rate after it.
const BUY_BURST: f64 = 3.0;
const BUY_PER_SECOND: f64 = 0.2;

/// One thing on offer. A credit is half a minute of watching at the default rate.
pub struct Offer {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub cost: u32,
    pub target: CreditTarget,
    pub group: CreditGroup,
}

const fn offer_of(
    id: &'static str,
    name: &'static str,
    description: &'static str,
    cost: u32,
    target: CreditTarget,
    group: CreditGroup,
) -> Offer {
    Offer { id, name, description, cost, target, group }
}

/// Everything credits buy, cheapest first within each group.
pub const OFFERS: [Offer; 11] = [
    offer_of("food", "Food", "A meal in the villager's pack.", 5, CreditTarget::Character, CreditGroup::Gift),
    offer_of(
        "potion",
        "Healing potion",
        "A potion for the villager's next fight.",
        8,
        CreditTarget::Character,
        CreditGroup::Gift,
    ),
    offer_of(
        "gift",
        "Small gift",
        "Something for the villager to give away.",
        10,
        CreditTarget::Character,
        CreditGroup::Gift,
    ),
    offer_of(
        "skeleton_minion",
        "Skeleton",
        "One of the dead rises where you are looking.",
        20,
        CreditTarget::Point,
        CreditGroup::Monster,
    ),
    offer_of(
        "skeleton_rogue",
        "Skeleton stalker",
        "Quick, with two daggers.",
        30,
        CreditTarget::Point,
        CreditGroup::Monster,
    ),
    offer_of(
        "skeleton_warrior",
        "Skeleton warrior",
        "Blade and shield; hard to put down.",
        40,
        CreditTarget::Point,
        CreditGroup::Monster,
    ),
    offer_of(
        "skeleton_mage",
        "Skeleton mage",
        "Strikes from a distance.",
        50,
        CreditTarget::Point,
        CreditGroup::Monster,
    ),
    offer_of("feast", "Feast", "Every villager's hunger is gone.", 80, CreditTarget::None, CreditGroup::Blessing),
    offer_of("healing", "Healing", "Every villager's wounds close.", 100, CreditTarget::None, CreditGroup::Blessing),
    offer_of(
        "raid",
        "Raid",
        "A band of the dead comes from the graveyard.",
        150,
        CreditTarget::None,
        CreditGroup::Threat,
    ),
    offer_of(
        "necromancer",
        "Necromancer",
        "A necromancer walks in the graveyard, raising more.",
        250,
        CreditTarget::None,
        CreditGroup::Threat,
    ),
];

pub fn offer(id: &str) -> Option<&'static Offer> {
    OFFERS.iter().find(|offer| offer.id == id)
}

/// The catalogue as the browser gets it.
pub fn offers() -> Vec<CreditOffer> {
    OFFERS
        .iter()
        .map(|o| CreditOffer {
            id: o.id.into(),
            name: o.name.into(),
            description: o.description.into(),
            cost: o.cost,
            target: o.target,
            group: o.group,
        })
        .collect()
}

/// What the sim is asked to do for `offer`, or what the request is missing.
pub fn event_for(offer: &Offer, body: &BuyRequest) -> Result<ViewerEvent, &'static str> {
    let gift = |item| match &body.character_id {
        Some(id) => Ok(ViewerEvent::Gift { character_id: id.clone(), item }),
        None => Err("select a villager first"),
    };
    let monster = |monster| match (body.x, body.z) {
        (Some(x), Some(z)) => Ok(ViewerEvent::Monster { monster, pos: vec2(x, z) }),
        _ => Err("a place is required"),
    };
    match offer.id {
        "food" => gift(ItemId::Food),
        "potion" => gift(ItemId::Potion),
        "gift" => gift(ItemId::Gift),
        "skeleton_minion" => monster(MonsterKind::SkeletonMinion),
        "skeleton_rogue" => monster(MonsterKind::SkeletonRogue),
        "skeleton_warrior" => monster(MonsterKind::SkeletonWarrior),
        "skeleton_mage" => monster(MonsterKind::SkeletonMage),
        "feast" => Ok(ViewerEvent::Feast),
        "healing" => Ok(ViewerEvent::Heal),
        "raid" => Ok(ViewerEvent::Raid),
        "necromancer" => Ok(ViewerEvent::Necromancer),
        _ => Err("no such offer"),
    }
}

/// A viewer's run of watching.
struct Watching {
    /// the last report that counted
    last: Instant,
    /// credits earned but not yet whole, in sixtieths
    sixtieths: u32,
}

/// The balance of a viewer without an account.
struct Tab {
    balance: i64,
    seen: Instant,
}

/// Who has prayed to one power on one game day.
#[derive(Default)]
struct Tally {
    day: i64,
    by: HashSet<String>,
    paid: bool,
}

#[derive(Default)]
struct Inner {
    watching: HashMap<ChatKey, Watching>,
    tabs: HashMap<String, Tab>,
    /// lower-cased name -> the tab that last spent under it
    tab_powers: HashMap<String, String>,
    /// lower-cased name -> today's prayers
    prayers: HashMap<String, Tally>,
}

impl Inner {
    fn tab(&mut self, tab: &str, now: Instant) -> &mut Tab {
        if self.tabs.len() > 256 {
            self.tabs.retain(|_, t| now.saturating_duration_since(t.seen) < TAB_IDLE);
            let tabs = &self.tabs;
            self.tab_powers.retain(|_, tab| tabs.contains_key(tab));
        }
        let entry = self.tabs.entry(tab.to_owned()).or_insert(Tab { balance: 0, seen: now });
        entry.seen = now;
        entry
    }
}

/// Every viewer's credits, shared by the routes and the prayer task.
pub struct CreditsHub {
    per_minute: u32,
    cap: u32,
    inner: Mutex<Inner>,
    limiter: RateLimiter<ChatKey>,
}

impl CreditsHub {
    pub fn new(per_minute: u32, cap: u32) -> Self {
        Self { per_minute, cap, inner: Mutex::default(), limiter: RateLimiter::new(BUY_BURST, BUY_PER_SECOND) }
    }

    /// False when `CREDITS_PER_MINUTE` is 0: nobody earns, and nothing is for sale.
    pub fn enabled(&self) -> bool {
        self.per_minute > 0
    }

    pub fn per_minute(&self) -> u32 {
        self.per_minute
    }

    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// Takes a report that `key` is watching at `now`; returns the whole credits it earned.
    pub fn beat(&self, key: &ChatKey, now: Instant) -> u32 {
        let mut inner = self.inner.lock();
        if inner.watching.len() > 4096 {
            inner.watching.retain(|_, w| now.saturating_duration_since(w.last) < TAB_IDLE);
        }
        let Some(watching) = inner.watching.get_mut(key) else {
            inner.watching.insert(key.clone(), Watching { last: now, sixtieths: 0 });
            return 0;
        };
        let gap = now.saturating_duration_since(watching.last);
        if gap < EARLY {
            return 0;
        }
        watching.last = now;
        if gap > LATE {
            return 0;
        }
        watching.sixtieths += BEAT_SECONDS * self.per_minute;
        let earned = watching.sixtieths / 60;
        watching.sixtieths %= 60;
        earned
    }

    /// Whether `key` may buy something now (a few at once, then one every five seconds).
    pub fn may_buy(&self, key: &ChatKey, now: Instant) -> bool {
        self.limiter.allow(key.clone(), now)
    }

    /// Counts a prayer to `power` by a villager on a game day. True the moment enough
    /// different villagers have prayed to it that day, and only once that day.
    pub fn prayed(&self, power: &str, character: &str, day: i64) -> bool {
        let mut inner = self.inner.lock();
        let tally = inner.prayers.entry(power.to_lowercase()).or_default();
        if tally.day != day {
            *tally = Tally { day, ..Default::default() };
        }
        tally.by.insert(character.to_owned());
        if tally.paid || tally.by.len() < PRAYERS_NEEDED {
            return false;
        }
        tally.paid = true;
        true
    }

    pub async fn balance(&self, store: &Store, key: &ChatKey, now: Instant) -> Result<i64, StoreError> {
        match key {
            ChatKey::User(id) => store.credits(*id).await,
            ChatKey::Tab(tab) => Ok(self.inner.lock().tab(tab, now).balance),
        }
    }

    /// Adds `amount` up to the cap; returns the new balance.
    pub async fn earn(&self, store: &Store, key: &ChatKey, amount: u32, now: Instant) -> Result<i64, StoreError> {
        let (amount, cap) = (i64::from(amount), i64::from(self.cap));
        match key {
            ChatKey::User(id) => store.earn_credits(*id, amount, cap).await,
            ChatKey::Tab(tab) => {
                let mut inner = self.inner.lock();
                let tab = inner.tab(tab, now);
                tab.balance = tab.balance.max(cap.min(tab.balance + amount));
                Ok(tab.balance)
            }
        }
    }

    /// Takes `cost` if `key` has it (`None` if not), and makes `name` theirs for prayers.
    pub async fn spend(
        &self,
        store: &Store,
        key: &ChatKey,
        cost: u32,
        name: &str,
        now: Instant,
    ) -> Result<Option<i64>, StoreError> {
        let (cost, power) = (i64::from(cost), name.to_lowercase());
        match key {
            ChatKey::User(id) => store.spend_credits(*id, cost, power).await,
            ChatKey::Tab(tab) => {
                let mut inner = self.inner.lock();
                let entry = inner.tab(tab, now);
                if entry.balance < cost {
                    return Ok(None);
                }
                entry.balance -= cost;
                let balance = entry.balance;
                inner.tab_powers.insert(power, tab.clone());
                Ok(Some(balance))
            }
        }
    }

    /// Gives back what [`CreditsHub::spend`] took.
    pub async fn refund(&self, store: &Store, key: &ChatKey, cost: u32, now: Instant) -> Result<i64, StoreError> {
        match key {
            ChatKey::User(id) => store.refund_credits(*id, i64::from(cost)).await,
            ChatKey::Tab(tab) => {
                let mut inner = self.inner.lock();
                let tab = inner.tab(tab, now);
                tab.balance += i64::from(cost);
                Ok(tab.balance)
            }
        }
    }

    /// The viewer the villagers mean by `power`: whoever last spent under that name.
    pub async fn owner(&self, store: &Store, power: &str) -> Result<Option<ChatKey>, StoreError> {
        let power = power.to_lowercase();
        if let Some(tab) = self.inner.lock().tab_powers.get(&power) {
            return Ok(Some(ChatKey::Tab(tab.clone())));
        }
        Ok(store.power_owner(power).await?.map(ChatKey::User))
    }
}

/// Pays the viewer the village has prayed to enough today, and tells the chat.
async fn answer_prayers(app: &AppState, power: &str) {
    let now = Instant::now();
    let owner = match app.credits.owner(&app.store, power).await {
        Ok(Some(owner)) => owner,
        Ok(None) => return,
        Err(error) => {
            tracing::error!(%error, "cannot find who the village prayed to");
            return;
        }
    };
    if let Err(error) = app.credits.earn(&app.store, &owner, PRAYER_BONUS, now).await {
        tracing::error!(%error, "cannot pay for the village's prayers");
        return;
    }
    app.chat.announce(power.to_owned(), format!("has the village's prayers: +{PRAYER_BONUS} credits"));
}

/// Listens to the sim for villagers' prayers and pays the viewers they are said to.
pub fn spawn_prayers(app: Arc<AppState>, shutdown: CancellationToken) -> JoinHandle<()> {
    let mut events = app.world.events.subscribe();
    tokio::spawn(async move {
        loop {
            let tick = tokio::select! {
                () = shutdown.cancelled() => break,
                tick = events.recv() => tick,
            };
            let tick = match tick {
                Ok(tick) => tick,
                // A prayer missed while behind is only a prayer uncounted.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            if !app.credits.enabled() {
                continue;
            }
            let day = game_time(tick.tick as i64).day;
            for event in &tick.events {
                if let SimEvent::Prayer { power, character_id } = event
                    && app.credits.prayed(power, character_id, day)
                {
                    answer_prayers(&app, power).await;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{STARTING_CREDITS, testing};

    fn tab(name: &str) -> ChatKey {
        ChatKey::Tab(name.into())
    }

    #[test]
    fn only_reports_that_come_on_time_earn() {
        let hub = CreditsHub::new(2, 400);
        let (key, start) = (tab("a"), Instant::now());
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        // The first report only starts the run.
        assert_eq!(hub.beat(&key, at(0)), 0);
        assert_eq!(hub.beat(&key, at(30)), 1);
        // Too soon: a second tab, or a script sending them fast.
        assert_eq!(hub.beat(&key, at(35)), 0);
        assert_eq!(hub.beat(&key, at(50)), 0);
        assert_eq!(hub.beat(&key, at(60)), 1);
        // The tab was out of sight for a while: this report starts again.
        assert_eq!(hub.beat(&key, at(200)), 0);
        assert_eq!(hub.beat(&key, at(230)), 1);
        // Someone else's run is their own.
        assert_eq!(hub.beat(&tab("b"), at(231)), 0);
    }

    #[test]
    fn rates_that_do_not_divide_evenly_keep_the_remainder() {
        let (start, key) = (Instant::now(), tab("a"));
        let earned = |per_minute: u32, beats: u64| {
            let hub = CreditsHub::new(per_minute, 400);
            (0..=beats).map(|i| hub.beat(&key, start + Duration::from_secs(30 * i))).sum::<u32>()
        };
        assert_eq!(earned(1, 4), 2);
        assert_eq!(earned(3, 4), 6);
        assert_eq!(earned(60, 2), 60);
        assert_eq!(earned(0, 10), 0);
    }

    #[tokio::test]
    async fn a_tab_earns_to_the_cap_spends_what_it_has_and_owns_its_name() {
        let (store, _dir) = testing::store().await;
        let hub = CreditsHub::new(2, 50);
        let (key, now) = (tab("a"), Instant::now());
        assert_eq!(hub.balance(&store, &key, now).await.unwrap(), 0);
        assert_eq!(hub.earn(&store, &key, 30, now).await.unwrap(), 30);
        assert_eq!(hub.earn(&store, &key, 30, now).await.unwrap(), 50);
        assert_eq!(hub.spend(&store, &key, 60, "Erik", now).await.unwrap(), None);
        assert_eq!(hub.owner(&store, "erik").await.unwrap(), None);
        assert_eq!(hub.spend(&store, &key, 20, "Erik", now).await.unwrap(), Some(30));
        assert_eq!(hub.owner(&store, "ERIK").await.unwrap(), Some(key.clone()));
        assert_eq!(hub.refund(&store, &key, 20, now).await.unwrap(), 50);
    }

    #[tokio::test]
    async fn an_account_keeps_its_credits_in_the_database() {
        let (store, _dir) = testing::store().await;
        let id = testing::user(&store, "ann@example.com").await;
        let (key, now) = (ChatKey::User(id), Instant::now());
        let hub = CreditsHub::new(2, 400);
        assert_eq!(hub.balance(&store, &key, now).await.unwrap(), STARTING_CREDITS);
        assert_eq!(hub.earn(&store, &key, 5, now).await.unwrap(), 15);
        assert_eq!(hub.spend(&store, &key, 8, "Ann", now).await.unwrap(), Some(7));
        // Another hub (a restart) sees the same balance and the same owner.
        let again = CreditsHub::new(2, 400);
        assert_eq!(again.balance(&store, &key, now).await.unwrap(), 7);
        assert_eq!(again.owner(&store, "ann").await.unwrap(), Some(key));
    }

    #[test]
    fn enough_different_villagers_praying_in_a_day_pays_once() {
        let hub = CreditsHub::new(2, 400);
        // One devout villager is not a village.
        assert!(!hub.prayed("Erik", "c1", 1));
        assert!(!hub.prayed("Erik", "c1", 1));
        assert!(!hub.prayed("erik", "c2", 1));
        assert!(!hub.prayed("Mara", "c3", 1));
        assert!(hub.prayed("Erik", "c3", 1));
        assert!(!hub.prayed("Erik", "c4", 1));
        // A new day, a new count.
        assert!(!hub.prayed("Erik", "c1", 2));
        assert!(!hub.prayed("Erik", "c2", 2));
        assert!(hub.prayed("Erik", "c5", 2));
    }

    #[test]
    fn every_offer_asks_the_sim_for_something() {
        let body = BuyRequest {
            viewer: "a".into(),
            offer: String::new(),
            character_id: Some("c1".into()),
            x: Some(1.0),
            z: Some(2.0),
        };
        for offer in &OFFERS {
            assert!(event_for(offer, &body).is_ok(), "{}", offer.id);
            assert_eq!(super::offer(offer.id).map(|o| o.cost), Some(offer.cost));
        }
        let bare = BuyRequest { character_id: None, x: None, z: None, ..body };
        assert_eq!(event_for(offer("food").unwrap(), &bare), Err("select a villager first"));
        assert_eq!(event_for(offer("skeleton_mage").unwrap(), &bare), Err("a place is required"));
        assert_eq!(event_for(offer("raid").unwrap(), &bare), Ok(ViewerEvent::Raid));
    }
}
