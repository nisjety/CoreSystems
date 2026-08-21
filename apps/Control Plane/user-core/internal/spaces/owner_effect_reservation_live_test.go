package spaces

import (
	"context"
	"errors"
	"fmt"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/jackc/pgx/v5/pgxpool"
)

// TestOwnerEffectReservationAuthorityRevisionCancelsOnlyUncommittedRows is a
// real-Postgres proof for the database half of the distributed fence. The
// Conversation owner still needs its own exact local-grant check, but this
// trigger is what makes a Control revocation win over an uncommitted effect.
func TestOwnerEffectReservationAuthorityRevisionCancelsOnlyUncommittedRows(t *testing.T) {
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live owner-effect reservation test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer pool.Close()
	if err := (&database.DB{Pool: pool}).RunMigrations(ctx, "../../migrations"); err != nil {
		t.Fatalf("migrate: %v", err)
	}

	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	spaceRef := "test-owner-effect-space-" + suffix
	reservedOperationID := "test-owner-effect-reserved-" + suffix
	committedOperationID := "test-owner-effect-committed-" + suffix
	t.Cleanup(func() {
		_, _ = pool.Exec(ctx, `DELETE FROM space_owner_effect_reservations WHERE space_ref=$1`, spaceRef)
		_, _ = pool.Exec(ctx, `DELETE FROM space_authority_revisions WHERE space_ref=$1`, spaceRef)
		_, _ = pool.Exec(ctx, `DELETE FROM registered_spaces WHERE space_ref=$1`, spaceRef)
	})

	if _, err := pool.Exec(ctx, `
		INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id, application_lifecycle_revision, registration_state)
		VALUES ($1,$2,'room',$3,1,'active')`, spaceRef, "test-owner-effect-org-"+suffix, "test-owner-effect-user-"+suffix); err != nil {
		t.Fatalf("seed space: %v", err)
	}
	if _, err := pool.Exec(ctx, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, spaceRef); err != nil {
		t.Fatalf("seed authority revision: %v", err)
	}
	insertReservation := func(operationID, reservationID, status string) {
		t.Helper()
		var committedAt any
		if status == "committed" {
			committedAt = time.Now().UTC()
		}
		if _, err := pool.Exec(ctx, `
			INSERT INTO space_owner_effect_reservations (
			 operation_id, reservation_id, org_id, space_ref, subject_id, run_id, thread_id,
			 action_id, action_schema_hash, payload_digest, idempotency_key, decision_ref, grant_ref,
			 recipient_audience_ref, recipient_audience_hash, recipient_audience_revision,
			 privacy_policy_ref, authority_revision, status, expires_at, committed_at
			) VALUES ($1,$2,$3,$4,$5,$6,$7,'tickets.create',$8,$9,$10,$11,$12,$13,$14,1,$15,1,$16,$17,$18)`,
			operationID, reservationID, "test-owner-effect-org-"+suffix, spaceRef, "test-owner-effect-user-"+suffix,
			"run-"+suffix, "thread-"+suffix,
			"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
			"sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
			"key-"+operationID, "decision-"+operationID, "grant-"+operationID,
			"audience-"+suffix, "audience-hash-"+suffix, "privacy-"+suffix,
			status, time.Now().UTC().Add(time.Minute), committedAt); err != nil {
			t.Fatalf("seed %s reservation: %v", status, err)
		}
	}
	insertReservation(reservedOperationID, "reservation-reserved-"+suffix, "reserved")
	insertReservation(committedOperationID, "reservation-committed-"+suffix, "committed")

	if _, err := pool.Exec(ctx, `UPDATE space_authority_revisions SET authority_revision=authority_revision+1 WHERE space_ref=$1`, spaceRef); err != nil {
		t.Fatalf("advance authority revision: %v", err)
	}
	for operationID, wantStatus := range map[string]string{
		reservedOperationID:  "cancelled",
		committedOperationID: "committed",
	} {
		var status, reason string
		var cancelledAt *time.Time
		if err := pool.QueryRow(ctx, `SELECT status, cancellation_reason, cancelled_at FROM space_owner_effect_reservations WHERE operation_id=$1`, operationID).Scan(&status, &reason, &cancelledAt); err != nil {
			t.Fatalf("read %s reservation: %v", operationID, err)
		}
		if status != wantStatus {
			t.Fatalf("reservation %s status=%q, want %q", operationID, status, wantStatus)
		}
		if wantStatus == "cancelled" && (cancelledAt == nil || reason != "control_authority_changed") {
			t.Fatalf("cancelled reservation evidence = reason:%q cancelled_at:%v", reason, cancelledAt)
		}
		if wantStatus == "committed" && (cancelledAt != nil || reason != "") {
			t.Fatalf("committed reservation was rewritten by later revocation: reason:%q cancelled_at:%v", reason, cancelledAt)
		}
	}
}

