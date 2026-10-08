//! The casting task: has the language model invent the founders of a new world, one request per
//! villager, and later each newcomer the sim asks for. It talks to the model with its own client,
//! outside the brain's queue, governor and decision log: a founding is a few dozen requests once,
//! and a newcomer one every few days. Without a model (or when the model will not answer) it hands
//! the work to the sim, which invents villagers from its own lists.
//!
//! The sim is reached only through commands (`Settle`, `FoundVillage`, `NewcomerDraft`,
//! `NewcomerLocal`), so a world stays reproducible from its command stream.

use std::sync::Arc;
use std::time::Duration;

use fsw_brain::casting::{CastingContext, CastingError, Purpose, generate_villager};
use fsw_brain::llm::client::LlmClient;
use fsw_brain::llm::transport::LlmTransport;
use fsw_brain::llm::types::LlmErrorKind;
use fsw_brain::settings::{ApiKey, LlmSettings};
use fsw_sim::{Command, SimEvent};
use parking_lot::Mutex;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use crate::world_task::{Outgoing, World, WorldInput};

/// The most requests in flight at once while founding, whatever `LLM_CONCURRENCY` allows.
const MOST_AT_ONCE: u32 = 4;
/// After this many failures or refusals in a row the sim founds the rest of the village itself.
const GIVE_UP_AFTER: u32 = 3;
/// How long a newcomer's description may take; the sim's own patience is the backstop anyway.
const NEWCOMER_TIMEOUT: Duration = Duration::from_secs(180);
/// A pause between founding rounds, so the sim has applied the last round's settlers (a tick is 50 ms).
const BETWEEN_ROUNDS: Duration = Duration::from_millis(300);

struct Caster {
    world: Arc<World>,
    /// `None` when the model is off: the sim founds the village and invents its newcomers
    client: Option<Arc<LlmClient>>,
    at_once: u32,
}

fn send(world: &World, command: Command) {
    // A full queue means the world task is gone or wedged; the casting can wait for the next ask.
    if world.inputs.try_send(WorldInput::Command { command, source: "casting" }).is_err() {
        tracing::warn!(event = "casting", "the world did not take a casting command");
    }
}

/// Starts the task over the real HTTP transport.
pub fn spawn(
    world: Arc<World>,
    settings: LlmSettings,
    key: ApiKey,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    spawn_with(world, settings, key, crate::brain_task::transport(), shutdown)
}

/// Starts the task over any transport (tests pass a fake).
pub fn spawn_with(
    world: Arc<World>,
    settings: LlmSettings,
    key: ApiKey,
    transport: Arc<dyn LlmTransport>,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    let at_once = settings.concurrency.clamp(1, MOST_AT_ONCE);
    let client = settings.enabled.then(|| Arc::new(LlmClient::new(settings, key, transport)));
    let caster = Arc::new(Caster { world, client, at_once });
    tokio::spawn(run(caster, shutdown))
}

async fn run(caster: Arc<Caster>, shutdown: CancellationToken) {
    let mut outgoing = caster.world.outgoing.subscribe();
    let mut events = caster.world.events.subscribe();
    let mut founding = check_founding(&caster);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            item = outgoing.recv() => match item {
                // A new world or an import: whatever was being founded is gone.
                Ok(Outgoing::Reset(_)) => {
                    if let Some(cancel) = founding.take() {
                        cancel.cancel();
                    }
                    founding = check_founding(&caster);
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => {
                    if founding.as_ref().is_none_or(CancellationToken::is_cancelled) {
                        founding = check_founding(&caster);
                    }
                }
                Err(RecvError::Closed) => break,
            },
            tick = events.recv() => match tick {
                Ok(tick) => {
                    for event in &tick.events {
                        if let SimEvent::NewcomerWanted { deadline_tick } = event {
                            tokio::spawn(newcomer(caster.clone(), *deadline_tick));
                        }
                    }
                }
                // Missed events: the sim's own patience brings a newcomer anyway.
                Err(RecvError::Lagged(missed)) => tracing::warn!(event = "casting", missed, "the casting missed ticks"),
                Err(RecvError::Closed) => break,
            },
        }
    }
    if let Some(cancel) = founding {
        cancel.cancel();
    }
}

