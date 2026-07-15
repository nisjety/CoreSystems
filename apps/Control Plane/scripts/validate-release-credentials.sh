#!/usr/bin/env bash
set -euo pipefail

# Release preflight for scoped broker and HTTP service principals. Values are
# never printed, even on failure. Run after secrets are injected and before
# Compose interpolation or any broker/service rollout.

credential_names=(
  AUTH_NATS_PASSWORD USER_NATS_PASSWORD ORG_NATS_PASSWORD
  BILLING_NATS_PASSWORD SESSION_NATS_PASSWORD AUDIT_CONTROL_NATS_PASSWORD
  CONTROL_NATS_PROVISIONER_PASSWORD
  AUTH_SHARED_NATS_PASSWORD USER_SHARED_NATS_PASSWORD ORG_SHARED_NATS_PASSWORD
  BILLING_SHARED_NATS_PASSWORD SESSION_SHARED_NATS_PASSWORD
  CONTROL_SHARED_BRIDGE_PASSWORD CONTROL_SHARED_NATS_PROVISIONER_PASSWORD
  APPLICATION_CONVEX_CONTROL_NATS_PASSWORD DOCUMENTS_GDPR_NATS_PASSWORD
  APPLICATION_CONVEX_CONTROL_PROJECTION_KEY
  CONVEX_INTERNAL_SERVICE_KEY CONVEX_RECONCILIATION_KEY
  MODEL_NATS_RUNTIME_PASSWORD MODEL_GATEWAY_NATS_PASSWORD MODEL_SESSION_CORE_NATS_PASSWORD
  AUDIT_MODEL_NATS_PASSWORD MODEL_NATS_PROVISIONER_PASSWORD
  APPLICATION_CONVEX_MODEL_NATS_PASSWORD APPLICATION_INSIGHT_MODEL_NATS_PASSWORD
  APPLICATION_CONVERSATION_NATS_PASSWORD APPLICATION_SOCIAL_NATS_PASSWORD
  APPLICATION_INSIGHT_NATS_PASSWORD APPLICATION_LEADS_NATS_PASSWORD
  APPLICATION_NOTIFICATION_NATS_PASSWORD
  AUDIT_APPLICATION_NATS_PASSWORD APPLICATION_NATS_PROVISIONER_PASSWORD
  SESSION_CORE_SERVICE_TOKEN GATEWAY_ORG_CORE_SERVICE_TOKEN
  GATEWAY_AUTH_GRPC_SERVICE_TOKEN RETRIEVAL_AUTH_GRPC_SERVICE_TOKEN
  GATEWAY_BILLING_CORE_SERVICE_TOKEN GATEWAY_AUDIT_CORE_SERVICE_TOKEN
  AUTH_ORG_CORE_SERVICE_TOKEN AUTH_BILLING_CORE_SERVICE_TOKEN
  BILLING_ORG_CORE_SERVICE_TOKEN SESSION_ORG_CORE_SERVICE_TOKEN
  SESSION_BILLING_CORE_SERVICE_TOKEN INTEGRATION_ORG_CORE_SERVICE_TOKEN
  INTEGRATION_BILLING_CORE_SERVICE_TOKEN INTEGRATION_AUDIT_CORE_SERVICE_TOKEN
  USER_CORE_GATEWAY_TOKEN USER_CORE_SESSION_TOKEN USER_CORE_ORG_TOKEN
  USER_CORE_AUTH_TOKEN USER_CORE_MEMBERSHIP_SERVICE_TOKEN
  APPLICATION_RECONCILER_AUTH_TOKEN USER_CORE_DOCUMENTS_TOKEN
  USER_CORE_RETRIEVAL_TOKEN
  AUTH_USER_CORE_GRPC_SERVICE_TOKEN USER_AUTH_INTERNAL_SERVICE_TOKEN
  QUARRY_AUTH_INTERNAL_SERVICE_TOKEN
)

if [[ "${REQUIRE_LEGACY_BRIDGE:-0}" == "1" ]]; then
  credential_names+=(VELION_NATS_TOKEN)
fi

fingerprints=()
fingerprint_owners=()
failed=0

