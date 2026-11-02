package spaces

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5"
)

// WS12-S5: the per-organization original-text rule for revised Space messages
// and the evidence resolvers of the `model.thread.revise` decision.
//
// space_message_policies has NO default row: an organization that never chose a
// rule resolves to 'purge' (the safe default), and a row can only say 'retain'
// or 'purge'. The rule is written by the Control workload only (operator path,
// ledgered in space_message_policy_events); a room's own owners cannot change
// it. Ids, enums and counts only, no content.

// MessagePolicy is one organization's original-text rule.
type MessagePolicy struct {
	OrgID             string `json:"org_id"`
	OriginalRetention string `json:"original_retention"`
}

// Validate reports whether the policy names an organization and a closed rule.
func (p MessagePolicy) Validate() error {
	if strings.TrimSpace(p.OrgID) == "" {
		return fmt.Errorf("space message policy requires an organization")
	}
	if !ValidOriginalRetention(p.OriginalRetention) {
		return fmt.Errorf("space message policy original_retention must be retain or purge")
	}
	return nil
}

// MessagePolicyHints is what a member's UI may show about message lifecycle. It
// is a hint, never an authorization: every revision still needs a Control
// decision.
type MessagePolicyHints struct {
	ReviseEntitled    bool   `json:"revise_entitled"`
	ModerateEntitled  bool   `json:"moderate_entitled"`
	OriginalRetention string `json:"original_retention"`
}

