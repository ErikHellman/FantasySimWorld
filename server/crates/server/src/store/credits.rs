//! Viewers' credits: each account's balance, and what was bought with them.
//!
//! A row appears the first time an account earns or spends, holding [`STARTING_CREDITS`].
//! Every change is one transaction on the store thread, so two tabs of one account cannot
//! spend the same credits twice.

use rusqlite::{Connection, OptionalExtension};

use super::{DAY_MS, Result, Store, now_ms};

/// What a new account has before it has watched at all: enough to try the cheapest things.
pub const STARTING_CREDITS: i64 = 10;
/// Purchases are kept this long.
pub const PURCHASES_TTL_MS: i64 = 90 * DAY_MS;

/// One thing a viewer paid for, for the record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Purchase {
    /// `None` for a viewer without an account (token or sign-in off)
    pub user_id: Option<i64>,
    /// the name the villagers know them by
    pub name: String,
    pub world_id: i64,
    /// the offer's id
    pub offer: String,
    pub cost: i64,
    pub tick: u64,
}

fn ensure(conn: &Connection, user_id: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO credits (user_id, balance, earned_total, spent_total, updated_at) VALUES (?1, ?2, 0, 0, ?3)
         ON CONFLICT (user_id) DO NOTHING",
        (user_id, STARTING_CREDITS, now_ms()),
    )?;
    Ok(())
}

fn balance_of(conn: &Connection, user_id: i64) -> Result<i64> {
    Ok(conn.query_row("SELECT balance FROM credits WHERE user_id = ?1", [user_id], |r| r.get(0))?)
}

impl Store {
    /// What this account has to spend.
    pub async fn credits(&self, user_id: i64) -> Result<i64> {
        self.read(move |conn| {
            let balance: Option<i64> =
                conn.query_row("SELECT balance FROM credits WHERE user_id = ?1", [user_id], |r| r.get(0)).optional()?;
            Ok(balance.unwrap_or(STARTING_CREDITS))
        })
        .await
    }

    /// Adds up to `amount`, stopping at `cap` (a balance already above it stays). Returns the new balance.
    pub async fn earn_credits(&self, user_id: i64, amount: i64, cap: i64) -> Result<i64> {
        self.write(move |conn| {
            let tx = conn.transaction()?;
            ensure(&tx, user_id)?;
            let before = balance_of(&tx, user_id)?;
            let after = before.max(cap.min(before + amount.max(0)));
            tx.execute(
                "UPDATE credits SET balance = ?1, earned_total = earned_total + ?2, updated_at = ?3 WHERE user_id = ?4",
                (after, after - before, now_ms(), user_id),
            )?;
            tx.commit()?;
            Ok(after)
        })
        .await
    }

    /// Takes `cost` if the account has it and returns the new balance; `None` (and nothing taken) if not.
    /// `power` is the name they spend under, lower-cased: from now on prayers to it are theirs alone.
    pub async fn spend_credits(&self, user_id: i64, cost: i64, power: String) -> Result<Option<i64>> {
        self.write(move |conn| {
            let tx = conn.transaction()?;
            ensure(&tx, user_id)?;
            let before = balance_of(&tx, user_id)?;
            if before < cost {
                return Ok(None);
            }
            tx.execute("UPDATE credits SET power_name = NULL WHERE power_name = ?1 AND user_id != ?2", (&power, user_id))?;
            tx.execute(
                "UPDATE credits SET balance = balance - ?1, spent_total = spent_total + ?1, power_name = ?2, updated_at = ?3
                 WHERE user_id = ?4",
                (cost, &power, now_ms(), user_id),
            )?;
            tx.commit()?;
            Ok(Some(before - cost))
        })
        .await
    }

    /// Gives back what [`Store::spend_credits`] took for something that then could not happen.
    pub async fn refund_credits(&self, user_id: i64, cost: i64) -> Result<i64> {
        self.write(move |conn| {
            conn.execute(
                "UPDATE credits SET balance = balance + ?1, spent_total = spent_total - ?1, updated_at = ?2
                 WHERE user_id = ?3",
                (cost, now_ms(), user_id),
            )?;
            balance_of(conn, user_id)
        })
        .await
    }