/// Starts founding the current world if it needs it; the token stops the founding on a new world.
fn check_founding(caster: &Arc<Caster>) -> Option<CancellationToken> {
    let (founding, settled, total) = {
        let core = caster.world.core.read();
        let state = &core.sim.state;
        (state.director.founding, state.order.len(), state.director.population_target)
    };
    if !founding {
        return None;
    }
    match &caster.client {
        None => {
            tracing::info!(event = "casting", settled, total, "no model: the sim founds the village itself");
            send(&caster.world, Command::FoundVillage);
            None
        }
        Some(_) => {
            tracing::info!(event = "casting", settled, total, "founding the village with the model");
            let cancel = CancellationToken::new();
            tokio::spawn(found(caster.clone(), cancel.clone()));
            Some(cancel)
        }
    }
}

/// Founds the village: rounds of up to `at_once` requests, each told who has settled and which names
/// are spoken for, until the sim says the founding is over, or the model keeps failing and the sim
/// takes over the rest.
async fn found(caster: Arc<Caster>, cancel: CancellationToken) {
    let Some(client) = caster.client.clone() else { return };
    // Full names accepted and sent, so a later request does not take the same one.
    let reserved: Arc<Mutex<Vec<String>>> = Arc::default();
    let mut setbacks = 0u32;
    loop {
        if cancel.is_cancelled() {
            return;
        }
        let (founding, settled, total) = {
            let core = caster.world.core.read();
            let state = &core.sim.state;
            (state.director.founding, state.order.len(), state.director.population_target)
        };
        if !founding {
            tracing::info!(event = "casting", settled, "the village is founded");
            return;
        }
        let in_hand = reserved.lock().len();
        let wanted = total.saturating_sub(settled.max(in_hand)).min(caster.at_once as usize);
        if wanted == 0 {
            // Everyone is described; the sim is settling them.
            tokio::select! {
                () = cancel.cancelled() => return,
                () = tokio::time::sleep(BETWEEN_ROUNDS) => continue,
            }
        }
        let context = {
            let core = caster.world.core.read();
            let purpose = Purpose::Founder { settled, total };
            CastingContext::from_state(&core.sim.state, purpose, &reserved.lock())
        };
        let mut round = tokio::task::JoinSet::new();
        for _ in 0..wanted {
            let (client, context) = (client.clone(), context.clone());
            round.spawn(async move { generate_villager(&client, &context).await });
        }
        let results = tokio::select! {
            () = cancel.cancelled() => return,
            results = round.join_all() => results,
        };
        for result in results {
            match result {
                Ok(villager) => {
                    let first = |name: &str| name.split_whitespace().next().unwrap_or("").to_lowercase();
                    let mut reserved = reserved.lock();
                    if reserved.iter().any(|name| first(name) == first(&villager.name)) {
                        setbacks += 1;
                        tracing::info!(event = "casting", name = %villager.name, "two answers chose one first name; asking again");
                        continue;
                    }
                    setbacks = 0;
                    tracing::info!(event = "casting", name = %villager.name, class = ?villager.class_id, "a founder");
                    reserved.push(villager.name.clone());
                    send(&caster.world, Command::Settle(Box::new(villager)));
                }
                Err(error) => {
                    setbacks += 1;
                    tracing::warn!(event = "casting", %error, "a founder could not be cast");
                    if matches!(&error, CastingError::Model(e) if e.kind == LlmErrorKind::Network) {
                        setbacks = GIVE_UP_AFTER;
                    }
                }
            }
        }
        if setbacks >= GIVE_UP_AFTER {
            tracing::warn!(
                event = "casting",
                "the model is not casting; the sim founds the rest of the village itself"
            );
            send(&caster.world, Command::FoundVillage);
            return;
        }
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(BETWEEN_ROUNDS) => {}
        }
    }
}

