#!/usr/bin/env bash
set -euo pipefail

# Development TLS material for INTERNAL service-to-service calls on
# inter-plane-bus. This is not the edge: traefik/nginx on coresystem-edge
# terminate public traffic and are untouched by this script.
#
# Why this exists. The governed agent ticket-action lane refuses plaintext to
# any host that is not an IP loopback, on BOTH ends:
#   - execution-core    Model Plane/rust/services/execution-core/src/ticket_tools.rs
#                       service_base_url() -> is_loopback_url() parses the host as
#                       an IP, so a Docker service name can never qualify, even
#                       with EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK set.
#   - conversation-core Application Plane/conversation-core/conversation-core-go
#                       rejects a non-HTTPS authority URL in config, fatally.
# Neither user-core nor conversation-core has a TLS listener of its own, so the
# lane cannot run at all without a terminating proxy in front of them.
#
# Everything written here is DEVELOPMENT-ONLY, gitignored (secrets/*), and
# regenerable. Production terminates TLS with real certificates and must not use
# this CA.
#
# Idempotent: existing material is reused, so re-running does not invalidate a CA
# that running containers already trust. Pass --force to rotate.
#
# --- Windows/MSYS notes, each learned the hard way here ---------------------
# 1. Git Bash rewrites arguments that look like POSIX paths, turning
#    `-subj "/O=.../CN=..."` into a Windows path; openssl then writes a key and
#    no certificate. MSYS_NO_PATHCONV=1 fixes that.
# 2. But with conversion off, ABSOLUTE paths stop being translated, and this
#    repo lives under a path containing a space. A native Windows openssl cannot
#    open `/c/Users/...`. So the script cd's into the output directory and passes
#    bare filenames, which need no conversion either way.
# 3. Process substitution `<(...)` hands openssl a /dev/fd path it cannot open,
#    so the extensions go in a real file.
# None of the openssl calls hide stderr: the first version of this script did,
# and a failing CA step looked exactly like a successful one.

export MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL="*"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="$root/secrets/internal-tls"
days_ca="${INTERNAL_TLS_CA_DAYS:-3650}"
days_leaf="${INTERNAL_TLS_LEAF_DAYS:-825}"
force="${1:-}"

# The hostnames the proxy answers to. Each is a network alias on
# inter-plane-bus, so the SAN and the DNS name the client dials are the same
# string -- a mismatch is the classic "certificate is not valid for" failure.
hosts=(user-core-tls conversation-core-tls)

if [[ "$force" == "--force" ]]; then
  rm -rf "$out"
fi

if [[ -s "$out/ca.crt" && -s "$out/server.crt" && -s "$out/server.key" ]]; then
  echo "internal TLS material already present in $out (pass --force to rotate)"
  (cd "$out" && openssl x509 -in server.crt -noout -subject -enddate -ext subjectAltName)
  exit 0
fi

mkdir -p "$out"
umask 077
cd "$out"

echo "==> Development CA"
openssl req -x509 -newkey rsa:4096 -sha256 -nodes \
  -days "$days_ca" \
  -keyout ca.key -out ca.crt \
  -subj "/O=CoreSystem Development/CN=CoreSystem Internal Dev CA" \
  -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
  -addext "keyUsage=critical,keyCertSign,cRLSign"

echo "==> Server key + CSR"
openssl req -newkey rsa:2048 -sha256 -nodes \
  -keyout server.key -out server.csr \
  -subj "/O=CoreSystem Development/CN=${hosts[0]}"

# SANs, not just CN: rustls and Go both ignore CN entirely and match dNSName.
san="subjectAltName=$(printf 'DNS:%s,' "${hosts[@]}" | sed 's/,$//')"

{
  echo "$san"
  echo "basicConstraints=critical,CA:FALSE"
  echo "extendedKeyUsage=serverAuth"
} > leaf.ext

echo "==> Signing leaf for: ${hosts[*]}"
openssl x509 -req -in server.csr -sha256 \
  -CA ca.crt -CAkey ca.key -CAcreateserial \
  -days "$days_leaf" -out server.crt \
  -extfile leaf.ext

rm -f server.csr ca.srl leaf.ext
chmod 644 ca.crt server.crt
chmod 600 ca.key server.key

echo "==> Verifying the chain"
openssl verify -CAfile ca.crt server.crt

echo
echo "Wrote $out:"
openssl x509 -in server.crt -noout -subject -enddate -ext subjectAltName
echo
echo "Clients trust it via secrets/internal-tls/ca.crt:"
echo "  conversation-core  SSL_CERT_FILE (Go reads it natively)"
echo "  execution-core     EXECUTION_CORE_TICKET_CA_BUNDLE (reqwest is built with"
echo "                     rustls webpki-roots, which ignores the system store,"
echo "                     so the bundle has to be loaded explicitly)"
