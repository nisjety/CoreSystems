"use strict";

const assert = require("node:assert/strict");
const postgres = require("/app/node_modules/postgres");
const { createClient } = require("/app/node_modules/redis");

// Exercise Better Auth's real invitation handler without sending email from an
// isolated fixture. All non-Resend traffic still uses the platform fetch.
const platformFetch = globalThis.fetch;
globalThis.fetch = async (input, init) => {
  const rawURL =
    typeof input === "string" || input instanceof URL
      ? String(input)
      : input.url;
  const target = new URL(rawURL);
  if (
    target.protocol === "https:" &&
    target.hostname === "api.resend.com" &&
    target.pathname === "/emails"
  ) {
    return new Response(JSON.stringify({ id: "isolated-email-delivery" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  }
  return platformFetch(input, init);
};

const { sqlClient: authSQL } = require("/app/dist/src/db/index.js");
const {
  InvitationAcceptanceController,
} = require("/app/dist/src/auth/invitation-acceptance.controller.js");
const { auth: canonicalAuth } = require("/app/dist/src/auth/auth.js");
const {
  flushInvitationAcceptanceRepairs,
  PostgresInvitationAcceptanceRepairRepository,
} = require("/app/dist/src/auth/invitation-acceptance-repair.js");
const {
  flushOrganizationDeletionOutbox,
  flushOrganizationInvitationAuditOutbox,
  flushOrganizationMembershipAuditOutbox,
  flushOrganizationMembershipOutbox,
  flushOrganizationProjectionOutbox,
  setOrganizationEventPublisher,
} = require("/app/dist/src/auth/organization-events.plugin.js");

const organizationID = "org_container_lifecycle";
const ownerUserID = "user_container_owner";
const inviteeUserID = "user_container_invitee";
const leaverUserID = "user_container_leaver";
const ownerSessionToken = "owner-container-session-token";
const inviteeSessionToken = "invitee-container-session-token";
const leaverSessionToken = "leaver-container-session-token";
const projectionPublications = [];
const membershipPublications = [];
const deletionPublications = [];
const auditPublications = [];

// The outbox implementations and scoped HTTP sinks are real. This fixture
// supplies the second, broker-facing sink explicitly so a missing publisher
// can never be mistaken for successful convergence.
setOrganizationEventPublisher({
  async publishOrganizationProjection(_data, eventID) {
    projectionPublications.push(eventID);
  },
  async publishOrganizationMembershipProjection(_data, eventID) {
    membershipPublications.push(eventID);
  },
  async publishOrganizationDeletionProjection(_data, eventID) {
    deletionPublications.push(eventID);
  },
  async publishVerevonAudit(payload) {
    auditPublications.push({ ...payload });
  },
});

function requiredEnvironment(name) {
  const value = (process.env[name] || "").trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

const fixtureID = requiredEnvironment("CONTROL_LIFECYCLE_FIXTURE_ID");
const postgresHost = requiredEnvironment("CONTROL_LIFECYCLE_POSTGRES_HOST");
const orgURL = requiredEnvironment("ORG_SERVICE_URL").replace(/\/$/, "");
const billingURL = requiredEnvironment("BILLING_CORE_URL").replace(/\/$/, "");
const orgToken = requiredEnvironment("ORG_CORE_SERVICE_TOKEN");
const billingToken = requiredEnvironment("BILLING_CORE_SERVICE_TOKEN");
const billingWriterToken = requiredEnvironment(
  "CONTROL_LIFECYCLE_BILLING_WRITER_TOKEN",
);
const dragonfly = createClient({
  url: requiredEnvironment("DRAGONFLY_URL"),
});

const orgSQL = postgres(
  requiredEnvironment("CONTROL_LIFECYCLE_ORG_DATABASE_URL"),
  {
    max: 1,
    prepare: false,
  },
);
const billingSQL = postgres(
  requiredEnvironment("CONTROL_LIFECYCLE_BILLING_DATABASE_URL"),
  { max: 1, prepare: false },
);

function validateContainerTarget(databaseURL, expectedDatabase) {
  const parsed = new URL(databaseURL);
  assert.ok(
    ["postgres:", "postgresql:"].includes(parsed.protocol),
    "fixture target must be Postgres",
  );
  assert.match(
    postgresHost,
    /^control-mvp-ct-[0-9]+-[0-9]+-postgres$/,
    "fixture Postgres must use the runner-generated container name",
  );
  assert.equal(
    parsed.hostname,
    postgresHost,
    "fixture target host must equal the runner-created Postgres container",
  );
  assert.equal(
    decodeURIComponent(parsed.pathname).replace(/^\//, ""),
    expectedDatabase,
    "fixture target database mismatch",
  );
  assert.match(fixtureID, /^[0-9a-f]{48}$/, "fixture marker is invalid");
}

async function verifyMarker(sql, databaseURL, expectedDatabase) {
  validateContainerTarget(databaseURL, expectedDatabase);
  const rows = await sql`
    SELECT fixture_id
    FROM control_lifecycle_fixture
    WHERE fixture_id = ${fixtureID}
      AND database_name = ${expectedDatabase}
      AND current_database() = ${expectedDatabase}
  `;
  assert.equal(rows.length, 1, `missing ${expectedDatabase} fixture marker`);
  assert.equal(rows[0].fixture_id, fixtureID);
}

async function requestJSON(url, options, expectedStatus) {
  const response = await fetch(url, options);
  const body = await response.text();
  assert.equal(
    response.status,
    expectedStatus,
    `${options.method || "GET"} ${url} returned ${response.status}: ${body}`,
  );
  if (!body) return null;
  return JSON.parse(body);
}

async function canonicalAuthRequest(path, sessionToken, body, expectedStatus) {
  const response = await canonicalAuth.handler(
    new Request(`http://localhost:3011/api/auth${path}`, {
      method: "POST",
      headers: {
        authorization: `Bearer ${sessionToken}`,
        "content-type": "application/json",
        origin: "http://localhost:3000",
        "user-agent": "control-lifecycle-container",
      },
      body: JSON.stringify(body),
    }),
  );
  const responseBody = await response.text();
  assert.equal(
    response.status,
    expectedStatus,
    `POST ${path} returned ${response.status}: ${responseBody}`,
  );
  return responseBody ? JSON.parse(responseBody) : null;
}

async function seedCanonicalSession(token, userID, name, email) {
  if (!dragonfly.isOpen) await dragonfly.connect();
  const now = new Date();
  const expiresAt = new Date(now.getTime() + 60 * 60 * 1000);
  await dragonfly.setEx(
    token,
    3600,
    JSON.stringify({
      session: {
        id: `session-${userID}`,
        token,
        userId: userID,
        activeOrganizationId: organizationID,
        expiresAt: expiresAt.toISOString(),
        createdAt: now.toISOString(),
        updatedAt: now.toISOString(),
        ipAddress: "127.0.0.1",
        userAgent: "control-lifecycle-container",
      },
      user: {
        id: userID,
        name,
        email,
        emailVerified: true,
        image: null,
        createdAt: now.toISOString(),
        updatedAt: now.toISOString(),
      },
    }),
  );
}

function serviceHeaders(principal, token) {
  return {
    "content-type": "application/json",
    "x-service-id": principal,
    "x-service-token": token,
  };
}

function plainRows(rows) {
  return Array.from(rows, (row) => ({ ...row }));
}

async function flushUntilDurable({
  flush,
  isDurable,
  label,
  attempt = 0,
  published = 0,
}) {
  const flushed = await flush();
  assert.ok(
    Number.isInteger(flushed) && flushed >= 0,
    `${label} returned an invalid publication count`,
  );
  const nextPublished = published + flushed;
  if (await isDurable()) return nextPublished;
  if (attempt >= 49) {
    throw new Error(`${label} did not become durable within 5 seconds`);
  }
  await new Promise((resolve) => setTimeout(resolve, 100));
  return flushUntilDurable({
    flush,
    isDurable,
    label,
    attempt: attempt + 1,
    published: nextPublished,
  });
}

async function verifyAllMarkers() {
  await verifyMarker(
    authSQL,
    requiredEnvironment("DATABASE_URL"),
    "auth_service",
  );
  await verifyMarker(
    orgSQL,
    requiredEnvironment("CONTROL_LIFECYCLE_ORG_DATABASE_URL"),
    "org_core",
  );
  await verifyMarker(
    billingSQL,
    requiredEnvironment("CONTROL_LIFECYCLE_BILLING_DATABASE_URL"),
    "billing_service",
  );
}

async function seedRepairAndConverge() {
  await requestJSON(
    `${orgURL}/internal/orgs/${organizationID}/reconcile`,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        name: "unauthenticated",
        ownerUserId: ownerUserID,
        revision: 1,
      }),
    },
    401,
  );

  await authSQL.begin(async (tx) => {
    await tx`
      INSERT INTO "user" (
        id, name, email, email_verified, created_at, updated_at
      ) VALUES
        (${ownerUserID}, 'Lifecycle Owner', 'owner@container.invalid', TRUE, NOW(), NOW()),
        (${inviteeUserID}, 'Lifecycle Invitee', 'Invitee@Container.Invalid', TRUE, NOW(), NOW())
    `;
    await tx`
      INSERT INTO organization (id, name, slug, metadata, created_at)
      VALUES (
        ${organizationID},
        'Lifecycle initial',
        'lifecycle-initial',
        '{"fixture":true}',
        NOW()
      )
    `;
    await tx`
      INSERT INTO member (id, organization_id, user_id, role, created_at)
      VALUES ('member_container_owner', ${organizationID}, ${ownerUserID}, 'owner', NOW())
    `;
  });

  await seedCanonicalSession(
    ownerSessionToken,
    ownerUserID,
    "Lifecycle Owner",
    "owner@container.invalid",
  );
  await seedCanonicalSession(
    inviteeSessionToken,
    inviteeUserID,
    "Lifecycle Invitee",
    "invitee@container.invalid",
  );

  const createdInvitation = await canonicalAuthRequest(
    "/organization/invite-member",
    ownerSessionToken,
    {
      email: "Invitee@Container.Invalid",
      role: "member",
      organizationId: organizationID,
      resend: false,
    },
    200,
  );
  assert.equal(createdInvitation.organizationId, organizationID);
  assert.equal(createdInvitation.email, "invitee@container.invalid");
  assert.equal(createdInvitation.role, "member");
  assert.match(createdInvitation.id, /^[A-Za-z0-9_-]+$/);
  const invitationID = createdInvitation.id;

  // Better Auth rejects the repeated mutation before inserting. Verevon's
  // gateway normalizes this exact canonical condition to a successful no-op.
  const duplicateInvitation = await canonicalAuthRequest(
    "/organization/invite-member",
    ownerSessionToken,
    {
      email: "invitee@container.invalid",
      role: "member",
      organizationId: organizationID,
      resend: false,
    },
    400,
  );
  assert.equal(
    duplicateInvitation.code,
    "USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION",
  );
  const invitationAudit = await authSQL`
    SELECT invitation_id, organization_id, inviter_user_id,
           invitee_email, role, action
    FROM organization_invitation_audit_outbox
    WHERE organization_id = ${organizationID}
      AND invitee_email = 'invitee@container.invalid'
  `;
  assert.deepEqual(plainRows(invitationAudit), [
    {
      invitation_id: invitationID,
      organization_id: organizationID,
      inviter_user_id: ownerUserID,
      invitee_email: "invitee@container.invalid",
      role: "member",
      action: "member_invited",
    },
  ]);
  const duplicateInvitationEvidence = await authSQL`
    SELECT
      (SELECT COUNT(*)::INT
       FROM organization_invitation_audit_outbox
       WHERE organization_id = ${organizationID}
         AND invitee_email = 'invitee@container.invalid') AS invitation_audits,
      (SELECT COUNT(*)::INT
       FROM organization_membership_outbox
       WHERE organization_id = ${organizationID}
         AND user_id = ${inviteeUserID}) AS membership_intents,
      (SELECT COUNT(*)::INT
       FROM organization_membership_audit_outbox
       WHERE organization_id = ${organizationID}
         AND user_id = ${inviteeUserID}) AS membership_audits
  `;
  assert.deepEqual(plainRows(duplicateInvitationEvidence), [
    {
      invitation_audits: 1,
      membership_intents: 0,
      membership_audits: 0,
    },
  ]);

  const invitationController = new InvitationAcceptanceController();
  const acceptanceRequest = {
    headers: {
      authorization: `Bearer ${inviteeSessionToken}`,
      origin: "http://localhost:3000",
      "user-agent": "control-lifecycle-container",
    },
  };
  const accepted = await invitationController.accept(
    invitationID,
    acceptanceRequest,
  );
  assert.deepEqual(accepted, {
    invitation: {
      id: invitationID,
      organizationId: organizationID,
      status: "accepted",
    },
    member: {
      id: accepted.member.id,
      organizationId: organizationID,
      role: "member",
    },
  });
  const repairAfterFirstAcceptance = await authSQL`
    SELECT state, inserted_member, repaired_member_id
    FROM invitation_acceptance_repair
    WHERE invitation_id = ${invitationID}
  `;
  assert.deepEqual(plainRows(repairAfterFirstAcceptance), [
    {
      state: "pending",
      inserted_member: false,
      repaired_member_id: accepted.member.id,
    },
  ]);
  const duplicateAcceptance = await invitationController.accept(
    invitationID,
    acceptanceRequest,
  );
  assert.deepEqual(duplicateAcceptance, accepted);
  await authSQL`
    UPDATE invitation_acceptance_repair
    SET not_before = NOW()
    WHERE invitation_id = ${invitationID}
  `;

  const repairRepository = new PostgresInvitationAcceptanceRepairRepository(
    authSQL,
  );
  const repairSweep = await flushInvitationAcceptanceRepairs(repairRepository);
  assert.deepEqual(repairSweep, {
    claimed: 0,
    completed: 0,
    superseded: 0,
    notRepairable: 0,
    retried: 0,
    deadLettered: 0,
  });
  assert.deepEqual(await flushInvitationAcceptanceRepairs(repairRepository), {
    claimed: 0,
    completed: 0,
    superseded: 0,
    notRepairable: 0,
    retried: 0,
    deadLettered: 0,
  });

  // Better Auth's member hook or the concurrent Auth reconciliation may win the claim
  // before this explicit sweep. The durable row and projected Org state below
  // prove delivery; these counters only prove this process did not over-publish.
  const projectionsBeforeSweep = projectionPublications.length;
  const membershipsBeforeSweep = membershipPublications.length;
  const projectionsFlushed = await flushUntilDurable({
    flush: flushOrganizationProjectionOutbox,
    isDurable: async () => {
      const rows = await authSQL`
        SELECT COUNT(*)::INT AS durable
        FROM organization_projection_outbox
        WHERE organization_id = ${organizationID}
          AND published_at IS NOT NULL
      `;
      return rows[0].durable === 1;
    },
    label: "projection outbox convergence",
  });
  const membershipsFlushed = await flushUntilDurable({
    flush: flushOrganizationMembershipOutbox,
    isDurable: async () => {
      const rows = await authSQL`
        SELECT COUNT(*)::INT AS durable
        FROM organization_membership_outbox
        WHERE organization_id = ${organizationID}
          AND synced_at IS NOT NULL
      `;
      return rows[0].durable === 2;
    },
    label: "membership outbox convergence",
  });
  assert.ok(projectionsFlushed >= 0);
  assert.ok(projectionsFlushed + projectionsBeforeSweep <= 1);
  assert.ok(membershipsFlushed >= 0);
  assert.ok(membershipsFlushed + membershipsBeforeSweep <= 2);
  const expectedProjectionPublications = [
    `organization:${organizationID}:1:upsert`,
  ];
  const expectedMembershipPublications = [
    `organization:${organizationID}:member:${inviteeUserID}:1:upsert`,
    `organization:${organizationID}:member:${ownerUserID}:1:upsert`,
  ];
  assert.equal(
    new Set(projectionPublications).size,
    projectionPublications.length,
  );
  assert.equal(
    new Set(membershipPublications).size,
    membershipPublications.length,
  );
  assert.ok(
    projectionPublications.every((eventID) =>
      expectedProjectionPublications.includes(eventID),
    ),
  );
  assert.ok(
    membershipPublications.every((eventID) =>
      expectedMembershipPublications.includes(eventID),
    ),
  );

  const projectionDelivery = await authSQL`
    SELECT projection_outbox.published_at IS NOT NULL AS published,
           projection_outbox.attempts,
           projection_outbox.last_error
    FROM organization_projection_outbox AS projection_outbox
    WHERE projection_outbox.organization_id = ${organizationID}
  `;
  assert.equal(projectionDelivery.length, 1);
  assert.equal(projectionDelivery[0].published, true);
  assert.ok(projectionDelivery[0].attempts >= 1);
  assert.ok(projectionDelivery[0].attempts <= 5);
  assert.equal(projectionDelivery[0].last_error, null);

  // Membership audit cannot overtake its invitation cause. Replaying the
  // invitation after a simulated lost database acknowledgement reuses the
  // immutable outbox event id and producer timestamp.
  assert.equal(await flushOrganizationMembershipAuditOutbox(), 0);
  assert.equal(await flushOrganizationInvitationAuditOutbox(), 1);
  await authSQL`
    UPDATE organization_invitation_audit_outbox
    SET published_at = NULL
    WHERE invitation_id = ${invitationID}
  `;
  assert.equal(await flushOrganizationInvitationAuditOutbox(), 1);
  assert.equal(await flushOrganizationMembershipAuditOutbox(), 1);
  assert.equal(await flushOrganizationMembershipAuditOutbox(), 0);
  const invitationAuditID = `invitation:${invitationID}:member_invited`;
  const membershipAddedAuditID = `membership:${organizationID}:${inviteeUserID}:1:member_added`;
  assert.deepEqual(
    auditPublications.map((publication) => publication.event_id),
    [invitationAuditID, invitationAuditID, membershipAddedAuditID],
  );
  assert.equal(
    new Date(auditPublications[0].occurred_at).toISOString(),
    new Date(auditPublications[1].occurred_at).toISOString(),
  );

  const repairRows = await authSQL`
    SELECT state, inserted_member
    FROM invitation_acceptance_repair
    WHERE invitation_id = ${invitationID}
  `;
  assert.deepEqual(plainRows(repairRows), [
    { state: "completed", inserted_member: false },
  ]);

  const outboxRows = await authSQL`
    SELECT user_id, desired_action, revision, attempts,
           synced_at IS NOT NULL AS synced
    FROM organization_membership_outbox
    WHERE organization_id = ${organizationID}
    ORDER BY user_id
  `;
  assert.deepEqual(plainRows(outboxRows), [
    {
      user_id: inviteeUserID,
      desired_action: "upsert",
      revision: "1",
      attempts: 1,
      synced: true,
    },
    {
      user_id: ownerUserID,
      desired_action: "upsert",
      revision: "1",
      attempts: 1,
      synced: true,
    },
  ]);

  const projectedMembers = await orgSQL`
    SELECT user_id, role, status
    FROM organization_members
    WHERE org_id = ${organizationID}
    ORDER BY user_id
  `;
  assert.deepEqual(plainRows(projectedMembers), [
    { user_id: inviteeUserID, role: "member", status: "active" },
    { user_id: ownerUserID, role: "owner", status: "active" },
  ]);

  const projectionV3 = await requestJSON(
    `${orgURL}/internal/orgs/${organizationID}/reconcile`,
    {
      method: "POST",
      headers: serviceHeaders("auth-core", orgToken),
      body: JSON.stringify({
        name: "Lifecycle current",
        slug: "lifecycle-current",
        metadata: { fixture: true, revision: 3 },
        ownerUserId: ownerUserID,
        revision: 3,
      }),
    },
    200,
  );
  assert.equal(projectionV3.applied, true);
  // This fixture injected revision 3 directly into Org to exercise its
  // reordering guard. Mirror that acknowledged revision into Auth's canonical
  // producer clock before the later deletion phase; production revisions are
  // generated by this Auth outbox and cannot legitimately diverge this way.
  await authSQL`
    UPDATE organization_projection_outbox
    SET name = 'Lifecycle current', slug = 'lifecycle-current',
        metadata = '{"fixture":true,"revision":3}'::JSONB,
        revision = 3, published_at = NOW(), updated_at = NOW()
    WHERE organization_id = ${organizationID} AND revision = 1
  `;
  const staleProjection = await requestJSON(
    `${orgURL}/internal/orgs/${organizationID}/reconcile`,
    {
      method: "POST",
      headers: serviceHeaders("auth-core", orgToken),
      body: JSON.stringify({
        name: "Lifecycle stale",
        ownerUserId: ownerUserID,
        revision: 2,
      }),
    },
    200,
  );
  assert.equal(staleProjection.applied, false);

  const orderedState = await orgSQL`
    SELECT o.name, m.role, pv.revision AS projection_revision,
           mv.revision AS membership_revision
    FROM organizations o
    JOIN organization_members m
      ON m.org_id = o.id AND m.user_id = ${inviteeUserID}
    JOIN auth_organization_projection_versions pv ON pv.org_id = o.id
    JOIN auth_membership_projection_versions mv
      ON mv.org_id = o.id AND mv.user_id = m.user_id
    WHERE o.id = ${organizationID}
  `;
  assert.deepEqual(plainRows(orderedState), [
    {
      name: "Lifecycle current",
      role: "member",
      projection_revision: "3",
      membership_revision: "1",
    },
  ]);

  await billingSQL`
    INSERT INTO billing_accounts (
      org_id, plan, plan_revision, subscription_state, credits,
      products, feature_flags, entitlements, quota_limits,
      provider_customer_id, metadata
    ) VALUES (
      ${organizationID}, 'pro', 7, 'active', 100,
      '{}'::jsonb, '{}'::jsonb,
      '{"feature.integrations":true}'::jsonb,
      '{"api_calls":10000}'::jsonb,
      '{}'::jsonb, '{"fixture":true}'::jsonb
    )
    ON CONFLICT (org_id) DO UPDATE SET
      plan = EXCLUDED.plan,
      plan_revision = EXCLUDED.plan_revision,
      subscription_state = EXCLUDED.subscription_state,
      credits = EXCLUDED.credits,
      products = EXCLUDED.products,
      feature_flags = EXCLUDED.feature_flags,
      entitlements = EXCLUDED.entitlements,
      quota_limits = EXCLUDED.quota_limits,
      provider_customer_id = EXCLUDED.provider_customer_id,
      metadata = EXCLUDED.metadata,
      trial_ends_at = NULL,
      updated_at = NOW()
  `;
  const billingAccountCardinality = await billingSQL`
    SELECT COUNT(*)::INT AS accounts,
           MAX(plan) AS plan,
           MAX(plan_revision) AS plan_revision,
           MAX(subscription_state) AS subscription_state
    FROM billing_accounts
    WHERE org_id = ${organizationID}
  `;
  assert.deepEqual(plainRows(billingAccountCardinality), [
    {
      accounts: 1,
      plan: "pro",
      plan_revision: "7",
      subscription_state: "active",
    },
  ]);
}

async function runCanonicalMembershipLifecycle() {
  const canonicalInvitations = await authSQL`
    SELECT id FROM invitation
    WHERE organization_id = ${organizationID}
      AND LOWER(BTRIM(email)) = 'invitee@container.invalid'
  `;
  assert.equal(canonicalInvitations.length, 1);
  const invitationID = canonicalInvitations[0].id;

  // Simulate a successful Org response lost before Auth persisted its local
  // acknowledgement. Replaying the exact revision must converge without a
  // second canonical membership or an authorization change.
  await authSQL`
    UPDATE organization_membership_outbox
    SET synced_at = NULL
    WHERE organization_id = ${organizationID}
      AND user_id = ${inviteeUserID}
      AND revision = 1
  `;
  assert.equal(await flushOrganizationMembershipOutbox(), 1);
  assert.equal(await flushOrganizationMembershipOutbox(), 0);

  const duplicateAcceptanceEvidence = await Promise.all([
    authSQL`
      SELECT state, inserted_member
      FROM invitation_acceptance_repair
      WHERE invitation_id = ${invitationID}
    `,
    authSQL`
      SELECT COUNT(*)::INT AS members
      FROM member
      WHERE organization_id = ${organizationID}
        AND user_id = ${inviteeUserID}
    `,
    orgSQL`
      SELECT revision, desired_action
      FROM auth_membership_projection_versions
      WHERE org_id = ${organizationID} AND user_id = ${inviteeUserID}
    `,
  ]);
  assert.deepEqual(plainRows(duplicateAcceptanceEvidence[0]), [
    { state: "completed", inserted_member: false },
  ]);
  assert.deepEqual(plainRows(duplicateAcceptanceEvidence[1]), [{ members: 1 }]);
  assert.deepEqual(plainRows(duplicateAcceptanceEvidence[2]), [
    { revision: "1", desired_action: "upsert" },
  ]);

  const canonicalMember = await authSQL`
    SELECT id FROM member
    WHERE organization_id = ${organizationID}
      AND user_id = ${inviteeUserID}
  `;
  assert.equal(canonicalMember.length, 1);
  const roleAuditID = `membership:${organizationID}:${inviteeUserID}:2:role_changed`;
  const removalAuditID = `membership:${organizationID}:${inviteeUserID}:3:member_removed`;

  // A real non-admin Auth session must be denied before canonical state or the
  // outbox changes.
  const nonAdminRoleChange = await canonicalAuthRequest(
    "/organization/update-member-role",
    inviteeSessionToken,
    {
      memberId: canonicalMember[0].id,
      role: "admin",
      organizationId: organizationID,
    },
    403,
  );
  assert.ok(nonAdminRoleChange);

  for (const invalidRole of [["admin", "member"], "admin,member", "sales"]) {
    await canonicalAuthRequest(
      "/organization/update-member-role",
      ownerSessionToken,
      {
        memberId: canonicalMember[0].id,
        role: invalidRole,
        organizationId: organizationID,
      },
      400,
    );
  }
  const unchangedAfterInvalidRoles = await authSQL`
    SELECT role, revision FROM organization_membership_outbox
    WHERE organization_id = ${organizationID}
      AND user_id = ${inviteeUserID}
  `;
  assert.deepEqual(plainRows(unchangedAfterInvalidRoles), [
    { role: "member", revision: "1" },
  ]);

  // Auth is the only mutation authority. Its real Better Auth route enqueues
  // revision 2; an exact repeated request is a no-op at the durable boundary.
  await canonicalAuthRequest(
    "/organization/update-member-role",
    ownerSessionToken,
    {
      memberId: canonicalMember[0].id,
      role: "admin",
      organizationId: organizationID,
    },
    200,
  );
  await canonicalAuthRequest(
    "/organization/update-member-role",
    ownerSessionToken,
    {
      memberId: canonicalMember[0].id,
      role: "admin",
      organizationId: organizationID,
    },
    200,
  );
  // Database hooks and endpoint hooks may race to claim the same durable row.
  // Drive one explicit reconciliation cycle before asserting convergence;
  // the outbox, not response timing, is the delivery contract.
  await flushOrganizationMembershipOutbox();
  await flushOrganizationMembershipAuditOutbox();
  const pendingRolePromotion = await authSQL`
    SELECT role, desired_action, revision, attempts,
           synced_at IS NOT NULL AS synced
    FROM organization_membership_outbox
    WHERE organization_id = ${organizationID}
      AND user_id = ${inviteeUserID}
  `;
  assert.deepEqual(plainRows(pendingRolePromotion), [
    {
      role: "admin",
      desired_action: "upsert",
      revision: "2",
      attempts: 3,
      synced: true,
    },
  ]);
  const duplicateRoleEvidence = await authSQL`
    SELECT outbox.revision,
           COUNT(audit.revision)::INT AS role_change_audits
    FROM organization_membership_outbox outbox
    LEFT JOIN organization_membership_audit_outbox audit
      ON audit.organization_id = outbox.organization_id
     AND audit.user_id = outbox.user_id
     AND audit.action = 'role_changed'
    WHERE outbox.organization_id = ${organizationID}
      AND outbox.user_id = ${inviteeUserID}
    GROUP BY outbox.revision
  `;
  assert.deepEqual(plainRows(duplicateRoleEvidence), [
    { revision: "2", role_change_audits: 1 },
  ]);
  const publishedRoleAudit = await authSQL`
    SELECT actor_user_id, actor_classification,
           published_at IS NOT NULL AS published
    FROM organization_membership_audit_outbox
    WHERE organization_id = ${organizationID}
      AND user_id = ${inviteeUserID}
      AND revision = 2
  `;
  assert.deepEqual(plainRows(publishedRoleAudit), [
    {
      actor_user_id: ownerUserID,
      actor_classification: "verified_user",
      published: true,
    },
  ]);
  assert.deepEqual(
    auditPublications.map((publication) => publication.event_id),
    [roleAuditID],
  );
  assert.equal(await flushOrganizationMembershipAuditOutbox(), 0);
  const firstRoleOccurredAt = new Date(
    auditPublications[0].occurred_at,
  ).toISOString();
  await authSQL`
    UPDATE organization_membership_audit_outbox
    SET published_at = NULL
    WHERE organization_id = ${organizationID}
      AND user_id = ${inviteeUserID}
      AND revision = 2
  `;
  assert.equal(await flushOrganizationMembershipAuditOutbox(), 1);
  assert.deepEqual(
    auditPublications.map((publication) => publication.event_id),
    [roleAuditID, roleAuditID],
  );
  assert.equal(
    new Date(auditPublications[1].occurred_at).toISOString(),
    firstRoleOccurredAt,
  );
  await authSQL`
    UPDATE organization_membership_outbox
    SET synced_at = NULL
    WHERE organization_id = ${organizationID}
      AND user_id = ${inviteeUserID}
      AND desired_action = 'upsert'
      AND revision = 2
  `;
  assert.equal(await flushOrganizationMembershipOutbox(), 1);
  const duplicateRole = await requestJSON(
    `${orgURL}/internal/orgs/${organizationID}/members/reconcile`,
    {
      method: "POST",
      headers: serviceHeaders("auth-core", orgToken),
      body: JSON.stringify({
        userId: inviteeUserID,
        role: "admin",
        action: "upsert",
        revision: 2,
      }),
    },
    200,
  );
  assert.equal(duplicateRole.applied, false);
  const reorderedRole = await requestJSON(
    `${orgURL}/internal/orgs/${organizationID}/members/reconcile`,
    {
      method: "POST",
      headers: serviceHeaders("auth-core", orgToken),
      body: JSON.stringify({
        userId: inviteeUserID,
        role: "member",
        action: "upsert",
        revision: 1,
      }),
    },
    200,
  );
  assert.equal(reorderedRole.applied, false);

  // Removal likewise crosses the real Auth authority route. A duplicate
  // request is rejected after the first canonical delete and cannot enqueue a
  // second revision; delivery retry remains idempotent at Org.
  await canonicalAuthRequest(
    "/organization/remove-member",
    ownerSessionToken,
    {
      memberIdOrEmail: canonicalMember[0].id,
      organizationId: organizationID,
    },
    200,
  );
  const duplicateRemovalRequest = await canonicalAuthRequest(
    "/organization/remove-member",
    ownerSessionToken,
    {
      memberIdOrEmail: canonicalMember[0].id,
      organizationId: organizationID,
    },
    400,
  );
  assert.equal(duplicateRemovalRequest.code, "MEMBER_NOT_FOUND");
  await flushOrganizationMembershipOutbox();
  await flushOrganizationMembershipAuditOutbox();
  const pendingRemoval = await authSQL`
    SELECT role, desired_action, revision, attempts,
           synced_at IS NOT NULL AS synced
    FROM organization_membership_outbox
    WHERE organization_id = ${organizationID}
      AND user_id = ${inviteeUserID}
  `;
  assert.deepEqual(plainRows(pendingRemoval), [
    {
      role: "admin",
      desired_action: "remove",
      revision: "3",
      attempts: 5,
      synced: true,
    },
  ]);
  const duplicateRemovalEvidence = await authSQL`
    SELECT outbox.revision,
           COUNT(audit.revision)::INT AS removal_audits
    FROM organization_membership_outbox outbox
    LEFT JOIN organization_membership_audit_outbox audit
      ON audit.organization_id = outbox.organization_id
     AND audit.user_id = outbox.user_id
     AND audit.action = 'member_removed'
    WHERE outbox.organization_id = ${organizationID}
      AND outbox.user_id = ${inviteeUserID}
    GROUP BY outbox.revision
  `;
  assert.deepEqual(plainRows(duplicateRemovalEvidence), [
    { revision: "3", removal_audits: 1 },
  ]);
  const publishedRemovalAudit = await authSQL`
    SELECT actor_user_id, actor_classification,
           published_at IS NOT NULL AS published
    FROM organization_membership_audit_outbox
    WHERE organization_id = ${organizationID}
      AND user_id = ${inviteeUserID}
      AND revision = 3
  `;
  assert.deepEqual(plainRows(publishedRemovalAudit), [
    {
      actor_user_id: ownerUserID,
      actor_classification: "verified_user",
      published: true,
    },
  ]);
  assert.deepEqual(
    auditPublications.map((publication) => publication.event_id),
    [roleAuditID, roleAuditID, removalAuditID],
  );
  assert.equal(await flushOrganizationMembershipAuditOutbox(), 0);
  const firstRemovalOccurredAt = new Date(
    auditPublications[2].occurred_at,
  ).toISOString();
  await authSQL`
    UPDATE organization_membership_audit_outbox
    SET published_at = NULL
    WHERE organization_id = ${organizationID}
      AND user_id = ${inviteeUserID}
      AND revision = 3
  `;
  assert.equal(await flushOrganizationMembershipAuditOutbox(), 1);
  assert.equal(await flushOrganizationMembershipAuditOutbox(), 0);
  assert.equal(
    new Date(auditPublications[3].occurred_at).toISOString(),
    firstRemovalOccurredAt,
  );
  assert.equal(await flushOrganizationMembershipOutbox(), 0);
  await authSQL`
    UPDATE organization_membership_outbox
    SET synced_at = NULL
    WHERE organization_id = ${organizationID}
      AND user_id = ${inviteeUserID}
      AND desired_action = 'remove'
      AND revision = 3
  `;
  assert.equal(await flushOrganizationMembershipOutbox(), 1);
  assert.equal(await flushOrganizationMembershipOutbox(), 0);

  const duplicateRemoval = await requestJSON(
    `${orgURL}/internal/orgs/${organizationID}/members/reconcile`,
    {
      method: "POST",
      headers: serviceHeaders("auth-core", orgToken),
      body: JSON.stringify({
        userId: inviteeUserID,
        role: "admin",
        action: "remove",
        revision: 3,
      }),
    },
    200,
  );
  assert.equal(duplicateRemoval.applied, false);
  const reorderedAfterRemoval = await requestJSON(
    `${orgURL}/internal/orgs/${organizationID}/members/reconcile`,
    {
      method: "POST",
      headers: serviceHeaders("auth-core", orgToken),
      body: JSON.stringify({
        userId: inviteeUserID,
        role: "admin",
        action: "upsert",
        revision: 2,
      }),
    },
    200,
  );
  assert.equal(reorderedAfterRemoval.applied, false);

  assert.deepEqual(
    auditPublications.map((publication) => publication.event_id),
    [roleAuditID, roleAuditID, removalAuditID, removalAuditID],
  );

  const durableEvidence = await Promise.all([
    authSQL`
      SELECT role, desired_action, revision, attempts,
             synced_at IS NOT NULL AS synced
      FROM organization_membership_outbox
      WHERE organization_id = ${organizationID}
        AND user_id = ${inviteeUserID}
    `,
    authSQL`
      SELECT
        (SELECT COUNT(*)::INT FROM member
         WHERE organization_id = ${organizationID}
           AND user_id = ${inviteeUserID}) AS invitee_members,
        (SELECT COUNT(*)::INT FROM member
         WHERE organization_id = ${organizationID}
           AND 'owner' = ANY(string_to_array(role, ','))) AS owners
    `,
    orgSQL`
      SELECT m.role, m.status, version.revision, version.desired_action
      FROM organization_members m
      JOIN auth_membership_projection_versions version
        ON version.org_id = m.org_id AND version.user_id = m.user_id
      WHERE m.org_id = ${organizationID} AND m.user_id = ${inviteeUserID}
    `,
    authSQL`
      SELECT action, previous_role, applied_role, revision,
             created_at >= LAG(created_at) OVER (ORDER BY revision) AS ordered,
             published_at IS NULL AS pending_delivery
      FROM organization_membership_audit_outbox
      WHERE organization_id = ${organizationID} AND user_id = ${inviteeUserID}
      ORDER BY revision
    `,
  ]);
  assert.deepEqual(plainRows(durableEvidence[0]), [
    {
      role: "admin",
      desired_action: "remove",
      revision: "3",
      attempts: 6,
      synced: true,
    },
  ]);
  assert.deepEqual(plainRows(durableEvidence[1]), [
    { invitee_members: 0, owners: 1 },
  ]);
  assert.deepEqual(plainRows(durableEvidence[2]), [
    {
      role: "admin",
      status: "removed",
      revision: "3",
      desired_action: "remove",
    },
  ]);
  assert.deepEqual(plainRows(durableEvidence[3]), [
    {
      action: "member_added",
      previous_role: null,
      applied_role: "member",
      revision: "1",
      ordered: null,
      pending_delivery: false,
    },
    {
      action: "role_changed",
      previous_role: "member",
      applied_role: "admin",
      revision: "2",
      ordered: true,
      pending_delivery: false,
    },
    {
      action: "member_removed",
      previous_role: "admin",
      applied_role: null,
      revision: "3",
      ordered: true,
      pending_delivery: false,
    },
  ]);

  // Self-leave is a separate Better Auth route and must still use the same
  // atomic Auth authority, verified actor binding, revision, and projections.
  await authSQL`
    INSERT INTO "user" (
      id, name, email, email_verified, created_at, updated_at
    ) VALUES (
      ${leaverUserID}, 'Lifecycle Leaver',
      'leaver@container.invalid', TRUE, NOW(), NOW()
    )
  `;
  await authSQL`
    INSERT INTO member (id, organization_id, user_id, role, created_at)
    VALUES (
      'member_container_leaver', ${organizationID}, ${leaverUserID},
      'member', NOW()
    )
  `;
  await authSQL`
    UPDATE organization_membership_audit_outbox
    SET actor_user_id = ${ownerUserID},
        actor_classification = 'verified_user'
    WHERE organization_id = ${organizationID}
      AND user_id = ${leaverUserID}
      AND revision = 1
      AND action = 'member_added'
  `;
  await seedCanonicalSession(
    leaverSessionToken,
    leaverUserID,
    "Lifecycle Leaver",
    "leaver@container.invalid",
  );
  assert.equal(await flushOrganizationMembershipOutbox(), 1);
  assert.equal(await flushOrganizationMembershipAuditOutbox(), 1);
  const publicationsBeforeLeave = auditPublications.length;

  const leaveResult = await canonicalAuthRequest(
    "/organization/leave",
    leaverSessionToken,
    { organizationId: organizationID },
    200,
  );
  assert.deepEqual(leaveResult, {
    id: "member_container_leaver",
    organizationId: organizationID,
    userId: leaverUserID,
    role: "member",
  });
  assert.equal(await flushOrganizationMembershipOutbox(), 1);
  assert.equal(await flushOrganizationMembershipAuditOutbox(), 1);

  const leaverSession = JSON.parse(await dragonfly.get(leaverSessionToken));
  assert.equal(leaverSession.session.activeOrganizationId, null);
  const leaveEvidence = await authSQL`
    SELECT
      (SELECT COUNT(*)::INT FROM member
       WHERE organization_id = ${organizationID}
         AND user_id = ${leaverUserID}) AS canonical_members,
      projection.desired_action,
      projection.revision,
      audit.actor_user_id,
      audit.actor_classification,
      audit.published_at IS NOT NULL AS audit_published
    FROM organization_membership_outbox projection
    JOIN organization_membership_audit_outbox audit
      ON audit.organization_id = projection.organization_id
     AND audit.user_id = projection.user_id
     AND audit.revision = projection.revision
    WHERE projection.organization_id = ${organizationID}
      AND projection.user_id = ${leaverUserID}
  `;
  assert.deepEqual(plainRows(leaveEvidence), [
    {
      canonical_members: 0,
      desired_action: "remove",
      revision: "2",
      actor_user_id: leaverUserID,
      actor_classification: "verified_user",
      audit_published: true,
    },
  ]);
  assert.deepEqual(
    auditPublications
      .slice(publicationsBeforeLeave)
      .map((publication) => publication.event_id),
    [`membership:${organizationID}:${leaverUserID}:2:member_removed`],
  );
}

async function deleteWithBillingUnavailable() {
  const authRevision = await authSQL`
    SELECT revision FROM organization_projection_outbox
    WHERE organization_id = ${organizationID}
  `;
  const orgRevision = await orgSQL`
    SELECT revision FROM auth_organization_projection_versions
    WHERE org_id = ${organizationID}
  `;
  const deleted = await authSQL`
    DELETE FROM organization
    WHERE id = ${organizationID}
    RETURNING id
  `;
  assert.equal(deleted.length, 1);

  assert.equal(await flushOrganizationDeletionOutbox(), 0);
  const outbox = await authSQL`
    SELECT billing_synced_at IS NOT NULL AS billing_synced,
           org_synced_at IS NOT NULL AS org_synced,
           completed_at IS NOT NULL AS completed,
           attempts,
           last_error
    FROM organization_deletion_outbox
    WHERE organization_id = ${organizationID}
  `;
  assert.equal(outbox.length, 1);
  assert.equal(outbox[0].billing_synced, false);
  assert.equal(
    outbox[0].org_synced,
    true,
    `Org deletion checkpoint missing: ${JSON.stringify({
      outbox: outbox[0],
      authRevision: plainRows(authRevision),
      orgRevision: plainRows(orgRevision),
    })}`,
  );
  assert.equal(outbox[0].completed, false);
  assert.equal(outbox[0].attempts, 1);
  assert.match(outbox[0].last_error, /^billing:/);

  const orgState = await orgSQL`
    SELECT
      (SELECT COUNT(*)::INT FROM organizations WHERE id = ${organizationID}) AS organizations,
      (SELECT COUNT(*)::INT FROM auth_organization_tombstones WHERE org_id = ${organizationID}) AS tombstones
  `;
  assert.deepEqual(plainRows(orgState), [{ organizations: 0, tombstones: 1 }]);
}

async function retryDeletionAndRejectResurrection() {
  await requestJSON(
    `${billingURL}/api/v1/billing/orgs/${organizationID}/deactivate`,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ reason: "unauthenticated" }),
    },
    401,
  );

  assert.equal(await flushOrganizationDeletionOutbox(), 1);
  assert.equal(await flushOrganizationDeletionOutbox(), 0);

  const outbox = await authSQL`
    SELECT billing_synced_at IS NOT NULL AS billing_synced,
           org_synced_at IS NOT NULL AS org_synced,
           completed_at IS NOT NULL AS completed,
           attempts,
           last_error
    FROM organization_deletion_outbox
    WHERE organization_id = ${organizationID}
  `;
  assert.deepEqual(plainRows(outbox), [
    {
      billing_synced: true,
      org_synced: true,
      completed: true,
      attempts: 2,
      last_error: null,
    },
  ]);

  const billingState = await billingSQL`
    SELECT a.plan, a.plan_revision, a.subscription_state,
           (SELECT COUNT(*)::INT
            FROM billing_organization_tombstones t
            WHERE t.org_id = a.org_id) AS tombstones
    FROM billing_accounts a
    WHERE a.org_id = ${organizationID}
  `;
  assert.deepEqual(plainRows(billingState), [
    {
      plan: "pro",
      plan_revision: "7",
      subscription_state: "canceled",
      tombstones: 1,
    },
  ]);

  await requestJSON(
    `${billingURL}/api/v1/billing/orgs/${organizationID}/account`,
    {
      method: "PUT",
      headers: serviceHeaders("lifecycle-writer", billingWriterToken),
      body: JSON.stringify({
        plan: "enterprise",
        plan_revision: 99,
        subscription_state: "active",
      }),
    },
    500,
  );
  await requestJSON(
    `${billingURL}/api/v1/billing/orgs/${organizationID}/deactivate`,
    {
      method: "POST",
      headers: serviceHeaders("auth-core", billingToken),
      body: JSON.stringify({ reason: "organization_deleted" }),
    },
    200,
  );

  await requestJSON(
    `${orgURL}/internal/orgs/${organizationID}/reconcile`,
    {
      method: "POST",
      headers: serviceHeaders("auth-core", orgToken),
      body: JSON.stringify({
        name: "Delayed resurrection",
        ownerUserId: ownerUserID,
        revision: 99,
      }),
    },
    409,
  );

  const finalState = await Promise.all([
    billingSQL`
      SELECT plan, plan_revision, subscription_state
      FROM billing_accounts WHERE org_id = ${organizationID}
    `,
    orgSQL`
      SELECT
        (SELECT COUNT(*)::INT FROM organizations WHERE id = ${organizationID}) AS organizations,
        (SELECT COUNT(*)::INT FROM auth_organization_tombstones WHERE org_id = ${organizationID}) AS tombstones
    `,
  ]);
  assert.deepEqual(plainRows(finalState[0]), [
    { plan: "pro", plan_revision: "7", subscription_state: "canceled" },
  ]);
  assert.deepEqual(plainRows(finalState[1]), [
    { organizations: 0, tombstones: 1 },
  ]);
}

async function main() {
  await verifyAllMarkers();
  switch (requiredEnvironment("CONTROL_LIFECYCLE_PHASE")) {
    case "seed-and-converge":
      await seedRepairAndConverge();
      break;
    case "membership-lifecycle":
      await runCanonicalMembershipLifecycle();
      break;
    case "partial-delete":
      await deleteWithBillingUnavailable();
      break;
    case "retry-delete":
      await retryDeletionAndRejectResurrection();
      break;
    default:
      throw new Error("unknown CONTROL_LIFECYCLE_PHASE");
  }
}

main()
  .then(() => {
    process.stdout.write(
      `container lifecycle phase ${process.env.CONTROL_LIFECYCLE_PHASE} passed\n`,
    );
  })
  .catch((error) => {
    console.error(error);
    process.exitCode = 1;
  })
  .finally(async () => {
    try {
      await Promise.all([
        authSQL.end({ timeout: 5 }),
        orgSQL.end({ timeout: 5 }),
        billingSQL.end({ timeout: 5 }),
        dragonfly.isOpen ? dragonfly.quit() : Promise.resolve(),
      ]);
    } finally {
      process.exit(process.exitCode ?? 0);
    }
  });
