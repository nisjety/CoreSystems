package spaces

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

var ErrInactiveOwnerMembership = errors.New("Space owner is not an active organization member")
var ErrNoCurrentMembership = errors.New("no current Space membership")

// RegisteredSpace is Control's durable acknowledgement of an
// Application-issued immutable Space reference.
type RegisteredSpace struct {
	SpaceRef                     string    `json:"space_ref"`
	OrgID                        string    `json:"org_id"`
	Kind                         Kind      `json:"kind"`
	OwnerPrincipalID             string    `json:"owner_principal_id"`
	ApplicationLifecycleRevision int64     `json:"application_lifecycle_revision"`
	RegistrationState            string    `json:"registration_state"`
	RegisteredAt                 time.Time `json:"registered_at"`
	UpdatedAt                    time.Time `json:"updated_at"`
}

type Repository struct {
	db *database.DB
}

// RegisteredRecipientAudience is the Control-verified receipt for one
// Application participant-set revision. It intentionally omits member IDs so
// callers can retain an audit/provenance reference without turning this API
// into a participant-directory read surface.
type RegisteredRecipientAudience struct {
	SpaceRef     string    `json:"space_ref"`
	AudienceRef  string    `json:"audience_ref"`
	AudienceHash string    `json:"audience_hash"`
	Revision     int64     `json:"revision"`
	RegisteredAt time.Time `json:"registered_at"`
}

func NewRepository(db *database.DB) *Repository {
	return &Repository{db: db}
}

