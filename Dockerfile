FROM rust:1 AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# sqlx::migrate!() is a compile-time macro — it embeds the migration files
# into the binary by reading this directory during macro expansion, not at
# runtime, so it has to exist in the build context before `cargo build`.
COPY migrations ./migrations
RUN cargo build --release --bin notion-ical-sync

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/notion-ical-sync /usr/local/bin/notion-ical-sync
EXPOSE 8080
ENTRYPOINT ["notion-ical-sync"]