// TestOwnerEffectReservationRepositoryRoundTripAgainstRealPostgres proves the
// Control repository path, not only the migration trigger: exact reservation
// replay is idempotent, commit is durable, and a revision change cancels a
// still-reserved operation before it can be committed.
func TestOwnerEffectReservationRepositoryRoundTripAgainstRealPostgres(t *testing.T) {
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live owner-effect repository test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer pool.Close()
	db := &database.DB{Pool: pool}
	if err := db.RunMigrations(ctx, "../../migrations"); err != nil {
		t.Fatalf("migrate: %v", err)
	}

	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	orgID := "owner-effect-repo-org-" + suffix
	spaceRef := "owner-effect-repo-space-" + suffix
	subjectID := "owner-effect-repo-user-" + suffix
	email := "owner-effect-repo-" + suffix + "@example.invalid"
	if _, err := pool.Exec(ctx,
		`INSERT INTO users (id, email, name) VALUES ($1, $2, 'Owner Effect Proof')`, subjectID, email,
	); err != nil {
		t.Fatalf("seed user: %v", err)
	}
	t.Cleanup(func() {
		_, _ = pool.Exec(ctx, `DELETE FROM space_owner_effect_reservations WHERE space_ref=$1`, spaceRef)
		_, _ = pool.Exec(ctx, `DELETE FROM space_effect_policies WHERE org_id=$1`, orgID)
		_, _ = pool.Exec(ctx, `DELETE FROM space_memberships WHERE space_ref=$1`, spaceRef)
		_, _ = pool.Exec(ctx, `DELETE FROM space_authority_revisions WHERE space_ref=$1`, spaceRef)
		_, _ = pool.Exec(ctx, `DELETE FROM registered_spaces WHERE space_ref=$1`, spaceRef)
		_, _ = pool.Exec(ctx, `DELETE FROM user_org_memberships WHERE user_id=$1`, subjectID)
		_, _ = pool.Exec(ctx, `DELETE FROM users WHERE id=$1`, subjectID)
	})
	if _, err := pool.Exec(ctx,
		`INSERT INTO user_org_memberships (user_id, org_id, role, status) VALUES ($1,$2,'owner','active')`, subjectID, orgID,
	); err != nil {
		t.Fatalf("seed org membership: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id, application_lifecycle_revision, registration_state)
		VALUES ($1,$2,'personal',$3,1,'active')`, spaceRef, orgID, subjectID); err != nil {
		t.Fatalf("seed space: %v", err)
	}
	if _, err := pool.Exec(ctx, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, spaceRef); err != nil {
		t.Fatalf("seed authority revision: %v", err)
	}
	if _, err := pool.Exec(ctx,
		`INSERT INTO space_memberships (space_ref, subject_type, subject_id, role) VALUES ($1,'user',$2,'owner')`, spaceRef, subjectID,
	); err != nil {
		t.Fatalf("seed Space membership: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO space_effect_policies (
		 org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
		 third_party_processing_allowed, retention_class, residency, deletion_scope,
		 zero_data_retention, thread_create_entitled, agent_action_entitled
		) VALUES ($1,'privacy-proof','support','contract','internal',FALSE,'standard','eu','space',FALSE,TRUE,TRUE)`, orgID); err != nil {
		t.Fatalf("seed effect policy: %v", err)
	}

	audienceHash, err := RecipientAudienceHash(subjectID)
	if err != nil {
		t.Fatalf("audience hash: %v", err)
	}
	now := time.Now().UTC()
	decision := RunActionDecision{
		DecisionRef: "decision-owner-effect-" + suffix, RunID: "run-owner-effect-" + suffix,
		ThreadID: "thread-owner-effect-" + suffix, OrgID: orgID, SpaceRef: spaceRef,
		SubjectID: subjectID, ServiceAudience: ticketsCreateServiceAudience,
		ActionID: "tickets.create", ActionSchemaHash: "sha256:" + strings.Repeat("a", 64),
		PayloadDigest: "sha256:" + strings.Repeat("b", 64), IdempotencyKey: "ticket-" + suffix,
		RecipientAudienceRef: "personal:" + spaceRef + ":recipient:1", RecipientAudienceHash: audienceHash,
		RecipientAudienceRevision: 1, PrivacyPolicyRef: "privacy-proof",
		RunContextAuthorizationRef: "control:" + spaceRef + ":thread-create:1", AuthorityRevision: 1,
		Permissions: []string{ticketsCreateActionPermission}, Purpose: "support", LawfulBasis: "contract",
		PrivacyClass: "internal", RetentionClass: "standard", Residency: "eu", DeletionScope: "space",
		IssuedAt: now.Add(-time.Second), ExpiresAt: now.Add(time.Minute), Nonce: "nonce-" + suffix,
	}
	commitment := OwnerEffectReservationCommitment{
		OperationID: "operation-" + suffix, ActionID: decision.ActionID,
		ActionSchemaHash: decision.ActionSchemaHash, PayloadDigest: decision.PayloadDigest,
		IdempotencyKey: decision.IdempotencyKey, DecisionRef: decision.DecisionRef, GrantRef: "grant-" + suffix,
	}
	repository := NewRepository(db)
	first, err := repository.ReserveOwnerEffect(ctx, decision, commitment, now)
	if err != nil {
		t.Fatalf("reserve: %v", err)
	}
	if first.Status != "reserved" || first.ReservationID == "" {
		t.Fatalf("first reservation = %#v", first)
	}
	replay, err := repository.ReserveOwnerEffect(ctx, decision, commitment, now)
	if err != nil || replay.ReservationID != first.ReservationID || replay.Status != "reserved" {
		t.Fatalf("reservation replay = %#v/%v", replay, err)
	}
	committed, err := repository.CommitOwnerEffectReservation(ctx, first.ReservationID, decision, now)
	if err != nil || committed.Status != "committed" {
		t.Fatalf("commit = %#v/%v", committed, err)
	}
	committedReplay, err := repository.CommitOwnerEffectReservation(ctx, first.ReservationID, decision, now)
	if err != nil || committedReplay.Status != "committed" {
		t.Fatalf("committed replay = %#v/%v", committedReplay, err)
	}

	second := decision
	second.DecisionRef = "decision-owner-effect-revoked-" + suffix
	second.PayloadDigest = "sha256:" + strings.Repeat("c", 64)
	second.IdempotencyKey = "ticket-revoked-" + suffix
	second.Nonce = "nonce-revoked-" + suffix
	secondCommitment := commitment
	secondCommitment.OperationID = "operation-revoked-" + suffix
	secondCommitment.DecisionRef = second.DecisionRef
	secondCommitment.PayloadDigest = second.PayloadDigest
	secondCommitment.IdempotencyKey = second.IdempotencyKey
	secondCommitment.GrantRef = "grant-revoked-" + suffix
	reserved, err := repository.ReserveOwnerEffect(ctx, second, secondCommitment, now)
	if err != nil {
		t.Fatalf("reserve before revocation: %v", err)
	}
	if _, err := pool.Exec(ctx, `UPDATE space_authority_revisions SET authority_revision=authority_revision+1 WHERE space_ref=$1`, spaceRef); err != nil {
		t.Fatalf("advance authority revision: %v", err)
	}
	if _, err := repository.CommitOwnerEffectReservation(ctx, reserved.ReservationID, second, now); !errors.Is(err, ErrOwnerEffectReservationDenied) {
		t.Fatalf("commit after revocation = %v, want ErrOwnerEffectReservationDenied", err)
	}
	var status, reason string
	if err := pool.QueryRow(ctx,
		`SELECT status, cancellation_reason FROM space_owner_effect_reservations WHERE reservation_id=$1`, reserved.ReservationID,
	).Scan(&status, &reason); err != nil {
		t.Fatalf("read cancelled reservation: %v", err)
	}
	if status != "cancelled" || reason != "control_authority_changed" {
		t.Fatalf("revoked reservation = status:%q reason:%q", status, reason)
	}
}
