FROM rust:1-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

# 先以空的 main.rs 編譯依賴，Cargo.toml / Cargo.lock 未變動時可沿用此 layer cache
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY src ./src
# 更新 main.rs 的 mtime，確保 cargo 重新編譯本專案
RUN touch src/main.rs && cargo build --release --locked

FROM alpine:3.22

COPY --from=builder /app/target/release/docker-network-autoconf /docker-network-autoconf

ENTRYPOINT ["/docker-network-autoconf"]
