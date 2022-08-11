FROM ubuntu:22.04
RUN apt-get update \
&& apt-get install -y ffmpeg libssl-dev pkg-config libopus-dev wget curl gcc \
&& apt-get -y clean \
&& rm -rf /var/lib/apt/lists/*
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable
ENV PATH $PATH:/root/.cargo/bin/
RUN rustup install stable
WORKDIR /usr/src/ncb-tts-r2
COPY Cargo.toml .
COPY src src
RUN cargo build --release \
&& cp /usr/src/ncb-tts-r2/target/release/ncb-tts-r2 /usr/bin/ncb-tts-r2 \
&& mkdir -p /ncb-tts-r2/audio
WORKDIR /ncb-tts-r2
CMD ["ncb-tts-r2"]