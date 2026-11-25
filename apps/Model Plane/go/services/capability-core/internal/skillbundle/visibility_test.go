package skillbundle

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// visibilityDB records the statements of a visible read and answers them with no rows. A read writes nothing, so any
// Exec or Begin fails the test.
type visibilityDB struct {
	t       *testing.T
	queries []string
	args    [][]any
}

func (db *visibilityDB) Begin(context.Context) (pgx.Tx, error) {
	db.t.Fatal("a visible read does not begin a transaction")
	return nil, nil
}

func (db *visibilityDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	db.t.Fatal("a visible read writes nothing")
	return pgconn.CommandTag{}, nil
}

func (db *visibilityDB) Query(_ context.Context, sql string, args ...any) (pgx.Rows, error) {
	db.queries = append(db.queries, sql)
	db.args = append(db.args, args)
	return visibilityRows{}, nil
}

func (db *visibilityDB) QueryRow(_ context.Context, sql string, args ...any) pgx.Row {
	db.queries = append(db.queries, sql)
	db.args = append(db.args, args)
	return retentionRow{err: pgx.ErrNoRows}
}

type visibilityRows struct{}

func (visibilityRows) Close()                                       {}
func (visibilityRows) Err() error                                   { return nil }
func (visibilityRows) CommandTag() pgconn.CommandTag                { return pgconn.CommandTag{} }
func (visibilityRows) FieldDescriptions() []pgconn.FieldDescription { return nil }
func (visibilityRows) Next() bool                                   { return false }
func (visibilityRows) Scan(...any) error                            { return pgx.ErrNoRows }
func (visibilityRows) Values() ([]any, error)                       { return nil, nil }
func (visibilityRows) RawValues() [][]byte                          { return nil }
func (visibilityRows) Conn() *pgx.Conn                              { return nil }

// The visibility rule is one text: the skill listing and get take it, with the placeholders they number. Pinned here so
// a change to the rule shows up in one place.
func TestSkillVisibilitySQLIsTheListingRule(t *testing.T) {
	want := "(scope = 'org' OR owner_user_id = $2 OR (review_state = 'accepted' AND (shared_with ? $2 OR ($3 AND jsonb_array_length(shared_with) > 0))))"
	if got := SkillVisibilitySQL("$2", "$3"); got != want {
		t.Fatalf("SkillVisibilitySQL = %q\nwant %q", got, want)
	}
}

// Only the literal "true" turns the scope rules on, so a typo or a stray "1" leaves every bundle route as it was.
func TestScopeRulesIsTheLiteralTrue(t *testing.T) {
	for raw, want := range map[string]bool{"true": true, "1": false, "TRUE": false, " true": false, "yes": false, "": false} {
		if got := ScopeRulesFromLookup(func(string) string { return raw }); got != want {
			t.Errorf("SKILL_BUNDLE_SCOPE_RULES=%q enabled = %v, want %v", raw, got, want)
		}
	}
}

// The listing names the caller and the admin flag as its arguments, after the org, the skill filter and the deck
// template path, and reads only the bundles whose skill the rule admits.
func TestListVisibleNamesTheCallerAndSeesNoneWithoutOne(t *testing.T) {
	db := &visibilityDB{t: t}
	store := NewStore(db)
	if _, err := store.ListVisible(context.Background(), "org-a", "user-a", true, []string{"skill-1"}); err != nil {
		t.Fatalf("ListVisible: %v", err)
	}
	if len(db.queries) != 1 {
		t.Fatalf("ListVisible issued %d statements, want 1", len(db.queries))
	}
	if !strings.Contains(db.queries[0], SkillVisibilitySQL("$4", "$5")) {
		t.Fatalf("the listing does not apply the visibility rule:\n%s", db.queries[0])
	}
	args := db.args[0]
	if len(args) != 5 || args[0] != "org-a" || args[2] != ThemePath || args[3] != "user-a" || args[4] != true {
		t.Fatalf("ListVisible args = %#v", args)
	}

	none := &visibilityDB{t: t}
	summaries, err := NewStore(none).ListVisible(context.Background(), "org-a", "", true, nil)
	if err != nil || len(summaries) != 0 || len(none.queries) != 0 {
		t.Fatalf("a caller with no id = %v, %v after %d statements; want nothing and no read", summaries, err, len(none.queries))
	}
}

// A manifest for a person names the caller and the admin flag, the live bundle when no digest is named, and a kept one
// when it is. A skill the caller may not see, or a caller with no id, is ErrNotFound and reads nothing.
func TestManifestVisibleNamesTheCallerAndFailsClosed(t *testing.T) {
	db := &visibilityDB{t: t}
	_, err := NewStore(db).ManifestVisible(context.Background(), "org-a", "user-a", false, "skill-1", "")
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("a skill the caller may not see = %v, want ErrNotFound", err)
	}
	if !strings.Contains(db.queries[0], "b.state = 'live'") || !strings.Contains(db.queries[0], SkillVisibilitySQL("$3", "$4")) {
		t.Fatalf("the live manifest read is not the visible one:\n%s", db.queries[0])
	}
	args := db.args[0]
	if len(args) != 5 || args[0] != "org-a" || args[1] != "skill-1" || args[2] != "user-a" || args[3] != false || args[4] != "" {
		t.Fatalf("ManifestVisible args = %#v", args)
	}

	kept := &visibilityDB{t: t}
	digest := strings.Repeat("a", 64)
	if _, err := NewStore(kept).ManifestVisible(context.Background(), "org-a", "user-a", true, "skill-1", digest); !errors.Is(err, ErrNotFound) {
		t.Fatalf("a kept bundle the caller may not see = %v, want ErrNotFound", err)
	}
	if args := kept.args[0]; args[4] != digest || args[3] != true {
		t.Fatalf("kept manifest args = %#v, want the digest and the admin flag", args)
	}

	none := &visibilityDB{t: t}
	if _, err := NewStore(none).ManifestVisible(context.Background(), "org-a", "", true, "skill-1", ""); !errors.Is(err, ErrNotFound) || len(none.queries) != 0 {
		t.Fatalf("a caller with no id = %v after %d statements; want ErrNotFound and no read", err, len(none.queries))
	}
}
