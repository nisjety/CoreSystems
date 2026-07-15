import {
  DiscardPolicy,
  RetentionPolicy,
  StorageType,
  connect,
  nanos,
} from "nats";
import {
  createHash,
  createHmac,
  createPublicKey,
  createVerify,
  randomBytes,
} from "node:crypto";
import { chmodSync, readFileSync, writeFileSync } from "node:fs";

const command = process.argv[2];
const auditStream = "CONTROL_AUTHORITY_AUDIT";
const auditConfig = {
  name: auditStream,
  subjects: ["velion.audit.v2.control.auth-core.>"],
  retention: RetentionPolicy.Limits,
  storage: StorageType.File,
  discard: DiscardPolicy.Old,
  max_msgs: 10_000,
  max_bytes: 8 * 1024 * 1024,
  max_age: nanos(60 * 60 * 1_000),
  max_msg_size: 64 * 1024,
  num_replicas: 1,
};

async function provisionAuditStream() {
  const connection = await connect({
    servers: required("NATS_URL"),
    token: required("NATS_TOKEN"),
    name: "real-authority-audit-provisioner",
    maxReconnectAttempts: 5,
  });
  try {
    const manager = await connection.jetstreamManager();
    let info;
    try {
      info = await manager.streams.info(auditStream);
    } catch (error) {
      if (!isNotFound(error)) throw error;
      info = await manager.streams.add(auditConfig);
    }
    assertBoundedAuditConfig(info.config);
  } finally {
    await connection.drain();
  }
}

