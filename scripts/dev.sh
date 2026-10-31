#!/usr/bin/env bash
# The one way to run this locally.
#
# `cargo leptos watch` is what actually serves the app -- it builds the wasm
# hydrate bundle as well as the server binary, which a bare `cargo run` does
# not, so a page served that way renders but never hydrates. Everything above
# the last line here exists only to hand it a database.
#
# Postgres lives in .devdb at the repo root rather than in a Homebrew service:
# nothing is registered to start at login, nothing collides with a system
# Postgres on 5432, and deleting the directory is a full reset. Migrations run
# themselves on startup (src/main.rs), so there is no separate step.
set -euo pipefail
cd "$(dirname "$0")/.."

DB_DIR="$PWD/.devdb"
DB_PORT=55433
DB_NAME=notioncal
export DATABASE_URL="postgres://postgres@localhost:$DB_PORT/$DB_NAME"

if [ ! -d "$DB_DIR" ]; then
  echo "dev: creating $DB_DIR"
  initdb -D "$DB_DIR" -U postgres --auth=trust >/dev/null
fi

if ! pg_ctl -D "$DB_DIR" status >/dev/null 2>&1; then
  echo "dev: starting postgres on $DB_PORT"
  pg_ctl -D "$DB_DIR" -o "-p $DB_PORT -k $DB_DIR" -l "$DB_DIR/server.log" start >/dev/null
  for _ in $(seq 1 30); do
    pg_isready -h "$DB_DIR" -p "$DB_PORT" -U postgres >/dev/null 2>&1 && break
    sleep 1
  done
fi

if ! psql -h "$DB_DIR" -p "$DB_PORT" -U postgres -lqt | cut -d'|' -f1 | grep -qw "$DB_NAME"; then
  echo "dev: creating database $DB_NAME"
  createdb -h "$DB_DIR" -p "$DB_PORT" -U postgres "$DB_NAME"
fi

echo "dev: http://127.0.0.1:8080"
exec cargo leptos watch "$@"
