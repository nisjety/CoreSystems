import { randomUUID } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import postgres from 'postgres';

import { verifyLifecycleFixtureMarker } from './lifecycle-test-fixture';

const testDatabaseUrl = process.env.TEST_DATABASE_URL?.trim();
const postgresDescribe = testDatabaseUrl ? describe : describe.skip;

postgresDescribe('GDPR erasure across Auth durable outboxes', () => {
  const schema = `gdpr_outbox_${randomUUID().replaceAll('-', '')}`;
  let admin: ReturnType<typeof postgres>;
  let fixture: ReturnType<typeof postgres>;

  beforeAll(async () => {
    admin = postgres(testDatabaseUrl!, { max: 1, prepare: false });
    await verifyLifecycleFixtureMarker(
      admin,
      testDatabaseUrl!,
      'auth_invitation',
      process.env.CONTROL_LIFECYCLE_FIXTURE_ID ?? '',
    );
    await admin`CREATE SCHEMA ${admin(schema)}`;
    fixture = postgres(testDatabaseUrl!, {
      max: 1,
      prepare: false,
      connection: { search_path: `${schema},public` },
    });
    for (const migration of [
      'init_better_auth.sql',
      'gdpr_hard_delete.sql',
      '014_normalized_identity_email.sql',
      '015_organization_projection_outbox.sql',
      '016_unique_organization_membership.sql',
      '017_invitation_acceptance_repair.sql',
      '018_historical_owner_preflight.sql',
      '019_membership_audit_outbox.sql',
      '020_auth_identity_event_outbox.sql',
      '021_invitation_created_at.sql',
      '022_membership_audit_actor_recovery.sql',
      '023_gdpr_outbox_erasure.sql',
      '024_revisioned_cross_plane_projection.sql',
      '025_gdpr_outbox_publish_fence.sql',
    ]) {
      await fixture.unsafe(
        readFileSync(join(process.cwd(), 'migrations', migration), 'utf8'),
      );
    }
  });

  afterAll(async () => {
    if (fixture) await fixture.end({ timeout: 5 });
    if (admin) {
      await admin`DROP SCHEMA IF EXISTS ${admin(schema)} CASCADE`;
      await admin.end({ timeout: 5 });
    }
  });

  async function seedUserEvidence(suffix: string): Promise<void> {
    const userId = `user-${suffix}`;
    const email = `${suffix}@example.invalid`;
    await fixture.begin(async (tx) => {
      await tx`
        INSERT INTO "user" (
          id, name, email, email_verified, created_at, updated_at
        ) VALUES (
          ${userId}, ${`Sensitive ${suffix}`}, ${email}, TRUE, NOW(), NOW()
        )
      `;
      await tx`
        INSERT INTO organization (id, name, created_at)
        VALUES (${`org-${suffix}`}, ${`Org ${suffix}`}, NOW())
      `;
      await tx`
        INSERT INTO invitation (
          id, organization_id, email, role, status, expires_at,
          inviter_id, created_at
        ) VALUES (
          ${`invitation-${suffix}`}, ${`org-${suffix}`}, ${email}, 'member',
          'pending', NOW() + INTERVAL '1 day', ${userId}, NOW()
        )
      `;
      await tx`
        INSERT INTO member (id, organization_id, user_id, role, created_at)
        VALUES (
          ${`member-${suffix}`}, ${`org-${suffix}`}, ${userId}, 'member', NOW()
        )
      `;
      await tx`
        INSERT INTO account (
          id, account_id, provider_id, user_id, scope, created_at, updated_at
        ) VALUES (
          ${`account-${suffix}`}, ${`provider-account-${suffix}`},
          'microsoft', ${userId}, 'openid email', NOW(), NOW()
        )
      `;
    });
    await fixture`
      UPDATE organization_membership_audit_outbox
      SET actor_user_id = ${userId}, actor_classification = 'verified_user'
      WHERE organization_id = ${`org-${suffix}`} AND user_id = ${userId}
    `;
  }

  async function expectErasedEvidence(
    suffix: string,
    expectedMembershipAudits: number,
  ): Promise<void> {
    const rawUserId = `user-${suffix}`;
    const rawEmail = `${suffix}@example.invalid`;
    const evidence = await fixture<
      Array<{
        membership_audits: number;
        raw_membership_subjects: number;
        raw_membership_actors: number;
        unresolved_or_erased_actors: number;
        invitation_audits: number;
        raw_invitation_pii: number;
        identity_events: number;
      }>
    >`
      SELECT
        (SELECT COUNT(*)::INT FROM organization_membership_audit_outbox
         WHERE organization_id = ${`org-${suffix}`}) AS membership_audits,
        (SELECT COUNT(*)::INT FROM organization_membership_audit_outbox
         WHERE user_id = ${rawUserId}) AS raw_membership_subjects,
        (SELECT COUNT(*)::INT FROM organization_membership_audit_outbox
         WHERE actor_user_id = ${rawUserId}) AS raw_membership_actors,
        (SELECT COUNT(*)::INT FROM organization_membership_audit_outbox
         WHERE organization_id = ${`org-${suffix}`}
           AND actor_user_id IS NULL
           AND actor_classification IN ('unresolved', 'erased_actor'))
          AS unresolved_or_erased_actors,
        (SELECT COUNT(*)::INT FROM organization_invitation_audit_outbox
         WHERE invitation_id = ${`invitation-${suffix}`}) AS invitation_audits,
        (SELECT COUNT(*)::INT FROM organization_invitation_audit_outbox
         WHERE inviter_user_id = ${rawUserId}
            OR invitee_email = ${rawEmail}) AS raw_invitation_pii,
        (SELECT COUNT(*)::INT FROM auth_identity_event_outbox
         WHERE user_id = ${rawUserId}
            OR payload::TEXT LIKE ${`%${rawUserId}%`}
            OR payload::TEXT ILIKE ${`%${rawEmail}%`}
            OR payload::TEXT ILIKE ${`%Sensitive ${suffix}%`}
            OR payload::TEXT ILIKE ${`%provider-account-${suffix}%`})
          AS identity_events
    `;
    expect(evidence).toEqual([
      {
        membership_audits: expectedMembershipAudits,
        raw_membership_subjects: 0,
        raw_membership_actors: 0,
        unresolved_or_erased_actors: expectedMembershipAudits,
        invitation_audits: 1,
        raw_invitation_pii: 0,
        identity_events: 0,
      },
    ]);

    const pseudonyms = await fixture<
      Array<{
        subject_pseudonymized: boolean;
        subject_erased: boolean;
        invitation_pseudonymized: boolean;
        invitation_salted: boolean;
        invitation_erased: boolean;
      }>
    >`
      SELECT
        membership.user_id LIKE 'erased:%' AS subject_pseudonymized,
        membership.subject_erased_at IS NOT NULL AS subject_erased,
        invitation.inviter_user_id LIKE 'erased:%' AND
          invitation.invitee_email LIKE 'erased+%@invalid.local'
          AS invitation_pseudonymized,
        invitation.invitee_email =
          'erased+' || md5(${rawUserId} || ':' || LOWER(${rawEmail})) ||
          '@invalid.local' AND
          invitation.invitee_email <>
          'erased+' || md5(LOWER(${rawEmail})) || '@invalid.local'
          AS invitation_salted,
        invitation.inviter_erased_at IS NOT NULL AND
          invitation.invitee_erased_at IS NOT NULL AS invitation_erased
      FROM organization_membership_audit_outbox membership
      JOIN organization_invitation_audit_outbox invitation
        ON invitation.organization_id = membership.organization_id
      WHERE membership.organization_id = ${`org-${suffix}`}
      ORDER BY membership.revision
      LIMIT 1
    `;
    expect(pseudonyms).toEqual([
      {
        subject_pseudonymized: true,
        subject_erased: true,
        invitation_pseudonymized: true,
        invitation_salted: true,
        invitation_erased: true,
      },
    ]);
  }

  it('hard-deletes identity delivery PII after cascade audit triggers and preserves pseudonymized evidence', async () => {
    await seedUserEvidence('hard-delete');
    const result = await fixture<Array<{ result: { success: boolean } }>>`
      SELECT gdpr_hard_delete_user('user-hard-delete') AS result
    `;
    expect(result[0].result.success).toBe(true);
    await expectErasedEvidence('hard-delete', 2);
  });

  it('waits for an in-flight publish fence before committing GDPR deletion', async () => {
    await seedUserEvidence('publish-fence');
    const contender = postgres(testDatabaseUrl!, {
      max: 1,
      prepare: false,
      connection: { search_path: `${schema},public` },
    });
    let releaseFence!: () => void;
    let reportFenceHeld!: () => void;
    const fenceRelease = new Promise<void>((resolve) => {
      releaseFence = resolve;
    });
    const fenceHeld = new Promise<void>((resolve) => {
      reportFenceHeld = resolve;
    });

    const publisherTransaction = fixture.begin(async (tx) => {
      await tx`
        SELECT acquire_auth_gdpr_identity_locks(
          'user-publish-fence', 'publish-fence@example.invalid'
        )
      `;
      reportFenceHeld();
      await fenceRelease;
    });
    await fenceHeld;

    let deletionResolved = false;
    const deletion = contender<Array<{ result: { success: boolean } }>>`
      SELECT gdpr_hard_delete_user('user-publish-fence') AS result
    `.then((result) => {
      deletionResolved = true;
      return result;
    });
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(deletionResolved).toBe(false);

    releaseFence();
    await publisherTransaction;
    const deletionResult = await deletion;
    expect(deletionResult[0]?.result.success).toBe(true);
    await contender.end({ timeout: 5 });
  });

  it('restricts the security-definer GDPR routines and pins them to the Auth schema', async () => {
    const functions = await fixture<
      Array<{
        function_name: string;
        public_execute: boolean;
        search_path: string | null;
      }>
    >`
      SELECT routine.proname AS function_name,
             has_function_privilege(
               'public', routine.oid, 'EXECUTE'
             ) AS public_execute,
             (
               SELECT setting
               FROM unnest(COALESCE(routine.proconfig, ARRAY[]::TEXT[])) setting
               WHERE setting LIKE 'search_path=%'
             ) AS search_path
      FROM pg_proc routine
      JOIN pg_namespace namespace ON namespace.oid = routine.pronamespace
      WHERE namespace.nspname = ${schema}
        AND routine.proname IN (
          'gdpr_anonymize_user', 'gdpr_hard_delete_user'
        )
      ORDER BY routine.proname
    `;
    expect(functions).toEqual([
      {
        function_name: 'gdpr_anonymize_user',
        public_execute: false,
        search_path: `search_path=pg_catalog, ${schema}`,
      },
      {
        function_name: 'gdpr_hard_delete_user',
        public_execute: false,
        search_path: `search_path=pg_catalog, ${schema}`,
      },
    ]);
  });

  it('does not let reviewed owner evidence block hard deletion or retain the erased owner', async () => {
    await seedUserEvidence('owner-evidence');
    await fixture`
      INSERT INTO owner_invariant_reviewed_mapping (
        mapping_id, organization_id, owner_user_id,
        expected_organization_created_at, expected_member_role,
        reviewed_by, reviewed_at, applied_at, applied_member_id
      )
      SELECT 'mapping-owner-evidence', organization.id, 'user-owner-evidence',
             organization.created_at, 'member', 'user-owner-evidence', NOW(),
             NOW(), 'member-owner-evidence'
      FROM organization
      WHERE organization.id = 'org-owner-evidence'
    `;
    await fixture`
      UPDATE member
      SET role = 'owner'
      WHERE id = 'member-owner-evidence'
    `;
    await fixture`
      INSERT INTO owner_invariant_repair_audit (
        mapping_id, organization_id, owner_user_id, member_id,
        previous_role, applied_role, reviewed_by, reviewed_at
      ) VALUES (
        'mapping-owner-evidence', 'org-owner-evidence',
        'user-owner-evidence', 'member-owner-evidence',
        'member', 'owner', 'user-owner-evidence', NOW()
      )
    `;

    const result = await fixture<Array<{ result: { success: boolean } }>>`
      SELECT gdpr_hard_delete_user('user-owner-evidence') AS result
    `;
    expect(result[0].result.success).toBe(true);

    const retained = await fixture<
      Array<{
        user_rows: number;
        reviewed_mappings: number;
        raw_owner_audit: number;
        raw_reviewer_audit: number;
        erased_owner_audit: number;
        erased_reviewer_audit: number;
        stale_projection_owner: number;
        pending_removal: number;
      }>
    >`
      SELECT
        (SELECT COUNT(*)::INT FROM "user"
         WHERE id = 'user-owner-evidence') AS user_rows,
        (SELECT COUNT(*)::INT FROM owner_invariant_reviewed_mapping
         WHERE mapping_id = 'mapping-owner-evidence') AS reviewed_mappings,
        (SELECT COUNT(*)::INT FROM owner_invariant_repair_audit
         WHERE owner_user_id = 'user-owner-evidence') AS raw_owner_audit,
        (SELECT COUNT(*)::INT FROM owner_invariant_repair_audit
         WHERE reviewed_by = 'user-owner-evidence') AS raw_reviewer_audit,
        (SELECT COUNT(*)::INT FROM owner_invariant_repair_audit
         WHERE mapping_id = 'mapping-owner-evidence'
           AND owner_user_id LIKE 'erased:%'
           AND owner_user_erased_at IS NOT NULL) AS erased_owner_audit,
        (SELECT COUNT(*)::INT FROM owner_invariant_repair_audit
         WHERE mapping_id = 'mapping-owner-evidence'
           AND reviewed_by LIKE 'erased:%'
           AND reviewer_erased_at IS NOT NULL) AS erased_reviewer_audit,
        (SELECT COUNT(*)::INT FROM organization_projection_outbox
         WHERE organization_id = 'org-owner-evidence'
           AND owner_user_id = 'user-owner-evidence') AS stale_projection_owner,
        (SELECT COUNT(*)::INT FROM organization_membership_outbox
         WHERE organization_id = 'org-owner-evidence'
           AND user_id = 'user-owner-evidence'
           AND desired_action = 'remove'
           AND gdpr_erasure_requested_at IS NOT NULL
           AND synced_at IS NULL) AS pending_removal
    `;
    expect(retained).toEqual([
      {
        user_rows: 0,
        reviewed_mappings: 0,
        raw_owner_audit: 0,
        raw_reviewer_audit: 0,
        erased_owner_audit: 1,
        erased_reviewer_audit: 1,
        stale_projection_owner: 0,
        pending_removal: 1,
      },
    ]);

    await fixture`
      UPDATE organization_membership_outbox
      SET synced_at = NOW()
      WHERE organization_id = 'org-owner-evidence'
        AND user_id = 'user-owner-evidence'
    `;
    await expect(
      fixture`SELECT purge_completed_gdpr_membership_outbox() AS purged`,
    ).resolves.toEqual([{ purged: 1 }]);
    await expect(
      fixture`
        SELECT COUNT(*)::INT AS retained
        FROM organization_membership_outbox
        WHERE user_id = 'user-owner-evidence'
      `,
    ).resolves.toEqual([{ retained: 0 }]);
  });

  it('pseudonymizes durable evidence when the softer GDPR routine retains the user row', async () => {
    await seedUserEvidence('anonymize');
    const result = await fixture<Array<{ result: { success: boolean } }>>`
      SELECT gdpr_anonymize_user('user-anonymize') AS result
    `;
    expect(result[0].result.success).toBe(true);
    await expectErasedEvidence('anonymize', 1);
    const user = await fixture`
      SELECT name, email FROM "user" WHERE id = 'user-anonymize'
    `;
    expect(user).toEqual([
      {
        name: 'Deleted User',
        email: 'deleted_user-anonymize@anonymized.local',
      },
    ]);
  });

  it('rejects direct audit rewrites even when a caller forges the transaction marker', async () => {
    await seedUserEvidence('forged-marker');
    const rawUserId = 'user-forged-marker';
    const rawEmail = 'forged-marker@example.invalid';
    await fixture`
      UPDATE member SET role = 'owner' WHERE id = 'member-forged-marker'
    `;
    await fixture`
      INSERT INTO owner_invariant_repair_audit (
        mapping_id, organization_id, owner_user_id, member_id,
        previous_role, applied_role, reviewed_by, reviewed_at
      ) VALUES (
        'mapping-forged-marker', 'org-forged-marker', ${rawUserId},
        'member-forged-marker', 'member', 'owner', ${rawUserId}, NOW()
      )
    `;
    await fixture`
      SELECT set_config('app.gdpr_erasure_user_id', ${rawUserId}, FALSE)
    `;
    await fixture`
      SELECT set_config('app.gdpr_erasure_email', ${rawEmail}, FALSE)
    `;

    await expect(
      fixture`
        UPDATE organization_membership_audit_outbox
        SET user_id = 'erased:' || md5(${rawUserId}),
            subject_erased_at = NOW()
        WHERE organization_id = 'org-forged-marker'
      `,
    ).rejects.toThrow(/membership audit identity is immutable/);

    await expect(
      fixture`
        UPDATE organization_invitation_audit_outbox
        SET inviter_user_id = 'erased:' || md5(${rawUserId}),
            inviter_erased_at = NOW(),
            invitee_email = 'erased+' ||
              md5(${rawUserId} || ':' || LOWER(${rawEmail})) ||
              '@invalid.local',
            invitee_erased_at = NOW()
        WHERE invitation_id = 'invitation-forged-marker'
      `,
    ).rejects.toThrow(/invitation audit identity is immutable/);

    await expect(
      fixture`
        UPDATE owner_invariant_repair_audit
        SET owner_user_id = 'erased:' || md5(${rawUserId}),
            owner_user_erased_at = NOW(),
            reviewed_by = 'erased:' || md5(
              ${rawUserId} || ':reviewer:' || LOWER(${rawUserId})
            ),
            reviewer_erased_at = NOW()
        WHERE mapping_id = 'mapping-forged-marker'
      `,
    ).rejects.toThrow(/owner invariant repair audit is append-only/);

    const untouched = await fixture`
      SELECT inviter_user_id, invitee_email
      FROM organization_invitation_audit_outbox
      WHERE invitation_id = 'invitation-forged-marker'
    `;
    expect(untouched).toEqual([
      { inviter_user_id: rawUserId, invitee_email: rawEmail },
    ]);
  });
});
