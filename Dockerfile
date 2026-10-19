# syntax=docker/dockerfile:1.7
# SPDX-License-Identifier: FSL-1.1-ALv2

FROM rust:1.99-bookworm@sha256:e12566d5abc17a0da741628782cdb504b68b8961f5bfcf677936bf3e8b32de29 AS builder
WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/build/target \
    --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    cargo build --release --locked -p hoststamp \
    && install -m 0555 target/release/hoststamp /usr/local/bin/hoststamp

FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS runtime
LABEL org.opencontainers.image.title="Hoststamp" \
      org.opencontainers.image.description="Deterministic hostname generator CLI, API server, and local UX" \
      org.opencontainers.image.source="https://github.com/hoststamp/hoststamp" \
      org.opencontainers.image.licenses="FSL-1.1-ALv2"

# hadolint ignore=DL3008
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 hoststamp \
    && useradd --uid 10001 --gid 10001 --home-dir /home/hoststamp --create-home --shell /usr/sbin/nologin --no-log-init hoststamp \
    && install -d -o 10001 -g 10001 -m 0700 /home/hoststamp/.config/hoststamp

COPY --from=builder --chown=root:root --chmod=0555 /usr/local/bin/hoststamp /usr/local/bin/hoststamp

WORKDIR /home/hoststamp
USER 10001:10001
ENV HOME=/home/hoststamp
ENV XDG_CONFIG_HOME=/home/hoststamp/.config
ENV HOSTSTAMP_ADDR=0.0.0.0:8080
EXPOSE 8080
STOPSIGNAL SIGTERM

HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD curl --fail --silent --show-error http://127.0.0.1:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/hoststamp"]
CMD ["serve"]
