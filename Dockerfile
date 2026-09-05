# syntax=docker/dockerfile:1.7

FROM lukemathwalker/cargo-chef:0.1.77-rust-1.95.0-slim-bookworm@sha256:e570dfdde51ef616090dd76469dc709679a39d460d76a960717459ad324297f8 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    libssl-dev \
    pkg-config \
    libopus-dev \
    gcc \
    make \
    file && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY . .
RUN cargo test --release --locked --all-targets
RUN cargo build --release --locked

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171 AS runtime
WORKDIR /ncb-tts-r2

RUN groupadd -r appgroup && useradd -r -g appgroup appuser

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        ffmpeg \
        libssl3 \
        libopus0 && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/* && \
    chown appuser:appgroup /ncb-tts-r2

COPY --from=builder /app/target/release/ncb-tts-r2 /usr/local/bin/ncb-tts-r2

USER appuser

ENTRYPOINT ["/usr/local/bin/ncb-tts-r2"]
