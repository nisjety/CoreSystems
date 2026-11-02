package spaces

import (
	"context"
	"fmt"
	"os"
	"reflect"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

// WS03-S2 live proof for DiagnoseDenial: the one read that labels a refusal
// whose authoritative evidence query returned no row. Gated on
// TEST_DATABASE_URL (a disposable Postgres; migrations are applied here).

type denialLiveFixture struct {
	pool   *pgxpool.Pool
	repo   *Repository
	org    string
	suffix string
}

func newDenialLiveFixture(t *testing.T) *denialLiveFixture {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live DiagnoseDenial test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	t.Cleanup(pool.Close)
	if err := (&database.DB{Pool: pool}).RunMigrations(ctx, "../../migrations"); err != nil {
		t.Fatalf("migrate: %v", err)
	}
	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	f := &denialLiveFixture{pool: pool, repo: NewRepository(&database.DB{Pool: pool}), org: "ws03s2-org-" + suffix, suffix: suffix}
	t.Cleanup(func() {
		_, _ = pool.Exec(ctx, `DELETE FROM space_recipient_audience_members WHERE space_ref LIKE 'ws03s2-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM space_recipient_audiences WHERE space_ref LIKE 'ws03s2-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM space_memberships WHERE space_ref LIKE 'ws03s2-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM space_authority_revisions WHERE space_ref LIKE 'ws03s2-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM registered_spaces WHERE space_ref LIKE 'ws03s2-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM user_org_memberships WHERE org_id = $1`, f.org)
		_, _ = pool.Exec(ctx, `DELETE FROM users WHERE id LIKE 'ws03s2-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM space_effect_policies WHERE org_id = $1`, f.org)
	})
	return f
}

func (f *denialLiveFixture) exec(t *testing.T, query string, args ...any) {
	t.Helper()
	if _, err := f.pool.Exec(context.Background(), query, args...); err != nil {
		t.Fatalf("%s: %v", query, err)
	}
}

func (f *denialLiveFixture) user(t *testing.T, name string, orgActive bool) string {
	t.Helper()
	id := "ws03s2-" + name + "-" + f.suffix
	f.exec(t, `INSERT INTO users (id, email, name) VALUES ($1, $1 || '@ws03s2.invalid', $1)`, id)
	status := "active"
	if !orgActive {
		status = "suspended"
	}
	f.exec(t, `INSERT INTO user_org_memberships (user_id, org_id, role, status) VALUES ($1,$2,'member',$3)`, id, f.org, status)
	return id
}

func (f *denialLiveFixture) space(t *testing.T, name, kind, state string) string {
	t.Helper()
	ref := "ws03s2-" + name + "-" + f.suffix
	f.exec(t, `
		INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id,
		    application_lifecycle_revision, registration_state)
		VALUES ($1,$2,$3,'ws03s2-owner',1,$4)`, ref, f.org, kind, state)
	f.exec(t, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, ref)
	return ref
}

func (f *denialLiveFixture) member(t *testing.T, ref, subject string) {
	t.Helper()
	f.exec(t, `INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by)
		VALUES ($1,'user',$2,'editor','ws03s2-test')`, ref, subject)
}

func (f *denialLiveFixture) audience(t *testing.T, ref string, subjects ...string) {
	t.Helper()
	f.exec(t, `INSERT INTO space_recipient_audiences (space_ref, revision, audience_ref, audience_hash, recipient_count)
		VALUES ($1, 1, 'audience:'||$1||':1', 'sha256:ws03s2', $2)`, ref, len(subjects))
	f.exec(t, `UPDATE space_authority_revisions SET recipient_audience_revision = 1 WHERE space_ref = $1`, ref)
	for _, subject := range subjects {
		f.exec(t, `INSERT INTO space_recipient_audience_members (space_ref, revision, subject_id) VALUES ($1, 1, $2)`, ref, subject)
	}
}

func TestDiagnoseDenialForEachFlagCombinationLive(t *testing.T) {
	f := newDenialLiveFixture(t)
	ctx := context.Background()
	repo := f.repo
	// Seeded by SQL: the policy audit outbox is not wired in this package's tests.
	f.exec(t, `INSERT INTO space_effect_policies (org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
		retention_class, residency, deletion_scope) VALUES ($1,'privacy:'||$1||':1','assistant_collaboration','contract','internal','standard','swedencentral','space')`, f.org)

	member := f.user(t, "member", true)
	stranger := f.user(t, "stranger", true)
	suspended := f.user(t, "suspended", false)

	room := f.space(t, "room", "room", "active")
	f.member(t, room, member)
	f.member(t, room, suspended)
	f.audience(t, room, member)
	personal := f.space(t, "personal", "personal", "active")
	f.member(t, personal, member)
	deleting := f.space(t, "deleting", "room", "deleting")
	f.member(t, deleting, member)
	f.audience(t, deleting, member)
	notInAudience := f.space(t, "outside", "room", "active")
	f.member(t, notInAudience, member)
	f.audience(t, notInAudience, stranger)

	type row struct {
		name    string
		space   string
		subject string
		expect  ExpectKind
		code    DenialCode
	}
	rows := []row{
		{"non member of a real Space", room, stranger, ExpectAny, DenialMembershipRequired},
		{"member suspended in the org", room, suspended, ExpectAny, DenialMembershipRequired},
		{"not active", deleting, member, ExpectAny, DenialNotActive},
		{"personal decision on a room", room, member, ExpectPersonal, DenialKindUnsupported},
		{"shared decision on a personal Space", personal, member, ExpectShared, DenialKindUnsupported},
		{"member not in the current audience", notInAudience, member, ExpectShared, DenialAudienceNotCurrent},
		{"everything resolves", room, member, ExpectShared, DenialAuthorityIncomplete},
		{"personal Space everything resolves", personal, member, ExpectPersonal, DenialAuthorityIncomplete},
		{"membership only on an active Space", room, member, ExpectMembershipOnly, DenialMembershipRequired},
	}
	for _, tc := range rows {
		denial, err := repo.DiagnoseDenial(ctx, tc.space, f.org, tc.subject, tc.expect)
		if err != nil || denial == nil {
			t.Fatalf("%s: denial=%v err=%v", tc.name, denial, err)
		}
		if denial.Code != tc.code {
			t.Errorf("%s: code %s, want %s", tc.name, denial.Code, tc.code)
		}
	}

	// No policy for the org: policy not provisioned.
	f.exec(t, `DELETE FROM space_effect_policies WHERE org_id = $1`, f.org)
	denial, err := repo.DiagnoseDenial(ctx, room, f.org, member, ExpectShared)
	if err != nil || denial.Code != DenialPolicyNotProvisioned {
		t.Fatalf("no policy: denial=%v err=%v, want space_policy_not_provisioned", denial, err)
	}
}

func TestDiagnoseDenialNonMemberOfRealAndMissingSpaceAreByteEqualLive(t *testing.T) {
	f := newDenialLiveFixture(t)
	ctx := context.Background()
	member := f.user(t, "member", true)
	stranger := f.user(t, "stranger", true)
	room := f.space(t, "room", "room", "active")
	f.member(t, room, member)

	real, err := f.repo.DiagnoseDenial(ctx, room, f.org, stranger, ExpectShared)
	if err != nil {
		t.Fatalf("real Space: %v", err)
	}
	missing, err := f.repo.DiagnoseDenial(ctx, "ws03s2-does-not-exist-"+f.suffix, f.org, stranger, ExpectShared)
	if err != nil {
		t.Fatalf("missing Space: %v", err)
	}
	otherOrg, err := f.repo.DiagnoseDenial(ctx, room, "ws03s2-other-org-"+f.suffix, stranger, ExpectShared)
	if err != nil {
		t.Fatalf("other org: %v", err)
	}
	for name, got := range map[string]*Denial{"missing Space": missing, "other org": otherOrg} {
		if got.Code != real.Code || got.Error() != real.Error() || got.Status() != real.Status() ||
			got.ResolvableBy() != real.ResolvableBy() || !reflect.DeepEqual(got.Details(), real.Details()) {
			t.Fatalf("%s is distinguishable from a non-member of a real Space: %+v vs %+v", name, got, real)
		}
	}
	if real.Code != DenialMembershipRequired {
		t.Fatalf("non-member code = %s", real.Code)
	}
}

func TestDiagnoseDenialFailsClosedWithoutADatabase(t *testing.T) {
	var repo *Repository
	if _, err := repo.DiagnoseDenial(context.Background(), "s", "o", "u", ExpectAny); err == nil {
		t.Fatal("a nil repository diagnosed a denial")
	}
	if _, err := (&Repository{}).DiagnoseDenial(context.Background(), "s", "o", "u", ExpectAny); err == nil {
		t.Fatal("a repository with no database diagnosed a denial")
	}
}
