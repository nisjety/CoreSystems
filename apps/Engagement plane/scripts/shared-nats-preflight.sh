#!/bin/sh
# Non-printing deployment check. Never create or rotate a broker credential.
set -eu
leaf_secret_file=${1:?A leaf secret file is required}
if [ ! -f "$leaf_secret_file" ] || [ -L "$leaf_secret_file" ]; then
  echo "Shared NATS leaf credential must be an existing protected regular file" >&2; exit 78
fi
leaf_secret_mode=$(stat -f '%Lp' "$leaf_secret_file" 2>/dev/null || stat -c '%a' "$leaf_secret_file")
case "$leaf_secret_mode" in 400|600) ;; *) echo "Shared NATS leaf credential must have mode 0400 or 0600" >&2; exit 78 ;; esac
leaf_secret_bytes=$(wc -c < "$leaf_secret_file" | tr -d '[:space:]')
case "$leaf_secret_bytes" in 64|65) ;; *) echo "Shared NATS leaf credential must contain 64 hexadecimal bytes and at most one newline" >&2; exit 78 ;; esac
leaf_secret_value=$(cat "$leaf_secret_file")
if [ "${#leaf_secret_value}" -ne 64 ]; then echo "Shared NATS leaf credential must contain 64 hexadecimal bytes" >&2; exit 78; fi
case "$leaf_secret_value" in *[!0-9a-f]*|"") echo "Shared NATS leaf credential must be hexadecimal" >&2; exit 78 ;; esac
unset leaf_secret_value
