FROM rust:1 AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --bin notion-ical-sync

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/notion-ical-sync /usr/local/bin/notion-ical-sync
EXPOSE 8080
ENTRYPOINT ["notion-ical-sync"]
