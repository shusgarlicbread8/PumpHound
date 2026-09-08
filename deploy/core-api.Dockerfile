# syntax=docker/dockerfile:1
FROM rust:1.85-bookworm AS builder
WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY services ./services
RUN cargo build --release -p pumphound-core-api

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/pumphound-core-api /usr/local/bin/pumphound-core-api
USER nobody
EXPOSE 8080
ENTRYPOINT ["pumphound-core-api"]
