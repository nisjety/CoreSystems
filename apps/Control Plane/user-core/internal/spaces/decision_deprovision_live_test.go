package spaces

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

// WS05-S9 live proof: once user_org_memberships.status is flipped to
// 'removed' for a user, EVERY effect-bound issuer route in this package
// refuses them, and the exact same user is issued a decision again the
// moment their status returns to 'active'. See
// .plan-work/space-build/units/WS05-S9.json and
// .plan-work/space-build/depth/WS05-S9.md.
//
// Every evidence resolver exercised below (resolvePersonalDecisionEvidence,
// ResolvePersonalThreadDecisionEvidence, ResolveOwnScheduledThreadReadDecisionEvidence,
// ResolvePersonalRetrievalDecisionEvidence, ResolvePersonalImportDecisionEvidence,
// ResolveAgentActionDecisionEvidence) runs the SAME authoritative query shape:
// ... AND EXISTS (SELECT 1 FROM user_org_memberships u WHERE u.user_id=$3 AND
// u.org_id=$2 AND u.status='active'). A removed user makes that EXISTS clause
// false, the query returns pgx.ErrNoRows, and every resolver turns that into
// the identical ErrNoCurrentMembership — which is what this test proves holds
// for every named effect, not just by reading the query text.
//
// Gated on TEST_DATABASE_URL; reuses newRoomLiveFixture (membership_ops_live_test.go),
// the same fixture this package's other live tests build on (see
// archive_live_test.go's TestArchivedStateIsNeverResolvedByPersonalWriteEvidenceLive
// for the identical manual personal-Space construction this test follows).

// deprovisionTestDigest is a syntactically valid sha256 commitment
// ("sha256:" plus 64 hex characters), accepted by every digest-shaped field
// exercised below (template/predicate/profile/payload digests).
var deprovisionTestDigest = "sha256:" + strings.Repeat("a", 64)

// deprovisionPersonalSpace creates a personal Space with a single owner
// member and an org effect policy with every personal-Space entitlement bit
// on, so each resolver/issuer pair below would succeed for an active member
// and only the removed org membership can refuse them.
func (f *roomLiveFixture) deprovisionPersonalSpace(t *testing.T, name string) (spaceRef, owner string) {
	t.Helper()
	owner = f.user(t, name+"-owner", f.org)
	spaceRef = "ws04s4-" + name + "-" + f.suffix
	f.exec(t, `INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id,
		application_lifecycle_revision, registration_state) VALUES ($1,$2,'personal',$3,1,'active')`,
		spaceRef, f.org, owner)
	f.exec(t, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, spaceRef)
	f.exec(t, `INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by, source)
		VALUES ($1,'user',$2,'owner','application-space-registration','registration')`, spaceRef, owner)
	f.exec(t, `INSERT INTO space_effect_policies (org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
		retention_class, residency, deletion_scope) VALUES ($1,'privacy:'||$1||':1','assistant_collaboration','contract','internal','standard','swedencentral','space')
		ON CONFLICT (org_id) DO NOTHING`, f.org)
	f.exec(t, `UPDATE space_effect_policies SET thread_create_entitled=TRUE, retrieval_read_entitled=TRUE,
		thread_read_entitled=TRUE, agent_action_entitled=TRUE, schedule_fire_entitled=TRUE,
		import_write_entitled=TRUE, sandbox_capability_entitled=TRUE, process_registry_entitled=TRUE,
		canvas_entitled=TRUE, browser_entitled=TRUE WHERE org_id=$1`, f.org)
	return spaceRef, owner
}

// setOrgMembershipStatus flips user_org_memberships.status for subject in
// f.org — the exact column and row every evidence resolver's EXISTS clause
// reads to decide whether the caller is still a provisioned org member.
func (f *roomLiveFixture) setOrgMembershipStatus(t *testing.T, subject, status string) {
	t.Helper()
	f.exec(t, `UPDATE user_org_memberships SET status=$1 WHERE user_id=$2 AND org_id=$3`, status, subject, f.org)
}

