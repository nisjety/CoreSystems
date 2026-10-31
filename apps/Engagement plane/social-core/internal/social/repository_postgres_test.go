package social

import (
	"context"
	"errors"
	"os"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/database"
)

// TestPGRepositoryOrgScopingRejectsCrossOrgAccess is a real, Postgres-backed
// integration test against the actual PGRepository implementation. Every
// other test in this package (service_test.go, catalog_test.go,
// publisher_test.go, metrics_test.go) exercises the hand-rolled
// fakeRepository, which can never prove that PGRepository's SQL-level
// `WHERE org_id = $1 AND id = $2` scoping (see getApprovalTx and
// UpdatePostSchedule in repository.go) genuinely rejects a cross-org access
// attempt instead of just happening to look right.
//
// This test seeds an approval and a post that belong to org_2 directly via
// SQL, then calls the real PGRepository.DecideApproval and
// PGRepository.UpdatePostSchedule as org_1 and asserts:
//  1. the call returns ErrNotFound rather than mutating org_2's row, and
//  2. a direct SQL read confirms org_2's row was left untouched, and
//  3. the same call still succeeds when made with the correct, owning org_2.
//
// It is gated behind SOCIAL_CORE_TEST_DATABASE_URL and skipped when unset.
// The caller must point that env var at a disposable, throwaway Postgres
// database -- this test applies social-core's real migrations
// (internal/database/migrations) and must never be pointed at a live or
// shared social-core database.
//
// Fixture rows use fixed ids (rather than randomly generated ones) so that
// the SQL seeded in this file is easy to read and cross-reference against
// the assertions below. Because the ids are fixed, this test resets its own
// fixtures both before and after running so it stays safe to re-run
// repeatedly against the same disposable database.
const (
	orgScopingDecidePostID     = "socpost_orgscope_decide"
	orgScopingDecideApprovalID = "socapr_orgscope_decide"
	orgScopingSchedulePostID   = "socpost_orgscope_schedule"
)

