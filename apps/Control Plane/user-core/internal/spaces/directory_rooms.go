package spaces

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// Room directory (WS04-S8). Control answers which rooms the caller may browse
// and join; names and topics are Application labels and are composed by the
// gateway. A room is listed only when ALL of these hold: it belongs to the
// caller's organization, is a room (not personal, project or case), is active
// (not archived, suspended or deleted), has roster_mode 'managed' (so never the
// organization room), is 'org_public', the caller is not already an active
// member, and the organization's directory policy is enabled. A policy that is
// off or absent answers enabled=false with no rooms (honest, not an error).

// DirectoryRoomsLimit is the most rooms one answer carries.
const DirectoryRoomsLimit = 500

// DirectoryRoom is one browsable room. Ids, a role and a count only.
type DirectoryRoom struct {
	SpaceRef     string    `json:"space_ref"`
	MemberCount  int       `json:"member_count"`
	JoinRole     string    `json:"join_role"`
	RegisteredAt time.Time `json:"registered_at"`
}

// DirectoryRooms is the directory answer.
type DirectoryRooms struct {
	Enabled   bool            `json:"enabled"`
	Rooms     []DirectoryRoom `json:"rooms"`
	Truncated bool            `json:"truncated"`
}

// ErrDirectoryCallerNotInOrg means the delegated subject has no active
// membership in the delegated organization.
var ErrDirectoryCallerNotInOrg = errors.New("spaces: directory caller is not an active organization member")

// ListDirectoryRooms reads the directory for one caller. OrgID and callerID come
// from the signed delegation.
func (r *Repository) ListDirectoryRooms(ctx context.Context, orgID, callerID string) (*DirectoryRooms, error) {
	orgID, callerID = strings.TrimSpace(orgID), strings.TrimSpace(callerID)
	if orgID == "" || callerID == "" {
		return nil, ErrDirectoryCallerNotInOrg
	}
	if r == nil || r.db == nil {
		return nil, roomError(503, CodeSpaceAuthorityUnavailable, "space authority is unavailable", nil)
	}
	tx, err := r.db.Pool.BeginTx(ctx, pgx.TxOptions{IsoLevel: pgx.RepeatableRead, AccessMode: pgx.ReadOnly})
	if err != nil {
		return nil, fmt.Errorf("begin directory rooms read: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	var callerActive bool
	if err := tx.QueryRow(ctx, `
		SELECT EXISTS (
			SELECT 1 FROM user_org_memberships u JOIN users x ON x.id=u.user_id
			WHERE u.user_id=$1 AND u.org_id=$2 AND u.status='active' AND x.status='active'
		)`, callerID, orgID).Scan(&callerActive); err != nil {
		return nil, fmt.Errorf("verify directory caller: %w", err)
	}
	if !callerActive {
		return nil, ErrDirectoryCallerNotInOrg
	}
	out := &DirectoryRooms{Rooms: []DirectoryRoom{}}
	enabled, err := directoryEnabledTx(ctx, tx, orgID)
	if err != nil {
		return nil, err
	}
	if !enabled {
		return out, nil
	}
	out.Enabled = true
	rows, err := tx.Query(ctx, `
		SELECT s.space_ref, s.join_role, s.registered_at,
		       (SELECT count(*) FROM space_memberships m
		         WHERE m.space_ref=s.space_ref AND m.subject_type='user' AND m.active=TRUE
		           AND EXISTS (SELECT 1 FROM user_org_memberships u
		                       WHERE u.user_id=m.subject_id AND u.org_id=s.org_id AND u.status='active')
		       ) AS member_count
		FROM registered_spaces s
		WHERE s.org_id=$1
		  AND s.registration_state='active'
		  AND s.space_kind='room'
		  AND s.roster_mode='managed'
		  AND s.visibility='org_public'
		  AND NOT EXISTS (
			SELECT 1 FROM space_memberships me
			WHERE me.space_ref=s.space_ref AND me.subject_type='user' AND me.subject_id=$2 AND me.active=TRUE
		  )
		ORDER BY s.registered_at, s.space_ref
		LIMIT $3`, orgID, callerID, DirectoryRoomsLimit+1)
	if err != nil {
		return nil, fmt.Errorf("read directory rooms: %w", err)
	}
	defer rows.Close()
	for rows.Next() {
		var room DirectoryRoom
		if err := rows.Scan(&room.SpaceRef, &room.JoinRole, &room.RegisteredAt, &room.MemberCount); err != nil {
			return nil, fmt.Errorf("scan directory room: %w", err)
		}
		room.RegisteredAt = room.RegisteredAt.UTC()
		out.Rooms = append(out.Rooms, room)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read directory rooms: %w", err)
	}
	if len(out.Rooms) > DirectoryRoomsLimit {
		out.Rooms, out.Truncated = out.Rooms[:DirectoryRoomsLimit], true
	}
	return out, nil
}
