package spaces

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5"
)

// Control-side checks for opening a direct message (WS12-S1). The same two
// checks run twice: once on the gateway's pre-check (DMEligibility) and again,
// inside the registration transaction, when Application registers the Space
// (Register). The second run is the authority; the first only lets the gateway
// refuse with a typed error before Application creates anything.

// dmEntitlementKeys are the three organization bits a DM needs, in the order
// they are reported in `details.missing`.
var dmEntitlementKeys = []string{"dm_create_entitled", "thread_create_entitled", "thread_read_entitled"}

type dmQuerier interface {
	QueryRow(context.Context, string, ...any) pgx.Row
	Query(context.Context, string, ...any) (pgx.Rows, error)
}

// SetDMRegistrationEnabled turns the SPACE_DM_REGISTRATION switch on or off.
// It is a rollout control only, never an authorization input: when off, no new
// DM is registered and the eligibility route answers dm_disabled; DMs that
// already exist keep resolving as shared Spaces.
func (r *Repository) SetDMRegistrationEnabled(enabled bool) {
	r.dmRegistration = enabled
}

// DMRegistrationEnabled reports the switch.
func (r *Repository) DMRegistrationEnabled() bool { return r != nil && r.dmRegistration }

// DMEligibility is the answer of the pre-check route.
type DMEligibility struct {
	Eligible        bool     `json:"eligible"`
	Kind            Kind     `json:"kind"`
	ParticipantHash string   `json:"participant_hash"`
	ParticipantIDs  []string `json:"participant_ids"`
}

// DMEligibility decides whether callerID may open a DM with otherIDs in orgID.
// Order matters: shape first (no database), then the organization entitlements,
// then the people, so an organization without the entitlement learns nothing
// about who else is in it. It writes nothing.
func (r *Repository) DMEligibility(ctx context.Context, orgID, callerID string, otherIDs []string) (*DMEligibility, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("space authority repository unavailable")
	}
	if !r.dmRegistration {
		return nil, ErrDMDisabled
	}
	orgID, callerID = strings.TrimSpace(orgID), strings.TrimSpace(callerID)
	if orgID == "" || callerID == "" {
		return nil, fmt.Errorf("organization and caller are required")
	}
	all := make([]string, 0, len(otherIDs)+1)
	all = append(all, callerID)
	for _, id := range otherIDs {
		if strings.TrimSpace(id) == callerID {
			return nil, &DMParticipantInvalidError{Reason: DMReasonSelf}
		}
		all = append(all, id)
	}
	canonical, err := CanonicalParticipants(all)
	if err != nil {
		return nil, err
	}
	kind, err := DMKindForCount(len(canonical))
	if err != nil {
		return nil, err
	}
	if err := requireDMEntitlements(ctx, r.db.Pool, orgID); err != nil {
		return nil, err
	}
	if err := requireActiveDMParticipants(ctx, r.db.Pool, orgID, canonical); err != nil {
		return nil, err
	}
	hash, err := ParticipantSetHash(canonical...)
	if err != nil {
		return nil, err
	}
	return &DMEligibility{Eligible: true, Kind: kind, ParticipantHash: hash, ParticipantIDs: canonical}, nil
}

// requireDMEntitlements reads the organization's effect policy and returns a
// *DMNotEntitledError naming every bit that is off. No policy row is deny. An
// organization under Space Zero Data Retention is also refused: a DM persists
// message content, so it is unusable there (reported as `zero_data_retention`).
func requireDMEntitlements(ctx context.Context, q dmQuerier, orgID string) error {
	var dmCreate, threadCreate, threadRead, zdr bool
	err := q.QueryRow(ctx, `
		SELECT dm_create_entitled, thread_create_entitled, thread_read_entitled, zero_data_retention
		FROM space_effect_policies WHERE org_id=$1`, orgID,
	).Scan(&dmCreate, &threadCreate, &threadRead, &zdr)
	if errors.Is(err, pgx.ErrNoRows) {
		return &DMNotEntitledError{Missing: append([]string(nil), dmEntitlementKeys...)}
	}
	if err != nil {
		return fmt.Errorf("read direct message entitlements: %w", err)
	}
	var missing []string
	for i, on := range []bool{dmCreate, threadCreate, threadRead} {
		if !on {
			missing = append(missing, dmEntitlementKeys[i])
		}
	}
	if zdr {
		missing = append(missing, "zero_data_retention")
	}
	if len(missing) > 0 {
		return &DMNotEntitledError{Missing: missing}
	}
	return nil
}

// requireActiveDMParticipants checks that every id is an active member of the
// organization with an active user. A person who is not in the organization is
// `not_in_org`; one who is but is suspended or removed is `inactive`.
func requireActiveDMParticipants(ctx context.Context, q dmQuerier, orgID string, ids []string) error {
	rows, err := q.Query(ctx, `
		SELECT u.user_id::text, u.status, COALESCE(x.status, '')
		FROM user_org_memberships u LEFT JOIN users x ON x.id=u.user_id
		WHERE u.org_id=$1 AND u.user_id::text = ANY($2::text[])`, orgID, ids)
	if err != nil {
		return fmt.Errorf("read direct message participants: %w", err)
	}
	defer rows.Close()
	found := make(map[string]bool, len(ids))
	inactive := false
	for rows.Next() {
		var id, memberStatus, userStatus string
		if err := rows.Scan(&id, &memberStatus, &userStatus); err != nil {
			return fmt.Errorf("scan direct message participant: %w", err)
		}
		found[id] = true
		if memberStatus != "active" || userStatus != "active" {
			inactive = true
		}
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("read direct message participants: %w", err)
	}
	for _, id := range ids {
		if !found[id] {
			return &DMParticipantInvalidError{Reason: DMReasonNotInOrg}
		}
	}
	if inactive {
		return &DMParticipantInvalidError{Reason: DMReasonInactive}
	}
	return nil
}
