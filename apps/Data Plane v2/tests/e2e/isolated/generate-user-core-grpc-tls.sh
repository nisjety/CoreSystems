#!/usr/bin/env bash
set -euo pipefail
set +x
umask 077

runtime_dir=${1:?runtime directory is required}
case "$runtime_dir" in
  */tests/e2e/.real-authority-runtime.* | */tests/e2e/.real-authority-browser-runtime.* ) ;;
  * ) echo "refusing User Core TLS fixture outside an isolated runtime directory" >&2; exit 2 ;;
esac
test -d "$runtime_dir"

ca_key="$runtime_dir/user-grpc-tls-ca-key.pem"
ca_certificate="$runtime_dir/user-grpc-tls-ca.pem"
server_key="$runtime_dir/user-grpc-tls-server-key.pem"
server_request="$runtime_dir/user-grpc-tls-server.csr"
server_certificate="$runtime_dir/user-grpc-tls-server.pem"

openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out "$ca_key" >/dev/null 2>&1
openssl req -x509 -new -key "$ca_key" -sha256 -days 1 \
  -subj '/CN=Disposable User Core gRPC CA' \
  -addext 'basicConstraints=critical,CA:TRUE' \
  -addext 'keyUsage=critical,keyCertSign,cRLSign' \
  -out "$ca_certificate" >/dev/null 2>&1
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out "$server_key" >/dev/null 2>&1
openssl req -new -key "$server_key" -sha256 -subj '/CN=user-core' \
  -addext 'subjectAltName=DNS:user-core' \
  -addext 'extendedKeyUsage=serverAuth' \
  -out "$server_request" >/dev/null 2>&1
openssl x509 -req -in "$server_request" \
  -CA "$ca_certificate" -CAkey "$ca_key" -CAcreateserial \
  -sha256 -days 1 -copy_extensions copy \
  -out "$server_certificate" >/dev/null 2>&1

chmod 600 "$ca_key" "$server_key"
chmod 644 "$ca_certificate" "$server_certificate"
