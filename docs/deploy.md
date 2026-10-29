# Deploying the server

One container runs everything: the shared world, the language-model brain, the event stream to every viewer, the guest seats and the admin's token, and the web client's files. The world lives in one SQLite file on a persistent volume. These steps use [Fly.io](https://fly.io) (`fly.toml` is in the repository); any host with a persistent volume and a proxy that passes server-sent events through unbuffered works the same way with the `Dockerfile`.

## What it costs to run

One `shared-cpu-1x` machine with 512 MB and a 3 GB volume. Measured locally: about 17 MB of memory for the server with a few viewers (Litestream adds about 40 MB), well under 1% of a CPU, and 1 to 6 KB/s per viewer. The database grows to roughly 180 MB in a month and stays under half a gigabyte for a year with the retention rules below. The language model is the only cost that scales with use; see "The language model" for the caps.

## First deployment

```bash
fly launch --no-deploy --copy-config --name <your-app>    # keeps fly.toml, picks the region
fly volumes create data --size 3 --region <region>         # the same region as primary_region
fly secrets set AUTH_TOKEN=$(openssl rand -hex 24) PUBLIC_ORIGIN=https://<your-app>.fly.dev
fly deploy
```

Open `https://<your-app>.fly.dev/auth/token`, paste the token, and you are an admin of a brand-new world (paused worlds and speeds are set in **🛠 Admin**). Everyone who has the token is an admin; everyone else who opens the page watches as a guest ([sign-in.md](sign-in.md)).

To check that the event stream reaches browsers unbuffered through Fly's proxy (headers flushed at once, keep-alives through idle periods, reconnection across a deploy):

```bash
AUTH_TOKEN=… node tools/sse-probe.mjs https://<your-app>.fly.dev 120
curl -sI --http2 https://<your-app>.fly.dev/healthz        # HTTP/2 to the client
```

## Deploying from GitHub

There is no deploy workflow: GitHub Actions only runs the checks and cuts releases. Deploy with `fly deploy` from a checkout of the branch or tag you want live.

## Releasing

A release is cut from `main` by the **Release** workflow (`.github/workflows/release.yml`), which also runs only by hand. Open **Actions → Release → Run workflow** on `main` and choose:

- **bump**: `minor` (the default) for new things in the village, `patch` for fixes only, `major` for a break with old saves or clients;
- **summary**: one optional line on what the release brings, which goes at the top of the notes and in the tag's message.

The workflow runs the same checks as CI (skipping them when CI on `main` has already passed on that commit), moves the version in `package.json`, the Cargo workspace and `Cargo.lock` (with `tools/release/bump-version.mjs`, which can also be run by hand), commits that to `main` as "Release vX.Y.Z" and tags it. It then builds the web client and a Linux server and publishes a GitHub release with both, a checksum file, and notes that list the pull requests merged since the last release. It stops without releasing if `main` moved on while the checks ran or the tag already exists.

The workflow pushes to `main` with the repository's own Actions token. If `main` is ever protected so that only pull requests may change it, allow GitHub Actions to bypass that rule, or the push step fails and nothing is released.

## Settings

Secrets (`fly secrets set NAME=value`):

| Name | What it is |
| --- | --- |
| `AUTH_TOKEN` | The admin's shared token for `AUTH_MODE=token` (16+ characters) |
| `PUBLIC_ORIGIN` | The address people use, e.g. `https://village.example.com`; the CSRF check uses it |
| `LLM_API_KEY`, `LLM_HEADERS` | Optional: the model endpoint's key and any extra request headers. Read at every start and never stored in the database or sent to a browser. |
| `EMBED_API_KEY` | Optional: the embedding endpoint's key. Read at every start and never stored in the database or sent to a browser. |
| `BUCKET_NAME`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_ENDPOINT_URL_S3`, `AWS_REGION` | Optional: Litestream backups (set by `fly storage create`) |

An app set up before guest seats replaced sign-in still carries the old settings as secrets, and the server refuses to start with them (`AUTH_MODE must be token or off, got "oidc"` in `fly logs`). Remove them once (`AUTH_MODE` alone is enough for the server; the others, including each provider's client id and secret, are simply unused), making sure `AUTH_TOKEN` is set:

```bash
fly secrets unset AUTH_MODE AUTH_PROVIDERS ADMIN_EMAILS GOOGLE_CLIENT_ID GOOGLE_CLIENT_SECRET
```

Plain settings (in `fly.toml` under `[env]`): `PORT` (8080), `DATA_DIR` (`/data`), `MAX_VIEWERS` (150: how many people may watch at once, the rest waiting in line; `0` for no cap), `TRUST_PROXY=true` (take the visitor's address from `Fly-Client-IP` for the rate limit on the token and guest routes; only behind a proxy that sets it), `INITIAL_SPEED` (0, 1, 2 or 4: the speed of a brand-new world; afterwards the speed an admin chose is kept), `LOG_FORMAT` (`json`, or `pretty` for people), `RUST_LOG` (`info` by default).

The language model (`LLM_BASE_URL`, `LLM_MODEL` and the tuning variables `LLM_CONCURRENCY`, `LLM_REQUESTS_PER_MINUTE`, `LLM_MAX_REQUESTS_PER_DAY`, `LLM_MAX_TOKENS_PER_DAY`, `LLM_PROTOCOL`, `LLM_CONVERSATIONS`, `LLM_REASONING`, `LLM_TEMPERATURE`, `LLM_MAX_TOKENS`) is configured only here, either under `[env]` or as secrets. The admin panel can show and test it but not change it. Without `LLM_BASE_URL` and `LLM_MODEL` the model is off. All the variables and their defaults are in the [README](../README.md#connecting-a-language-model). A change takes effect when the machine restarts, which `fly secrets set` does on its own. Memory by meaning is set the same way, with `EMBED_BASE_URL` and `EMBED_MODEL` (and `EMBED_API_KEY` as a secret); see the [README](../README.md#memory-by-meaning).

Guest seats need no secret: a seat is 32 random bytes in an HttpOnly cookie, and the server keeps only its SHA-256, in memory. See the [README](../README.md#guests).

## Data, backups and restarts

Everything lives in `/data/world.db` (SQLite in WAL mode): world snapshots, the chronicle, the command record, the decision log and settings.

- **Snapshots** (zstd-compressed): every 30 seconds, 1.5 seconds after a notable event (a death, an arrival, the end of an attack, a new game day), and on shutdown, import and new world. Kept: the newest 20 of the 30-second ones (about ten minutes), the event ones for 24 hours, the first one of each game day for 14 days, and one per real day forever (a few hundred KB each). The newest snapshot is never removed.
- **Restarts and deploys**: on SIGTERM the server stops the world, writes a final snapshot and closes the database; on start it resumes from the newest snapshot, at the same tick. Viewers' pages reconnect by themselves. A deploy pauses the world for the 10 to 30 seconds the new machine takes to start.
- **Nightly copy**: once a day the server writes a consistent copy to `/data/backup/latest.db` (`VACUUM INTO`). It protects against a bad write, not against losing the volume.
- **Retention** (checked hourly): the command record is kept 2 days, the decision log 30 days (prompts and replies only for the newest 5,000 rows), memory vectors a week past their last use. The chronicle is kept whole.
- **Litestream** (recommended): with a bucket configured, [Litestream](https://litestream.io) streams every change to object storage within a second, and a machine that starts with an empty volume restores from it first. On Fly.io:

  ```bash
  fly storage create      # a Tigris bucket; sets BUCKET_NAME and the AWS_* secrets
  fly deploy
  ```

  Without `BUCKET_NAME` the container runs the server alone.

  To restore by hand (to look at an old state, or onto a new volume):

  ```bash
  litestream restore -config deploy/litestream.yml -o world.db /data/world.db                     # newest
  litestream restore -config deploy/litestream.yml -timestamp 2026-10-01T12:00:00Z -o world.db /data/world.db
  ```

  (with `DATA_DIR=/data` and the bucket's `BUCKET_NAME` and `AWS_*` variables in the environment).

- **Export and import**: **🛠 Admin → Export save** downloads the world as JSON (the same format the old browser version wrote); **Import save** replaces the running world with one. Neither includes the model's key.

The volume belongs to one machine, so there is exactly one machine: do not scale the app out. Scaling up (`fly scale vm`, `fly scale memory`) is fine.

## Health

- `/healthz` (no sign-in; Fly's health check) answers `ok tick <n>`, or 503 when the world has not ticked for 5 seconds while running.
- **🛠 Admin → Server health** (`GET /api/admin/health`): the tick, tick timing jitter (median and 99th percentile), how often the clock fell behind and skipped ahead, viewers dropped for falling behind, the database size, the age of the newest snapshot, and the model's requests, failures and tokens in the last 24 hours.
- Logs are JSON lines (`fly logs`); notable events carry an `event` field: `token_refused`, `guest_admitted`, `guest_queued`, `guest_left`, `rate_limited`, `csrf_refused`, `clock_skip`, `prune`, `backup`.

## The language model

It is off until `LLM_BASE_URL` and `LLM_MODEL` are set, for example with `fly secrets set LLM_BASE_URL=https://api.openai.com/v1 LLM_MODEL=<model> LLM_API_KEY=sk-…`. Then use **🛠 Admin → Language model → Test connection** to check it. Running around the clock, a village asks a fast model for about 20,000 decisions a day, so set the daily caps `LLM_MAX_REQUESTS_PER_DAY` and `LLM_MAX_TOKENS_PER_DAY`. They are spread evenly through the day (with half an hour's worth as a burst), so the village does not go quiet in the evening once a cap is near. The kill switch is `fly secrets unset LLM_BASE_URL LLM_MODEL`: the machine restarts and the villagers follow their routines. The decision log (🧠) shows every request.

To measure a model before letting it run the village:

```bash
cd server && cargo run --release -p fsw-brain --bin probe -- <base-url> <model> 20
```

## Building the image elsewhere

```bash
docker build -t fantasy-sim-world .
docker run --rm -p 8080:8080 -v fsw-data:/data -e AUTH_TOKEN=dev-token-0123456789 -e PUBLIC_ORIGIN=http://localhost:8080 fantasy-sim-world
```

The image builds the web client with Node, the server with Rust (dependencies cached in their own layer by `cargo-chef`, so a code change rebuilds only the server's own crates), and runs on `debian:bookworm-slim` as an unprivileged user. `deploy/entrypoint.sh` hands the mounted volume to that user, restores from Litestream if needed, and starts the server (under Litestream when a bucket is set).
