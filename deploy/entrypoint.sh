#!/bin/sh
# Starts the server, under Litestream when a bucket is configured.
#
# - The volume is mounted owned by root: hand it to `world` and drop to that user.
# - With BUCKET_NAME set, a machine with an empty volume first restores the
#   newest replica, then Litestream runs the server as its child: it forwards
#   SIGTERM, waits for the server's shutdown snapshot, and syncs once more.
set -eu

: "${DATA_DIR:=/data}"
export DATA_DIR
: "${AWS_REGION:=auto}"
export AWS_REGION

if [ "$(id -u)" = 0 ]; then
  mkdir -p "$DATA_DIR"
  chown -R world:world "$DATA_DIR"
  exec setpriv --reuid=world --regid=world --init-groups "$0" "$@"
fi

if [ -n "${BUCKET_NAME:-}" ]; then
  litestream restore -config /etc/litestream.yml -if-db-not-exists -if-replica-exists "$DATA_DIR/world.db"
  exec litestream replicate -config /etc/litestream.yml -exec fsw-server
fi
exec fsw-server
