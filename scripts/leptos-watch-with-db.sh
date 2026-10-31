#!/usr/bin/env bash
# `cargo leptos watch` against a throwaway database instead of the real one.
#
# Plain `cargo leptos watch` is the normal way to run this, and it needs no
# arguments: DATABASE_URL falls back to the Postgres on :5433 that already
# holds your working data. Use this script only when you want that data left
# alone — seeding rows to exercise a code path, testing a migration, or
# wanting a first-run account.
#
# The database lives in .devdb/ at the repo root. Nothing registers to start
# at login, nothing collides with :5433, and `rm -rf .devdb` is the reset.
# Migrations run themselves on startup.
set -euo pipefail
cd "$(dirname "$0")/.."

DB_DIR="$PWD/.devdb"
DB_PORT=55433
DB_NAME=notioncal
export DATABASE_URL="postgres://postgres@localhost:$DB_PORT/$DB_NAME"

if [ ! -d "$DB_DIR" ]; then
  echo "throwaway db: creating $DB_DIR"
  initdb -D "$DB_DIR" -U postgres --auth=trust >/dev/null
fi

if ! pg_ctl -D "$DB_DIR" status >/dev/null 2>&1; then
  echo "throwaway db: starting postgres on $DB_PORT"
  pg_ctl -D "$DB_DIR" -o "-p $DB_PORT -k $DB_DIR" -l "$DB_DIR/server.log" start >/dev/null
  for _ in $(seq 1 30); do
    pg_isready -h "$DB_DIR" -p "$DB_PORT" -U postgres >/dev/null 2>&1 && break
    sleep 1
  done
fi

if ! psql -h "$DB_DIR" -p "$DB_PORT" -U postgres -lqt | cut -d'|' -f1 | grep -qw "$DB_NAME"; then
  echo "throwaway db: creating database $DB_NAME"
  createdb -h "$DB_DIR" -p "$DB_PORT" -U postgres "$DB_NAME"
fi

# Leaves the database running on exit so the next start is instant; stop it
# with `pg_ctl -D .devdb stop`, or delete the directory to start over.
echo "throwaway db: $DATABASE_URL"
exec cargo leptos watch "$@"