for name in "${credential_names[@]}"; do
  value="${!name-}"
  normalized="$(printf '%s' "$value" | tr '[:upper:]' '[:lower:]')"
  if [[ -z "$value" ]]; then
    printf 'credential preflight: %s is missing\n' "$name" >&2
    failed=1
    continue
  fi
  if (( ${#value} < 32 )); then
    printf 'credential preflight: %s is shorter than 32 characters\n' "$name" >&2
    failed=1
  fi
  if [[ "$normalized" =~ (change[-_]?me|placeholder|example|your[-_]|default[-_]?secret|insecure|dummy) ]]; then
    printf 'credential preflight: %s contains a placeholder marker\n' "$name" >&2
    failed=1
  fi

  # Hash only for in-process comparison; neither the value nor fingerprint is
  # emitted. `cksum` is insufficient because collisions could miss reuse.
  fingerprint="$(printf '%s' "$value" | shasum -a 256 | awk '{print $1}')"
  reused_by=''
  for index in "${!fingerprints[@]}"; do
    if [[ "${fingerprints[$index]}" == "$fingerprint" ]]; then
      reused_by="${fingerprint_owners[$index]}"
      break
    fi
  done
  if [[ -n "$reused_by" ]]; then
    printf 'credential preflight: %s reuses the value assigned to %s\n' \
      "$name" "$reused_by" >&2
    failed=1
  else
    fingerprints+=("$fingerprint")
    fingerprint_owners+=("$name")
  fi
done

# Validate the production Auth file-backed contracts without ever echoing file
# contents, tokens, key material, paths, or fingerprints.
if ! node <<'NODE'
const {
  X509Certificate,
  createPrivateKey,
  createPublicKey,
} = require('node:crypto');
const { readFileSync, statSync } = require('node:fs');

const fail = (name, reason) => {
  process.stderr.write(`credential preflight: ${name} ${reason}\n`);
  process.exit(1);
};
const readBounded = (name, maximumBytes, privateFile = false) => {
  const path = (process.env[name] || '').trim();
  if (!path) fail(name, 'is missing');
  let stat;
  let value;
  try {
    stat = statSync(path);
    value = readFileSync(path);
  } catch {
    fail(name, 'is not readable');
  }
  if (!stat.isFile() || value.length === 0 || value.length > maximumBytes) {
    fail(name, 'is not a bounded regular file');
  }
  if (privateFile && (stat.mode & 0o077) !== 0) {
    fail(name, 'must not grant group or other permissions');
  }
  return value;
};
const identifier = (value) =>
  typeof value === 'string' &&
  value === value.trim() &&
  /^[a-z0-9][a-z0-9._-]{1,127}$/i.test(value);
const placeholder = /(change[-_]?me|placeholder|example|your[-_]|default[-_]?secret|insecure|dummy)/i;
const credentialToken = (value) =>
  typeof value === 'string' &&
  value === value.trim() &&
  value.length >= 32 &&
  !placeholder.test(value);
const exactKeys = (entry, expected) =>
  entry &&
  typeof entry === 'object' &&
  !Array.isArray(entry) &&
  Object.keys(entry).sort().join(',') === [...expected].sort().join(',');
const parseJsonFile = (name, maximumBytes = 1024 * 1024) => {
  const bytes = readBounded(name, maximumBytes, true);
  try {
    return JSON.parse(bytes.toString('utf8'));
  } catch {
    fail(name, 'is malformed');
  }
};

const registryBytes = readBounded(
  'AUTH_GRPC_SERVICE_CREDENTIALS_FILE',
  1024 * 1024,
  true,
);
let registry;
try {
  registry = JSON.parse(registryBytes.toString('utf8'));
} catch {
  fail('AUTH_GRPC_SERVICE_CREDENTIALS_FILE', 'is malformed');
}
const allowedScopes = new Set(['auth:token:validate', 'auth:user:read']);
const ids = new Set();
const tokens = new Set();
if (!Array.isArray(registry) || registry.length === 0) {
  fail('AUTH_GRPC_SERVICE_CREDENTIALS_FILE', 'has no credentials');
}
for (const entry of registry) {
  const keys =
    entry && typeof entry === 'object' && !Array.isArray(entry)
      ? Object.keys(entry).sort()
      : [];
  const scopes = entry?.scopes;
  if (
    keys.join(',') !== 'audience,credentialId,principal,scopes,token' ||
    !identifier(entry.credentialId) ||
    !identifier(entry.principal) ||
    entry.audience !== 'auth-core' ||
    !credentialToken(entry.token) ||
    !Array.isArray(scopes) ||
    scopes.length === 0 ||
    scopes.some((scope) => !allowedScopes.has(scope)) ||
    new Set(scopes).size !== scopes.length ||
    ids.has(entry.credentialId) ||
    tokens.has(entry.token)
  ) {
    fail('AUTH_GRPC_SERVICE_CREDENTIALS_FILE', 'contains an invalid policy');
  }
  ids.add(entry.credentialId);
  tokens.add(entry.token);
}
const hasCurrent = (principal, tokenName, requiredScope) => {
  const token = process.env[tokenName] || '';
  return registry.some(
    (entry) =>
      entry.principal === principal &&
      entry.token === token &&
      entry.scopes.includes(requiredScope),
  );
};
if (!hasCurrent('velion-gateway', 'GATEWAY_AUTH_GRPC_SERVICE_TOKEN', 'auth:user:read')) {
  fail('AUTH_GRPC_SERVICE_CREDENTIALS_FILE', 'omits the current gateway credential');
}
if (!hasCurrent('retrieval-engine', 'RETRIEVAL_AUTH_GRPC_SERVICE_TOKEN', 'auth:token:validate')) {
  fail('AUTH_GRPC_SERVICE_CREDENTIALS_FILE', 'omits the current retrieval credential');
}

const internalRegistryName = 'AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE';
const internalRegistry = parseJsonFile(internalRegistryName);
const allowedInternalScopes = new Set([
  'oauth:token:read',
  'oauth:token:refresh',
  'agent:provision',
  'nats:authenticate',
  'auth:admin',
]);
const internalIds = new Set();
const internalTokens = new Set();
if (!Array.isArray(internalRegistry) || internalRegistry.length === 0) {
  fail(internalRegistryName, 'has no credentials');
}
for (const entry of internalRegistry) {
  const scopes = entry?.scopes;
  if (
    !exactKeys(entry, [
      'credentialId',
      'principal',
      'audience',
      'token',
      'scopes',
    ]) ||
    !identifier(entry.credentialId) ||
    !identifier(entry.principal) ||
    entry.audience !== 'auth-core-internal' ||
    !credentialToken(entry.token) ||
    !Array.isArray(scopes) ||
    scopes.length === 0 ||
    scopes.some((scope) => !allowedInternalScopes.has(scope)) ||
    new Set(scopes).size !== scopes.length ||
    internalIds.has(entry.credentialId) ||
    internalTokens.has(entry.token)
  ) {
    fail(internalRegistryName, 'contains an invalid policy');
  }
  internalIds.add(entry.credentialId);
  internalTokens.add(entry.token);
}
const findCurrentInternal = (principal, tokenName, requiredScopes) => {
  const token = process.env[tokenName] || '';
  return internalRegistry.find(
    (entry) =>
      entry.principal === principal &&
      entry.token === token &&
      requiredScopes.every((scope) => entry.scopes.includes(scope)),
  );
};
const currentUserInternal = findCurrentInternal(
  'user-core',
  'USER_AUTH_INTERNAL_SERVICE_TOKEN',
  [
    'oauth:token:read',
    'oauth:token:refresh',
    'nats:authenticate',
    'auth:admin',
  ],
);
if (!currentUserInternal) {
  fail(internalRegistryName, 'omits the current User Core policy');
}
if (
  !findCurrentInternal(
    'quarry-control',
    'QUARRY_AUTH_INTERNAL_SERVICE_TOKEN',
    ['agent:provision'],
  )
) {
  fail(internalRegistryName, 'omits the current Quarry policy');
}

const parseClientCredential = (name, audience, principal, tokenName) => {
  const entry = parseJsonFile(name, 64 * 1024);
  if (
    !exactKeys(entry, ['credentialId', 'principal', 'audience', 'token']) ||
    !identifier(entry.credentialId) ||
    entry.principal !== principal ||
    entry.audience !== audience ||
    !credentialToken(entry.token) ||
    entry.token !== (process.env[tokenName] || '')
  ) {
    fail(name, 'contains an invalid current client policy');
  }
  return entry;
};

const userAuthClientName = 'USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE';
const userAuthClient = parseClientCredential(
  userAuthClientName,
  'auth-core-internal',
  'user-core',
  'USER_AUTH_INTERNAL_SERVICE_TOKEN',
);
if (
  currentUserInternal.credentialId !== userAuthClient.credentialId ||
  currentUserInternal.principal !== userAuthClient.principal ||
  currentUserInternal.token !== userAuthClient.token
) {
  fail(userAuthClientName, 'does not match the Auth internal registry');
}

const userGrpcClientName = 'USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE';
const userGrpcClient = parseClientCredential(
  userGrpcClientName,
  'user-core-grpc',
  'auth-core',
  'AUTH_USER_CORE_GRPC_SERVICE_TOKEN',
);
const userGrpcRegistryName = 'USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE';
const userGrpcRegistry = parseJsonFile(userGrpcRegistryName);
const userServiceMethods = [
  'CreateUser',
  'GetUser',
  'GetUserByEmail',
  'UpdateUser',
  'DeleteUser',
  'ListUsers',
  'ActivateUser',
  'DeactivateUser',
  'BlockUser',
  'UnblockUser',
  'SuspendUser',
  'UnsuspendUser',
  'GetUserProfile',
  'UpdateUserProfile',
  'CreateSession',
  'ListSessions',
  'InvalidateSession',
  'InvalidateAllSessions',
  'LogActivity',
  'ListActivities',
  'AssignRole',
  'ListUserRoles',
  'RemoveRole',
  'RegisterDevice',
  'ListDevices',
  'UpdateDevice',
  'DeactivateDevice',
  'HealthCheck',
].map((method) => `/user.v1.UserService/${method}`);
const documentAccessMethods = [
  'GrantDocumentAccess',
  'RevokeDocumentAccess',
  'GetDocumentAccess',
  'ListDocumentAccess',
  'CheckDocumentAccess',
].map((method) => `/user.v1.DocumentAccessService/${method}`);
const allowedUserGrpcMethods = new Set([
  ...userServiceMethods,
  ...documentAccessMethods,
]);
const requiredAuthUserMethods = [
  'CreateUser',
  'UpdateUser',
  'GetUserByEmail',
  'CreateSession',
  'GetUser',
  'DeleteUser',
  'HealthCheck',
].map((method) => `/user.v1.UserService/${method}`);
const userGrpcIds = new Set();
const userGrpcTokens = new Set();
if (!Array.isArray(userGrpcRegistry) || userGrpcRegistry.length === 0) {
  fail(userGrpcRegistryName, 'has no credentials');
}
for (const entry of userGrpcRegistry) {
  const methods = entry?.methods;
  if (
    !exactKeys(entry, [
      'credentialId',
      'principal',
      'audience',
      'token',
      'methods',
    ]) ||
    !identifier(entry.credentialId) ||
    !identifier(entry.principal) ||
    entry.audience !== 'user-core-grpc' ||
    !credentialToken(entry.token) ||
    !Array.isArray(methods) ||
    methods.length === 0 ||
    methods.some((method) => !allowedUserGrpcMethods.has(method)) ||
    new Set(methods).size !== methods.length ||
    userGrpcIds.has(entry.credentialId) ||
    userGrpcTokens.has(entry.token)
  ) {
    fail(userGrpcRegistryName, 'contains an invalid policy');
  }
  userGrpcIds.add(entry.credentialId);
  userGrpcTokens.add(entry.token);
}
const currentAuthUserGrpc = userGrpcRegistry.find(
  (entry) =>
    entry.credentialId === userGrpcClient.credentialId &&
    entry.principal === userGrpcClient.principal &&
    entry.token === userGrpcClient.token,
);
if (
  !currentAuthUserGrpc ||
  !requiredAuthUserMethods.every((method) =>
    currentAuthUserGrpc.methods.includes(method),
  )
) {
  fail(userGrpcRegistryName, 'does not authorize the current Auth client');
}

const parseCertificateFile = (name) => {
  const bytes = readBounded(name, 64 * 1024);
  try {
    return new X509Certificate(bytes);
  } catch {
    fail(name, 'is not a valid X.509 certificate');
  }
};
const userGrpcCAName = 'USER_CORE_GRPC_TLS_CA_FILE';
const userGrpcCertificateName = 'USER_CORE_GRPC_TLS_CERT_FILE';
const userGrpcKeyName = 'USER_CORE_GRPC_TLS_KEY_FILE';
const userGrpcCA = parseCertificateFile(userGrpcCAName);
const userGrpcCertificate = parseCertificateFile(userGrpcCertificateName);
const userGrpcKeyBytes = readBounded(userGrpcKeyName, 64 * 1024, true);
let userGrpcPrivateKey;
try {
  userGrpcPrivateKey = createPrivateKey(userGrpcKeyBytes);
} catch {
  fail(userGrpcKeyName, 'is not a valid private key');
}
if (
  userGrpcPrivateKey.asymmetricKeyType !== 'rsa' ||
  (userGrpcPrivateKey.asymmetricKeyDetails?.modulusLength || 0) < 2048
) {
  fail(userGrpcKeyName, 'is not an RSA key of at least 2048 bits');
}
const userGrpcDerivedPublic = createPublicKey(userGrpcPrivateKey).export({
  type: 'spki',
  format: 'der',
});
const userGrpcCertificatePublic = userGrpcCertificate.publicKey.export({
  type: 'spki',
  format: 'der',
});
if (!userGrpcDerivedPublic.equals(userGrpcCertificatePublic)) {
  fail(userGrpcKeyName, 'does not match the configured server certificate');
}
if (!userGrpcCA.ca) {
  fail(userGrpcCAName, 'is not a certificate authority');
}
if (userGrpcCertificate.ca || !userGrpcCertificate.verify(userGrpcCA.publicKey)) {
  fail(userGrpcCertificateName, 'is not a server certificate signed by the configured CA');
}
const now = Date.now();
if (
  !Number.isFinite(Date.parse(userGrpcCA.validFrom)) ||
  !Number.isFinite(Date.parse(userGrpcCA.validTo)) ||
  Date.parse(userGrpcCA.validFrom) > now ||
  Date.parse(userGrpcCA.validTo) <= now
) {
  fail(userGrpcCAName, 'is not currently valid');
}
if (
  !Number.isFinite(Date.parse(userGrpcCertificate.validFrom)) ||
  !Number.isFinite(Date.parse(userGrpcCertificate.validTo)) ||
  Date.parse(userGrpcCertificate.validFrom) > now ||
  Date.parse(userGrpcCertificate.validTo) <= now
) {
  fail(userGrpcCertificateName, 'is not currently valid');
}
if (userGrpcCertificate.checkHost('user-core') !== 'user-core') {
  fail(userGrpcCertificateName, 'does not authorize the user-core DNS name');
}
if (
  !userGrpcCertificate.keyUsage?.includes('1.3.6.1.5.5.7.3.1')
) {
  fail(userGrpcCertificateName, 'is not authorized for TLS server use');
}

const privateBytes = readBounded(
  'CONVEX_AUTH_PRIVATE_KEY_FILE',
  64 * 1024,
  true,
);
const publicBytes = readBounded('CONVEX_AUTH_PUBLIC_KEY_FILE', 64 * 1024);
let privateKey;
let publicKey;
try {
  privateKey = createPrivateKey(privateBytes);
  publicKey = createPublicKey(publicBytes);
} catch {
  fail('CONVEX_AUTH_PRIVATE_KEY_FILE', 'or public key is not a valid key');
}
if (
  privateKey.asymmetricKeyType !== 'rsa' ||
  (privateKey.asymmetricKeyDetails?.modulusLength || 0) < 2048
) {
  fail('CONVEX_AUTH_PRIVATE_KEY_FILE', 'is not an RSA key of at least 2048 bits');
}
const derived = createPublicKey(privateKey).export({ type: 'spki', format: 'der' });
const configured = publicKey.export({ type: 'spki', format: 'der' });
if (!derived.equals(configured)) {
  fail('CONVEX_AUTH_PRIVATE_KEY_FILE', 'does not match the configured public key');
}
NODE
then
  failed=1
fi

if (( failed != 0 )); then
  exit 1
fi
printf 'credential preflight: %d distinct scoped credentials validated\n' \
  "${#credential_names[@]}"
