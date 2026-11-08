FROM rust:1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml ./
COPY src ./src
COPY migrations ./migrations
COPY web ./web
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home --shell /usr/sbin/nologin messagepusher
WORKDIR /app
COPY --from=builder /app/target/release/messagepusher /usr/local/bin/messagepusher
USER 10001:10001
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/messagepusher"]
