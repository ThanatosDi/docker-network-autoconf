FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM alpine:3.22

COPY --from=builder /app/target/release/docker-network-autoconf /docker-network-autoconf

ENTRYPOINT ["/docker-network-autoconf"]
