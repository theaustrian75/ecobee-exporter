# syntax=docker/dockerfile:1

FROM --platform=$TARGETPLATFORM rust:1-alpine3.23 AS builder

RUN apk add --no-cache musl-dev pkgconfig

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY src ./src

ARG TARGETARCH
RUN set -eu; \
    case "${TARGETARCH}" in \
        amd64) RUST_TARGET=x86_64-unknown-linux-musl ;; \
        arm64) RUST_TARGET=aarch64-unknown-linux-musl ;; \
        *) echo "unsupported architecture: ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    rustup target add "${RUST_TARGET}"; \
    cargo build --locked --release --target "${RUST_TARGET}" --bin ecobee-exporter --bin ecobee-login; \
    install -Dm755 "/build/target/${RUST_TARGET}/release/ecobee-exporter" /build/ecobee-exporter; \
    install -Dm755 "/build/target/${RUST_TARGET}/release/ecobee-login" /build/ecobee-login

FROM --platform=$TARGETPLATFORM alpine:3.23

LABEL org.opencontainers.image.description="Prometheus exporter for Ecobee thermostats."

# tzdata lets Alpine honor the standard TZ env var (e.g. America/New_York)
# for log timestamps and any libc localtime() callers. Set at runtime:
#   docker run -e TZ=America/New_York ...
# Default ecobee user (uid/gid 1000). docker-entrypoint.sh recreates this account
# at container start when UID/GID (or PUID/PGID) env vars differ.
RUN apk add --no-cache ca-certificates tzdata su-exec wget \
    && addgroup -g 1000 -S ecobee \
    && adduser -D -H -u 1000 -G ecobee -s /sbin/nologin ecobee

COPY --from=builder /build/ecobee-exporter /usr/local/bin/ecobee-exporter
COPY --from=builder /build/ecobee-login /usr/local/bin/ecobee-login
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

WORKDIR /var/lib/ecobee-exporter

ENV ECOBEE_STATE_FILE=/var/lib/ecobee-exporter/state.json
ENV UID=1000
ENV GID=1000

EXPOSE 9098

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD wget -qO- http://127.0.0.1:9098/healthz >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
