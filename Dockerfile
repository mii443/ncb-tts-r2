FROM lukemathwalker/cargo-chef:latest-rust-1.82 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ffmpeg \
    libssl-dev \
    pkg-config \
    libopus-dev \
    gcc && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release

FROM ubuntu:22.04 AS runtime
WORKDIR /ncb-tts-r2

# 非rootユーザーの作成
RUN groupadd -r appgroup && useradd -r -g appgroup appuser

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    openssl \
    ca-certificates \
    ffmpeg \
    libssl-dev \
    libopus-dev && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/ncb-tts-r2 /usr/local/bin/ncb-tts-r2
RUN chmod +x /usr/local/bin/ncb-tts-r2

# 非rootユーザーに切り替え
USER appuser

ENTRYPOINT ["/usr/local/bin/ncb-tts-r2"]
