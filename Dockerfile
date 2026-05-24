# syntax=docker/dockerfile:1

FROM rust:1-alpine3.23 AS builder

RUN apk add --no-cache musl-dev pkgconfig

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY crates/housekey ./crates/housekey
COPY src ./src

RUN cargo build --locked --release --bin ecobee-exporter --bin ecobee-login

FROM alpine:3.23

# tzdata lets Alpine honor the standard TZ env var (e.g. America/New_York)
# for log timestamps and any libc localtime() callers. Set at runtime:
#   docker run -e TZ=America/New_York ...
RUN apk add --no-cache ca-certificates tzdata \
    && adduser -D -H -u 1000 -s /sbin/nologin ecobee

COPY --from=builder /build/target/release/ecobee-exporter /usr/local/bin/ecobee-exporter
COPY --from=builder /build/target/release/ecobee-login /usr/local/bin/ecobee-login

WORKDIR /var/lib/ecobee-exporter

ENV ECOBEE_STATE_FILE=/var/lib/ecobee-exporter/state.json

USER ecobee

EXPOSE 9098

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD wget -qO- http://127.0.0.1:9098/healthz >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/ecobee-exporter"]
