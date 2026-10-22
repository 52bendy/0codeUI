# syntax=docker/dockerfile:1

# ---- 构建阶段 ----
FROM rust:1-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY server ./server
RUN cargo build --release --locked

# ---- 运行阶段 ----
FROM debian:bookworm-slim AS runtime
RUN useradd --system --uid 10001 --create-home intimeui \
    && mkdir -p /data \
    && chown intimeui:intimeui /data
COPY --from=builder /build/target/release/intimeui-server /usr/local/bin/intimeui-server
COPY --from=builder /build/target/release/intimeui-seed /usr/local/bin/intimeui-seed

USER intimeui
ENV APP_ENV=prod \
    APP_HOST=0.0.0.0 \
    APP_PORT=8080 \
    LOG_FORMAT=json \
    DATABASE_URL=sqlite:///data/intimeui.db
EXPOSE 8080
ENTRYPOINT ["intimeui-server"]
