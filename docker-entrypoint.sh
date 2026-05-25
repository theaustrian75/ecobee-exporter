#!/bin/sh
set -e

# Recreate the ecobee account to match UID/GID from the environment so volume
# mounts align with the host user. PUID/PGID are accepted as aliases.
uid="${UID:-${PUID:-1000}}"
gid="${GID:-${PGID:-1000}}"

case "$uid$gid" in
    *[!0-9]*)
        echo "docker-entrypoint: UID/PUID and GID/PGID must be numeric (got UID=$uid GID=$gid)" >&2
        exit 1
        ;;
esac

sync_ecobee_user() {
    if id ecobee >/dev/null 2>&1 \
        && [ "$(id -u ecobee)" = "$uid" ] \
        && [ "$(id -g ecobee)" = "$gid" ]; then
        return 0
    fi

    if getent passwd "$uid" >/dev/null 2>&1; then
        existing=$(getent passwd "$uid" | cut -d: -f1)
        if [ "$existing" != "ecobee" ]; then
            echo "docker-entrypoint: UID $uid is already assigned to user '$existing'" >&2
            exit 1
        fi
    fi

    deluser ecobee 2>/dev/null || true
    if getent group ecobee >/dev/null 2>&1; then
        delgroup ecobee 2>/dev/null || true
    fi

    if getent group "$gid" >/dev/null 2>&1; then
        group=$(getent group "$gid" | cut -d: -f1)
    else
        addgroup -g "$gid" -S ecobee
        group=ecobee
    fi

    adduser -D -H -u "$uid" -G "$group" -s /sbin/nologin ecobee
}

run_as() {
    if [ "$(id -u)" = "0" ]; then
        sync_ecobee_user
        chown -R ecobee:"$(id -gn ecobee)" /var/lib/ecobee-exporter 2>/dev/null || true
        exec su-exec ecobee "$@"
    fi
    exec "$@"
}

if [ "$#" -eq 0 ]; then
    run_as /usr/local/bin/ecobee-exporter
fi

case "$1" in
    ecobee-exporter | /usr/local/bin/ecobee-exporter)
        shift
        run_as /usr/local/bin/ecobee-exporter "$@"
        ;;
    ecobee-login | /usr/local/bin/ecobee-login)
        shift
        run_as /usr/local/bin/ecobee-login "$@"
        ;;
    *)
        run_as "$@"
        ;;
esac
