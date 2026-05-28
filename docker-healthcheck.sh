#!/bin/sh
set -e

listen="${ECOBEE_LISTEN_ADDR:-0.0.0.0:9098}"
port="${listen##*:}"
probe_url="http://127.0.0.1:${port}/healthz"
wget_extra=""

tls_cert="${ECOBEE_TLS__CERT_FILE:-${ECOBEE_TLS_CERT_FILE:-${TLS_CERT_FILE:-}}}"
tls_key="${ECOBEE_TLS__KEY_FILE:-${ECOBEE_TLS_KEY_FILE:-${TLS_KEY_FILE:-}}}"

if [ -n "$tls_cert" ] && [ -n "$tls_key" ] && [ -f "$tls_cert" ] && [ -f "$tls_key" ]; then
    probe_url="https://127.0.0.1:${port}/healthz"
    wget_extra="--no-check-certificate"
fi

# Omit wget -q so connection errors are written to stderr.
if ! wget $wget_extra -O- "$probe_url" >/dev/null; then
    echo "healthcheck failed: GET $probe_url" >&2
    exit 1
fi