/// The sim asked for a newcomer: one description, or word that none is coming.
async fn newcomer(caster: Arc<Caster>, deadline_tick: i64) {
    let Some(client) = caster.client.clone() else {
        send(&caster.world, Command::NewcomerLocal);
        return;
    };
    let context = {
        let core = caster.world.core.read();
        CastingContext::from_state(&core.sim.state, Purpose::Newcomer, &[])
    };
    match tokio::time::timeout(NEWCOMER_TIMEOUT, generate_villager(&client, &context)).await {
        Ok(Ok(villager)) => {
            tracing::info!(event = "casting", name = %villager.name, deadline_tick, "a newcomer");
            send(&caster.world, Command::NewcomerDraft(Box::new(villager)));
        }
        Ok(Err(error)) => {
            tracing::warn!(event = "casting", %error, "no newcomer from the model; the sim invents one");
            send(&caster.world, Command::NewcomerLocal);
        }
        Err(_) => {
            tracing::warn!(event = "casting", "the model took too long over a newcomer; the sim invents one");
            send(&caster.world, Command::NewcomerLocal);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use fsw_brain::llm::transport::{FakeTransport, HttpRequest, HttpResponse};
    use fsw_sim::FOUNDERS;
    use fsw_sim::util::living;
    use serde_json::{Value, json};

    use super::*;
    use crate::store::{SnapshotReason, testing};
    use crate::world_task::{TickEvents, start};

    fn model() -> LlmSettings {
        LlmSettings {
            enabled: true,
            base_url: "http://llm.test/v1".into(),
            model: "m".into(),
            ..LlmSettings::default()
        }
    }

    /// A model that invents numbered people, never two with one first name.
    fn inventive() -> Arc<FakeTransport> {
        let counter = Arc::new(AtomicUsize::new(0));
        FakeTransport::new(move |request: HttpRequest| {
            let n = counter.fetch_add(1, Ordering::Relaxed);
            // Names are letters only, as a person's are: Fakea, Fakeb, …
            let first = format!("Fake{}", (b'a' + (n % 26) as u8) as char);
            let body = request.json();
            let newcomer = body["messages"][1]["content"].as_str().unwrap_or("").contains("west road");
            let class = ["farmer", "knight", "ranger", "druid"][n % 4];
            let args = json!({
                "firstName": first, "lastName": "Person", "pronouns": "they/them", "age": 30 + n,
                "class": class, "occupation": "invented villager",
                "backstory": "Made up by the fake model.", "speechStyle": "Plain.", "timbre": "even",
                "traits": ["made", "up"], "goals": ["exist", "persist"], "temper": 0.5, "bravery": 0.5, "sociability": 0.5,
                "reason": if newcomer { "to see the valley" } else { "" }
            });
            let body = json!({ "choices": [{ "message": { "tool_calls": [
                { "function": { "name": "create_villager", "arguments": args.to_string() } },
            ] } }] });
            async move { Ok(HttpResponse::json(200, &body)) }
        })
    }

    async fn settled(world: &World) -> (usize, bool) {
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let core = world.core.read();
            if !core.sim.state.director.founding {
                return (fsw_sim::util::adult_count(&core.sim.state), false);
            }
        }
        let core = world.core.read();
        (fsw_sim::util::adult_count(&core.sim.state), core.sim.state.director.founding)
    }

    #[tokio::test]
    async fn without_a_model_the_sim_founds_a_new_world_at_once() {
        let (store, _dir) = testing::store().await;
        let shutdown = CancellationToken::new();
        let (world, task) = start(store, 4, shutdown.clone()).await.unwrap();
        let transport = FakeTransport::always(500, json!({}));
        let casting =
            spawn_with(world.clone(), LlmSettings::default(), ApiKey::default(), transport.clone(), shutdown.clone());
        assert_eq!(settled(&world).await, (FOUNDERS, false));
        assert!(transport.requests().is_empty());
        assert!(living(&world.core.read().sim.state).all(|c| !c.profile.name.starts_with("Fake")));
        shutdown.cancel();
        let _ = tokio::join!(task, casting);
    }

    #[tokio::test]
    async fn with_a_model_the_founders_are_the_ones_it_invents() {
        let (store, _dir) = testing::store().await;
        let shutdown = CancellationToken::new();
        let (world, task) = start(store, 4, shutdown.clone()).await.unwrap();
        let transport = inventive();
        let casting = spawn_with(world.clone(), model(), ApiKey::default(), transport.clone(), shutdown.clone());
        assert_eq!(settled(&world).await, (FOUNDERS, false));
        {
            let core = world.core.read();
            // The grown-ups are the model's; the children are the sim's own.
            assert!(
                fsw_sim::util::adults(&core.sim.state).all(|c| c.profile.name.starts_with("Fake")),
                "the model's people"
            );
            assert!(core.sim.state.chronicle.iter().any(|e| e.text.starts_with("The village is founded")));
        }
        let asked = transport.requests().len();
        assert!((FOUNDERS..FOUNDERS + 4).contains(&asked), "{asked} requests");
        // Every request told the model who had settled by then.
        let last = transport.bodies().last().unwrap()["messages"][1]["content"].as_str().unwrap().to_owned();
        assert!(last.contains("- Fakea Person, invented villager"), "{last}");
        shutdown.cancel();
        let _ = tokio::join!(task, casting);
    }

    #[tokio::test]
    async fn a_model_that_keeps_failing_hands_the_founding_to_the_sim() {
        let (store, _dir) = testing::store().await;
        let shutdown = CancellationToken::new();
        let (world, task) = start(store, 4, shutdown.clone()).await.unwrap();
        let transport = FakeTransport::always(500, json!({ "error": "down" }));
        let casting = spawn_with(world.clone(), model(), ApiKey::default(), transport.clone(), shutdown.clone());
        assert_eq!(settled(&world).await, (FOUNDERS, false));
        assert!(living(&world.core.read().sim.state).all(|c| !c.profile.name.starts_with("Fake")));
        shutdown.cancel();
        let _ = tokio::join!(task, casting);
    }

    #[tokio::test]
    async fn a_newcomer_the_sim_asks_for_is_described_or_declined() {
        let (store, _dir) = testing::store().await;
        let shutdown = CancellationToken::new();
        let (world, task) = start(store, 0, shutdown.clone()).await.unwrap();
        let (reply, answer) = tokio::sync::oneshot::channel();
        let founded = Box::new(fsw_testing::create_test_state(2));
        world
            .inputs
            .send(WorldInput::Replace { state: founded, reason: SnapshotReason::NewWorld, reply })
            .await
            .unwrap();
        answer.await.unwrap().unwrap();
        let transport = inventive();
        let casting = spawn_with(world.clone(), model(), ApiKey::default(), transport.clone(), shutdown.clone());
        tokio::time::sleep(Duration::from_millis(100)).await;
        world
            .events
            .send(Arc::new(TickEvents { tick: 1, events: vec![SimEvent::NewcomerWanted { deadline_tick: 7200 }] }))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        // The world is paused, so the draft waits in the input queue; it is what the sim will apply.
        let pending = world.core.read().sim.pending().to_vec();
        assert!(pending.iter().any(|c| matches!(c, Command::NewcomerDraft(v) if v.name == "Fakea Person" && v.reason == "to see the valley")), "{pending:?}");
        let body: Value = transport.bodies()[0].clone();
        assert!(body["messages"][1]["content"].as_str().unwrap().contains("Aldric Vane, captain of the village watch"));
        shutdown.cancel();
        let _ = tokio::join!(task, casting);

        // Without a model, the sim is told to invent the newcomer itself.
        let (store, _dir) = testing::store().await;
        let shutdown = CancellationToken::new();
        let (world, task) = start(store, 0, shutdown.clone()).await.unwrap();
        let casting = spawn_with(
            world.clone(),
            LlmSettings::default(),
            ApiKey::default(),
            FakeTransport::always(500, json!({})),
            shutdown.clone(),
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        world
            .events
            .send(Arc::new(TickEvents { tick: 1, events: vec![SimEvent::NewcomerWanted { deadline_tick: 7200 }] }))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let pending = world.core.read().sim.pending().to_vec();
        assert!(pending.iter().any(|c| matches!(c, Command::NewcomerLocal)), "{pending:?}");
        shutdown.cancel();
        let _ = tokio::join!(task, casting);
    }
}
