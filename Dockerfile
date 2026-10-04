# The web build and the server in one image, with Litestream streaming the
# database to object storage when a bucket is configured. See docs/deploy.md.

# --- the web client -----------------------------------------------------------
FROM node:22-bookworm-slim AS web
WORKDIR /app
RUN corepack enable
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY tsconfig.json vite.config.ts index.html ./
COPY public public
COPY src src
RUN pnpm exec vite build

# --- the server, with dependencies in their own cached layer (cargo-chef) -----
FROM rust:1-bookworm AS chef
RUN cargo install cargo-chef --locked --version 0.1.78
WORKDIR /app/server

FROM chef AS planner
COPY server .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/server/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json --bin fsw-server
COPY server .
# How many people may watch as guests at once is fixed when the server is built:
# `--build-arg GUEST_STREAMS=10`, or 0 for no guests (docs/deploy.md).
ARG GUEST_STREAMS=25
RUN cargo build --release --locked --bin fsw-server

# --- Litestream ---------------------------------------------------------------
FROM debian:bookworm-slim AS litestream
ARG LITESTREAM_VERSION=0.3.13
ARG TARGETARCH
ADD https://github.com/benbjohnson/litestream/releases/download/v${LITESTREAM_VERSION}/litestream-v${LITESTREAM_VERSION}-linux-${TARGETARCH}.tar.gz /tmp/litestream.tar.gz
RUN tar -xzf /tmp/litestream.tar.gz -C /usr/local/bin litestream

# --- runtime ------------------------------------------------------------------
FROM debian:bookworm-slim
RUN useradd --system --home /app world && mkdir -p /data && chown world /data
# The model client and Litestream verify TLS against the system roots; the builder image has them.
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
WORKDIR /app
COPY --from=web /app/dist dist
COPY --from=builder /app/server/target/release/fsw-server /usr/local/bin/fsw-server
COPY --from=litestream /usr/local/bin/litestream /usr/local/bin/litestream
COPY deploy/litestream.yml /etc/litestream.yml
COPY deploy/entrypoint.sh /usr/local/bin/entrypoint.sh
ENV STATIC_DIR=/app/dist DATA_DIR=/data PORT=8080
EXPOSE 8080
# Starts as root only to hand the mounted volume to the `world` user, then drops to it.
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
