package directory

import (
	"context"
	"encoding/base64"
	"errors"
	"regexp"
	"strings"
	"testing"
)

// WS04-S3: the people directory. Pure logic here; the SQL is in
// people_live_test.go (gated on TEST_DATABASE_URL).

func TestEscapeLikeEscapesBackslashPercentAndUnderscore(t *testing.T) {
	cases := map[string]string{
		"plain":      "plain",
		"50%":        `50\%`,
		"a_b":        `a\_b`,
		`back\slash`: `back\\slash`,
		`%_\`:        `\%\_\\`,
		"":           "",
		"o'brien":    "o'brien",
		"ünï_cødé%":  `ünï\_cødé\%`,
	}
	for in, want := range cases {
		if got := escapeLike(in); got != want {
			t.Errorf("escapeLike(%q) = %q, want %q", in, got, want)
		}
	}
	// The backslash is escaped first, so an escaped wildcard cannot be
	// un-escaped by the user's own backslash.
	if got := escapeLike(`\%`); got != `\\\%` {
		t.Errorf(`escapeLike("\%%") = %q`, got)
	}
}

func TestCursorRoundTripAndRankRange(t *testing.T) {
	for _, original := range []peopleCursor{
		{Rank: 0, LName: "ada lovelace", ID: "user-1"},
		{Rank: 1, LName: "ünï cødé", ID: "user-ü"},
		{Rank: 0, LName: "", ID: "u"},
	} {
		decoded, err := decodeCursor(encodeCursor(original))
		if err != nil || decoded != original {
			t.Errorf("round trip of %+v = %+v, %v", original, decoded, err)
		}
	}
	encode := func(raw string) string { return base64.RawURLEncoding.EncodeToString([]byte(raw)) }
	for name, token := range map[string]string{
		"rank 2":                 encode(`{"r":2,"n":"x","i":"u"}`),
		"rank -1":                encode(`{"r":-1,"n":"x","i":"u"}`),
		"rank 99":                encode(`{"r":99,"n":"x","i":"u"}`),
		"missing id":             encode(`{"r":0,"n":"x"}`),
		"empty id":               encode(`{"r":0,"n":"x","i":""}`),
		"not json":               encode(`nope`),
		"json but not an object": encode(`[1,2]`),
		"not base64":             "!!!not-base64!!!",
		"empty":                  "",
		"padded base64 std":      "eyJyIjowLCJuIjoieCIsImkiOiJ1In0=",
		"oversized":              encode(`{"r":0,"n":"` + strings.Repeat("x", 600) + `","i":"u"}`),
	} {
		if _, err := decodeCursor(token); !errors.Is(err, ErrInvalidRequest) {
			t.Errorf("%s: decodeCursor err = %v, want ErrInvalidRequest", name, err)
		}
	}
}

// SearchPeople validates the request before it touches the database, so these
// run against a nil store.
func TestSearchPeopleValidatesBeforeTheDatabase(t *testing.T) {
	var store *Store
	base := PeopleQuery{OrgID: "org-1", CallerID: "user-1"}

	cases := []struct {
		name  string
		query func(PeopleQuery) PeopleQuery
	}{
		{"q over 100 runes", func(q PeopleQuery) PeopleQuery { q.Q = strings.Repeat("a", MaxQueryRunes+1); return q }},
		{"q over 100 multibyte runes", func(q PeopleQuery) PeopleQuery { q.Q = strings.Repeat("ü", MaxQueryRunes+1); return q }},
		{"limit over the maximum", func(q PeopleQuery) PeopleQuery { q.Limit = MaxPeopleLimit + 1; return q }},
		{"negative limit", func(q PeopleQuery) PeopleQuery { q.Limit = -1; return q }},
		{"malformed cursor", func(q PeopleQuery) PeopleQuery { q.Cursor = "garbage"; return q }},
		{"oversized cursor", func(q PeopleQuery) PeopleQuery { q.Cursor = strings.Repeat("A", maxCursorBytes+1); return q }},
		{"rank outside 0..1 in a cursor", func(q PeopleQuery) PeopleQuery {
			q.Cursor = base64.RawURLEncoding.EncodeToString([]byte(`{"r":5,"n":"x","i":"u"}`))
			return q
		}},
		{"oversized exclude_member_of", func(q PeopleQuery) PeopleQuery {
			q.ExcludeMemberOf = strings.Repeat("s", maxSpaceRefBytes+1)
			return q
		}},
	}
	for _, tc := range cases {
		_, err := store.SearchPeople(context.Background(), tc.query(base))
		if !errors.Is(err, ErrInvalidRequest) {
			t.Errorf("%s: err = %v, want ErrInvalidRequest", tc.name, err)
		}
	}

	// A hundred runes exactly, limit 0 (the default), limit 1 and limit 50 are
	// all valid: they fail only at the (nil) database.
	for name, q := range map[string]PeopleQuery{
		"100 runes":                 {OrgID: "o", CallerID: "u", Q: strings.Repeat("é", MaxQueryRunes)},
		"limit 0":                   {OrgID: "o", CallerID: "u", Limit: 0},
		"limit 1":                   {OrgID: "o", CallerID: "u", Limit: 1},
		"limit 50":                  {OrgID: "o", CallerID: "u", Limit: MaxPeopleLimit},
		"good cursor":               {OrgID: "o", CallerID: "u", Cursor: encodeCursor(peopleCursor{Rank: 1, LName: "a", ID: "u"})},
		"padded q is trimmed first": {OrgID: "o", CallerID: "u", Q: "  " + strings.Repeat("a", MaxQueryRunes) + "  "},
	} {
		_, err := store.SearchPeople(context.Background(), q)
		if errors.Is(err, ErrInvalidRequest) || err == nil {
			t.Errorf("%s: err = %v, want the unavailable-store error", name, err)
		}
	}
}

func TestSearchPeopleWithoutAnOrgOrCallerIsNotInOrg(t *testing.T) {
	var store *Store
	for _, q := range []PeopleQuery{
		{}, {OrgID: "org-1"}, {CallerID: "user-1"}, {OrgID: "  ", CallerID: "user-1"}, {OrgID: "org-1", CallerID: " "},
	} {
		if _, err := store.SearchPeople(context.Background(), q); !errors.Is(err, ErrCallerNotInOrg) {
			t.Errorf("query %+v: err = %v, want ErrCallerNotInOrg", q, err)
		}
	}
}

func TestDirectoryLimitsMatchThePlan(t *testing.T) {
	if DefaultPeopleLimit != 20 || MaxPeopleLimit != 50 || MaxQueryRunes != 100 {
		t.Fatalf("limits drifted: default=%d max=%d q=%d", DefaultPeopleLimit, MaxPeopleLimit, MaxQueryRunes)
	}
}

// A bound parameter the SQL never references cannot be typed by Postgres (SQLSTATE 42P18), and no
// stubbed test notices. The search statement must use every placeholder from $1 to its last, so the
// bind list in SearchPeople can never carry a stray value again.
func TestSearchPeopleSQLReferencesEveryPlaceholderInOrder(t *testing.T) {
	seen := map[int]bool{}
	highest := 0
	for _, m := range regexp.MustCompile(`\$(\d+)`).FindAllStringSubmatch(searchPeopleSQL, -1) {
		n := 0
		for _, c := range m[1] {
			n = n*10 + int(c-'0')
		}
		seen[n] = true
		if n > highest {
			highest = n
		}
	}
	if highest != 9 {
		t.Fatalf("search SQL's highest placeholder is $%d; SearchPeople binds 9 values", highest)
	}
	for n := 1; n <= highest; n++ {
		if !seen[n] {
			t.Errorf("placeholder $%d is never referenced; Postgres cannot type an unreferenced parameter", n)
		}
	}
}
