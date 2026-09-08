# syntax=docker/dockerfile:1
FROM rust:1.85-bookworm AS builder
WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY services ./services
RUN cargo build --release -p pumphound-collector

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates procps \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/pumphound-collector /usr/local/bin/pumphound-collector
USER nobody
WORKDIR /data
ENTRYPOINT ["pumphound-collector"]