    /// The account that last spent under this (lower-cased) name.
    pub async fn power_owner(&self, power: String) -> Result<Option<i64>> {
        self.read(move |conn| {
            Ok(conn
                .query_row("SELECT user_id FROM credits WHERE power_name = ?1", [&power], |r| r.get(0))
                .optional()?)
        })
        .await
    }

    pub async fn record_purchase(&self, purchase: Purchase) -> Result<()> {
        self.write(move |conn| {
            conn.execute(
                "INSERT INTO credit_purchases (user_id, name, world_id, offer, cost, tick, at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (
                    purchase.user_id,
                    &purchase.name,
                    purchase.world_id,
                    &purchase.offer,
                    purchase.cost,
                    purchase.tick as i64,
                    now_ms(),
                ),
            )?;
            Ok(())
        })
        .await
    }

    /// The purchases on record, oldest first.
    pub async fn purchases(&self) -> Result<Vec<Purchase>> {
        self.read(|conn| {
            let mut rows =
                conn.prepare("SELECT user_id, name, world_id, offer, cost, tick FROM credit_purchases ORDER BY id")?;
            let found = rows
                .query_map([], |r| {
                    Ok(Purchase {
                        user_id: r.get(0)?,
                        name: r.get(1)?,
                        world_id: r.get(2)?,
                        offer: r.get(3)?,
                        cost: r.get(4)?,
                        tick: r.get::<_, i64>(5)?.max(0) as u64,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(found)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing;
    use super::*;

    #[tokio::test]
    async fn an_account_starts_with_a_little_earns_up_to_the_cap_and_spends_what_it_has() {
        let (store, _dir) = testing::store().await;
        let id = testing::user(&store, "ann@example.com").await;
        assert_eq!(store.credits(id).await.unwrap(), STARTING_CREDITS);
        assert_eq!(store.earn_credits(id, 5, 400).await.unwrap(), 15);
        assert_eq!(store.earn_credits(id, 1000, 400).await.unwrap(), 400);
        assert_eq!(store.earn_credits(id, 1, 400).await.unwrap(), 400);

        assert_eq!(store.spend_credits(id, 150, "ann".into()).await.unwrap(), Some(250));
        assert_eq!(store.spend_credits(id, 251, "ann".into()).await.unwrap(), None);
        assert_eq!(store.credits(id).await.unwrap(), 250);
        assert_eq!(store.refund_credits(id, 150).await.unwrap(), 400);
        // A refund may leave more than the cap; earning never takes it away.
        assert_eq!(store.refund_credits(id, 20).await.unwrap(), 420);
        assert_eq!(store.earn_credits(id, 5, 400).await.unwrap(), 420);
    }

    #[tokio::test]
    async fn a_name_belongs_to_whoever_spent_under_it_last() {
        let (store, _dir) = testing::store().await;
        let ann = testing::user(&store, "ann@example.com").await;
        let bob = testing::user(&store, "bob@example.com").await;
        assert_eq!(store.power_owner("erik".into()).await.unwrap(), None);
        store.spend_credits(ann, 5, "erik".into()).await.unwrap();
        assert_eq!(store.power_owner("erik".into()).await.unwrap(), Some(ann));
        store.spend_credits(bob, 5, "erik".into()).await.unwrap();
        assert_eq!(store.power_owner("erik".into()).await.unwrap(), Some(bob));
        store.spend_credits(ann, 5, "mara".into()).await.unwrap();
        assert_eq!(store.power_owner("mara".into()).await.unwrap(), Some(ann));
    }

    #[tokio::test]
    async fn purchases_are_recorded_and_pruned_after_ninety_days() {
        let (store, _dir) = testing::store().await;
        let purchase = Purchase {
            user_id: None,
            name: "Quiet Fox".into(),
            world_id: 1,
            offer: "raid".into(),
            cost: 150,
            tick: 4200,
        };
        store.record_purchase(purchase.clone()).await.unwrap();
        assert_eq!(store.purchases().await.unwrap(), vec![purchase]);
        assert_eq!(store.prune(now_ms()).await.unwrap().purchases, 0);
        assert_eq!(store.prune(now_ms() + PURCHASES_TTL_MS + 1000).await.unwrap().purchases, 1);
        assert!(store.purchases().await.unwrap().is_empty());
    }
}
