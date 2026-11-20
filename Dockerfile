FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS chef
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

# All three ship in the `mock` stage below, for staging only -- never the
# production image. Built here, not a separate stage, so they reuse this
# stage's already-cooked dependency cache instead of paying for a second
# `cargo chef cook`. mock_paddle covers only the server-to-server Paddle
# calls (portal sessions, cancel, defer-to-trial-end) -- not checkout,
# which is client-side Paddle.js and always talks to real Paddle.
RUN cargo build --release --bin mock_notion --bin mock_resend --bin mock_paddle

# Builds crates/app to WASM for client-side hydration — every page's Shell
# is server-rendered via leptos_axum::render_app_to_stream (see crates/app/
# src/lib.rs), then this bundle hydrates it in the browser. Separate cargo
# invocation with --features hydrate, never combined with the server's own
# `ssr`-featured build above (hydrate pulls in browser-only web-sys/DOM APIs
# that don't exist on this stage's host target). Used to also build a
# separate crates/islands bundle (islands.js/islands_bg.wasm) for a handful
# of small interactive widgets — deleted once the last page migrated off it
# and every widget folded into this same app.js/app_bg.wasm bundle instead.
FROM chef AS wasm-builder
RUN rustup target add wasm32-unknown-unknown
RUN cargo install wasm-bindgen-cli --version 0.2.126 --locked
COPY . .
RUN cargo build -p app --release --target wasm32-unknown-unknown --no-default-features --features hydrate
RUN wasm-bindgen target/wasm32-unknown-unknown/release/app.wasm --out-dir pkg --target web --no-typescript

# Compiles Tailwind ahead of time (see tailwind/README.md) instead of
# shipping the cdn.tailwindcss.com Play CDN script, which was compiling the
# whole utility set in every visitor's browser — ~7s of render-blocking JS
# on a throttled mobile connection. Pinned to 3.4.17 + the exact plugin
# versions the Play CDN was resolving to in production, so output matches
# byte-for-byte what was already live.
FROM node:20-slim AS tailwind-builder
WORKDIR /build
RUN npm install --no-save tailwindcss@3.4.17 @tailwindcss/forms@0.5.10 @tailwindcss/container-queries@0.1.1
COPY tailwind ./tailwind
COPY src ./src
COPY crates ./crates
RUN mkdir -p /assets && cd tailwind && \
    npx tailwindcss -c auth-a.config.js -i input.css -o /assets/style-auth-a.css --minify

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/notion-ical-sync /usr/local/bin/notion-ical-sync
COPY --from=wasm-builder /app/pkg /pkg
COPY --from=tailwind-builder /assets /assets
COPY static /static
WORKDIR /
EXPOSE 8080
ENTRYPOINT ["notion-ical-sync"]

# Selected via `docker build --target mock` -- a stand-in Notion API for
# staging's load/regression tests, so they never touch the real Notion API
# (rate limits, and writing test data into a real connected workspace).
# No WASM/Tailwind/static assets: it's a pure JSON API, nothing it serves
# is a browser page. Pointed at by setting NOTION_API_BASE_URL on the real
# app (src/main.rs already supports this -- it was built for exactly this).
FROM debian:bookworm-slim AS mock
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/mock_notion /usr/local/bin/mock_notion
COPY --from=builder /app/target/release/mock_resend /usr/local/bin/mock_resend
COPY --from=builder /app/target/release/mock_paddle /usr/local/bin/mock_paddle
WORKDIR /
EXPOSE 3001 3002 3003
# Default to mock_notion -- the mock_resend and mock_paddle Deployments
# override this with their own `command` in k8s, same image either way.
ENTRYPOINT ["mock_notion"]
