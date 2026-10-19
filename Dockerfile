FROM rust:1-bookworm@sha256:e12566d5abc17a0da741628782cdb504b68b8961f5bfcf677936bf3e8b32de29 AS builder

RUN apt-get update && apt-get install -y libpcap-dev pkg-config && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src

COPY . .
RUN touch src/main.rs && cargo build --release --locked

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171

RUN apt-get update && apt-get install -y libpcap0.8 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/tylomon /usr/local/bin/tylomon

ENTRYPOINT ["tylomon"]