// Register is idempotent for an immutable `(space_ref, org_id, kind)` tuple.
// A replay may advance the Application lifecycle revision but may never move a
// reference to another organization or kind.
func (r *Repository) Register(ctx context.Context, registration Registration) (*RegisteredSpace, error) {
	if err := registration.Validate(); err != nil {
		return nil, err
	}
	registrationState, err := registration.RegistrationState()
	if err != nil {
		return nil, err
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin Space registration: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	current, err := registeredSpaceForUpdate(ctx, tx, registration.SpaceRef)
	if err != nil && err != pgx.ErrNoRows {
		return nil, err
	}
	if err == pgx.ErrNoRows {
		if registration.Lifecycle != LifecyclePendingRegistration {
			return nil, fmt.Errorf("first Space lifecycle event must be pending_registration")
		}
		if err := requireActiveOwnerMembership(ctx, tx, registration.OwnerPrincipalID, registration.OrgID); err != nil {
			return nil, err
		}
		current = &RegisteredSpace{
			SpaceRef:                     registration.SpaceRef,
			OrgID:                        registration.OrgID,
			Kind:                         registration.Kind,
			OwnerPrincipalID:             registration.OwnerPrincipalID,
			ApplicationLifecycleRevision: registration.LifecycleRevision,
			RegistrationState:            "active",
		}
		if err := tx.QueryRow(ctx, `
			INSERT INTO registered_spaces
				(space_ref, org_id, space_kind, owner_principal_id, application_lifecycle_revision, registration_state)
			VALUES ($1, $2, $3, $4, $5, 'active')
			RETURNING registered_at, updated_at`,
			current.SpaceRef, current.OrgID, current.Kind, current.OwnerPrincipalID, current.ApplicationLifecycleRevision,
		).Scan(&current.RegisteredAt, &current.UpdatedAt); err != nil {
			return nil, fmt.Errorf("insert registered Space: %w", err)
		}
		if _, err := tx.Exec(ctx, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, current.SpaceRef); err != nil {
			return nil, fmt.Errorf("initialize Space authority revision: %w", err)
		}
		if _, err := tx.Exec(ctx, `
			INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by)
			VALUES ($1, 'user', $2, 'owner', 'application-space-registration')`,
			current.SpaceRef, current.OwnerPrincipalID,
		); err != nil {
			return nil, fmt.Errorf("seed verified Space owner membership: %w", err)
		}
	} else {
		if current.OrgID != registration.OrgID || current.Kind != registration.Kind || current.OwnerPrincipalID != registration.OwnerPrincipalID {
			return nil, fmt.Errorf("Space reference is already registered with a different organization, kind, or owner")
		}
		if registration.LifecycleRevision > current.ApplicationLifecycleRevision {
			if err := tx.QueryRow(ctx, `
				UPDATE registered_spaces
				SET application_lifecycle_revision=$2, registration_state=$3, updated_at=NOW()
				WHERE space_ref=$1
				RETURNING registration_state, updated_at`, current.SpaceRef, registration.LifecycleRevision, registrationState,
			).Scan(&current.RegistrationState, &current.UpdatedAt); err != nil {
				return nil, fmt.Errorf("advance registered Space lifecycle revision: %w", err)
			}
			current.ApplicationLifecycleRevision = registration.LifecycleRevision
			if registrationState == "deleting" || registrationState == "deleted" {
				if err := purgeControlSpaceProjections(ctx, tx, current.SpaceRef); err != nil {
					return nil, err
				}
			}
		} else if registration.LifecycleRevision == current.ApplicationLifecycleRevision && current.RegistrationState != registrationState {
			return nil, fmt.Errorf("Space lifecycle revision conflicts with registered authorization state")
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit Space registration: %w", err)
	}
	return current, nil
}

// purgeControlSpaceProjections removes Control-owned access and recipient
// projections once Application's immutable lifecycle event has fenced the
// Space. The registered reference, legal-hold row, deletion request, and
// authority revisions remain as minimum reconciliation evidence. It does not
// claim any other owner has purged its own records.
func purgeControlSpaceProjections(ctx context.Context, tx pgx.Tx, spaceRef string) error {
	// Space deletion is a revocation event even though the lifecycle transition
	// does not itself advance space_authority_revisions. Cancel all uncommitted
	// owner effects in the same transaction before access projections disappear;
	// a committed reservation has already linearized before this deletion and
	// remains an honest reconciliation receipt.
	if _, err := tx.Exec(ctx, `UPDATE space_owner_effect_reservations
		SET status='cancelled', cancelled_at=NOW(), cancellation_reason='space_deleted', updated_at=NOW()
		WHERE space_ref=$1 AND status='reserved'`, spaceRef); err != nil {
		return fmt.Errorf("cancel pending owner effects for deleted Space: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM space_memberships WHERE space_ref=$1`, spaceRef); err != nil {
		return fmt.Errorf("purge Control Space memberships: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM space_recipient_audience_members WHERE space_ref=$1`, spaceRef); err != nil {
		return fmt.Errorf("purge Control Space recipient members: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM space_recipient_audiences WHERE space_ref=$1`, spaceRef); err != nil {
		return fmt.Errorf("purge Control Space recipient audiences: %w", err)
	}
	return nil
}

// AuthorizeDeletion repeats Control's owner, current-membership, entitlement,
// and legal-hold checks for the immutable Application request. It records the
// resulting decision durably so transport retries cannot turn one request into
// conflicting decisions. This is authorization only: each plane still owns
// its own deletion adapter and receipt.
func (r *Repository) AuthorizeDeletion(ctx context.Context, request DeletionAuthorizationRequest) (DeletionAuthorizationReceipt, error) {
	if r == nil || r.db == nil {
		return DeletionAuthorizationReceipt{}, fmt.Errorf("Space authority repository unavailable")
	}
	if err := request.Validate(); err != nil {
		return DeletionAuthorizationReceipt{}, err
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return DeletionAuthorizationReceipt{}, fmt.Errorf("begin Space deletion authorization: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	var existing DeletionAuthorizationReceipt
	err = tx.QueryRow(ctx, `SELECT request_id, status FROM space_deletion_requests WHERE request_id=$1 FOR UPDATE`, request.RequestID).
		Scan(&existing.RequestID, &existing.Status)
	if err == nil {
		var spaceRef, orgID, ownerID, idempotencyKey string
		if err := tx.QueryRow(ctx, `SELECT space_ref, org_id, owner_principal_id, idempotency_key FROM space_deletion_requests WHERE request_id=$1`, request.RequestID).
			Scan(&spaceRef, &orgID, &ownerID, &idempotencyKey); err != nil {
			return DeletionAuthorizationReceipt{}, fmt.Errorf("read existing Space deletion request: %w", err)
		}
		if spaceRef != request.SpaceRef || orgID != request.OrgID || ownerID != request.OwnerPrincipalID || idempotencyKey != request.IdempotencyKey {
			return DeletionAuthorizationReceipt{}, fmt.Errorf("Space deletion request conflicts with immutable intent")
		}
		if err := tx.Commit(ctx); err != nil {
			return DeletionAuthorizationReceipt{}, fmt.Errorf("commit Space deletion replay: %w", err)
		}
		return existing, nil
	}
	if err != pgx.ErrNoRows {
		return DeletionAuthorizationReceipt{}, fmt.Errorf("lock Space deletion request: %w", err)
	}

	space, err := registeredSpaceForUpdate(ctx, tx, request.SpaceRef)
	if errors.Is(err, pgx.ErrNoRows) {
		return r.recordDeletionAuthorization(ctx, tx, request, DeletionRejected)
	}
	if err != nil {
		return DeletionAuthorizationReceipt{}, fmt.Errorf("lock registered Space for deletion: %w", err)
	}
	if space.OrgID != request.OrgID || space.OwnerPrincipalID != request.OwnerPrincipalID || space.RegistrationState != "active" {
		return r.recordDeletionAuthorization(ctx, tx, request, DeletionRejected)
	}
	if err := requireActiveOwnerMembership(ctx, tx, request.OwnerPrincipalID, request.OrgID); err != nil {
		return r.recordDeletionAuthorization(ctx, tx, request, DeletionRejected)
	}
	var held bool
	if err := tx.QueryRow(ctx, `SELECT EXISTS(SELECT 1 FROM space_legal_holds WHERE space_ref=$1)`, request.SpaceRef).Scan(&held); err != nil {
		return DeletionAuthorizationReceipt{}, fmt.Errorf("check Space legal hold: %w", err)
	}
	if held {
		return r.recordDeletionAuthorization(ctx, tx, request, DeletionBlockedLegalHold)
	}
	var policy DeletionPolicy
	err = tx.QueryRow(ctx, `SELECT org_id, deletion_entitled, personal_rollout_enabled FROM space_deletion_policies WHERE org_id=$1`, request.OrgID).
		Scan(&policy.OrgID, &policy.DeletionEntitled, &policy.PersonalRolloutEnabled)
	if errors.Is(err, pgx.ErrNoRows) || !policy.DeletionEntitled || space.Kind != KindPersonal || !policy.PersonalRolloutEnabled {
		return r.recordDeletionAuthorization(ctx, tx, request, DeletionRejected)
	}
	if err != nil {
		return DeletionAuthorizationReceipt{}, fmt.Errorf("read Space deletion policy: %w", err)
	}
	return r.recordDeletionAuthorization(ctx, tx, request, DeletionAuthorized)
}

// UpsertDeletionPolicy is deliberately Control-workload-only at the HTTP
// boundary. A missing row is deny-by-default; the first rollout permits only
// personal Spaces even where entitlement is enabled. Every attempted policy
// change creates immutable operator evidence, including an idempotent replay.
func (r *Repository) UpsertDeletionPolicy(ctx context.Context, policy DeletionPolicy, actorPrincipalID string) (bool, error) {
	if r == nil || r.db == nil {
		return false, fmt.Errorf("Space authority repository unavailable")
	}
	if err := policy.Validate(); err != nil {
		return false, err
	}
	if strings.TrimSpace(actorPrincipalID) == "" {
		return false, fmt.Errorf("Space deletion policy actor is required")
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return false, fmt.Errorf("begin Space deletion policy update: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	tag, err := tx.Exec(ctx, `
		INSERT INTO space_deletion_policies (org_id, deletion_entitled, personal_rollout_enabled)
		VALUES ($1,$2,$3)
		ON CONFLICT (org_id) DO UPDATE SET
			deletion_entitled=EXCLUDED.deletion_entitled,
			personal_rollout_enabled=EXCLUDED.personal_rollout_enabled,
			updated_at=NOW()
		WHERE space_deletion_policies.deletion_entitled IS DISTINCT FROM EXCLUDED.deletion_entitled
		   OR space_deletion_policies.personal_rollout_enabled IS DISTINCT FROM EXCLUDED.personal_rollout_enabled`,
		policy.OrgID, policy.DeletionEntitled, policy.PersonalRolloutEnabled)
	if err != nil {
		return false, fmt.Errorf("upsert Space deletion policy: %w", err)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO space_deletion_operator_events
			(org_id, event_type, actor_principal_id, deletion_entitled, personal_rollout_enabled)
		VALUES ($1, 'policy_updated', $2, $3, $4)`,
		policy.OrgID, actorPrincipalID, policy.DeletionEntitled, policy.PersonalRolloutEnabled); err != nil {
		return false, fmt.Errorf("record Space deletion policy audit: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return false, fmt.Errorf("commit Space deletion policy update: %w", err)
	}
	return tag.RowsAffected() > 0, nil
}

// ApplyLegalHold persists a Control-only retention fence for a canonical
// Space. It locks the registered reference so an arbitrary string cannot
// create a hold, and records the operator action in the same transaction.
func (r *Repository) ApplyLegalHold(ctx context.Context, hold LegalHold, actorPrincipalID string) (bool, error) {
	if r == nil || r.db == nil {
		return false, fmt.Errorf("Space authority repository unavailable")
	}
	if err := hold.Validate(); err != nil {
		return false, err
	}
	if strings.TrimSpace(actorPrincipalID) == "" {
		return false, fmt.Errorf("Space legal hold actor is required")
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return false, fmt.Errorf("begin Space legal hold: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	space, err := registeredSpaceForUpdate(ctx, tx, hold.SpaceRef)
	if errors.Is(err, pgx.ErrNoRows) {
		return false, ErrNoCurrentMembership
	}
	if err != nil {
		return false, fmt.Errorf("lock registered Space for legal hold: %w", err)
	}
	tag, err := tx.Exec(ctx, `
		INSERT INTO space_legal_holds (space_ref, hold_ref)
		VALUES ($1, $2)
		ON CONFLICT (space_ref) DO UPDATE SET hold_ref=EXCLUDED.hold_ref, applied_at=NOW()
		WHERE space_legal_holds.hold_ref IS DISTINCT FROM EXCLUDED.hold_ref`, hold.SpaceRef, hold.HoldRef)
	if err != nil {
		return false, fmt.Errorf("apply Space legal hold: %w", err)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO space_deletion_operator_events
			(org_id, space_ref, event_type, actor_principal_id, hold_ref)
		VALUES ($1, $2, 'legal_hold_applied', $3, $4)`,
		space.OrgID, hold.SpaceRef, actorPrincipalID, hold.HoldRef); err != nil {
		return false, fmt.Errorf("record Space legal-hold audit: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return false, fmt.Errorf("commit Space legal hold: %w", err)
	}
	return tag.RowsAffected() > 0, nil
}

// ReleaseLegalHold records the release attempt even when the operation is a
// replay. It never deletes the operator evidence that explains why a prior
// deletion request was blocked.
func (r *Repository) ReleaseLegalHold(ctx context.Context, spaceRef, actorPrincipalID string) (bool, error) {
	if r == nil || r.db == nil {
		return false, fmt.Errorf("Space authority repository unavailable")
	}
	if strings.TrimSpace(spaceRef) == "" || strings.TrimSpace(actorPrincipalID) == "" {
		return false, fmt.Errorf("Space legal hold reference and actor are required")
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return false, fmt.Errorf("begin Space legal-hold release: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	space, err := registeredSpaceForUpdate(ctx, tx, spaceRef)
	if errors.Is(err, pgx.ErrNoRows) {
		return false, ErrNoCurrentMembership
	}
	if err != nil {
		return false, fmt.Errorf("lock registered Space for legal-hold release: %w", err)
	}
	tag, err := tx.Exec(ctx, `DELETE FROM space_legal_holds WHERE space_ref=$1`, spaceRef)
	if err != nil {
		return false, fmt.Errorf("release Space legal hold: %w", err)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO space_deletion_operator_events
			(org_id, space_ref, event_type, actor_principal_id)
		VALUES ($1, $2, 'legal_hold_released', $3)`, space.OrgID, spaceRef, actorPrincipalID); err != nil {
		return false, fmt.Errorf("record Space legal-hold release audit: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return false, fmt.Errorf("commit Space legal-hold release: %w", err)
	}
	return tag.RowsAffected() > 0, nil
}

func (r *Repository) recordDeletionAuthorization(ctx context.Context, tx pgx.Tx, request DeletionAuthorizationRequest, status DeletionAuthorizationStatus) (DeletionAuthorizationReceipt, error) {
	receipt := DeletionAuthorizationReceipt{RequestID: request.RequestID, Status: status}
	if _, err := tx.Exec(ctx, `
		INSERT INTO space_deletion_requests (request_id, space_ref, org_id, owner_principal_id, idempotency_key, status)
		VALUES ($1,$2,$3,$4,$5,$6)`, request.RequestID, request.SpaceRef, request.OrgID, request.OwnerPrincipalID, request.IdempotencyKey, status); err != nil {
		return DeletionAuthorizationReceipt{}, fmt.Errorf("record Space deletion authorization: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return DeletionAuthorizationReceipt{}, fmt.Errorf("commit Space deletion authorization: %w", err)
	}
	return receipt, nil
}

// RegisterRecipientAudience records an Application-owned recipient snapshot
// only after Control verifies every member's current Space role and current
// organization membership. The first snapshot consumes the initial recipient
// revision created with the Space; each later revision atomically advances the
// aggregate and recipient-audience revisions, fencing old decisions.
func (r *Repository) RegisterRecipientAudience(ctx context.Context, registration RecipientAudienceRegistration) (*RegisteredRecipientAudience, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("Space authority repository unavailable")
	}
	if err := registration.Validate(); err != nil {
		return nil, err
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin recipient audience registration: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	space, err := registeredSpaceForUpdate(ctx, tx, registration.SpaceRef)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNoCurrentMembership
	}
	if err != nil {
		return nil, fmt.Errorf("lock recipient audience Space: %w", err)
	}
	if space.OrgID != registration.OrgID || space.RegistrationState != "active" || space.Kind == KindPersonal {
		return nil, fmt.Errorf("recipient audience Space is not an active shared Space")
	}
	var current AuthorityRevision
	if err := tx.QueryRow(ctx, `SELECT authority_revision, membership_revision, privacy_revision, recipient_audience_revision, entitlement_revision
		FROM space_authority_revisions WHERE space_ref=$1 FOR UPDATE`, space.SpaceRef).Scan(
		&current.Authority, &current.Membership, &current.Privacy, &current.RecipientAudience, &current.Entitlement,
	); err != nil {
		return nil, fmt.Errorf("lock recipient audience revision: %w", err)
	}
	var existing RegisteredRecipientAudience
	err = tx.QueryRow(ctx, `SELECT space_ref, audience_ref, audience_hash, revision, registered_at
		FROM space_recipient_audiences WHERE space_ref=$1 AND revision=$2`, space.SpaceRef, registration.Revision).Scan(
		&existing.SpaceRef, &existing.AudienceRef, &existing.AudienceHash, &existing.Revision, &existing.RegisteredAt,
	)
	if err == nil {
		if existing.AudienceRef != registration.AudienceRef || existing.AudienceHash != registration.AudienceHash {
			return nil, fmt.Errorf("recipient audience revision conflicts with a different snapshot")
		}
		var members []string
		rows, queryErr := tx.Query(ctx, `SELECT subject_id FROM space_recipient_audience_members
			WHERE space_ref=$1 AND revision=$2 ORDER BY subject_id`, space.SpaceRef, registration.Revision)
		if queryErr != nil {
			return nil, fmt.Errorf("read existing recipient audience members: %w", queryErr)
		}
		defer rows.Close()
		for rows.Next() {
			var subject string
			if scanErr := rows.Scan(&subject); scanErr != nil {
				return nil, scanErr
			}
			members = append(members, subject)
		}
		if rows.Err() != nil {
			return nil, rows.Err()
		}
		expected, canonicalErr := canonicalRecipientSubjects(registration.Recipients)
		if canonicalErr != nil || strings.Join(members, "\x00") != strings.Join(expected, "\x00") {
			return nil, fmt.Errorf("recipient audience replay changes members")
		}
		if err := tx.Commit(ctx); err != nil {
			return nil, fmt.Errorf("commit recipient audience replay: %w", err)
		}
		return &existing, nil
	}
	if err != pgx.ErrNoRows {
		return nil, fmt.Errorf("read recipient audience snapshot: %w", err)
	}
	var latestRevision *int64
	if err := tx.QueryRow(ctx, `SELECT max(revision) FROM space_recipient_audiences WHERE space_ref=$1`, space.SpaceRef).Scan(&latestRevision); err != nil {
		return nil, fmt.Errorf("read latest recipient audience revision: %w", err)
	}
	expectedRevision := current.RecipientAudience
	if latestRevision != nil {
		expectedRevision++
	}
	if registration.Revision != expectedRevision {
		return nil, fmt.Errorf("recipient audience revision is not the next current revision")
	}
	canonical, err := canonicalRecipientSubjects(registration.Recipients)
	if err != nil {
		return nil, err
	}
	for _, subjectID := range canonical {
		var authorized bool
		if err := tx.QueryRow(ctx, `SELECT EXISTS(
			SELECT 1 FROM space_memberships m
			JOIN user_org_memberships u ON u.user_id=m.subject_id AND u.org_id=$2 AND u.status='active'
			WHERE m.space_ref=$1 AND m.subject_type='user' AND m.subject_id=$3 AND m.active=TRUE
		)`, space.SpaceRef, space.OrgID, subjectID).Scan(&authorized); err != nil {
			return nil, fmt.Errorf("verify recipient authority: %w", err)
		}
		if !authorized {
			return nil, fmt.Errorf("recipient %q lacks current Space authority", subjectID)
		}
	}
	registered := RegisteredRecipientAudience{SpaceRef: space.SpaceRef, AudienceRef: registration.AudienceRef, AudienceHash: registration.AudienceHash, Revision: registration.Revision}
	if err := tx.QueryRow(ctx, `INSERT INTO space_recipient_audiences
		(space_ref, revision, audience_ref, audience_hash, recipient_count)
		VALUES ($1,$2,$3,$4,$5) RETURNING registered_at`, registered.SpaceRef, registered.Revision, registered.AudienceRef, registered.AudienceHash, len(canonical)).Scan(&registered.RegisteredAt); err != nil {
		return nil, fmt.Errorf("insert recipient audience snapshot: %w", err)
	}
	for _, subjectID := range canonical {
		if _, err := tx.Exec(ctx, `INSERT INTO space_recipient_audience_members (space_ref, revision, subject_id) VALUES ($1,$2,$3)`, registered.SpaceRef, registered.Revision, subjectID); err != nil {
			return nil, fmt.Errorf("insert recipient audience member: %w", err)
		}
	}
	if latestRevision != nil {
		if _, err := tx.Exec(ctx, `UPDATE space_authority_revisions
			SET authority_revision=authority_revision+1, recipient_audience_revision=recipient_audience_revision+1, updated_at=NOW()
			WHERE space_ref=$1`, space.SpaceRef); err != nil {
			return nil, fmt.Errorf("advance recipient audience authority revision: %w", err)
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit recipient audience registration: %w", err)
	}
	return &registered, nil
}

// RosterForSpace lists who is in a Space, for a caller who is in it themselves.
//
// Membership is the gate: the roster of a room is only visible from inside it.
// The same organization-membership backstop as the index applies, so someone
// who has left the organization cannot read a roster even if their per-Space
// revocation has not synced.
//
// Sanitized deliberately. It carries display identity and role — what a Members
// tab needs to show who you work with — and NOT email. An email is a contact
// detail and a durable identifier for someone who may only have consented to
// being in a room, not to having their address published to everyone else in
// it. Nothing here is a credential, an audience, or a decision.
func (r *Repository) RosterForSpace(ctx context.Context, spaceRef, orgID, subjectID string) ([]RosterMember, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("Space authority repository unavailable")
	}
	spaceRef = strings.TrimSpace(spaceRef)
	orgID = strings.TrimSpace(orgID)
	subjectID = strings.TrimSpace(subjectID)
	if spaceRef == "" || orgID == "" || subjectID == "" {
		return nil, fmt.Errorf("Space, organization and subject are required")
	}
	rows, err := r.db.Pool.Query(ctx, `
		SELECT m.subject_type, m.subject_id, m.role, m.revision, COALESCE(u.name, '')
		FROM space_memberships m
		JOIN registered_spaces s ON s.space_ref = m.space_ref
		LEFT JOIN users u ON u.id = m.subject_id AND m.subject_type = 'user'
		WHERE m.space_ref = $1 AND m.active
		  AND s.org_id = $2 AND s.registration_state = 'active'
		  AND EXISTS (
		      SELECT 1 FROM space_memberships caller
		      WHERE caller.space_ref = $1 AND caller.subject_type = 'user'
		        AND caller.subject_id = $3 AND caller.active
		  )
		  AND EXISTS (
		      SELECT 1 FROM user_org_memberships uo
		      WHERE uo.user_id = $3 AND uo.org_id = $2 AND uo.status = 'active'
		  )
		ORDER BY m.role, m.subject_id`, spaceRef, orgID, subjectID)
	if err != nil {
		return nil, fmt.Errorf("read Space roster: %w", err)
	}
	defer rows.Close()

	members := make([]RosterMember, 0, 16)
	for rows.Next() {
		var member RosterMember
		if err := rows.Scan(&member.SubjectType, &member.SubjectID, &member.Role,
			&member.Revision, &member.DisplayName); err != nil {
			return nil, fmt.Errorf("scan roster member: %w", err)
		}
		members = append(members, member)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read Space roster: %w", err)
	}
	return members, nil
}

// SpacesForSubject lists the registered Spaces a subject currently belongs to.
//
// This is the actor-filtered index: Control decides what a caller may see,
// because Control owns memberships. Application can name and describe a Space,
// but it must not be the thing that decides whether you are in it — a
// projection can lag a revocation, and an index that lags is an index that
// shows a room somebody was removed from.
//
// Both memberships are checked, not one: the Space membership AND a live
// organization membership. Leaving an organization must remove its rooms from
// your index even if the per-Space revocation has not been synced yet, so the
// org check is the backstop that makes the sync's timing non-security-critical.
func (r *Repository) SpacesForSubject(ctx context.Context, orgID, subjectID string) ([]SpaceIndexEntry, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("Space authority repository unavailable")
	}
	orgID = strings.TrimSpace(orgID)
	subjectID = strings.TrimSpace(subjectID)
	if orgID == "" || subjectID == "" {
		return nil, fmt.Errorf("organization and subject are required")
	}
	rows, err := r.db.Pool.Query(ctx, `
		SELECT s.space_ref, s.org_id, s.space_kind, m.role
		FROM registered_spaces s
		JOIN space_memberships m ON m.space_ref = s.space_ref
		WHERE s.org_id = $1
		  AND s.registration_state = 'active'
		  AND m.subject_type = 'user' AND m.subject_id = $2 AND m.active = TRUE
		  AND EXISTS (
		      SELECT 1 FROM user_org_memberships u
		      WHERE u.user_id = $2 AND u.org_id = $1 AND u.status = 'active'
		  )
		ORDER BY s.space_kind, s.registered_at`, orgID, subjectID)
	if err != nil {
		return nil, fmt.Errorf("list Spaces for subject: %w", err)
	}
	defer rows.Close()

	entries := make([]SpaceIndexEntry, 0, 8)
	for rows.Next() {
		var entry SpaceIndexEntry
		if err := rows.Scan(&entry.SpaceRef, &entry.OrgID, &entry.Kind, &entry.Role); err != nil {
			return nil, fmt.Errorf("scan Space index entry: %w", err)
		}
		entries = append(entries, entry)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read Space index: %w", err)
	}
	return entries, nil
}

// ReplaceMemberships converges a Space's membership on the declared set and
// returns the resulting authority revisions.
//
// Subjects present are upserted active with their declared role. Subjects
// absent are DEACTIVATED, not deleted: `space_memberships.active` exists so a
// revocation stays auditable, and a delete would also drop the row's revision
// history. Either way the subject stops resolving.
//
// # The owner is never revoked by absence
//
// Registration seeds exactly one membership — the owner principal, granted by
// `application-space-registration`. A roster sync that simply omitted them
// (an organization list that does not happen to include the registrar, a
// partially-built payload) would lock a Space's owner out of their own Space,
// and nothing else in the system would restore it. Absence is not evidence of
// intent to revoke ownership, so the owner is preserved unless the caller
// names them with a different role explicitly.
//
// # Revisions move only on real change
//
// `membership_revision` and `authority_revision` advance only when a row was
// actually written, mirroring the recipient-audience path. A no-op sync must
// not invalidate every cached decision that keys on the revision.
// ReplaceMemberships converges a Space's roster to the declared set. Its
// second return value lists the "user"-subject IDs whose membership was
// just deactivated (the stale-member path below), and its third is the
// Space's org_id — the caller publishes a revocation event per subject so
// other planes can invalidate resource-scoped authorization tied to this
// Space, not just Control's own roster. org_id is returned rather than
// re-derived from the caller's own identity because this endpoint's
// principal (application-space-lifecycle) acts across every org's rosters,
// not one verified-delegation org at a time.
func (r *Repository) ReplaceMemberships(ctx context.Context, replacement MembershipReplacement) (*AuthorityRevision, []string, []string, string, error) {
	if r == nil || r.db == nil {
		return nil, nil, nil, "", fmt.Errorf("Space authority repository unavailable")
	}
	if err := replacement.Validate(); err != nil {
		return nil, nil, nil, "", err
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, nil, nil, "", fmt.Errorf("begin Space membership replacement: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	space, err := registeredSpaceForUpdate(ctx, tx, replacement.SpaceRef)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil, nil, "", ErrNoCurrentMembership
	}
	if err != nil {
		return nil, nil, nil, "", fmt.Errorf("lock Space for membership replacement: %w", err)
	}
	if space.RegistrationState != "active" {
		return nil, nil, nil, "", fmt.Errorf("Space is not registered as active")
	}
	if space.Kind == KindPersonal {
		// A personal Space has exactly one subject by construction. Letting a
		// roster sync widen it would quietly turn private storage into shared
		// storage.
		return nil, nil, nil, "", fmt.Errorf("a personal Space membership cannot be replaced")
	}

	var revisions AuthorityRevision
	if err := tx.QueryRow(ctx, `
		SELECT authority_revision, membership_revision, privacy_revision,
		       recipient_audience_revision, entitlement_revision
		FROM space_authority_revisions WHERE space_ref=$1 FOR UPDATE`, space.SpaceRef).Scan(
		&revisions.Authority, &revisions.Membership, &revisions.Privacy,
		&revisions.RecipientAudience, &revisions.Entitlement,
	); err != nil {
		return nil, nil, nil, "", fmt.Errorf("lock Space membership revision: %w", err)
	}

	ownerKey := "user\x00" + strings.TrimSpace(space.OwnerPrincipalID)
	managed := replacement.managedSubjectTypeSet()
	declared := make(map[string]struct{}, len(replacement.Members)+1)
	changed := false
	// A subject reactivated here (previously active=FALSE, now rejoining)
	// must have its earlier revocation event fully undone cross-plane too —
	// otherwise a legitimately rejoined member stays permanently denied by
	// session-core's space_membership_revocations projection, which only
	// this repository can ever tell them to clear.
	var reactivatedUserSubjects []string
	for _, member := range replacement.Members {
		subjectType := strings.TrimSpace(member.SubjectType)
		subjectID := strings.TrimSpace(member.SubjectID)
		role := strings.TrimSpace(member.Role)
		declared[subjectType+"\x00"+subjectID] = struct{}{}
		// A roster sync must not demote the owner. The source roster almost
		// always CONTAINS them — they are an ordinary member of the
		// organization too — so without this the owner is silently downgraded
		// on every single sync, and no later sync can restore them because the
		// same list keeps naming the lower role. Preserving presence alone was
		// not enough; the realistic harm here is demotion, not removal.
		//
		// Transferring ownership is a deliberate act and needs its own path,
		// not a side effect of a roster converging.
		if subjectType+"\x00"+subjectID == ownerKey && role != "owner" {
			continue
		}
		var wasActive sql.NullBool
		if err := tx.QueryRow(ctx, `
			SELECT active FROM space_memberships
			WHERE space_ref=$1 AND subject_type=$2 AND subject_id=$3 FOR UPDATE`,
			space.SpaceRef, subjectType, subjectID).Scan(&wasActive); err != nil && !errors.Is(err, pgx.ErrNoRows) {
			return nil, nil, nil, "", fmt.Errorf("read prior Space member state: %w", err)
		}
		tag, err := tx.Exec(ctx, `
			INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, active, granted_by)
			VALUES ($1, $2, $3, $4, TRUE, 'application-space-membership-sync')
			ON CONFLICT (space_ref, subject_type, subject_id) DO UPDATE
			SET role=EXCLUDED.role,
			    active=TRUE,
			    revision=space_memberships.revision+1,
			    granted_by=EXCLUDED.granted_by,
			    updated_at=NOW()
			WHERE space_memberships.role IS DISTINCT FROM EXCLUDED.role
			   OR space_memberships.active IS DISTINCT FROM TRUE`,
			space.SpaceRef, subjectType, subjectID, role)
		if err != nil {
			return nil, nil, nil, "", fmt.Errorf("upsert Space member: %w", err)
		}
		if tag.RowsAffected() > 0 {
			changed = true
			if subjectType == "user" && wasActive.Valid && !wasActive.Bool {
				reactivatedUserSubjects = append(reactivatedUserSubjects, subjectID)
			}
		}
	}
	// Preserve the owner even when the declared set omits them.
	declared[ownerKey] = struct{}{}

	rows, err := tx.Query(ctx, `
		SELECT subject_type, subject_id FROM space_memberships
		WHERE space_ref=$1 AND active`, space.SpaceRef)
	if err != nil {
		return nil, nil, nil, "", fmt.Errorf("read current Space members: %w", err)
	}
	var stale []MemberGrant
	for rows.Next() {
		var current MemberGrant
		if err := rows.Scan(&current.SubjectType, &current.SubjectID); err != nil {
			rows.Close()
			return nil, nil, nil, "", fmt.Errorf("scan current Space member: %w", err)
		}
		if managed != nil {
			// Outside the caller's declared scope. org-core's roster sync knows
			// people and nothing else, so without this an agent bound to the room
			// is revoked the next time a human roster converges — a binding
			// destroyed as a side effect of a sync that never knew it existed.
			if _, owned := managed[current.SubjectType]; !owned {
				continue
			}
		}
		if _, keep := declared[current.SubjectType+"\x00"+current.SubjectID]; !keep {
			stale = append(stale, current)
		}
	}
	rows.Close()

	var revokedUserSubjects []string
	for _, member := range stale {
		if _, err := tx.Exec(ctx, `
			UPDATE space_memberships
			SET active=FALSE, revision=revision+1, updated_at=NOW()
			WHERE space_ref=$1 AND subject_type=$2 AND subject_id=$3`,
			space.SpaceRef, member.SubjectType, member.SubjectID); err != nil {
			return nil, nil, nil, "", fmt.Errorf("revoke Space member: %w", err)
		}
		changed = true
		if member.SubjectType == "user" {
			revokedUserSubjects = append(revokedUserSubjects, member.SubjectID)
		}
	}

	if changed {
		if err := tx.QueryRow(ctx, `
			UPDATE space_authority_revisions
			SET authority_revision=authority_revision+1,
			    membership_revision=membership_revision+1,
			    updated_at=NOW()
			WHERE space_ref=$1
			RETURNING authority_revision, membership_revision, privacy_revision,
			          recipient_audience_revision, entitlement_revision`, space.SpaceRef).Scan(
			&revisions.Authority, &revisions.Membership, &revisions.Privacy,
			&revisions.RecipientAudience, &revisions.Entitlement,
		); err != nil {
			return nil, nil, nil, "", fmt.Errorf("advance Space membership revision: %w", err)
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, nil, nil, "", fmt.Errorf("commit Space membership replacement: %w", err)
	}
	return &revisions, revokedUserSubjects, reactivatedUserSubjects, space.OrgID, nil
}

func canonicalRecipientSubjects(recipients []string) ([]string, error) {
	if _, err := RecipientAudienceHash(recipients...); err != nil {
		return nil, err
	}
	canonical := make([]string, 0, len(recipients))
	for _, recipient := range recipients {
		canonical = append(canonical, strings.TrimSpace(recipient))
	}
	sort.Strings(canonical)
	return canonical, nil
}

// ResolveCurrentUserMembership reads Control's current membership and all
// authority revisions in one query. It intentionally does not issue a Space
// access decision: recipient, policy, and owner-resource facts are owned by
// separate authoritative inputs and must be intersected by a later issuer.
func (r *Repository) ResolveCurrentUserMembership(ctx context.Context, spaceRef, orgID, subjectID string) (*CurrentMembership, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("Space authority repository unavailable")
	}
	membership := &CurrentMembership{}
	err := r.db.Pool.QueryRow(ctx, `
		SELECT s.space_ref, s.org_id, m.subject_id, s.space_kind, m.role,
		       r.authority_revision, r.membership_revision, r.privacy_revision,
		       r.recipient_audience_revision, r.entitlement_revision
		FROM registered_spaces s
		JOIN space_memberships m
		  ON m.space_ref=s.space_ref
		JOIN space_authority_revisions r
		  ON r.space_ref=s.space_ref
		WHERE s.space_ref=$1
		  AND s.org_id=$2
		  AND s.registration_state='active'
		  AND m.subject_type='user'
		  AND m.subject_id=$3
		  AND m.active=TRUE
		  AND EXISTS (
			SELECT 1 FROM user_org_memberships u
			WHERE u.user_id=$3 AND u.org_id=$2 AND u.status='active'
		  )`,
		strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), strings.TrimSpace(subjectID),
	).Scan(
		&membership.SpaceRef, &membership.OrgID, &membership.SubjectID, &membership.Kind, &membership.Role,
		&membership.Revisions.Authority, &membership.Revisions.Membership, &membership.Revisions.Privacy,
		&membership.Revisions.RecipientAudience, &membership.Revisions.Entitlement,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNoCurrentMembership
	}
	if err != nil {
		return nil, fmt.Errorf("resolve current Space membership: %w", err)
	}
	if err := membership.Validate(); err != nil {
		return nil, fmt.Errorf("invalid current Space membership: %w", err)
	}
	return membership, nil
}

// ResolvePersonalThreadDecisionEvidence reads every Control-owned fact needed
// by the first scoped Model effect in one tenant-scoped query. It derives the
// sole personal recipient and resource authorization from current authority;
// neither is accepted from a gateway request. A missing policy row is an
// authorization denial, never an implicit permissive default.
func (r *Repository) ResolvePersonalThreadDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (PersonalThreadDecisionEvidence, error) {
	evidence, err := r.resolvePersonalDecisionEvidence(ctx, spaceRef, orgID, subjectID)
	if err != nil {
		return PersonalThreadDecisionEvidence{}, err
	}
	evidence.ResourceAuthorizationRef = fmt.Sprintf("control:%s:thread-create:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if err := evidence.Validate(); err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("invalid personal Space thread authority: %w", err)
	}
	return evidence, nil
}

// ResolveSharedThreadDecisionEvidence resolves a Control-acknowledged
// Application recipient snapshot at the exact current Control audience
// revision. The actor must remain an active Space/org member and an active
// recipient at query time; a prior membership in the Space is insufficient.
func (r *Repository) ResolveSharedThreadDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (PersonalThreadDecisionEvidence, error) {
	if r == nil || r.db == nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("Space authority repository unavailable")
	}
	var evidence PersonalThreadDecisionEvidence
	err := r.db.Pool.QueryRow(ctx, `
		SELECT s.space_ref, s.org_id, m.subject_id, s.space_kind, m.role,
		       r.authority_revision, r.membership_revision, r.privacy_revision,
		       r.recipient_audience_revision, r.entitlement_revision,
		       a.audience_ref, a.audience_hash,
		       p.privacy_policy_ref, p.purpose, p.lawful_basis, p.privacy_class,
		       p.third_party_processing_allowed, p.retention_class, p.residency,
		       p.deletion_scope, p.zero_data_retention, p.thread_create_entitled, p.agent_action_entitled, p.schedule_fire_entitled
		FROM registered_spaces s
		JOIN space_memberships m ON m.space_ref=s.space_ref
		JOIN space_authority_revisions r ON r.space_ref=s.space_ref
		JOIN space_recipient_audiences a ON a.space_ref=s.space_ref AND a.revision=r.recipient_audience_revision
		JOIN space_recipient_audience_members am ON am.space_ref=a.space_ref AND am.revision=a.revision AND am.subject_id=m.subject_id
		JOIN space_effect_policies p ON p.org_id=s.org_id
		WHERE s.space_ref=$1 AND s.org_id=$2 AND s.registration_state='active'
		  AND s.space_kind <> 'personal'
		  AND m.subject_type='user' AND m.subject_id=$3 AND m.active=TRUE
		  AND EXISTS (SELECT 1 FROM user_org_memberships u WHERE u.user_id=$3 AND u.org_id=$2 AND u.status='active')`,
		strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), strings.TrimSpace(subjectID),
	).Scan(
		&evidence.Membership.SpaceRef, &evidence.Membership.OrgID, &evidence.Membership.SubjectID,
		&evidence.Membership.Kind, &evidence.Membership.Role,
		&evidence.Membership.Revisions.Authority, &evidence.Membership.Revisions.Membership,
		&evidence.Membership.Revisions.Privacy, &evidence.Membership.Revisions.RecipientAudience,
		&evidence.Membership.Revisions.Entitlement,
		&evidence.RecipientAudienceRef, &evidence.RecipientAudienceHash,
		&evidence.Privacy.PolicyRef, &evidence.Privacy.Purpose, &evidence.Privacy.LawfulBasis,
		&evidence.Privacy.PrivacyClass, &evidence.Privacy.ThirdPartyAllowed,
		&evidence.Privacy.RetentionClass, &evidence.Privacy.Residency, &evidence.Privacy.DeletionScope,
		&evidence.Privacy.ZeroDataRetention, &evidence.ThreadCreateEntitled, &evidence.AgentActionEntitled, &evidence.ScheduleFireEntitled,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return PersonalThreadDecisionEvidence{}, ErrNoCurrentMembership
	}
	if err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("resolve shared Space thread authority: %w", err)
	}
	evidence.ResourceAuthorizationRef = fmt.Sprintf("control:%s:thread-create:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if err := evidence.ValidateForSharedThread(); err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("invalid shared Space thread authority: %w", err)
	}
	return evidence, nil
}

// ResolveSharedRetrievalDecisionEvidence mirrors
// ResolveSharedThreadDecisionEvidence's current-membership and current-
// recipient-audience resolution, but binds the independent retrieval
// entitlement and resource reference instead of thread-create's. This must
// never fall back to the shared thread-create grant.
func (r *Repository) ResolveSharedRetrievalDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (PersonalThreadDecisionEvidence, error) {
	if r == nil || r.db == nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("Space authority repository unavailable")
	}
	var evidence PersonalThreadDecisionEvidence
	err := r.db.Pool.QueryRow(ctx, `
		SELECT s.space_ref, s.org_id, m.subject_id, s.space_kind, m.role,
		       r.authority_revision, r.membership_revision, r.privacy_revision,
		       r.recipient_audience_revision, r.entitlement_revision,
		       a.audience_ref, a.audience_hash,
		       p.privacy_policy_ref, p.purpose, p.lawful_basis, p.privacy_class,
		       p.third_party_processing_allowed, p.retention_class, p.residency,
		       p.deletion_scope, p.zero_data_retention, p.retrieval_read_entitled
		FROM registered_spaces s
		JOIN space_memberships m ON m.space_ref=s.space_ref
		JOIN space_authority_revisions r ON r.space_ref=s.space_ref
		JOIN space_recipient_audiences a ON a.space_ref=s.space_ref AND a.revision=r.recipient_audience_revision
		JOIN space_recipient_audience_members am ON am.space_ref=a.space_ref AND am.revision=a.revision AND am.subject_id=m.subject_id
		JOIN space_effect_policies p ON p.org_id=s.org_id
		WHERE s.space_ref=$1 AND s.org_id=$2 AND s.registration_state='active'
		  AND s.space_kind <> 'personal'
		  AND m.subject_type='user' AND m.subject_id=$3 AND m.active=TRUE
		  AND EXISTS (SELECT 1 FROM user_org_memberships u WHERE u.user_id=$3 AND u.org_id=$2 AND u.status='active')`,
		strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), strings.TrimSpace(subjectID),
	).Scan(
		&evidence.Membership.SpaceRef, &evidence.Membership.OrgID, &evidence.Membership.SubjectID,
		&evidence.Membership.Kind, &evidence.Membership.Role,
		&evidence.Membership.Revisions.Authority, &evidence.Membership.Revisions.Membership,
		&evidence.Membership.Revisions.Privacy, &evidence.Membership.Revisions.RecipientAudience,
		&evidence.Membership.Revisions.Entitlement,
		&evidence.RecipientAudienceRef, &evidence.RecipientAudienceHash,
		&evidence.Privacy.PolicyRef, &evidence.Privacy.Purpose, &evidence.Privacy.LawfulBasis,
		&evidence.Privacy.PrivacyClass, &evidence.Privacy.ThirdPartyAllowed,
		&evidence.Privacy.RetentionClass, &evidence.Privacy.Residency, &evidence.Privacy.DeletionScope,
		&evidence.Privacy.ZeroDataRetention, &evidence.RetrievalReadEntitled,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return PersonalThreadDecisionEvidence{}, ErrNoCurrentMembership
	}
	if err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("resolve shared Space retrieval authority: %w", err)
	}
	evidence.ResourceAuthorizationRef = fmt.Sprintf("control:%s:retrieval-read:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if err := evidence.ValidateForSharedRetrieval(); err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("invalid shared Space retrieval authority: %w", err)
	}
	return evidence, nil
}

// ResolveSharedThreadReadDecisionEvidence resolves the authority to read a
// shared Space's conversation record. It reuses the current-membership and
// current-recipient-audience joins of its sibling resolvers — in particular
// the join against space_recipient_audience_members, which is what makes a
// removed participant stop resolving — and binds the independent
// thread_read_entitled bit and its own resource reference.
//
// It must never fall back to the shared thread-create grant: writing into a
// room and reading everyone else's turns in it are separate disclosures.
func (r *Repository) ResolveSharedThreadReadDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (PersonalThreadDecisionEvidence, error) {
	if r == nil || r.db == nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("Space authority repository unavailable")
	}
	var evidence PersonalThreadDecisionEvidence
	err := r.db.Pool.QueryRow(ctx, `
		SELECT s.space_ref, s.org_id, m.subject_id, s.space_kind, m.role,
		       r.authority_revision, r.membership_revision, r.privacy_revision,
		       r.recipient_audience_revision, r.entitlement_revision,
		       a.audience_ref, a.audience_hash,
		       p.privacy_policy_ref, p.purpose, p.lawful_basis, p.privacy_class,
		       p.third_party_processing_allowed, p.retention_class, p.residency,
		       p.deletion_scope, p.zero_data_retention, p.thread_read_entitled
		FROM registered_spaces s
		JOIN space_memberships m ON m.space_ref=s.space_ref
		JOIN space_authority_revisions r ON r.space_ref=s.space_ref
		JOIN space_recipient_audiences a ON a.space_ref=s.space_ref AND a.revision=r.recipient_audience_revision
		JOIN space_recipient_audience_members am ON am.space_ref=a.space_ref AND am.revision=a.revision AND am.subject_id=m.subject_id
		JOIN space_effect_policies p ON p.org_id=s.org_id
		WHERE s.space_ref=$1 AND s.org_id=$2 AND s.registration_state='active'
		  AND s.space_kind <> 'personal'
		  AND m.subject_type='user' AND m.subject_id=$3 AND m.active=TRUE
		  AND EXISTS (SELECT 1 FROM user_org_memberships u WHERE u.user_id=$3 AND u.org_id=$2 AND u.status='active')`,
		strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), strings.TrimSpace(subjectID),
	).Scan(
		&evidence.Membership.SpaceRef, &evidence.Membership.OrgID, &evidence.Membership.SubjectID,
		&evidence.Membership.Kind, &evidence.Membership.Role,
		&evidence.Membership.Revisions.Authority, &evidence.Membership.Revisions.Membership,
		&evidence.Membership.Revisions.Privacy, &evidence.Membership.Revisions.RecipientAudience,
		&evidence.Membership.Revisions.Entitlement,
		&evidence.RecipientAudienceRef, &evidence.RecipientAudienceHash,
		&evidence.Privacy.PolicyRef, &evidence.Privacy.Purpose, &evidence.Privacy.LawfulBasis,
		&evidence.Privacy.PrivacyClass, &evidence.Privacy.ThirdPartyAllowed,
		&evidence.Privacy.RetentionClass, &evidence.Privacy.Residency, &evidence.Privacy.DeletionScope,
		&evidence.Privacy.ZeroDataRetention, &evidence.ThreadReadEntitled,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return PersonalThreadDecisionEvidence{}, ErrNoCurrentMembership
	}
	if err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("resolve shared Space thread read authority: %w", err)
	}
	evidence.ResourceAuthorizationRef = fmt.Sprintf("control:%s:thread-read:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if err := evidence.ValidateForSharedThreadRead(); err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("invalid shared Space thread read authority: %w", err)
	}
	return evidence, nil
}

// ResolvePersonalRetrievalDecisionEvidence returns the same current Control
// authority facts as thread issuance, but binds them to the independent
// retrieval entitlement and resource reference. This must never fall back to
// the thread-create grant.
func (r *Repository) ResolvePersonalRetrievalDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (PersonalThreadDecisionEvidence, error) {
	evidence, err := r.resolvePersonalDecisionEvidence(ctx, spaceRef, orgID, subjectID)
	if err != nil {
		return PersonalThreadDecisionEvidence{}, err
	}
	evidence.ResourceAuthorizationRef = fmt.Sprintf("control:%s:retrieval-read:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if err := evidence.ValidateForRetrieval(); err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("invalid personal Space retrieval authority: %w", err)
	}
	return evidence, nil
}

// ResolvePersonalImportDecisionEvidence keeps durable Ingestion write
// authority separate from both Model thread creation and Data retrieval.
func (r *Repository) ResolvePersonalImportDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (PersonalThreadDecisionEvidence, error) {
	evidence, err := r.resolvePersonalDecisionEvidence(ctx, spaceRef, orgID, subjectID)
	if err != nil {
		return PersonalThreadDecisionEvidence{}, err
	}
	evidence.ResourceAuthorizationRef = fmt.Sprintf("control:%s:ingestion-import:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if err := evidence.ValidateForImport(); err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("invalid personal Space import authority: %w", err)
	}
	return evidence, nil
}

// ResolveAgentActionDecisionEvidence returns fresh source-context evidence for
// one Model run requesting a target owner action. Its resource reference is
// the thread's original source-context binding, not authority for a
// ticket/conversation: the target owner must make its own resource decision at
// effect time.
func (r *Repository) ResolveAgentActionDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (PersonalThreadDecisionEvidence, error) {
	membership, err := r.ResolveCurrentUserMembership(ctx, spaceRef, orgID, subjectID)
	if err != nil {
		return PersonalThreadDecisionEvidence{}, err
	}
	var evidence PersonalThreadDecisionEvidence
	if membership.Kind == KindPersonal {
		evidence, err = r.resolvePersonalDecisionEvidence(ctx, spaceRef, orgID, subjectID)
		if err != nil {
			return PersonalThreadDecisionEvidence{}, err
		}
	} else {
		evidence, err = r.ResolveSharedThreadDecisionEvidence(ctx, spaceRef, orgID, subjectID)
		if err != nil {
			return PersonalThreadDecisionEvidence{}, err
		}
	}
	evidence.ResourceAuthorizationRef = fmt.Sprintf("control:%s:thread-create:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if err := evidence.ValidateForAgentAction(); err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("invalid Space agent action authority: %w", err)
	}
	return evidence, nil
}

func (r *Repository) resolvePersonalDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (PersonalThreadDecisionEvidence, error) {
	if r == nil || r.db == nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("Space authority repository unavailable")
	}
	var evidence PersonalThreadDecisionEvidence
	err := r.db.Pool.QueryRow(ctx, `
		SELECT s.space_ref, s.org_id, m.subject_id, s.space_kind, m.role,
		       r.authority_revision, r.membership_revision,
		       r.privacy_revision, r.recipient_audience_revision, r.entitlement_revision,
		       p.privacy_policy_ref, p.purpose, p.lawful_basis, p.privacy_class,
		       p.third_party_processing_allowed, p.retention_class, p.residency,
		       p.deletion_scope, p.zero_data_retention, p.thread_create_entitled,
		       p.retrieval_read_entitled, p.import_write_entitled, p.agent_action_entitled, p.schedule_fire_entitled
		FROM registered_spaces s
		JOIN space_memberships m ON m.space_ref=s.space_ref
		JOIN space_authority_revisions r ON r.space_ref=s.space_ref
		JOIN space_effect_policies p ON p.org_id=s.org_id
		WHERE s.space_ref=$1 AND s.org_id=$2 AND s.registration_state='active'
		  AND m.subject_type='user' AND m.subject_id=$3 AND m.active=TRUE
		  AND EXISTS (
			SELECT 1 FROM user_org_memberships u
			WHERE u.user_id=$3 AND u.org_id=$2 AND u.status='active'
		  )`,
		strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), strings.TrimSpace(subjectID),
	).Scan(
		&evidence.Membership.SpaceRef, &evidence.Membership.OrgID, &evidence.Membership.SubjectID,
		&evidence.Membership.Kind, &evidence.Membership.Role,
		&evidence.Membership.Revisions.Authority, &evidence.Membership.Revisions.Membership,
		&evidence.Membership.Revisions.Privacy, &evidence.Membership.Revisions.RecipientAudience,
		&evidence.Membership.Revisions.Entitlement,
		&evidence.Privacy.PolicyRef, &evidence.Privacy.Purpose, &evidence.Privacy.LawfulBasis,
		&evidence.Privacy.PrivacyClass, &evidence.Privacy.ThirdPartyAllowed,
		&evidence.Privacy.RetentionClass, &evidence.Privacy.Residency, &evidence.Privacy.DeletionScope,
		&evidence.Privacy.ZeroDataRetention, &evidence.ThreadCreateEntitled,
		&evidence.RetrievalReadEntitled, &evidence.ImportWriteEntitled, &evidence.AgentActionEntitled, &evidence.ScheduleFireEntitled,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return PersonalThreadDecisionEvidence{}, ErrNoCurrentMembership
	}
	if err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("resolve personal Space authority: %w", err)
	}
	evidence.RecipientSubjectID = evidence.Membership.SubjectID
	evidence.RecipientAudienceRef = fmt.Sprintf("personal:%s:recipient:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.RecipientAudience)
	evidence.RecipientAudienceHash, err = RecipientAudienceHash(evidence.RecipientSubjectID)
	if err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("derive personal recipient audience hash: %w", err)
	}
	return evidence, nil
}

