# syntax=docker/dockerfile:1

# ---- 构建阶段 ----
FROM rust:1-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY server ./server
RUN cargo build --release --locked

# ---- 运行阶段 ----
FROM debian:bookworm-slim AS runtime
RUN useradd --system --uid 10001 --create-home intimeui
COPY --from=builder /build/target/release/intimeui-server /usr/local/bin/intimeui-server

USER intimeui
ENV APP_ENV=prod \
    APP_HOST=0.0.0.0 \
    APP_PORT=8080 \
    LOG_FORMAT=json
EXPOSE 8080
ENTRYPOINT ["intimeui-server"]
