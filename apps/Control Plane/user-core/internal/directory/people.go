// Package directory answers Control's people-directory reads (WS04-S3).
//
// It owns no state: it reads users and user_org_memberships (both Control
// tables) and, only to apply the exclude_member_of filter, space_memberships.
// One SQL join, no fan-out to org-core (whose search truncates at 50 profile
// fetches). The organization and the caller always come from the signed
// gateway delegation, never from the request.
package directory

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"unicode/utf8"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

const (
	DefaultPeopleLimit = 20
	MaxPeopleLimit     = 50
	MaxQueryRunes      = 100
	maxSpaceRefBytes   = 200
	maxCursorBytes     = 512
)

var (
	// ErrInvalidRequest is a caller error (q too long, bad limit, bad cursor).
	ErrInvalidRequest = errors.New("directory: invalid request")
	// ErrCallerNotInOrg means the delegated subject has no active membership in
	// the delegated organization, so nothing in that organization is listed.
	ErrCallerNotInOrg = errors.New("directory: caller is not an active organization member")
)

// PeopleQuery is one search page. OrgID and CallerID come from the verified
// delegation.
type PeopleQuery struct {
	OrgID    string
	CallerID string
	Q        string
	Limit    int
	Cursor   string
	// ExcludeMemberOf is a space_ref. It applies only when the caller is an
	// active member of that Space in OrgID; otherwise it is ignored, so the
	// parameter cannot be used to probe other Spaces' rosters.
	ExcludeMemberOf string
}

// Person is the minimum a picker needs. Email stays because list_members
// already shows it to every Space member (PD-WS04).
type Person struct {
	SubjectID   string `json:"subject_id"`
	DisplayName string `json:"display_name"`
	Email       string `json:"email"`
	IsSelf      bool   `json:"is_self"`
}

// PeopleResult is a page. NextCursor is empty on the last page.
type PeopleResult struct {
	People     []Person `json:"people"`
	NextCursor string   `json:"next_cursor"`
}

// Store reads the people directory from Control's database.
type Store struct {
	db *database.DB
}

func NewStore(db *database.DB) *Store { return &Store{db: db} }

// peopleCursor is the keyset position: (rank, lower(name), id). Rank 0 is a
// prefix match (or no query), rank 1 a substring match.
type peopleCursor struct {
	Rank  int    `json:"r"`
	LName string `json:"n"`
	ID    string `json:"i"`
}

func encodeCursor(c peopleCursor) string {
	raw, _ := json.Marshal(c)
	return base64.RawURLEncoding.EncodeToString(raw)
}

func decodeCursor(token string) (peopleCursor, error) {
	var c peopleCursor
	if len(token) > maxCursorBytes {
		return c, ErrInvalidRequest
	}
	raw, err := base64.RawURLEncoding.DecodeString(token)
	if err != nil {
		return c, ErrInvalidRequest
	}
	if err := json.Unmarshal(raw, &c); err != nil || (c.Rank != 0 && c.Rank != 1) || c.ID == "" {
		return peopleCursor{}, ErrInvalidRequest
	}
	return c, nil
}