// UpsertMessagePolicy writes the organization's rule and appends one operator
// evidence row per attempt, even when the value is unchanged.
func (r *Repository) UpsertMessagePolicy(ctx context.Context, policy MessagePolicy, actorPrincipalID string) error {
	if r == nil || r.db == nil {
		return fmt.Errorf("space authority repository unavailable")
	}
	policy.OrgID = strings.TrimSpace(policy.OrgID)
	if err := policy.Validate(); err != nil {
		return err
	}
	if strings.TrimSpace(actorPrincipalID) == "" {
		return fmt.Errorf("space message policy actor is required")
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin Space message policy update: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `
		INSERT INTO space_message_policies (org_id, original_retention)
		VALUES ($1,$2)
		ON CONFLICT (org_id) DO UPDATE SET
			original_retention=EXCLUDED.original_retention, updated_at=NOW()`,
		policy.OrgID, policy.OriginalRetention); err != nil {
		return fmt.Errorf("upsert Space message policy: %w", err)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO space_message_policy_events (org_id, actor_principal_id, original_retention)
		VALUES ($1,$2,$3)`, policy.OrgID, actorPrincipalID, policy.OriginalRetention); err != nil {
		return fmt.Errorf("record Space message policy audit: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit Space message policy update: %w", err)
	}
	return nil
}

// MessagePolicyHints returns the lifecycle hints for one current member of a
// Space (an archived room still resolves). A non-member gets
// ErrNoCurrentMembership and learns nothing. Under Zero Data Retention both
// entitlement hints are false, matching the refusal the decision would give.
func (r *Repository) MessagePolicyHints(ctx context.Context, spaceRef, orgID, subjectID string) (MessagePolicyHints, error) {
	membership, err := r.ResolveReadableUserMembership(ctx, spaceRef, orgID, subjectID)
	if err != nil {
		return MessagePolicyHints{}, err
	}
	var revise, moderate, zdr bool
	var retention string
	err = r.db.Pool.QueryRow(ctx, `
		SELECT p.message_revise_entitled, p.message_moderate_entitled, p.zero_data_retention,
		       COALESCE(mp.original_retention, 'purge')
		FROM space_effect_policies p
		LEFT JOIN space_message_policies mp ON mp.org_id=p.org_id
		WHERE p.org_id=$1`, strings.TrimSpace(orgID)).Scan(&revise, &moderate, &zdr, &retention)
	if errors.Is(err, pgx.ErrNoRows) {
		return MessagePolicyHints{OriginalRetention: OriginalRetentionPurge}, nil
	}
	if err != nil {
		return MessagePolicyHints{}, fmt.Errorf("read Space message policy: %w", err)
	}
	if !ValidOriginalRetention(retention) {
		retention = OriginalRetentionPurge
	}
	hints := MessagePolicyHints{OriginalRetention: retention}
	if zdr {
		return hints, nil
	}
	hints.ReviseEntitled = revise && matchesOneOf(membership.Role, "editor", "manager", "owner")
	hints.ModerateEntitled = hints.ReviseEntitled && moderate &&
		membership.Kind != KindPersonal && !membership.Kind.IsDM() &&
		matchesOneOf(membership.Role, "manager", "owner")
	return hints, nil
}

// ResolvePersonalThreadReviseDecisionEvidence resolves the authority to revise
// a message in a personal Space. The personal base evidence is the same query
// the append decision uses; the lifecycle bits and the retention rule come from
// one extra read of the same organization policy row.
func (r *Repository) ResolvePersonalThreadReviseDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (PersonalThreadDecisionEvidence, error) {
	evidence, err := r.resolvePersonalDecisionEvidence(ctx, spaceRef, orgID, subjectID)
	if err != nil {
		return PersonalThreadDecisionEvidence{}, err
	}
	err = r.db.Pool.QueryRow(ctx, `
		SELECT p.message_revise_entitled, p.message_moderate_entitled,
		       COALESCE(mp.original_retention, 'purge')
		FROM space_effect_policies p
		LEFT JOIN space_message_policies mp ON mp.org_id=p.org_id
		WHERE p.org_id=$1`, strings.TrimSpace(orgID),
	).Scan(&evidence.MessageReviseEntitled, &evidence.MessageModerateEntitled, &evidence.OriginalRetention)
	if errors.Is(err, pgx.ErrNoRows) {
		return PersonalThreadDecisionEvidence{}, ErrNoCurrentMembership
	}
	if err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("resolve personal Space message revise authority: %w", err)
	}
	evidence.ResourceAuthorizationRef = fmt.Sprintf("control:%s:thread-revise:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if err := evidence.ValidateForPersonalThreadRevise(); err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("invalid personal Space message revise authority: %w", err)
	}
	return evidence, nil
}

// ResolveSharedThreadReviseDecisionEvidence resolves the authority to revise a
// message in a shared Space (room, project, case, DM). It reuses the
// current-membership and current-recipient-audience joins of the shared thread
// resolvers (a removed participant stops resolving), reads the Space only while
// it is active (an archived room cannot be edited), and binds the independent
// message_revise_entitled bit. It must never fall back to thread_create.
func (r *Repository) ResolveSharedThreadReviseDecisionEvidence(ctx context.Context, spaceRef, orgID, subjectID string) (PersonalThreadDecisionEvidence, error) {
	if r == nil || r.db == nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("space authority repository unavailable")
	}
	var evidence PersonalThreadDecisionEvidence
	dest := append(sharedDecisionEvidenceScanDestinations(&evidence, &evidence.MessageReviseEntitled),
		&evidence.MessageModerateEntitled, &evidence.OriginalRetention)
	err := r.db.Pool.QueryRow(ctx, `
		SELECT s.space_ref, s.org_id, m.subject_id, s.space_kind, m.role,
		       r.authority_revision, r.membership_revision, r.privacy_revision,
		       r.recipient_audience_revision, r.entitlement_revision,
		       a.audience_ref, a.audience_hash,
		       p.privacy_policy_ref, p.purpose, p.lawful_basis, p.privacy_class,
		       p.third_party_processing_allowed, p.retention_class, p.residency,
		       p.deletion_scope, p.zero_data_retention, p.message_revise_entitled,
		       p.message_moderate_entitled, COALESCE(mp.original_retention, 'purge')
		FROM registered_spaces s
		JOIN space_memberships m ON m.space_ref=s.space_ref
		JOIN space_authority_revisions r ON r.space_ref=s.space_ref
		JOIN space_recipient_audiences a ON a.space_ref=s.space_ref AND a.revision=r.recipient_audience_revision
		JOIN space_recipient_audience_members am ON am.space_ref=a.space_ref AND am.revision=a.revision AND am.subject_id=m.subject_id
		JOIN space_effect_policies p ON p.org_id=s.org_id
		LEFT JOIN space_message_policies mp ON mp.org_id=s.org_id
		WHERE s.space_ref=$1 AND s.org_id=$2 AND s.registration_state='active'
		  AND s.space_kind <> 'personal'
		  AND m.subject_type='user' AND m.subject_id=$3 AND m.active=TRUE
		  AND EXISTS (SELECT 1 FROM user_org_memberships u WHERE u.user_id=$3 AND u.org_id=$2 AND u.status='active')`,
		strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), strings.TrimSpace(subjectID),
	).Scan(dest...)
	if errors.Is(err, pgx.ErrNoRows) {
		return PersonalThreadDecisionEvidence{}, ErrNoCurrentMembership
	}
	if err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("resolve shared Space message revise authority: %w", err)
	}
	evidence.ResourceAuthorizationRef = fmt.Sprintf("control:%s:thread-revise:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if err := evidence.ValidateForSharedThreadRevise(); err != nil {
		return PersonalThreadDecisionEvidence{}, fmt.Errorf("invalid shared Space message revise authority: %w", err)
	}
	return evidence, nil
}
