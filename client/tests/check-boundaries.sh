#!/bin/sh
set -eu
manifest="$(dirname "$0")/boundaries/Cargo.toml"
diagnostic="$(mktemp)"
trap 'rm -f "$diagnostic"' EXIT HUP INT TERM
check_rejected() {
    if cargo check --offline --manifest-path "$manifest" --target x86_64-unknown-linux-gnu --features forbidden --bin "$1" >"$diagnostic" 2>&1; then
        cat "$diagnostic"
        printf 'unexpected success: %s\n' "$1"
        exit 1
    fi
    if ! rg --quiet "$2" "$diagnostic"; then
        cat "$diagnostic"
        printf 'unexpected failure: %s\n' "$1"
        exit 1
    fi
}
check_rejected api-trap 'error\[E0432\]'
check_rejected api-call 'error\[E0599\]'
check_rejected tls-send 'error\[E0277\]'
check_rejected tls-copy 'error\[E0277\]'
check_rejected tls-sync 'error\[E0277\]'
printf 'API effect absence and TLS ownership probes passed\n'