// escapeLike escapes the LIKE metacharacters so user text is matched literally.
func escapeLike(s string) string {
	r := strings.NewReplacer(`\`, `\\`, `%`, `\%`, `_`, `\_`)
	return r.Replace(s)
}

// Parameters: $1 org, $2 contains pattern, $3 prefix pattern, $4 excluded Space ('' for none),
// $5 whether a cursor is applied, $6..$8 the cursor (rank, lower name, id), $9 page size + 1.
// Every placeholder must appear in the text: Postgres cannot type a bound parameter that is never
// referenced (SQLSTATE 42P18), which is what an unused caller-id bind used to cause here.
const searchPeopleSQL = `
SELECT id, name, email, rank, lname FROM (
  SELECT u.id AS id, u.name AS name, u.email AS email, lower(u.name) AS lname,
         CASE WHEN $2::text = '' THEN 0
              WHEN u.name ILIKE $3::text OR u.email ILIKE $3::text THEN 0
              ELSE 1 END AS rank
    FROM user_org_memberships m
    JOIN users u ON u.id = m.user_id
   WHERE m.org_id = $1
     AND m.status = 'active'
     AND u.status = 'active'
     AND ($2::text = '' OR u.name ILIKE $2::text OR u.email ILIKE $2::text)
     AND NOT ($4::text <> '' AND EXISTS (
           SELECT 1 FROM space_memberships sm
            WHERE sm.space_ref = $4::text AND sm.subject_type = 'user'
              AND sm.subject_id = u.id AND sm.active = TRUE))
) ranked
WHERE (NOT $5::boolean OR (rank, lname, id) > ($6::int, $7::text, $8::text))
ORDER BY rank, lname, id
LIMIT $9`

const callerScopeSQL = `
SELECT
  EXISTS (SELECT 1 FROM user_org_memberships c
           WHERE c.user_id = $2 AND c.org_id = $1 AND c.status = 'active'),
  CASE WHEN $3::text = '' THEN FALSE ELSE EXISTS (
        SELECT 1 FROM space_memberships cm
          JOIN registered_spaces rs ON rs.space_ref = cm.space_ref
         WHERE cm.space_ref = $3::text AND rs.org_id = $1
           AND rs.registration_state = 'active'
           AND cm.subject_type = 'user' AND cm.subject_id = $2 AND cm.active = TRUE) END`

// SearchPeople returns one page of active members of q.OrgID matching q.Q
// (name or email, wildcards escaped), ordered prefix matches first, then by
// lower(name) and id.
func (s *Store) SearchPeople(ctx context.Context, q PeopleQuery) (PeopleResult, error) {
	orgID, callerID := strings.TrimSpace(q.OrgID), strings.TrimSpace(q.CallerID)
	if orgID == "" || callerID == "" {
		return PeopleResult{}, ErrCallerNotInOrg
	}
	text := strings.TrimSpace(q.Q)
	if utf8.RuneCountInString(text) > MaxQueryRunes {
		return PeopleResult{}, fmt.Errorf("%w: q is longer than %d characters", ErrInvalidRequest, MaxQueryRunes)
	}
	limit := q.Limit
	if limit == 0 {
		limit = DefaultPeopleLimit
	}
	if limit < 1 || limit > MaxPeopleLimit {
		return PeopleResult{}, fmt.Errorf("%w: limit must be 1..%d", ErrInvalidRequest, MaxPeopleLimit)
	}
	exclude := strings.TrimSpace(q.ExcludeMemberOf)
	if len(exclude) > maxSpaceRefBytes {
		return PeopleResult{}, fmt.Errorf("%w: exclude_member_of", ErrInvalidRequest)
	}
	var cursor peopleCursor
	hasCursor := strings.TrimSpace(q.Cursor) != ""
	if hasCursor {
		var err error
		if cursor, err = decodeCursor(strings.TrimSpace(q.Cursor)); err != nil {
			return PeopleResult{}, fmt.Errorf("%w: cursor", ErrInvalidRequest)
		}
	}
	if s == nil || s.db == nil || s.db.Pool == nil {
		return PeopleResult{}, errors.New("directory: store unavailable")
	}

	var inOrg, excludeApplies bool
	if err := s.db.Pool.QueryRow(ctx, callerScopeSQL, orgID, callerID, exclude).Scan(&inOrg, &excludeApplies); err != nil {
		return PeopleResult{}, fmt.Errorf("directory: resolve caller scope: %w", err)
	}
	if !inOrg {
		return PeopleResult{}, ErrCallerNotInOrg
	}
	if !excludeApplies {
		exclude = ""
	}

	contains, prefix := "", ""
	if text != "" {
		escaped := escapeLike(text)
		contains, prefix = "%"+escaped+"%", escaped+"%"
	}
	rows, err := s.db.Pool.Query(ctx, searchPeopleSQL,
		orgID, contains, prefix, exclude, hasCursor, cursor.Rank, cursor.LName, cursor.ID, limit+1)
	if err != nil {
		return PeopleResult{}, fmt.Errorf("directory: search people: %w", err)
	}
	defer rows.Close()

	people := make([]Person, 0, limit)
	var last peopleCursor
	more := false
	for rows.Next() {
		var p Person
		var c peopleCursor
		if err := rows.Scan(&p.SubjectID, &p.DisplayName, &p.Email, &c.Rank, &c.LName); err != nil {
			return PeopleResult{}, fmt.Errorf("directory: scan person: %w", err)
		}
		if len(people) == limit {
			more = true
			break
		}
		c.ID = p.SubjectID
		p.IsSelf = p.SubjectID == callerID
		people = append(people, p)
		last = c
	}
	if err := rows.Err(); err != nil {
		return PeopleResult{}, fmt.Errorf("directory: read people: %w", err)
	}
	result := PeopleResult{People: people}
	if more {
		result.NextCursor = encodeCursor(last)
	}
	return result, nil
}
