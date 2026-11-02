package spaces

import (
	"context"
	"crypto/ed25519"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// WS14-S2: the Space side of the room-file grant (route file-grant).
//
// A room file's owner is its uploader. The attach act adds ONE extra
// resource_grants row whose subject is the Space. The row is inert on its own:
// it counts only inside a Space read that carries a current Space decision and
// current membership/audience (the space-visible join), so a Space subject can
// only narrow or co-require, never widen an owner ACL.
//
// documents-api is not a delegated user caller during an import (the worker has
// no user bearer), so the write is proved by the Control-signed execution
// decision the import already carries. Control verifies its OWN signature here,
// then re-resolves membership, audience, role, entitlement and ZDR NOW: a grant
// is never minted from a token alone.

// SpaceUploadSourceType is the only import source type a shared Space accepts.
const SpaceUploadSourceType = "space_upload"

// FileGrantAuthority is the current Control authority behind one file grant.
type FileGrantAuthority struct {
	OrgID     string
	SpaceRef  string
	SubjectID string
	Role      string
}

// VerifyIssuedDecision verifies a decision token Control itself signed:
// envelope, key id, signature, payload validity and the time window. It binds
// no caller intent (there is none to bind); the caller must check the claims it
// relies on, which ValidateForFileGrant does for the file-grant route.
func VerifyIssuedDecision(key SigningKey, token string, now time.Time) (*Decision, error) {
	if strings.TrimSpace(key.ID) == "" || len(key.PrivateKey) != ed25519.PrivateKeySize {
		return nil, fmt.Errorf("valid Space decision signing key is required")
	}
	public, ok := key.PrivateKey.Public().(ed25519.PublicKey)
	if !ok {
		return nil, fmt.Errorf("valid Space decision signing key is required")
	}
	decision, err := parseVerifiedDecision(VerificationKey{ID: key.ID, PublicKey: public}, token)
	if err != nil {
		return nil, err
	}
	if err := checkDecisionWindow(decision, now); err != nil {
		return nil, err
	}
	return decision, nil
}

// ValidateForFileGrant checks that a verified decision is an import execution
// decision for a room upload. It is deliberately strict: any other action,
// audience or source type, or a ZDR decision, is refused.
func (d Decision) ValidateForFileGrant() error {
	if d.ActionID != personalImportAction || d.ActionSchemaHash != personalImportSchema {
		return fmt.Errorf("space decision is not an import decision")
	}
	if d.ServiceAudience != personalImportExecutionAudience {
		return fmt.Errorf("space decision is not an import execution decision")
	}
	if d.ImportSourceType != SpaceUploadSourceType {
		return fmt.Errorf("space decision source type is not a room upload")
	}
	if d.ZeroDataRetention {
		return zdrForbidsDenial("import_write", "zero data retention forbids durable import")
	}
	return nil
}

// ResolveFileGrantAuthority re-resolves, now, everything a room-file grant
// depends on: an active shared Space of the decision's org, the subject as an
// active org member AND active Space member AND in the current recipient
// audience, role editor or above, the org's import entitlement on, and ZDR off.
// A refusal is a typed *Denial; a subject that no longer resolves is
// ErrNoCurrentMembership.
func (r *Repository) ResolveFileGrantAuthority(ctx context.Context, decision Decision) (FileGrantAuthority, error) {
	if r == nil || r.db == nil {
		return FileGrantAuthority{}, fmt.Errorf("space authority repository unavailable")
	}
	var role string
	var zdr, importEntitled bool
	err := r.db.Pool.QueryRow(ctx, `
		SELECT m.role, p.zero_data_retention, p.import_write_entitled
		FROM registered_spaces s
		JOIN space_memberships m ON m.space_ref=s.space_ref
		JOIN space_authority_revisions r ON r.space_ref=s.space_ref
		JOIN space_recipient_audience_members am ON am.space_ref=s.space_ref AND am.revision=r.recipient_audience_revision AND am.subject_id=m.subject_id
		JOIN space_effect_policies p ON p.org_id=s.org_id
		WHERE s.space_ref=$1 AND s.org_id=$2 AND s.registration_state='active'
		  AND s.space_kind <> 'personal'
		  AND m.subject_type='user' AND m.subject_id=$3 AND m.active=TRUE
		  AND EXISTS (SELECT 1 FROM user_org_memberships u WHERE u.user_id=$3 AND u.org_id=$2 AND u.status='active')`,
		strings.TrimSpace(decision.SpaceRef), strings.TrimSpace(decision.OrgID), strings.TrimSpace(decision.SubjectID),
	).Scan(&role, &zdr, &importEntitled)
	if errors.Is(err, pgx.ErrNoRows) {
		return FileGrantAuthority{}, ErrNoCurrentMembership
	}
	if err != nil {
		return FileGrantAuthority{}, fmt.Errorf("resolve room file grant authority: %w", err)
	}
	if !matchesOneOf(role, RoleEditor, RoleManager, RoleOwner) {
		return FileGrantAuthority{}, roleBelowFloorDenial(fmt.Sprintf("space role %q cannot attach files", role), RoleEditor, RoleManager, RoleOwner)
	}
	if zdr {
		return FileGrantAuthority{}, zdrForbidsDenial("import_write", "zero data retention forbids durable import")
	}
	if !importEntitled {
		return FileGrantAuthority{}, entitlementOffDenial("import_write", "import entitlement is not active")
	}
	return FileGrantAuthority{
		OrgID: decision.OrgID, SpaceRef: decision.SpaceRef, SubjectID: decision.SubjectID, Role: role,
	}, nil
}
