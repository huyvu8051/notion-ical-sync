FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev pkgconfig libressl-dev
WORKDIR /app
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release --bin notion-ical-sync

FROM alpine:latest
RUN apk add --no-cache ca-certificates
COPY --from=builder /app/target/release/notion-ical-sync /usr/local/bin/notion-ical-sync
EXPOSE 8080
ENTRYPOINT ["notion-ical-sync"]
