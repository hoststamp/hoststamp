# syntax=docker/dockerfile:1.7
# SPDX-License-Identifier: FSL-1.1-ALv2

FROM rust:1.95-bookworm@sha256:503651ea31e66ecb74623beabde781059a5978df1595a9e8ed03974d5fec1bf0 AS builder
WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/build/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --locked -p hoststamp \
    && cp target/release/hoststamp /usr/local/bin/hoststamp

FROM debian:bookworm-slim@sha256:67b30a61dc87758f0caf819646104f29ecbda97d920aaf5edc834128ac8493d3 AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --shell /usr/sbin/nologin hoststamp

COPY --from=builder /usr/local/bin/hoststamp /usr/local/bin/hoststamp

USER hoststamp
ENV HOSTSTAMP_ADDR=0.0.0.0:8080
EXPOSE 8080

HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD curl --fail --silent --show-error http://127.0.0.1:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/hoststamp"]
CMD ["serve"]
