# syntax=docker/dockerfile:1.7
# nexc backend: Rust (axum) API + scheduler. The build compiles C kernels via the `cc` crate.
# Build: docker build -t nexc-engine-backend ./backend

ARG RUST_IMAGE=rust:1.99-bookworm
ARG RUNTIME_IMAGE=debian:bookworm-slim

# ---------------------------------------------------------------------------------------------
FROM ${RUST_IMAGE} AS builder
# gcc/make/perl ship with the image; cmake covers native crates (aws-lc-sys) that may want it.
# hadolint ignore=DL3008
RUN apt-get update \
 && apt-get install -y --no-install-recommends cmake \
 && rm -rf /var/lib/apt/lists/*
ENV CARGO_TERM_COLOR=never \
    CARGO_NET_RETRY=5 \
    SQLX_OFFLINE=true
WORKDIR /src

# Everything except target/ (see .dockerignore). rust-toolchain.toml is excluded too: the image
# tag pins the toolchain, so the build never downloads another one.
COPY . .

# Registry, git and target caches persist across builds (BuildKit cache mounts).
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked --bin nexc \
 && install -Dm755 target/release/nexc /out/nexc

# ---------------------------------------------------------------------------------------------
FROM ${RUNTIME_IMAGE} AS runtime
LABEL org.opencontainers.image.title="nexc-engine-backend" \
      org.opencontainers.image.description="nexc-engine API, scheduler and orchestrator" \
      org.opencontainers.image.source="https://github.com/f8fvwgzc/nexc-engine" \
      org.opencontainers.image.licenses="Apache-2.0"

# Debian security updates move package versions; the base image tag is the pin.
# hadolint ignore=DL3008
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl tini \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --system --gid 10001 nexc \
 && useradd --system --uid 10001 --gid nexc --no-create-home --home-dir /nonexistent \
        --shell /usr/sbin/nologin nexc \
 && install -d -o nexc -g nexc -m 0750 /data /data/symphony

COPY --from=builder /out/nexc /usr/local/bin/nexc

ENV NEXC_ENV=production \
    NEXC_HOST=0.0.0.0 \
    NEXC_PORT=8080 \
    NEXC_DATA_DIR=/data \
    NEXC_SYMPHONY_WORKFLOW=/data/symphony/WORKFLOW.md \
    NEXC_TRUST_PROXY=true

USER 10001:10001
WORKDIR /data
VOLUME ["/data"]
EXPOSE 8080

HEALTHCHECK --interval=15s --timeout=5s --start-period=30s --retries=3 \
  CMD ["curl", "-fsS", "-o", "/dev/null", "http://127.0.0.1:8080/api/v1/healthz"]

# `serve` applies pending database migrations before it starts listening.
ENTRYPOINT ["tini", "--", "nexc"]
CMD ["serve"]
