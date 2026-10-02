# Phase 0 image: the web build plus the server, enough to prove the event
# stream behind the platform's proxy. Phase 6 adds cargo-chef layering and
# Litestream.

FROM node:22-bookworm-slim AS web
WORKDIR /app
RUN corepack enable
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY tsconfig.json vite.config.ts index.html ./
COPY public public
COPY src src
RUN pnpm exec vite build

FROM rust:1-bookworm AS server
WORKDIR /app/server
COPY server .
RUN cargo build --release --locked --bin fsw-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && useradd --system --home /app world && mkdir -p /data && chown world /data
WORKDIR /app
COPY --from=web /app/dist dist
COPY --from=server /app/server/target/release/fsw-server /usr/local/bin/fsw-server
USER world
ENV STATIC_DIR=/app/dist DATA_DIR=/data PORT=8080
EXPOSE 8080
CMD ["fsw-server"]
