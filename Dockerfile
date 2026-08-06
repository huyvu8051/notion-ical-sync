FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this layer is cached!
RUN cargo chef cook --release --recipe-path recipe.json

# Build application
COPY . .
RUN cargo build --release --bin notion-ical-sync

# Builds crates/islands (small interactive widgets, e.g. the
# regenerate-password confirm button — partial "islands" hydration) and
# crates/app (whole-page Leptos SSR+CSR components, see
# ~/.claude/plans/mighty-scribbling-floyd.md — full-body hydration) to WASM.
# Both are separate cargo invocations with --features hydrate, never combined
# with the server's own `ssr`-featured build above (hydrate pulls in
# browser-only web-sys/DOM APIs that don't exist on this stage's host
# target). Distinct output filenames (islands.js/app.js) so they coexist
# under the same /pkg directory without colliding.
FROM chef AS wasm-builder
RUN rustup target add wasm32-unknown-unknown
RUN cargo install wasm-bindgen-cli --version 0.2.126 --locked
COPY . .
RUN cargo build -p islands --release --target wasm32-unknown-unknown --no-default-features --features hydrate
RUN wasm-bindgen target/wasm32-unknown-unknown/release/islands.wasm --out-dir pkg --target web --no-typescript
RUN cargo build -p app --release --target wasm32-unknown-unknown --no-default-features --features hydrate
RUN wasm-bindgen target/wasm32-unknown-unknown/release/app.wasm --out-dir pkg --target web --no-typescript

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/notion-ical-sync /usr/local/bin/notion-ical-sync
COPY --from=wasm-builder /app/pkg /pkg
WORKDIR /
EXPOSE 8080
ENTRYPOINT ["notion-ical-sync"]
