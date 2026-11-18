package spaces

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"strings"
	"time"
)

// Room membership feed (WS15-S11).
//
// A read-only view of the membership ledger (space_membership_events, WS04-S4)
// for one time interval, for the room timeline's system rows. The ledger rows
// are written only by Control's own writers, so a client cannot forge one. The
// caller must hold a live (active or archived) membership of the room under
// the signed gateway delegation; the organization comes from that delegation.
// Rows carry ids and closed words only (no names, no content), so a Zero Data
// Retention organization reads the same feed. The feed applies the same history
// floor as the transcript (WS04-S11 and D18): a member sees no membership row older
// than their floor instant (the join for since_join, the date for since_date). A
// member with no floor sees the whole interval.

// Feed size bounds. A page of the timeline asks for at most MembershipFeedMaxLimit rows and says
// truncated when the interval held more.
const (
	MembershipFeedDefaultLimit = 100
	MembershipFeedMaxLimit     = 100
)

// ErrInvalidMembershipFeed is returned for an interval or limit that cannot be served.
var ErrInvalidMembershipFeed = errors.New("invalid membership feed request")

// MembershipFeedParams is the validated request. After is inclusive and Before is exclusive, so
// consecutive page intervals [t_lo, t_hi) tile without a gap or an overlap. A zero time is
// unbounded on that side.
type MembershipFeedParams struct {
	After  time.Time
	Before time.Time
	Limit  int
}

// NormalizeMembershipFeedParams parses the query strings (RFC 3339, limit 1..100). An empty
// limit means the default. An interval whose start is not before its end is invalid.
func NormalizeMembershipFeedParams(after, before, limit string) (MembershipFeedParams, error) {
	params := MembershipFeedParams{Limit: MembershipFeedDefaultLimit}
	parse := func(raw string) (time.Time, error) {
		raw = strings.TrimSpace(raw)
		if raw == "" {
			return time.Time{}, nil
		}
		parsed, err := time.Parse(time.RFC3339Nano, raw)
		if err != nil {
			return time.Time{}, ErrInvalidMembershipFeed
		}
		return parsed.UTC(), nil
	}
	var err error
	if params.After, err = parse(after); err != nil {
		return MembershipFeedParams{}, err
	}
	if params.Before, err = parse(before); err != nil {
		return MembershipFeedParams{}, err
	}
	if !params.After.IsZero() && !params.Before.IsZero() && !params.After.Before(params.Before) {
		return MembershipFeedParams{}, ErrInvalidMembershipFeed
	}
	if raw := strings.TrimSpace(limit); raw != "" {
		var n int
		if _, err := fmt.Sscanf(raw, "%d", &n); err != nil || fmt.Sprintf("%d", n) != raw ||
			n < 1 || n > MembershipFeedMaxLimit {
			return MembershipFeedParams{}, ErrInvalidMembershipFeed
		}
		params.Limit = n
	}
	return params, nil
}

// MembershipFeedEvent is one ledger fact as the timeline shows it.
type MembershipFeedEvent struct {
	EventID     string `json:"event_id"`
	SpaceSeq    int64  `json:"space_seq"`
	Action      string `json:"action"`
	ActorKind   string `json:"actor_kind"`
	ActorID     string `json:"actor_id"`
	SubjectType string `json:"subject_type,omitempty"`
	SubjectID   string `json:"subject_id,omitempty"`
	FromRole    string `json:"from_role,omitempty"`
	ToRole      string `json:"to_role,omitempty"`
	CreatedAt   string `json:"created_at"`
}

// MembershipFeed is the page: newest first, with truncated set when the interval held more rows
// than the limit.
type MembershipFeed struct {
	Events    []MembershipFeedEvent `json:"events"`
	Truncated bool                  `json:"truncated"`
}

// MembershipFeed lists the ledger rows of one room inside the interval. The caller's live
// membership is resolved first (ErrNoCurrentMembership otherwise, byte-equal for an unknown room),
// and the ledger is read under the room's own organization, never a body field.
func (r *Repository) MembershipFeed(ctx context.Context, spaceRef, orgID, userID string, params MembershipFeedParams) (*MembershipFeed, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("space authority repository unavailable")
	}
	membership, err := r.ResolveReadableUserMembership(ctx, spaceRef, orgID, userID)
	if err != nil {
		return nil, err
	}
	// The same floor the transcript reads (WS04-S11, D18): no membership row before it.
	floor, err := r.resolveHistoryFloor(ctx, membership.SpaceRef, membership.SubjectID)
	if err != nil {
		return nil, err
	}
	var floorAt any
	if floor.At != nil {
		floorAt = *floor.At
	}
	limit := params.Limit
	if limit < 1 || limit > MembershipFeedMaxLimit {
		limit = MembershipFeedDefaultLimit
	}
	var after, before any
	if !params.After.IsZero() {
		after = params.After
	}
	if !params.Before.IsZero() {
		before = params.Before
	}
	rows, err := r.db.Pool.Query(ctx, `
		SELECT event_id, space_seq, action, actor_kind, actor_id,
		       subject_type, subject_id, from_role, to_role, created_at
		FROM space_membership_events
		WHERE space_ref=$1
		  AND org_id=$2
		  AND ($3::timestamptz IS NULL OR created_at >= $3)
		  AND ($4::timestamptz IS NULL OR created_at < $4)
		  AND ($6::timestamptz IS NULL OR created_at >= $6)
		ORDER BY created_at DESC, event_id DESC
		LIMIT $5`,
		membership.SpaceRef, membership.OrgID, after, before, limit+1, floorAt,
	)
	if err != nil {
		return nil, fmt.Errorf("read Space membership feed: %w", err)
	}
	defer rows.Close()
	feed := &MembershipFeed{Events: []MembershipFeedEvent{}}
	for rows.Next() {
		var event MembershipFeedEvent
		var subjectType, subjectID, fromRole, toRole sql.NullString
		var createdAt time.Time
		if err := rows.Scan(
			&event.EventID, &event.SpaceSeq, &event.Action, &event.ActorKind, &event.ActorID,
			&subjectType, &subjectID, &fromRole, &toRole, &createdAt,
		); err != nil {
			return nil, fmt.Errorf("scan Space membership feed: %w", err)
		}
		event.SubjectType, event.SubjectID = subjectType.String, subjectID.String
		event.FromRole, event.ToRole = fromRole.String, toRole.String
		event.CreatedAt = createdAt.UTC().Format(time.RFC3339Nano)
		feed.Events = append(feed.Events, event)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read Space membership feed: %w", err)
	}
	if len(feed.Events) > limit {
		feed.Events = feed.Events[:limit]
		feed.Truncated = true
	}
	return feed, nil
}
