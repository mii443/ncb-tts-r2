FROM ubuntu:22.04
WORKDIR /usr/src/ncb-tts-r2
COPY Cargo.toml .
COPY src src
ENV PATH $PATH:/root/.cargo/bin/
RUN apt-get update \
&& apt-get install -y ffmpeg libssl-dev pkg-config libopus-dev wget curl gcc \
&& apt-get -y clean \
&& rm -rf /var/lib/apt/lists/* \
&& curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable \
&& rustup install stable \
&& cargo build --release \
&& cp /usr/src/ncb-tts-r2/target/release/ncb-tts-r2 /usr/bin/ncb-tts-r2 \
&& mkdir -p /ncb-tts-r2/audio \
&& apt-get purge -y pkg-config wget curl gcc \
&& rustup self uninstall
WORKDIR /ncb-tts-r2
CMD ["ncb-tts-r2"]