// TestDeprovisionedUserRefusedByEveryIssuerRouteThenReinstatedLive is the
// WS05-S9 per-issuer fail-closed proof. It is a single table-driven test over
// the ~15 effect-bound issuer routes named in tests_owed: each row resolves
// the SAME evidence the real handler resolves and then issues the SAME real
// decision the real handler issues, with minimal valid fixture data. No row
// is stubbed: every one calls its actual exported Resolve*/Issue* pair.
func TestDeprovisionedUserRefusedByEveryIssuerRouteThenReinstatedLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	ctx := context.Background()
	ref, owner := f.deprovisionPersonalSpace(t, "deprovision")

	type issuerRoute struct {
		name    string
		resolve func(ctx context.Context) error
	}

	routes := []issuerRoute{
		{"thread (thread-create)", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalThreadDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			_, err = IssuePersonalThreadCreateDecision(evidence, PersonalThreadDecisionRequest{
				DecisionRef: "dr-thread-create", IdempotencyKey: "idem-thread-create", Nonce: "nonce-thread-create",
			}, time.Now().UTC())
			return err
		}},
		{"thread-append", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalThreadDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			_, err = IssueThreadAppendDecision(evidence, ThreadAppendDecisionRequest{
				DecisionRef: "dr-thread-append", ThreadID: "thread-1", ContentDigest: deprovisionTestDigest,
				IdempotencyKey: "idem-thread-append", Nonce: "nonce-thread-append",
			}, time.Now().UTC())
			return err
		}},
		{"thread-read", func(ctx context.Context) error {
			evidence, err := f.repo.ResolveOwnScheduledThreadReadDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			_, err = IssueOwnScheduledThreadReadDecision(evidence, SharedThreadReadDecisionRequest{
				DecisionRef: "dr-thread-read", IdempotencyKey: "idem-thread-read", Nonce: "nonce-thread-read",
			}, time.Now().UTC())
			return err
		}},
		{"retrieval", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalRetrievalDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			_, err = IssuePersonalRetrievalDecision(evidence, PersonalRetrievalDecisionRequest{
				DecisionRef: "dr-retrieval", IdempotencyKey: "idem-retrieval", Nonce: "nonce-retrieval",
			}, time.Now().UTC())
			return err
		}},
		{"import", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalImportDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			_, err = IssuePersonalImportDecision(evidence, PersonalImportDecisionRequest{
				DecisionRef: "dr-import", IdempotencyKey: "idem-import", SourceType: "upload", Nonce: "nonce-import",
			}, time.Now().UTC())
			return err
		}},
		{"import-execution", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalImportDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			intent := PersonalImportExecutionIntent{
				OrgID: f.org, SpaceRef: ref, SubjectID: owner,
				ActionSchemaHash: personalImportSchema, PayloadDigest: deprovisionTestDigest,
				IdempotencyKey: "idem-import-exec", SourceType: "upload",
			}
			_, err = IssuePersonalImportExecutionDecision(evidence, intent, "dr-import-exec", "nonce-import-exec", time.Now().UTC())
			return err
		}},
		{"schedule-create", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalThreadDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			_, err = IssueScheduleCreateDecision(evidence, ScheduleCreateRequest{
				DecisionRef: "dr-schedule-create", ScheduleID: "sched-1", TemplateDigest: deprovisionTestDigest,
				IdempotencyKey: "idem-schedule-create", Nonce: "nonce-schedule-create",
			}, time.Now().UTC())
			return err
		}},
		{"schedule-fire", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalThreadDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			intent := ScheduleFireIntent{
				OrgID: f.org, SpaceRef: ref, SubjectID: owner, ScheduleID: "sched-1", FireKey: "fire-1",
				TemplateDigest: deprovisionTestDigest, IdempotencyKey: "idem-schedule-fire",
			}
			_, err = IssueScheduleFireDecision(evidence, intent, "dr-schedule-fire", "nonce-schedule-fire", time.Now().UTC())
			return err
		}},
		{"watch-create", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalThreadDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			_, err = IssueWatchCreateDecision(evidence, WatchCreateRequest{
				DecisionRef: "dr-watch-create", WatchID: "watch-1", SourceKind: "process", SourceRef: "proc-1",
				PredicateDigest: deprovisionTestDigest, IdempotencyKey: "idem-watch-create", Nonce: "nonce-watch-create",
			}, time.Now().UTC())
			return err
		}},
		{"watch-observe", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalThreadDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			intent := WatchObserveIntent{
				OrgID: f.org, SpaceRef: ref, SubjectID: owner, WatchID: "watch-1",
				SourceKind: "process", SourceRef: "proc-1", PredicateDigest: deprovisionTestDigest,
				IdempotencyKey: "idem-watch-observe",
			}
			_, err = IssueWatchObserveDecision(evidence, intent, "dr-watch-observe", "nonce-watch-observe", time.Now().UTC())
			return err
		}},
		{"sandbox-capability", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalThreadDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			intent := SpaceCapabilityIntent{
				OrgID: f.org, SpaceRef: ref, SubjectID: owner, BackendID: "backend-1",
				ProfileDigest: deprovisionTestDigest, Persistence: "ephemeral", Processes: ProcessesBoundedOneshot,
				Egress: EgressDisabledByDefault, CredentialMode: "none", IdempotencyKey: "idem-sandbox-capability",
			}
			_, err = IssueSpaceCapabilityDecision(evidence, intent, "dr-sandbox-capability", "nonce-sandbox-capability", time.Now().UTC())
			return err
		}},
		{"scheduled-run", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalThreadDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			intent := ScheduledRunIntent{
				OrgID: f.org, SpaceRef: ref, SubjectID: owner, ScheduleID: "sched-1", FireKey: "fire-1",
				TaskID: "task-1", TemplateDigest: deprovisionTestDigest, IdempotencyKey: "idem-scheduled-run",
			}
			_, err = IssueScheduledRunDecision(evidence, intent, "dr-scheduled-run", "nonce-scheduled-run", time.Now().UTC())
			return err
		}},
		{"scheduled-step", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalThreadDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			intent := ScheduledStepIntent{
				OrgID: f.org, SpaceRef: ref, SubjectID: owner, RunID: "run-1", ThreadID: "thread-1",
				ScheduleID: "sched-1", FireKey: "fire-1", TemplateDigest: deprovisionTestDigest,
				StepID: "step-1", StepIndex: 0, PolicyDigest: deprovisionTestDigest, IdempotencyKey: "idem-scheduled-step",
			}
			_, err = IssueScheduledStepDecision(evidence, intent, "dr-scheduled-step", "nonce-scheduled-step", time.Now().UTC())
			return err
		}},
		{"owner-grant", func(ctx context.Context) error {
			evidence, err := f.repo.ResolvePersonalThreadDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			_, err = IssueOwnerGrantDecision(evidence, OwnerGrantDecisionRequest{
				ConversationID: "conv-1", ActionID: "tickets.create", Operation: "create",
				IdempotencyKey: "idem-owner-grant", DecisionRef: "dr-owner-grant", Nonce: "nonce-owner-grant",
			}, time.Now().UTC())
			return err
		}},
		{"run-action", func(ctx context.Context) error {
			evidence, err := f.repo.ResolveAgentActionDecisionEvidence(ctx, ref, f.org, owner)
			if err != nil {
				return err
			}
			authority := RunActionAuthority{
				RunID: "run-1", OrgID: f.org, SubjectID: owner, ThreadID: "thread-1", SpaceRef: ref,
				RecipientAudienceRef:       evidence.RecipientAudienceRef,
				RecipientAudienceRevision:  evidence.Membership.Revisions.RecipientAudience,
				RecipientAudienceHash:      evidence.RecipientAudienceHash,
				PrivacyPolicyRef:           evidence.Privacy.PolicyRef,
				RunContextAuthorizationRef: evidence.ResourceAuthorizationRef,
				AuthorityRevision:          evidence.Membership.Revisions.Authority,
				RunStatus:                  "running",
			}
			request := RunActionDecisionRequest{
				ActionID: "tickets.create", ActionSchemaHash: ticketsCreateActionSchemaSHA256, PayloadDigest: deprovisionTestDigest,
				IdempotencyKey: "idem-run-action", DecisionRef: "dr-run-action", Nonce: "nonce-run-action",
			}
			_, err = IssueRunActionDecision(evidence, authority, request, time.Now().UTC())
			return err
		}},
	}

	const wantRoutes = 15
	if len(routes) != wantRoutes {
		t.Fatalf("effect-bound issuer routes = %d, want %d (tests_owed names exactly these 15: thread, "+
			"thread-append, thread-read, retrieval, import, import-execution, schedule-create, schedule-fire, "+
			"watch-create, watch-observe, sandbox-capability, scheduled-run, scheduled-step, owner-grant, run-action)",
			len(routes), wantRoutes)
	}

	// Baseline: every route succeeds for the active, provisioned owner. This
	// also proves the fixture itself is valid before the refusal is asserted
	// below — a route that fails here for the wrong reason (missing
	// entitlement, bad request shape, …) would otherwise be indistinguishable
	// from a true membership refusal.
	for _, r := range routes {
		if err := r.resolve(ctx); err != nil {
			t.Fatalf("%s: baseline (active member) must succeed before deprovisioning is tested: %v", r.name, err)
		}
	}

	// Deprovision: flipping user_org_memberships.status to 'removed' must make
	// EVERY route above refuse, through the identical ErrNoCurrentMembership
	// every evidence resolver's EXISTS(...status='active') clause produces.
	f.setOrgMembershipStatus(t, owner, "removed")
	for _, r := range routes {
		if err := r.resolve(ctx); !errors.Is(err, ErrNoCurrentMembership) {
			t.Errorf("%s: removed user was not refused with ErrNoCurrentMembership: %v", r.name, err)
		}
	}

	// Reinstate: the exact same user, in the exact same Space, on every one of
	// the same routes, must succeed again once status is back to 'active'.
	f.setOrgMembershipStatus(t, owner, "active")
	for _, r := range routes {
		if err := r.resolve(ctx); err != nil {
			t.Errorf("%s: reinstated user was refused: %v", r.name, err)
		}
	}
}
