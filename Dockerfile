FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg libssl-dev pkg-config libopus-dev gcc && apt-get -y clean
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim AS runtime
WORKDIR /ncb-tts-r2
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates ffmpeg libssl-dev libopus-dev && apt-get -y clean && mkdir audio
COPY --from=builder /app/target/release/ncb-tts-r2 /usr/local/bin
ENTRYPOINT ["/usr/local/bin/ncb-tts-r2"]
