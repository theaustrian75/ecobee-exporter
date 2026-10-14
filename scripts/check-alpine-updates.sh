#!/usr/bin/env bash
# Compare each published image with a fresh Alpine install of the same packages.
# A difference means the floating stable tag or an apk package has moved.
# Multi-stage Dockerfiles are compared using the last runtime "apk add".
#
# Usage:
#   check-alpine-updates.sh Dockerfile=ghcr.io/owner/image [...]
#   check-alpine-updates.sh --list-packages Dockerfile [...]
#
# Writes changed, new_branch, and summary to $GITHUB_OUTPUT when that file exists.
set -euo pipefail

packages_in() {
  awk '
    function remember(text,    line, parts, i, nparts) {
      line = text
      sub(/#.*/, "", line)
      sub(/&&.*/, "", line)
      gsub(/\\/, "", line)
      gsub(/^[ \t]+|[ \t]+$/, "", line)
      if (line == "") return
      nparts = split(line, parts, /[ \t]+/)
      for (i = 1; i <= nparts; i++) if (parts[i] != "") pkgs[++n] = parts[i]
    }
    /apk add --no-cache/ && !/--virtual/ {
      inadd = 1
      n = 0
      line = $0
      sub(/^.*apk add --no-cache/, "", line)
      remember(line)
      if ($0 !~ /\\[[:space:]]*$/) inadd = 0
      next
    }
    inadd {
      stop = ($0 ~ /&&/)
      remember($0)
      if (stop || $0 !~ /\\[[:space:]]*$/) inadd = 0
    }
    END {
      for (i = 1; i <= n; i++) print pkgs[i]
    }
  ' "$1"
}

if [ "${1:-}" = "--list-packages" ]; then
  shift
  if [ "$#" -lt 1 ]; then
    echo "usage: $0 --list-packages Dockerfile [...]" >&2
    exit 1
  fi
  for dockerfile in "$@"; do
    packages_in "$dockerfile"
  done
  exit 0
fi

if [ "$#" -lt 1 ]; then
  echo "usage: $0 Dockerfile=ghcr.io/owner/image ..." >&2
  exit 1
fi

image_inventory() {
  docker run --rm --user root --entrypoint /bin/sh "$1" -c '
    echo "RELEASE=$(cat /etc/alpine-release)"
    apk info -v | sort
  '
}

candidate_inventory() {
  local branch="$1"
  shift
  docker run --rm --user root --entrypoint /bin/sh "alpine:${branch}" -c '
    set -eu
    apk upgrade --no-cache >/dev/null
    apk add --no-cache "$@" >/dev/null
    echo "RELEASE=$(cat /etc/alpine-release)"
    apk info -v | sort
  ' sh "$@"
}

append() {
  summary="${summary}${1}"$'\n'
}

latest_release="$(docker run --rm alpine:latest cat /etc/alpine-release)"
latest_branch="${latest_release%.*}"

changed=false
new_branch=""
summary=""

for spec in "$@"; do
  dockerfile="${spec%%=*}"
  image="${spec#*=}"
  if [ ! -f "$dockerfile" ]; then
    echo "missing Dockerfile: ${dockerfile}" >&2
    exit 1
  fi

  from="$(awk '/^FROM alpine:/{ print $2; exit }' "$dockerfile")"
  branch="${from#alpine:}"
  if [[ "$branch" =~ ^([0-9]+\.[0-9]+)\.[0-9]+$ ]]; then
    branch="${BASH_REMATCH[1]}"
  fi

  if [ "$latest_branch" != "$branch" ]; then
    new_branch="$latest_branch"
    append "${dockerfile} tracks alpine:${branch}; latest stable is alpine:${latest_branch} (${latest_release})."
  fi

  mapfile -t packages < <(packages_in "$dockerfile")
  if [ "${#packages[@]}" -eq 0 ]; then
    echo "no apk packages found in ${dockerfile}" >&2
    exit 1
  fi

  echo "Checking ${image}:latest against alpine:${branch} (${packages[*]})"
  if ! published="$(image_inventory "${image}:latest" 2>/dev/null)"; then
    changed=true
    append "${image}:latest could not be read."
    continue
  fi
  upstream="$(candidate_inventory "$branch" "${packages[@]}")"

  if [ "$published" != "$upstream" ]; then
    changed=true
    append "${image}:latest differs from alpine:${branch} after apk upgrade."
    append "$(diff -u \
      --label "${image}:latest" \
      --label "alpine:${branch}" \
      <(printf '%s\n' "$published") \
      <(printf '%s\n' "$upstream") || true)"
  else
    append "${image}:latest matches alpine:${branch}."
  fi
done

printf '%s' "$summary"

if [ -n "${GITHUB_OUTPUT:-}" ]; then
  {
    echo "changed=${changed}"
    echo "new_branch=${new_branch}"
    echo "summary<<ALPINE_SUMMARY_EOF"
    printf '%s\n' "$summary"
    echo "ALPINE_SUMMARY_EOF"
  } >>"$GITHUB_OUTPUT"
fi
