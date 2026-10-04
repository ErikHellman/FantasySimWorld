# Memory by meaning

Villagers recall what matters to the moment, not just what shares a word with it. Someone standing by a burning house should remember the night the dead set fire to the mill, even if nobody said "fire" back then and the memory says "flames". Today the brain matches memories by the people they are about and by shared words of four letters or more. This adds an embedding model, set up on the server like the language model, and uses it to judge how close a memory is in meaning to the situation at hand. Without one, nothing changes.

## What happens

1. **The server is told where the embedding model is.** `EMBED_BASE_URL` and `EMBED_MODEL` (both, or neither) name any OpenAI-compatible `/v1/embeddings` endpoint: LM Studio, Ollama, OpenAI or another host. `EMBED_API_KEY` is optional. These are separate from the `LLM_*` settings because many chat hosts do not serve embeddings, and a small local embedding model works well next to a hosted chat model.
2. **Memories are embedded in the background.** Every few seconds the brain looks over the memories of the living, picks out the texts it has no vector for yet, and sends them in batches of up to 32. A village of thirteen makes a few hundred memories a day, so this is a handful of small requests a day.
3. **Vectors are kept, not saved with the world.** Each vector is stored in SQLite, keyed by a hash of the model name and the memory's text. The same text written to two villagers is embedded once. A memory that is pruned or belongs to the dead simply stops being used, and a daily sweep deletes vectors nobody has used for seven days. Changing the model starts a fresh set.
4. **A prompt asks what the moment is about.** When the brain prepares a decision, a reply, a conversation or a plea for mercy, it already builds a cue: the people involved and a line of text about what is happening. That text is embedded too (with a two-second limit). Plan prompts, which have no text cue today, use the villager's goals as their cue text, but only for the embedding: without vectors the plan's cue stays empty, so its keyword result does not change.
5. **Memories are scored by meaning.** The score stays as it is (recency, importance, the people involved, a bonus for reflections), but the shared-words part of relevance is replaced by how close the memory is to the cue: `clamp((cosine − 0.2) / 0.6, 0, 1) × 1.5`. A memory or a cue without a vector falls back to shared words, memory by memory, so a half-built index still works.
6. **Nothing goes wrong when the model is missing or slow.** If embeddings are not set up, are failing, or the cue takes longer than two seconds, retrieval is exactly what it is today.

## Where it lives

- **Settings** (`server/crates/server/src/config.rs`, `server/crates/brain/src/settings.rs`): `EMBED_BASE_URL`, `EMBED_MODEL`, `EMBED_API_KEY`, read once at start like the `LLM_*` variables. The key uses the same masked type as `ApiKey` and never reaches a browser, a log or an export. The admin panel's model section shows the endpoint and model read-only, and the connection test (`POST /api/admin/llm/test`) also embeds one sentence and reports the vector size and time.
- **The client** (`server/crates/brain/src/llm/embed.rs`): `POST {base}/embeddings` with `{model, input: [...]}`, reading `data[].embedding` in `index` order. Vectors are normalised to unit length and kept as `f32`. It has its own queue (one request at a time, its own per-minute limit and its own breaker), so embeddings never hold up a villager's decision and the governor does not count them. Requests and tokens are added to `brain_usage` under the kind `embed`.
- **The cache** (`server/crates/server/src/store/vectors.rs`, migration 4): `memory_vectors(key BLOB PRIMARY KEY, model TEXT NOT NULL, dim INTEGER NOT NULL, vec BLOB NOT NULL, used_ms INTEGER NOT NULL) STRICT`. The brain keeps the vectors it has loaded in memory (a village's worth is a few megabytes at 768 dimensions) and asks the store thread for the rest.
- **The indexer** (`server/crates/server/src/brain_task.rs`): a loop beside the brain that takes the world's read lock only long enough to copy out unindexed texts.
- **Retrieval** (`server/crates/brain/src/retrieval.rs`): `retrieve` gains an optional `&dyn VectorLookup` and an optional cue vector. The prompt builders (`prompts.rs`) pass them through. They stay pure functions of the world plus what they are given.
- **Jobs** (`server/crates/brain/src/brain.rs`): a job builds its cue under the read lock, waits for the cue's vector, then takes the read lock again to build the prompt, as it does today.
- **The mock** (`tools/mock-llm.mjs`): answers `/v1/embeddings` with deterministic vectors built from hashed words, so the whole path can be run without a model.
- **The probe**: `cargo run --release -p fsw-brain --bin probe -- <base-url> <model> --embed <embed-model>` checks an embedding endpoint, too.

Vectors never enter `SimState`. Saves, replay, the command record and determinism are untouched, and so is the prompt fixture, which is built without a lookup.

## Tests

- `retrieval.rs` unit tests with hand-made vectors: a cue about flames recalls a memory about a fire over a more recent one about bread, and with no vectors the result equals today's for the same input.
- The embed client against a fake transport: batching, `index` order, normalising, an error body and a timeout.
- The vector store: put, get, the sweep, and a model change missing the cache.
- `brain/tests/prompts.rs` is unchanged and must stay green.
- End to end against `tools/mock-llm.mjs`: the admin test reports a vector size, the cache fills, and the decision log shows recalled memories.

## Left out on purpose

- No vectors for the rumour lookup or for pruning in the sim: those choose what villagers keep and say, and must stay deterministic.
- No embedding of reflections' input (`since` stays as it is): reflection looks back over the whole day.
- No approximate-nearest-neighbour index: a villager has at most 140 memories, and a straight scan is fast.
- No re-ranking by the language model.
