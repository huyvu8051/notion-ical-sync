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

# Builds the islands crate to WASM (see crates/islands) for Leptos Islands —
# small interactive widgets like the regenerate-password confirm button. This
# is a separate cargo invocation with --features hydrate, never combined with
# the server's own `ssr`-featured build above (hydrate pulls in browser-only
# web-sys/DOM APIs that don't exist on this stage's host target).
FROM chef AS wasm-builder
RUN rustup target add wasm32-unknown-unknown
RUN cargo install wasm-bindgen-cli --version 0.2.126 --locked
COPY . .
RUN cargo build -p islands --release --target wasm32-unknown-unknown --no-default-features --features hydrate
RUN wasm-bindgen target/wasm32-unknown-unknown/release/islands.wasm --out-dir pkg --target web --no-typescript

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
    npx tailwindcss -c webview.config.js -i input.css -o /assets/style-webview.css --minify && \
    npx tailwindcss -c auth-a.config.js -i input.css -o /assets/style-auth-a.css --minify && \
    npx tailwindcss -c auth-b.config.js -i input.css -o /assets/style-auth-b.css --minify && \
    npx tailwindcss -c oauth.config.js -i input.css -o /assets/style-oauth.css --minify

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/notion-ical-sync /usr/local/bin/notion-ical-sync
COPY --from=wasm-builder /app/pkg /pkg
COPY --from=tailwind-builder /assets /assets
WORKDIR /
EXPOSE 8080
ENTRYPOINT ["notion-ical-sync"]