async function verifyAuthorityFlow() {
  const authBase = required("AUTH_BASE_URL").replace(/\/+$/, "");
  const userBase = required("USER_BASE_URL").replace(/\/+$/, "");
  const issuer = required("AUTH_CORE_ISSUER");
  const runId = required("RUN_ID").toLowerCase();
  const publicPem = readFileSync("/fixtures/public.pem", "utf8");

  await waitFor(`${authBase}/api/convex-auth/jwks`);
  await waitFor(`${userBase}/health`);
  await verifyAuditStream();
  const jwks = await requestJson(`${authBase}/api/convex-auth/jwks`, {
    expected: [200],
    label: "Auth JWKS",
  });
  assertJwksMatchesPublicKey(jwks.data, publicPem);

  const users = [];
  for (let index = 1; index <= 2; index += 1) {
    users.push(
      await createUserAndOrganization({
        authBase,
        issuer,
        publicPem,
        runId,
        index,
      }),
    );
  }
  if (
    users.length !== 2 ||
    users[0].orgId === users[1].orgId ||
    users[0].userId === users[1].userId
  ) {
    throw new Error("two isolated users and organizations were not created");
  }

  for (const user of users) {
    await verifyMembership(authBase, user.userId, user.orgId, true);
    const sessionContext = await requestJson(
      `${userBase}/api/v1/me/session-context`,
      {
        headers: {
          Authorization: `Bearer ${user.sessionToken}`,
          "X-Org-Id": user.orgId,
        },
        expected: [200],
        label: "User session context",
      },
    );
    if (
      sessionContext.data?.userId !== user.userId ||
      sessionContext.data?.orgId !== user.orgId ||
      sessionContext.data?.role !== "owner"
    ) {
      throw new Error(
        "User Core session context did not match canonical membership",
      );
    }

    await requestJson(`${userBase}/api/v1/users/onboarding/complete`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${user.sessionToken}`,
        "X-Org-Id": user.orgId,
      },
      expected: [200],
      label: "User Core onboarding completion",
    });
    const completedSessionContext = await requestJson(
      `${userBase}/api/v1/me/session-context`,
      {
        headers: {
          Authorization: `Bearer ${user.sessionToken}`,
          "X-Org-Id": user.orgId,
        },
        expected: [200],
        label: "completed User session context",
      },
    );
    if (
      completedSessionContext.data?.userId !== user.userId ||
      completedSessionContext.data?.orgId !== user.orgId ||
      completedSessionContext.data?.role !== "owner" ||
      completedSessionContext.data?.onboardingStatus !== "COMPLETED"
    ) {
      throw new Error(
        "User Core session context did not retain canonical completed onboarding",
      );
    }
  }

  await verifyMembership(authBase, users[0].userId, users[1].orgId, false);
  await verifyVisibilityFacade(userBase, users[0], users[1].orgId);
  await verifyControlPolicy(authBase, users);
  writeFixtureOutput(users);

  process.stdout.write("PASS: real Auth/User/Control authority flow\n");
}

async function createUserAndOrganization({
  authBase,
  issuer,
  publicPem,
  runId,
  index,
}) {
  const email = `authority-${runId}-${index}@example.test`;
  const password = `A!${randomBytes(24).toString("base64url")}`;
  const name = `Authority Fixture ${index}`;
  const jar = new CookieJar();

  const signup = await requestJson(`${authBase}/api/auth/sign-up/email`, {
    method: "POST",
    body: { email, password, name },
    jar,
    expected: [200, 201],
    label: "supported signup",
  });
  const userId = findString(signup.data, ["user.id", "data.user.id"]);
  if (!userId) throw new Error("supported signup omitted the user identity");

  const signin = await requestJson(`${authBase}/api/auth/sign-in/email`, {
    method: "POST",
    body: { email, password },
    jar,
    expected: [200],
    label: "supported signin",
  });
  const sessionToken = bearerSessionToken(signin);
  if (!sessionToken)
    throw new Error("supported signin omitted the Better Auth bearer");

  const slug = `authority-${runId}-${index}`.slice(0, 60);
  const created = await requestJson(
    `${authBase}/api/auth/organization/create`,
    {
      method: "POST",
      headers: { Authorization: `Bearer ${sessionToken}` },
      body: { name: `Authority Org ${index}`, slug },
      jar,
      expected: [200, 201],
      label: "organization create",
    },
  );
  const orgId = findString(created.data, [
    "organization.id",
    "data.organization.id",
    "id",
    "data.id",
  ]);
  if (!orgId)
    throw new Error("organization create omitted the organization identity");

  await requestJson(`${authBase}/api/auth/organization/set-active`, {
    method: "POST",
    headers: { Authorization: `Bearer ${sessionToken}` },
    body: { organizationId: orgId },
    jar,
    expected: [200],
    label: "organization set-active",
  });
  const session = await requestJson(`${authBase}/api/auth/get-session`, {
    headers: { Authorization: `Bearer ${sessionToken}` },
    jar,
    expected: [200],
    label: "active Auth session",
  });
  if (
    findString(session.data, ["user.id"]) !== userId ||
    findString(session.data, ["session.activeOrganizationId"]) !== orgId
  ) {
    throw new Error(
      "active Auth session did not retain the selected organization",
    );
  }

  const plane = await requestJson(`${authBase}/api/data-plane/token`, {
    headers: { Authorization: `Bearer ${sessionToken}` },
    jar,
    expected: [200],
    label: "Data Plane token issuance",
  });
  const dataPlaneToken = findString(plane.data, ["token"]);
  if (!dataPlaneToken)
    throw new Error("Data Plane token issuance omitted its bearer");
  const claims = verifyJwt(dataPlaneToken, publicPem);
  if (
    claims.iss !== issuer ||
    claims.aud !== "data-plane" ||
    claims.sub !== userId ||
    claims.user_id !== userId ||
    claims.org_id !== orgId ||
    claims.principal_type !== "user" ||
    claims.zdr !== true
  ) {
    throw new Error(
      "Data Plane token claims did not pin the canonical user, tenant, audience, and ZDR posture",
    );
  }

  const persistentPlane = await requestWhenDurableAuditReady(
    `${authBase}/api/data-plane/internal-token`,
    {
      method: "POST",
      headers: {
        "X-Service-Id": required("PERSISTENCE_SERVICE_ID"),
        "X-Service-Api-Key": required("PERSISTENCE_SERVICE_API_KEY"),
      },
      body: {
        orgId,
        scopes: ["documents:write", "org:data:write_all"],
        reason: "seed disposable real-authority browser fixture",
        zdr: false,
      },
      expected: [200, 201],
      label: "audited persistent Data Plane service token issuance",
    },
  );
  const persistentDataPlaneToken = findString(persistentPlane.data, ["token"]);
  if (!persistentDataPlaneToken)
    throw new Error("persistent Data Plane token issuance omitted its bearer");
  const persistentClaims = verifyJwt(persistentDataPlaneToken, publicPem);
  assertExactScopes(persistentClaims.scopes, [
    "documents:write",
    "org:data:write_all",
  ]);
  if (
    persistentClaims.iss !== issuer ||
    persistentClaims.aud !== "data-plane" ||
    persistentClaims.sub !== `service:${required("PERSISTENCE_SERVICE_ID")}` ||
    persistentClaims.org_id !== orgId ||
    persistentClaims.principal_type !== "service" ||
    persistentClaims.zdr !== false ||
    "user_id" in persistentClaims
  ) {
    throw new Error(
      "persistent Data Plane token did not retain its bounded audited service policy",
    );
  }
  await verifyDurableTokenAudit(persistentDataPlaneToken);

  return {
    email,
    password,
    name,
    userId,
    orgId,
    sessionToken,
    dataPlaneToken,
    persistentDataPlaneToken,
  };
}

function writeFixtureOutput(users) {
  const path = (process.env.FIXTURE_OUTPUT_FILE ?? "").trim();
  if (!path) return;
  const browserUsers = users.map(
    ({
      email,
      password,
      name,
      userId,
      orgId,
      dataPlaneToken,
      persistentDataPlaneToken,
    }) => ({
      email,
      password,
      name,
      userId,
      orgId,
      dataPlaneToken,
      persistentDataPlaneToken,
    }),
  );
  writeFileSync(path, `${JSON.stringify({ users: browserUsers })}\n`, {
    encoding: "utf8",
    mode: 0o600,
  });
  chmodSync(path, 0o600);
}

async function verifyMembership(authBase, userId, orgId, expectedMember) {
  const response = await requestJson(
    `${authBase}/api/v1/internal/membership/decision`,
    {
      method: "POST",
      headers: {
        "X-User-Core-Membership-Token": required(
          "USER_CORE_MEMBERSHIP_SERVICE_TOKEN",
        ),
      },
      body: { userId, orgId },
      expected: [200],
      label: "canonical membership decision",
    },
  );
  if (
    response.data?.version !== "v1" ||
    response.data?.member !== expectedMember
  ) {
    throw new Error("canonical membership decision returned the wrong result");
  }
  if (expectedMember && response.data?.role !== "owner") {
    throw new Error("canonical membership decision omitted the owner role");
  }
  if (!expectedMember && response.data?.role !== null) {
    throw new Error("canonical membership denial leaked a role");
  }
}

async function verifyVisibilityFacade(userBase, user, otherOrgId) {
  const ownUri = `/api/v1/internal/authz/visible?org_id=${encodeURIComponent(user.orgId)}&subject_id=${encodeURIComponent(user.userId)}&resource_type=document`;
  const ownHeaders = signedVisibilityHeaders(ownUri, user.userId, user.orgId);
  const visible = await requestJson(`${userBase}${ownUri}`, {
    headers: { ...ownHeaders, Authorization: `Bearer ${user.dataPlaneToken}` },
    expected: [200],
    label: "visibility facade allow",
  });
  if (
    !Array.isArray(visible.data?.ids) ||
    visible.data.ids.length !== 0 ||
    visible.data?.all_org !== false
  ) {
    throw new Error(
      "visibility facade returned an invalid empty-grant decision",
    );
  }

  const spoofedUri = `/api/v1/internal/authz/visible?org_id=${encodeURIComponent(otherOrgId)}&subject_id=${encodeURIComponent(user.userId)}&resource_type=document`;
  const spoofedHeaders = signedVisibilityHeaders(
    spoofedUri,
    user.userId,
    otherOrgId,
  );
  const mismatch = await requestJson(`${userBase}${spoofedUri}`, {
    headers: {
      ...spoofedHeaders,
      Authorization: `Bearer ${user.dataPlaneToken}`,
    },
    expected: [403],
    label: "visibility tenant mismatch",
  });
  if (mismatch.status !== 403)
    throw new Error("visibility tenant mismatch was not rejected");
}

function signedVisibilityHeaders(uri, userId, orgId) {
  const timestamp = new Date().toISOString();
  const nonce = randomBytes(18).toString("base64url");
  const actualBodyDigest = createHash("sha256").update("").digest("base64url");
  const values = [
    "v2",
    "retrieval-engine",
    "user-core",
    timestamp,
    "GET",
    uri,
    userId,
    orgId,
    "authz:visible",
    "document",
    "",
    "resolve explicit grants",
    "true",
    nonce,
    actualBodyDigest,
  ];
  const signature = createHmac("sha256", required("USER_CORE_RETRIEVAL_TOKEN"))
    .update(values.join("\n"))
    .digest("base64url");
  return {
    "X-Service-Id": "retrieval-engine",
    "X-Service-Token": required("USER_CORE_RETRIEVAL_TOKEN"),
    "X-User-Id": userId,
    "X-Org-Id": orgId,
    "X-Delegation-Version": "v2",
    "X-Delegation-Timestamp": timestamp,
    "X-Delegation-Body-SHA256": actualBodyDigest,
    "X-Delegation-Operation": "authz:visible",
    "X-Delegation-Resource-Type": "document",
    "X-Delegation-Resource-Id": "",
    "X-Delegation-Reason": "resolve explicit grants",
    "X-Delegation-ZDR": "true",
    "X-Delegation-Nonce": nonce,
    "X-Delegation-Signature": signature,
  };
}

async function verifyControlPolicy(authBase, users) {
  const noAuth = await requestJson(
    `${authBase}/api/v1/internal/authorization/data-plane/decision`,
    {
      method: "POST",
      body: {
        userId: users[0].userId,
        orgId: users[0].orgId,
        action: "data.read",
      },
      expected: [401],
      label: "unauthenticated control-policy decision",
    },
  );
  if (noAuth.status !== 401)
    throw new Error("unauthenticated control-policy request was not rejected");

  const ownPolicyToken = await mintPolicyToken(authBase, users[0].orgId);
  const ownDecision = await policyDecision(
    authBase,
    ownPolicyToken,
    users[0].userId,
    users[0].orgId,
    "data.read",
    [200, 201],
  );
  if (
    ownDecision.data?.version !== "v1" ||
    ownDecision.data?.allowed !== true ||
    ownDecision.data?.reason !== "member"
  ) {
    throw new Error("control-policy member allow decision was invalid");
  }
  const adminDecision = await policyDecision(
    authBase,
    ownPolicyToken,
    users[0].userId,
    users[0].orgId,
    "data.admin",
    [200, 201],
  );
  if (
    adminDecision.data?.allowed !== true ||
    !adminDecision.data?.permissions?.includes("data:admin")
  ) {
    throw new Error("control-policy owner admin decision was invalid");
  }

  const mismatch = await policyDecision(
    authBase,
    ownPolicyToken,
    users[0].userId,
    users[1].orgId,
    "data.read",
    [403],
  );
  if (mismatch.status !== 403)
    throw new Error("cross-tenant policy request was not rejected");

  const otherPolicyToken = await mintPolicyToken(authBase, users[1].orgId);
  const denied = await policyDecision(
    authBase,
    otherPolicyToken,
    users[0].userId,
    users[1].orgId,
    "data.read",
    [200, 201],
  );
  if (
    denied.data?.allowed !== false ||
    denied.data?.reason !== "not_member" ||
    denied.data?.role !== null
  ) {
    throw new Error("control-policy non-member denial was invalid");
  }
}

async function mintPolicyToken(authBase, orgId) {
  const response = await requestJson(
    `${authBase}/api/control-policy/internal-token`,
    {
      method: "POST",
      headers: {
        "X-Service-Id": "retrieval-engine",
        "X-Service-Api-Key": required("CONTROL_POLICY_SERVICE_API_KEY"),
      },
      body: {
        orgId,
        scopes: ["data:authorization:decide"],
        reason: "verify isolated retrieval authorization",
      },
      expected: [200, 201],
      label: "control-policy service-token issuance",
    },
  );
  const token = findString(response.data, ["token"]);
  if (!token)
    throw new Error("control-policy service-token issuance omitted its bearer");
  return token;
}

function policyDecision(authBase, token, userId, orgId, action, expected) {
  return requestJson(
    `${authBase}/api/v1/internal/authorization/data-plane/decision`,
    {
      method: "POST",
      headers: { Authorization: `Bearer ${token}` },
      body: { userId, orgId, action },
      expected,
      label: "control-policy decision",
    },
  );
}

async function verifyAuditStream() {
  const connection = await connect({
    servers: required("NATS_URL"),
    token: required("NATS_TOKEN"),
    name: "real-authority-audit-verifier",
    maxReconnectAttempts: 5,
  });
  try {
    const manager = await connection.jetstreamManager();
    const info = await manager.streams.info(auditStream);
    assertBoundedAuditConfig(info.config);
  } finally {
    await connection.drain();
  }
}

function assertBoundedAuditConfig(config) {
  if (
    config?.name !== auditStream ||
    config?.subjects?.length !== 1 ||
    config.subjects[0] !== "velion.audit.v2.control.auth-core.>" ||
    config.max_msgs !== auditConfig.max_msgs ||
    config.max_bytes !== auditConfig.max_bytes ||
    config.max_age !== auditConfig.max_age ||
    config.storage !== auditConfig.storage
  ) {
    throw new Error("bounded durable audit stream contract is not active");
  }
}

function assertJwksMatchesPublicKey(value, publicPem) {
  const mounted = createPublicKey(publicPem).export({ format: "jwk" });
  const keys = Array.isArray(value?.keys) ? value.keys : [];
  if (
    !keys.some(
      (key) =>
        key?.kty === "RSA" &&
        key?.n === mounted.n &&
        key?.e === mounted.e &&
        key?.alg === "RS256" &&
        key?.use === "sig",
    )
  ) {
    throw new Error("JWKS does not match the mounted Auth public key");
  }
}

function verifyJwt(token, publicPem) {
  const parts = token.split(".");
  if (parts.length !== 3) throw new Error("issued bearer is not a JWT");
  const header = JSON.parse(Buffer.from(parts[0], "base64url").toString("utf8"));
  if (header.alg !== "RS256" || header.typ !== "JWT") {
    throw new Error("issued JWT did not use the reviewed JOSE contract");
  }
  const verifier = createVerify("RSA-SHA256");
  verifier.update(`${parts[0]}.${parts[1]}`);
  verifier.end();
  if (!verifier.verify(publicPem, Buffer.from(parts[2], "base64url"))) {
    throw new Error("issued JWT signature did not match mounted Auth key");
  }
  const claims = JSON.parse(
    Buffer.from(parts[1], "base64url").toString("utf8"),
  );
  const now = Math.floor(Date.now() / 1_000);
  if (
    !Number.isSafeInteger(claims.iat) ||
    !Number.isSafeInteger(claims.exp) ||
    claims.iat > now + 30 ||
    claims.exp < now - 30 ||
    claims.exp <= claims.iat ||
    claims.exp - claims.iat > 15 * 60 ||
    (claims.nbf !== undefined &&
      (!Number.isSafeInteger(claims.nbf) || claims.nbf > now + 30))
  ) {
    throw new Error("issued JWT temporal claims were invalid or unbounded");
  }
  return claims;
}

function assertExactScopes(actual, expected) {
  if (
    !Array.isArray(actual) ||
    actual.length !== expected.length ||
    new Set(actual).size !== expected.length ||
    expected.some((scope) => !actual.includes(scope))
  ) {
    throw new Error("issued service token scopes exceeded the requested policy");
  }
}

async function requestJson(url, options = {}) {
  const headers = new Headers(options.headers ?? {});
  headers.set("Accept", "application/json");
  if (options.body !== undefined) {
    headers.set("Content-Type", "application/json");
    headers.set("Origin", "http://localhost:3011");
  }
  if (options.jar) {
    const cookie = options.jar.header();
    if (cookie) headers.set("Cookie", cookie);
  }
  const response = await fetch(url, {
    method: options.method ?? "GET",
    headers,
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
    redirect: "manual",
    signal: options.signal ?? AbortSignal.timeout(10_000),
  });
  options.jar?.capture(response.headers);
  const text = await response.text();
  let data = null;
  if (text.trim() !== "") {
    try {
      data = JSON.parse(text);
    } catch {
      data = null;
    }
  }
  const expected = options.expected ?? [200];
  if (!expected.includes(response.status)) {
    throw new Error(
      `${options.label ?? "request"} returned HTTP ${response.status}`,
    );
  }
  return { status: response.status, data, headers: response.headers };
}

async function requestWhenDurableAuditReady(url, options) {
  for (let attempt = 0; attempt < 30; attempt += 1) {
    const response = await requestJson(url, {
      ...options,
      expected: [200, 201, 503],
    });
    if (response.status !== 503) return response;
    await new Promise((resolve) => setTimeout(resolve, 1_000));
  }
  throw new Error(
    `${options.label ?? "durably audited request"} remained unavailable`,
  );
}

async function verifyDurableTokenAudit(token) {
  const subject = "velion.audit.v2.control.auth-core.plane_service_token_issued";
  const expectedEventId = `plane-token:${createHash("sha256")
    .update(token)
    .digest("hex")}`;
  const connection = await connect({
    servers: required("NATS_URL"),
    token: required("NATS_TOKEN"),
    name: "real-authority-token-audit-verifier",
    maxReconnectAttempts: 5,
    timeout: 5_000,
  });
  try {
    const manager = await connection.jetstreamManager();
    const message = await manager.streams.getMessage(auditStream, {
      last_by_subj: subject,
    });
    const payload = JSON.parse(new TextDecoder().decode(message.data));
    if (
      payload.event_id !== expectedEventId ||
      payload.event !== "plane_service_token_issued" ||
      payload.producer !== "auth-core" ||
      payload.plane !== "control"
    ) {
      throw new Error("durable service-token audit identity was not observed");
    }
  } finally {
    await connection.drain();
  }
}

async function waitFor(url) {
  for (let attempt = 0; attempt < 120; attempt += 1) {
    try {
      const response = await fetch(url, { redirect: "manual" });
      if (response.ok) return;
    } catch {
      // Service startup is expected to race the probe in an isolated stack.
    }
    await new Promise((resolve) => setTimeout(resolve, 1_000));
  }
  throw new Error(`service readiness timed out for ${new URL(url).pathname}`);
}

class CookieJar {
  #cookies = new Map();

  capture(headers) {
    const values =
      typeof headers.getSetCookie === "function"
        ? headers.getSetCookie()
        : [headers.get("set-cookie")].filter(Boolean);
    for (const value of values) {
      const first = value.split(";", 1)[0];
      const separator = first.indexOf("=");
      if (separator > 0)
        this.#cookies.set(
          first.slice(0, separator),
          first.slice(separator + 1),
        );
    }
  }

  header() {
    return [...this.#cookies.entries()]
      .map(([name, value]) => `${name}=${value}`)
      .join("; ");
  }
}

function bearerSessionToken(response) {
  const header = (response.headers.get("set-auth-token") ?? "").trim();
  const token = header || findString(response.data, ["token", "data.token"]);
  return token?.replace(/^Bearer\s+/i, "") ?? "";
}

function findString(value, paths) {
  for (const path of paths) {
    let current = value;
    for (const key of path.split(".")) current = current?.[key];
    if (typeof current === "string" && current.trim() !== "")
      return current.trim();
  }
  return "";
}

function required(name) {
  const value = (process.env[name] ?? "").trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function isNotFound(error) {
  return (
    String(error?.code ?? "") === "404" || error?.api_error?.err_code === 10059
  );
}

async function main() {
  try {
    if (command === "provision-audit") {
      await provisionAuditStream();
    } else if (command === "verify") {
      await verifyAuthorityFlow();
    } else {
      throw new Error(
        "usage: real-authority-fixture.mjs provision-audit|verify",
      );
    }
  } catch (error) {
    const message =
      error instanceof Error ? error.message : "unknown fixture failure";
    process.stderr.write(`FAIL: ${message}\n`);
    process.exitCode = 1;
  }
}

await main();