func TestPGRepositoryOrgScopingRejectsCrossOrgAccess(t *testing.T) {
	dsn := os.Getenv("SOCIAL_CORE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("SOCIAL_CORE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()

	if err := database.RunMigrations(ctx, db); err != nil {
		t.Fatalf("apply social-core migrations: %v", err)
	}

	repo := NewRepository(db.Pool)

	// social_approvals.post_id has ON DELETE CASCADE from social_posts, so
	// deleting the posts also removes any approval fixture rows. This runs
	// with the test's own ctx/db, both of which are still live here -- it
	// intentionally is NOT wired through t.Cleanup, because t.Cleanup
	// callbacks run only after this function's own `defer cancel()` and
	// `defer db.Close()` have already fired, which would hand the cleanup
	// query an already-canceled context and an already-closed pool.
	resetOrgScopingFixtures := func() {
		if _, err := db.Pool.Exec(ctx, `DELETE FROM social_posts WHERE id = ANY($1)`,
			[]string{orgScopingDecidePostID, orgScopingSchedulePostID}); err != nil {
			t.Fatalf("reset org-scoping fixtures: %v", err)
		}
	}
	resetOrgScopingFixtures()

	t.Run("DecideApproval", func(t *testing.T) {
		testDecideApprovalOrgScoping(ctx, t, db, repo)
	})
	t.Run("UpdatePostSchedule", func(t *testing.T) {
		testUpdatePostScheduleOrgScoping(ctx, t, db, repo)
	})

	resetOrgScopingFixtures()
}

func testDecideApprovalOrgScoping(ctx context.Context, t *testing.T, db *database.DB, repo *PGRepository) {
	const (
		owningOrg   = "org_2"
		attackerOrg = "org_1"
	)
	postID := orgScopingDecidePostID
	approvalID := orgScopingDecideApprovalID

	if _, err := db.Pool.Exec(ctx, `
INSERT INTO social_posts (
	id, org_id, title, body, status, approval_required, approval_state,
	created_by_user_id, updated_by_user_id
) VALUES (
	$1, $2, 'org-scoping fixture', 'seeded directly by repository_postgres_test.go',
	'draft', TRUE, 'pending', $3, $3
)`, postID, owningOrg, "seed-actor"); err != nil {
		t.Fatalf("seed org_2 post: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO social_approvals (id, org_id, post_id, state, requested_by_user_id)
VALUES ($1, $2, $3, 'pending', $4)
`, approvalID, owningOrg, postID, "seed-actor"); err != nil {
		t.Fatalf("seed org_2 approval: %v", err)
	}

	// Cross-org: org_1 must not be able to decide org_2's approval.
	decidedAt := time.Now().UTC()
	approval, err := repo.DecideApproval(ctx, DecideApprovalInput{
		OrgID:          attackerOrg,
		ApprovalID:     approvalID,
		Decision:       ApprovalApproved,
		DecisionReason: "cross-org attempt",
		ActorUserID:    "attacker-actor",
		DecidedAt:      decidedAt,
	})
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("cross-org DecideApproval error = %v, want ErrNotFound", err)
	}
	if approval != nil {
		t.Fatalf("cross-org DecideApproval returned a non-nil approval: %+v", approval)
	}

	// Assert org_2's row was NOT mutated by the rejected cross-org attempt.
	var state, decidedBy string
	var decidedAtCol *time.Time
	if err := db.Pool.QueryRow(ctx, `
SELECT state, decided_by_user_id, decided_at FROM social_approvals WHERE org_id = $1 AND id = $2
`, owningOrg, approvalID).Scan(&state, &decidedBy, &decidedAtCol); err != nil {
		t.Fatalf("read org_2 approval after cross-org attempt: %v", err)
	}
	if state != ApprovalPending || decidedBy != "" || decidedAtCol != nil {
		t.Fatalf("org_2 approval mutated by cross-org attempt: state=%q decided_by=%q decided_at=%v", state, decidedBy, decidedAtCol)
	}
	var postApprovalState, postUpdatedBy string
	if err := db.Pool.QueryRow(ctx, `
SELECT approval_state, updated_by_user_id FROM social_posts WHERE org_id = $1 AND id = $2
`, owningOrg, postID).Scan(&postApprovalState, &postUpdatedBy); err != nil {
		t.Fatalf("read org_2 post after cross-org attempt: %v", err)
	}
	if postApprovalState != ApprovalPending || postUpdatedBy != "seed-actor" {
		t.Fatalf("org_2 post mutated by cross-org attempt: approval_state=%q updated_by_user_id=%q", postApprovalState, postUpdatedBy)
	}

	// Positive case: the legitimate owning org can still decide its own
	// approval -- proving the cross-org rejection above is not vacuous.
	approval, err = repo.DecideApproval(ctx, DecideApprovalInput{
		OrgID:          owningOrg,
		ApprovalID:     approvalID,
		Decision:       ApprovalApproved,
		DecisionReason: "legitimate decision",
		ActorUserID:    "owning-actor",
		DecidedAt:      decidedAt,
	})
	if err != nil {
		t.Fatalf("same-org DecideApproval failed: %v", err)
	}
	if approval == nil || approval.State != ApprovalApproved {
		t.Fatalf("same-org DecideApproval did not apply: %+v", approval)
	}
}

func testUpdatePostScheduleOrgScoping(ctx context.Context, t *testing.T, db *database.DB, repo *PGRepository) {
	const (
		owningOrg   = "org_2"
		attackerOrg = "org_1"
	)
	postID := orgScopingSchedulePostID

	if _, err := db.Pool.Exec(ctx, `
INSERT INTO social_posts (
	id, org_id, title, body, status, approval_required, approval_state,
	created_by_user_id, updated_by_user_id
) VALUES (
	$1, $2, 'org-scoping schedule fixture', 'seeded directly by repository_postgres_test.go',
	'draft', FALSE, 'not_required', $3, $3
)`, postID, owningOrg, "seed-actor"); err != nil {
		t.Fatalf("seed org_2 post for schedule: %v", err)
	}

	// Cross-org: org_1 must not be able to schedule org_2's post.
	attackerSchedule := time.Now().Add(24 * time.Hour).UTC()
	post, err := repo.UpdatePostSchedule(ctx, SchedulePostInput{
		OrgID:       attackerOrg,
		PostID:      postID,
		ScheduledAt: attackerSchedule,
		ActorUserID: "attacker-actor",
	})
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("cross-org UpdatePostSchedule error = %v, want ErrNotFound", err)
	}
	if post != nil {
		t.Fatalf("cross-org UpdatePostSchedule returned a non-nil post: %+v", post)
	}

	// Assert org_2's row was NOT mutated by the rejected cross-org attempt.
	var status, updatedBy string
	var scheduledAt *time.Time
	if err := db.Pool.QueryRow(ctx, `
SELECT status, scheduled_at, updated_by_user_id FROM social_posts WHERE org_id = $1 AND id = $2
`, owningOrg, postID).Scan(&status, &scheduledAt, &updatedBy); err != nil {
		t.Fatalf("read org_2 post after cross-org schedule attempt: %v", err)
	}
	if status != PostStatusDraft || scheduledAt != nil || updatedBy != "seed-actor" {
		t.Fatalf("org_2 post mutated by cross-org schedule attempt: status=%q scheduled_at=%v updated_by_user_id=%q", status, scheduledAt, updatedBy)
	}

	// Positive case: the legitimate owning org can still schedule its own
	// post -- proving the cross-org rejection above is not vacuous.
	ownerSchedule := time.Now().Add(48 * time.Hour).UTC()
	post, err = repo.UpdatePostSchedule(ctx, SchedulePostInput{
		OrgID:       owningOrg,
		PostID:      postID,
		ScheduledAt: ownerSchedule,
		ActorUserID: "owning-actor",
	})
	if err != nil {
		t.Fatalf("same-org UpdatePostSchedule failed: %v", err)
	}
	if post == nil || post.Status != PostStatusScheduled {
		t.Fatalf("same-org UpdatePostSchedule did not apply: %+v", post)
	}
}
