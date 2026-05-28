#!/bin/sh
set -e

# Used by the image HEALTHCHECK, compose, and Podman quadlets.
if [ -n "${ECOBEE_TLS__CERT_FILE:-}" ] || [ -n "${ECOBEE_TLS_CERT_FILE:-}" ]; then
    exec wget --no-check-certificate -qO- "https://127.0.0.1:9098/liveness" >/dev/null
fi

exec wget -qO- "http://127.0.0.1:9098/liveness" >/dev/null