// UpsertEffectPolicy stores a Control-owned privacy/entitlement floor and
// fences every active Space in that organization when the effective policy
// changes. Replays are idempotent: identical policy material does not make
// outstanding decisions stale merely because a sync retried.
func (r *Repository) UpsertEffectPolicy(ctx context.Context, policy EffectPolicy) (bool, error) {
	if r == nil || r.db == nil {
		return false, fmt.Errorf("Space authority repository unavailable")
	}
	if err := policy.Validate(); err != nil {
		return false, err
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return false, fmt.Errorf("begin Space effect policy upsert: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	var current EffectPolicy
	err = tx.QueryRow(ctx, `
		SELECT org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
		       third_party_processing_allowed, retention_class, residency,
		       deletion_scope, zero_data_retention, thread_create_entitled, retrieval_read_entitled,
		       import_write_entitled, agent_action_entitled, schedule_fire_entitled, thread_read_entitled
		FROM space_effect_policies WHERE org_id=$1 FOR UPDATE`, policy.OrgID,
	).Scan(&current.OrgID, &current.PrivacyPolicyRef, &current.Purpose, &current.LawfulBasis,
		&current.PrivacyClass, &current.ThirdPartyProcessingAllowed, &current.RetentionClass,
		&current.Residency, &current.DeletionScope, &current.ZeroDataRetention,
		&current.ThreadCreateEntitled, &current.RetrievalReadEntitled, &current.ImportWriteEntitled, &current.AgentActionEntitled, &current.ScheduleFireEntitled,
		&current.ThreadReadEntitled)
	if err != nil && err != pgx.ErrNoRows {
		return false, fmt.Errorf("read Space effect policy: %w", err)
	}
	if err == pgx.ErrNoRows {
		if _, err := tx.Exec(ctx, `
			INSERT INTO space_effect_policies
			(org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
			 third_party_processing_allowed, retention_class, residency, deletion_scope,
			 zero_data_retention, thread_create_entitled, retrieval_read_entitled, import_write_entitled, agent_action_entitled, schedule_fire_entitled,
			 thread_read_entitled)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)`,
			policy.OrgID, policy.PrivacyPolicyRef, policy.Purpose, policy.LawfulBasis,
			policy.PrivacyClass, policy.ThirdPartyProcessingAllowed, policy.RetentionClass,
			policy.Residency, policy.DeletionScope, policy.ZeroDataRetention,
			policy.ThreadCreateEntitled, policy.RetrievalReadEntitled, policy.ImportWriteEntitled, policy.AgentActionEntitled, policy.ScheduleFireEntitled,
			policy.ThreadReadEntitled); err != nil {
			return false, fmt.Errorf("insert Space effect policy: %w", err)
		}
		if err := tx.Commit(ctx); err != nil {
			return false, fmt.Errorf("commit Space effect policy: %w", err)
		}
		return true, nil
	}
	if current == policy {
		if err := tx.Commit(ctx); err != nil {
			return false, fmt.Errorf("commit Space effect policy replay: %w", err)
		}
		return false, nil
	}
	privacyChanged := current.PrivacyPolicyRef != policy.PrivacyPolicyRef || current.Purpose != policy.Purpose ||
		current.LawfulBasis != policy.LawfulBasis || current.PrivacyClass != policy.PrivacyClass ||
		current.ThirdPartyProcessingAllowed != policy.ThirdPartyProcessingAllowed || current.RetentionClass != policy.RetentionClass ||
		current.Residency != policy.Residency || current.DeletionScope != policy.DeletionScope ||
		current.ZeroDataRetention != policy.ZeroDataRetention
	entitlementChanged := current.ThreadCreateEntitled != policy.ThreadCreateEntitled ||
		current.RetrievalReadEntitled != policy.RetrievalReadEntitled ||
		current.ImportWriteEntitled != policy.ImportWriteEntitled ||
		current.AgentActionEntitled != policy.AgentActionEntitled ||
		current.ScheduleFireEntitled != policy.ScheduleFireEntitled ||
		current.ThreadReadEntitled != policy.ThreadReadEntitled
	if _, err := tx.Exec(ctx, `
		UPDATE space_effect_policies SET privacy_policy_ref=$2, purpose=$3, lawful_basis=$4,
		privacy_class=$5, third_party_processing_allowed=$6, retention_class=$7,
		residency=$8, deletion_scope=$9, zero_data_retention=$10,
		thread_create_entitled=$11, retrieval_read_entitled=$12, import_write_entitled=$13, agent_action_entitled=$14, schedule_fire_entitled=$15,
		thread_read_entitled=$16,
		policy_revision=policy_revision + CASE WHEN $17 THEN 1 ELSE 0 END,
		entitlement_revision=entitlement_revision + CASE WHEN $18 THEN 1 ELSE 0 END,
		updated_at=NOW() WHERE org_id=$1`,
		policy.OrgID, policy.PrivacyPolicyRef, policy.Purpose, policy.LawfulBasis,
		policy.PrivacyClass, policy.ThirdPartyProcessingAllowed, policy.RetentionClass,
		policy.Residency, policy.DeletionScope, policy.ZeroDataRetention,
		policy.ThreadCreateEntitled, policy.RetrievalReadEntitled, policy.ImportWriteEntitled, policy.AgentActionEntitled, policy.ScheduleFireEntitled,
		policy.ThreadReadEntitled, privacyChanged, entitlementChanged); err != nil {
		return false, fmt.Errorf("update Space effect policy: %w", err)
	}
	if _, err := tx.Exec(ctx, `
		UPDATE space_authority_revisions r
		SET authority_revision=authority_revision + 1,
		    privacy_revision=privacy_revision + CASE WHEN $2 THEN 1 ELSE 0 END,
		    entitlement_revision=entitlement_revision + CASE WHEN $3 THEN 1 ELSE 0 END,
		    updated_at=NOW()
		FROM registered_spaces s
		WHERE r.space_ref=s.space_ref AND s.org_id=$1 AND s.registration_state='active'`,
		policy.OrgID, privacyChanged, entitlementChanged); err != nil {
		return false, fmt.Errorf("fence Space authorities for effect policy: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return false, fmt.Errorf("commit Space effect policy update: %w", err)
	}
	return true, nil
}

func registeredSpaceForUpdate(ctx context.Context, tx pgx.Tx, spaceRef string) (*RegisteredSpace, error) {
	space := &RegisteredSpace{}
	err := tx.QueryRow(ctx, `
		SELECT space_ref, org_id, space_kind, owner_principal_id, application_lifecycle_revision,
		       registration_state, registered_at, updated_at
		FROM registered_spaces WHERE space_ref=$1 FOR UPDATE`, strings.TrimSpace(spaceRef),
	).Scan(
		&space.SpaceRef, &space.OrgID, &space.Kind, &space.OwnerPrincipalID, &space.ApplicationLifecycleRevision,
		&space.RegistrationState, &space.RegisteredAt, &space.UpdatedAt,
	)
	if err != nil {
		return nil, err
	}
	return space, nil
}

func requireActiveOwnerMembership(ctx context.Context, tx pgx.Tx, ownerID, orgID string) error {
	var active bool
	if err := tx.QueryRow(ctx, `
		SELECT EXISTS(
			SELECT 1 FROM user_org_memberships
			WHERE user_id=$1 AND org_id=$2 AND status='active'
		)`, ownerID, orgID,
	).Scan(&active); err != nil {
		return fmt.Errorf("verify Space owner organization membership: %w", err)
	}
	if !active {
		return ErrInactiveOwnerMembership
	}
	return nil
}
