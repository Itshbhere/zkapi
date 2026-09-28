FROM rust:bookworm AS builder

# The Rust daemon embeds its small local help page. OA Chat builds separately
# and consumes the browser SDK; no browser toolchain is required here.
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY protocol ./protocol

RUN cargo build --locked --release -p zkapi-cli -p zkapi-clientd -p zkapi-indexerd -p zkapi-serverd

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 zkapi

WORKDIR /srv/zkapi

COPY --from=builder /app/target/release/zkapi /usr/local/bin/zkapi
COPY --from=builder /app/target/release/zkapi-clientd /usr/local/bin/zkapi-clientd
COPY --from=builder /app/target/release/zkapi-indexerd /usr/local/bin/zkapi-indexerd
COPY --from=builder /app/target/release/zkapi-challenged /usr/local/bin/zkapi-challenged
COPY --from=builder /app/protocol/setup/v2 /srv/zkapi/protocol/setup/v2
COPY --chmod=755 docker/aws/run-daemon.sh /usr/local/bin/zkapi-run

USER zkapi

ENV RUST_LOG=info
ENV ZKAPI_PROOF_SETUP_DIR=/srv/zkapi/protocol/setup/v2
VOLUME ["/data"]

ENTRYPOINT ["zkapi-clientd"]
CMD ["--listen", "0.0.0.0:11434", "--state-dir", "/data/.zkapi", "--protocol-server-url", "http://zkapi-serverd:3000", "--indexer-url", "http://zkapi-indexerd:3001"]
