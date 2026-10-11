//! Embedding vectors for memory texts (see `fsw_brain::recall`). They are a cache kept
//! apart from the world: saves and replay never see them, and losing them only costs
//! requests to the embedding endpoint.

use std::collections::HashSet;
use std::sync::Arc;

use rusqlite::types::ValueRef;

use super::{DAY_MS, Result, Store, now_ms};

/// Vectors nobody has used for this long are deleted by maintenance.
pub const VECTORS_TTL_MS: i64 = 7 * DAY_MS;

fn to_blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn from_blob(blob: &[u8]) -> Option<Arc<[f32]>> {
    let (numbers, rest) = blob.as_chunks::<4>();
    (!numbers.is_empty() && rest.is_empty()).then(|| numbers.iter().map(|b| f32::from_le_bytes(*b)).collect())
}

impl Store {
    /// The kept vectors made under this model for the `wanted` keys. The table also holds the vectors of the dead
    /// and the forgotten until maintenance sweeps them a week on, so it can be far larger than what the living
    /// recall: reading all of it at start once took the server past its machine's memory before it could listen.
    pub async fn load_vectors(&self, model: &str, wanted: HashSet<i64>) -> Result<Vec<(i64, Arc<[f32]>)>> {
        let model = model.to_owned();
        self.read(move |conn| {
            let mut stmt = conn.prepare("SELECT key, vec FROM memory_vectors WHERE model = ?1")?;
            let mut rows = stmt.query([&model])?;
            let mut vectors = Vec::new();
            while let Some(row) = rows.next()? {
                let key = row.get::<_, i64>(0)?;
                // The blob is read in place, and only for a wanted key.
                if wanted.contains(&key)
                    && let ValueRef::Blob(blob) = row.get_ref(1)?
                    && let Some(vector) = from_blob(blob)
                {
                    vectors.push((key, vector));
                }
            }
            Ok(vectors)
        })
        .await
    }

    /// Keeps new vectors, marked as used now.
    pub async fn put_vectors(&self, model: &str, rows: Vec<(i64, Arc<[f32]>)>) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let model = model.to_owned();
        self.write(move |conn| {
            let tx = conn.transaction()?;
            {
                let mut put = tx.prepare_cached(
                    "INSERT INTO memory_vectors (key, model, dim, vec, used_ms) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (key) DO UPDATE SET model = excluded.model, dim = excluded.dim,
                         vec = excluded.vec, used_ms = excluded.used_ms",
                )?;
                let now = now_ms();
                for (key, vector) in &rows {
                    put.execute((key, &model, vector.len() as i64, to_blob(vector), now))?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Marks these vectors as still in use, so maintenance keeps them.
    pub async fn touch_vectors(&self, keys: Vec<i64>, at_ms: i64) -> Result<()> {
        self.write(move |conn| {
            let tx = conn.transaction()?;
            {
                let mut touch = tx.prepare_cached("UPDATE memory_vectors SET used_ms = ?2 WHERE key = ?1")?;
                for key in &keys {
                    touch.execute((key, at_ms))?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing;
    use super::*;

    #[tokio::test]
    async fn vectors_are_kept_by_model_and_swept_when_unused() {
        let (store, _dir) = testing::store().await;
        store
            .put_vectors("e1", vec![(1, Arc::from(vec![0.6f32, 0.8])), (2, Arc::from(vec![1.0f32, 0.0]))])
            .await
            .unwrap();
        store.put_vectors("e2", vec![(3, Arc::from(vec![0.0f32, 1.0, 0.0]))]).await.unwrap();
        let all = || HashSet::from([1, 2, 3]);
        let mut kept = store.load_vectors("e1", all()).await.unwrap();
        kept.sort_by_key(|(key, _)| *key);
        assert_eq!(kept, vec![(1, Arc::from(vec![0.6, 0.8])), (2, Arc::from(vec![1.0, 0.0]))]);
        assert_eq!(store.load_vectors("e2", all()).await.unwrap().len(), 1);
        assert!(store.load_vectors("another", all()).await.unwrap().is_empty());
        // Only the wanted keys are read.
        assert_eq!(store.load_vectors("e1", HashSet::from([2])).await.unwrap(), vec![(2, Arc::from(vec![1.0, 0.0]))]);

        // A week on, only what was used lately is left.
        let later = now_ms() + VECTORS_TTL_MS + 1_000;
        store.touch_vectors(vec![1], later).await.unwrap();
        let report = store.prune(later).await.unwrap();
        assert_eq!(report.vectors, 2);
        assert_eq!(store.load_vectors("e1", all()).await.unwrap(), vec![(1, Arc::from(vec![0.6, 0.8]))]);
    }

    #[test]
    fn from_blob_refuses_empty_and_partial_data() {
        assert_eq!(from_blob(&[]), None);
        assert_eq!(from_blob(&[0, 0, 0]), None);
        assert_eq!(from_blob(&[0, 0, 0, 0]).map(|v| v.len()), Some(1));
    }
}
