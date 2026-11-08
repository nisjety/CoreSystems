package directory

import (
	"context"
	"errors"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

// WS04-S3 live proofs for the people directory SQL. Gated on
// TEST_DATABASE_URL (a disposable Postgres; migrations are applied here).

type peopleFixture struct {
	pool   *pgxpool.Pool
	store  *Store
	org    string
	suffix string
}

func newPeopleFixture(t *testing.T) *peopleFixture {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live people directory test")
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
	f := &peopleFixture{pool: pool, store: NewStore(&database.DB{Pool: pool}), org: "ws04s3-org-" + suffix, suffix: suffix}
	t.Cleanup(func() {
		_, _ = pool.Exec(ctx, `DELETE FROM space_memberships WHERE space_ref LIKE 'ws04s3-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM space_authority_revisions WHERE space_ref LIKE 'ws04s3-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM registered_spaces WHERE space_ref LIKE 'ws04s3-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM user_org_memberships WHERE org_id LIKE 'ws04s3-%-'||$1`, suffix)
		_, _ = pool.Exec(ctx, `DELETE FROM users WHERE id LIKE 'ws04s3-%-'||$1`, suffix)
	})
	return f
}

func (f *peopleFixture) exec(t *testing.T, query string, args ...any) {
	t.Helper()
	if _, err := f.pool.Exec(context.Background(), query, args...); err != nil {
		t.Fatalf("%s: %v", query, err)
	}
}

// person seeds a user and (optionally) an org membership; it returns the user id.
func (f *peopleFixture) person(t *testing.T, key, name, email, userStatus, orgID, memberStatus string) string {
	t.Helper()
	id := "ws04s3-" + key + "-" + f.suffix
	f.exec(t, `INSERT INTO users (id, email, name, status) VALUES ($1,$2,$3,$4)`, id, email+"."+f.suffix+"@ws04s3.invalid", name, userStatus)
	if orgID != "" {
		f.exec(t, `INSERT INTO user_org_memberships (user_id, org_id, role, status) VALUES ($1,$2,'member',$3)`, id, orgID, memberStatus)
	}
	return id
}

func (f *peopleFixture) member(t *testing.T, key, name, email string) string {
	return f.person(t, key, name, email, "active", f.org, "active")
}

func (f *peopleFixture) room(t *testing.T, key, org, state string, members ...string) string {
	t.Helper()
	ref := "ws04s3-" + key + "-" + f.suffix
	f.exec(t, `INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id,
		application_lifecycle_revision, registration_state) VALUES ($1,$2,'room','ws04s3-owner',1,$3)`, ref, org, state)
	f.exec(t, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, ref)
	for _, m := range members {
		f.exec(t, `INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by) VALUES ($1,'user',$2,'editor','ws04s3-test')`, ref, m)
	}
	return ref
}

func ids(people []Person) []string {
	out := make([]string, len(people))
	for i, p := range people {
		out[i] = p.SubjectID
	}
	return out
}

func TestSearchPeopleListsOnlyActiveOrgMembersWithActiveUsersLive(t *testing.T) {
	f := newPeopleFixture(t)
	caller := f.member(t, "caller", "Caller Person", "caller")
	active := f.member(t, "active", "Active Person", "active")
	f.person(t, "suspended-org", "Suspended Org", "so", "active", f.org, "suspended")
	f.person(t, "inactive-user", "Inactive User", "iu", "inactive", f.org, "active")
	f.person(t, "blocked-user", "Blocked User", "bu", "blocked", f.org, "active")
	f.person(t, "other-org", "Other Org", "oo", "active", "ws04s3-other-"+f.suffix, "active")
	f.person(t, "no-org", "No Org", "no", "active", "", "")

	result, err := f.store.SearchPeople(context.Background(), PeopleQuery{OrgID: f.org, CallerID: caller})
	if err != nil {
		t.Fatalf("SearchPeople: %v", err)
	}
	got := map[string]Person{}
	for _, p := range result.People {
		got[p.SubjectID] = p
	}
	if len(got) != 2 || got[caller].SubjectID == "" || got[active].SubjectID == "" {
		t.Fatalf("listed %v, want exactly the caller and the active member", ids(result.People))
	}
	if !got[caller].IsSelf || got[active].IsSelf {
		t.Fatalf("is_self wrong: %+v", result.People)
	}
	if got[active].DisplayName != "Active Person" || got[active].Email == "" {
		t.Fatalf("person fields = %+v", got[active])
	}
	if result.NextCursor != "" {
		t.Fatalf("a single page has a next cursor: %q", result.NextCursor)
	}
}

func TestSearchPeopleMatchesNameOrEmailCaseInsensitivelyWithLiteralWildcardsLive(t *testing.T) {
	f := newPeopleFixture(t)
	caller := f.member(t, "caller", "Zed Caller", "zedcaller")
	ada := f.member(t, "ada", "Ada Lovelace", "ada.l")
	pct := f.member(t, "pct", "Fifty%Percent", "pct")
	under := f.member(t, "under", "Under_Score", "under")
	f.member(t, "plain", "Fiftyxpercent Underxscore", "plain")
	byEmail := f.person(t, "mail", "Quiet Person", "grace.hopper", "active", f.org, "active")

	search := func(q string) []string {
		t.Helper()
		result, err := f.store.SearchPeople(context.Background(), PeopleQuery{OrgID: f.org, CallerID: caller, Q: q})
		if err != nil {
			t.Fatalf("SearchPeople(%q): %v", q, err)
		}
		return ids(result.People)
	}
	contains := func(list []string, want string) bool {
		for _, v := range list {
			if v == want {
				return true
			}
		}
		return false
	}

	if got := search("LOVELACE"); len(got) != 1 || got[0] != ada {
		t.Errorf("case-insensitive name match = %v", got)
	}
	if got := search("GRACE.HOPPER"); len(got) != 1 || got[0] != byEmail {
		t.Errorf("case-insensitive email match = %v", got)
	}
	// % and _ are literals, not wildcards.
	if got := search("%"); len(got) != 1 || got[0] != pct {
		t.Errorf("literal %% matched %v, want only the name containing a percent sign", got)
	}
	if got := search("_"); !contains(got, under) || contains(got, ada) {
		t.Errorf("literal _ matched %v", got)
	}
	if got := search("fifty%percent"); len(got) != 1 || got[0] != pct {
		t.Errorf("literal %% inside a word = %v", got)
	}
	if got := search(`\`); len(got) != 0 {
		t.Errorf("a lone backslash matched %v", got)
	}
	if got := search("nobody-has-this"); len(got) != 0 {
		t.Errorf("no-match query = %v", got)
	}
}

func TestSearchPeoplePrefixMatchesSortBeforeSubstringMatchesLive(t *testing.T) {
	f := newPeopleFixture(t)
	caller := f.member(t, "caller", "Zzz Caller", "zzzcaller")
	substring := f.member(t, "sub", "Aaron Mariana", "sub") // "ana" inside the name
	prefixLate := f.member(t, "pl", "Anabel Zane", "pl")    // prefix, later alphabetically
	prefixEarly := f.member(t, "pe", "Ana Abbott", "pe")    // prefix, earliest
	emailPrefix := f.person(t, "ep", "Mid Person", "anaemail", "active", f.org, "active")

	result, err := f.store.SearchPeople(context.Background(), PeopleQuery{OrgID: f.org, CallerID: caller, Q: "ana"})
	if err != nil {
		t.Fatalf("SearchPeople: %v", err)
	}
	got := ids(result.People)
	if len(got) != 4 {
		t.Fatalf("results = %v", got)
	}
	// Rank 0 first (name or email prefix), by lower(name); the substring match last.
	want := []string{prefixEarly, prefixLate, emailPrefix, substring}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("order = %v, want %v", got, want)
		}
	}
}

func TestSearchPeopleKeysetPagingHasNoDuplicatesOrGapsLive(t *testing.T) {
	f := newPeopleFixture(t)
	caller := f.member(t, "caller", "Caller 00", "c00")
	want := map[string]bool{caller: true}
	for i := 1; i <= 6; i++ {
		id := f.member(t, fmt.Sprintf("p%02d", i), fmt.Sprintf("Person %02d", i), fmt.Sprintf("p%02d", i))
		want[id] = true
	}
	// 7 people, pages of 3: 3 + 3 + 1.
	seen := map[string]int{}
	cursor := ""
	pages := 0
	for {
		result, err := f.store.SearchPeople(context.Background(), PeopleQuery{OrgID: f.org, CallerID: caller, Limit: 3, Cursor: cursor})
		if err != nil {
			t.Fatalf("page %d: %v", pages, err)
		}
		pages++
		if len(result.People) > 3 {
			t.Fatalf("page %d has %d people", pages, len(result.People))
		}
		for _, p := range result.People {
			seen[p.SubjectID]++
		}
		if result.NextCursor == "" {
			break
		}
		cursor = result.NextCursor
		if pages > 5 {
			t.Fatal("paging did not terminate")
		}
	}
	if pages != 3 {
		t.Fatalf("pages = %d, want 3", pages)
	}
	for id := range want {
		if seen[id] != 1 {
			t.Errorf("person %s seen %d times across pages", id, seen[id])
		}
	}
	if len(seen) != len(want) {
		t.Errorf("paging listed %d people, want %d", len(seen), len(want))
	}
}

func TestSearchPeopleExcludeMemberOfAppliesOnlyForAMemberCallerLive(t *testing.T) {
	f := newPeopleFixture(t)
	caller := f.member(t, "caller", "Caller", "caller")
	inRoom := f.member(t, "inroom", "In Room", "inroom")
	outside := f.member(t, "outside", "Outside", "outside")
	room := f.room(t, "room", f.org, "active", caller, inRoom)
	// A room the caller is NOT in, and a room of another org.
	foreign := f.room(t, "foreign", f.org, "active", inRoom)
	otherOrgRoom := f.room(t, "otherorg", "ws04s3-other-"+f.suffix, "active", caller, inRoom)
	draining := f.room(t, "draining", f.org, "deleting", caller, inRoom)

	list := func(exclude string) []string {
		result, err := f.store.SearchPeople(context.Background(), PeopleQuery{OrgID: f.org, CallerID: caller, ExcludeMemberOf: exclude})
		if err != nil {
			t.Fatalf("SearchPeople(exclude=%q): %v", exclude, err)
		}
		return ids(result.People)
	}
	has := func(list []string, id string) bool {
		for _, v := range list {
			if v == id {
				return true
			}
		}
		return false
	}

	// The caller is a member of the room: its members are removed from the picker.
	got := list(room)
	if has(got, caller) || has(got, inRoom) || !has(got, outside) {
		t.Fatalf("exclude a room the caller is in = %v", got)
	}
	// Otherwise the parameter is ignored, so it cannot probe other rosters.
	for name, ref := range map[string]string{
		"a room the caller is not in": foreign,
		"a room of another org":       otherOrgRoom,
		"a Space that is not active":  draining,
		"a Space that does not exist": "ws04s3-missing-" + f.suffix,
	} {
		got := list(ref)
		if !has(got, inRoom) || !has(got, caller) || !has(got, outside) {
			t.Errorf("exclude_member_of %s changed the result: %v", name, got)
		}
	}
}

func TestSearchPeopleCallerOutsideTheOrgIsRefusedLive(t *testing.T) {
	f := newPeopleFixture(t)
	f.member(t, "member", "A Member", "member")
	outsider := f.person(t, "outsider", "Outsider", "outsider", "active", "ws04s3-other-"+f.suffix, "active")
	suspended := f.person(t, "suspended", "Suspended", "suspended", "active", f.org, "suspended")
	for name, caller := range map[string]string{
		"member of another org": outsider,
		"suspended org member":  suspended,
		"unknown user":          "ws04s3-nobody-" + f.suffix,
	} {
		_, err := f.store.SearchPeople(context.Background(), PeopleQuery{OrgID: f.org, CallerID: caller})
		if !errors.Is(err, ErrCallerNotInOrg) {
			t.Errorf("%s: err = %v, want ErrCallerNotInOrg", name, err)
		}
	}
}
