package spaces

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
)

// WS10-S2 storage of the Space approval ceiling and the standing rules. Every
// write is one transaction that also writes its append-only event row. Nothing
// here stores tool arguments or content.

var (
	// ErrApprovalScopeNotPermitted: the request asks for more than the Space
	// ceiling or the tool class allows.
	ErrApprovalScopeNotPermitted = errors.New("approval scope is not permitted")
	// ErrApprovalRuleNotFound: no such rule in this Space and organisation.
	ErrApprovalRuleNotFound = errors.New("approval rule not found")
	// ErrNoApplicableApprovalRule: no live rule covers this call.
	ErrNoApplicableApprovalRule = errors.New("no applicable approval rule")
	// ErrApprovalPolicyConflict: the expected_revision no longer matches.
	ErrApprovalPolicyConflict = errors.New("approval policy revision conflict")
	// ErrApprovalAgentNotInSpace: the named agent is not an active service
	// member of the Space.
	ErrApprovalAgentNotInSpace = errors.New("agent is not an active member of the Space")
	// ErrApprovalIdempotencyReused: the key was used for a different rule.
	ErrApprovalIdempotencyReused = errors.New("approval idempotency key reused")
)

// Event kinds of space_approval_grant_events.
const (
	approvalEventCreated   = "created"
	approvalEventUsed      = "used"
	approvalEventRevoked   = "revoked"
	approvalEventExpired   = "expired"
	approvalEventUseDenied = "use_denied"
)

const approvalRuleColumns = `rule_id, org_id, space_ref, scope, agent_subject_id, tool_key, action_key,
	action_schema_hash, source_approval_id, granted_by_subject_id, granted_by_role, authority_revision,
	created_at, review_by, revoked_at, COALESCE(revoked_by, ''), COALESCE(revoke_reason, ''),
	use_count, last_used_at`

func scanApprovalRule(row pgx.Row) (ApprovalRule, error) {
	var rule ApprovalRule
	err := row.Scan(&rule.RuleID, &rule.OrgID, &rule.SpaceRef, &rule.Scope, &rule.AgentSubjectID,
		&rule.ToolKey, &rule.ActionKey, &rule.ActionSchemaHash, &rule.SourceApprovalID,
		&rule.GrantedBySubjectID, &rule.GrantedByRole, &rule.AuthorityRevision, &rule.CreatedAt,
		&rule.ReviewBy, &rule.RevokedAt, &rule.RevokedBy, &rule.RevokeReason, &rule.UseCount, &rule.LastUsedAt)
	return rule, err
}

func insertApprovalEvent(ctx context.Context, tx pgx.Tx, ruleID, orgID, spaceRef, kind, actor, runID, reason string) error {
	if _, err := tx.Exec(ctx, `
		INSERT INTO space_approval_grant_events (rule_id, org_id, space_ref, kind, actor_subject_id, run_id, reason_code)
		VALUES ($1,$2,$3,$4,$5,$6,$7)`, ruleID, orgID, spaceRef, kind, actor, runID, reason); err != nil {
		return fmt.Errorf("record approval rule event: %w", err)
	}
	return nil
}

// revokeApprovalRulesForAgent revokes every live rule of an agent inside the
// caller's transaction (an agent leaving a room takes its rules with it).
func revokeApprovalRulesForAgent(ctx context.Context, tx pgx.Tx, spaceRef, agentSubjectID, reason string) error {
	if _, err := tx.Exec(ctx, `
		WITH revoked AS (
			UPDATE space_approval_grants
			SET revoked_at=NOW(), revoked_by='system', revoke_reason=$3
			WHERE space_ref=$1 AND agent_subject_id=$2 AND revoked_at IS NULL
			RETURNING rule_id, org_id, space_ref
		)
		INSERT INTO space_approval_grant_events (rule_id, org_id, space_ref, kind, actor_subject_id, reason_code)
		SELECT rule_id, org_id, space_ref, 'revoked', 'system', $3 FROM revoked`,
		spaceRef, agentSubjectID, reason); err != nil {
		return fmt.Errorf("revoke approval rules of removed agent: %w", err)
	}
	return nil
}

// GetApprovalPolicy returns the Space's stored ceiling, or the most restrictive
// default (revision 0) when none is stored.
func (r *Repository) GetApprovalPolicy(ctx context.Context, spaceRef, orgID string) (ApprovalPolicy, error) {
	if r == nil || r.db == nil {
		return ApprovalPolicy{}, fmt.Errorf("space authority repository unavailable")
	}
	spaceRef, orgID = strings.TrimSpace(spaceRef), strings.TrimSpace(orgID)
	policy := ApprovalPolicy{SpaceRef: spaceRef, OrgID: orgID}
	err := r.db.Pool.QueryRow(ctx, `
		SELECT max_grant_scope, default_addressee_kind, quarantine_mode, revision, updated_by
		FROM space_approval_policies WHERE space_ref=$1 AND org_id=$2`, spaceRef, orgID,
	).Scan(&policy.MaxGrantScope, &policy.DefaultAddresseeKind, &policy.QuarantineMode, &policy.Revision, &policy.UpdatedBy)
	if errors.Is(err, pgx.ErrNoRows) {
		return DefaultApprovalPolicy(spaceRef, orgID), nil
	}
	if err != nil {
		return ApprovalPolicy{}, fmt.Errorf("read Space approval policy: %w", err)
	}
	return policy, nil
}

// UpsertApprovalPolicy writes the ceiling with a revision compare-and-set. The
// caller has already proven manager/owner authority (and, for a loosening, the
// entitlement and non-ZDR posture).
func (r *Repository) UpsertApprovalPolicy(ctx context.Context, spaceRef, orgID, actorID string, update ApprovalPolicyUpdate) (ApprovalPolicy, error) {
	if r == nil || r.db == nil {
		return ApprovalPolicy{}, fmt.Errorf("space authority repository unavailable")
	}
	if err := update.Validate(); err != nil {
		return ApprovalPolicy{}, err
	}
	spaceRef, orgID, actorID = strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), strings.TrimSpace(actorID)
	var policy ApprovalPolicy
	var err error
	if update.ExpectedRevision == 0 {
		err = r.db.Pool.QueryRow(ctx, `
			INSERT INTO space_approval_policies (space_ref, org_id, max_grant_scope, default_addressee_kind, quarantine_mode, updated_by)
			SELECT s.space_ref, s.org_id, $3, $4, $5, $6 FROM registered_spaces s WHERE s.space_ref=$1 AND s.org_id=$2
			ON CONFLICT (space_ref) DO NOTHING
			RETURNING space_ref, org_id, max_grant_scope, default_addressee_kind, quarantine_mode, revision, updated_by`,
			spaceRef, orgID, update.MaxGrantScope, update.DefaultAddresseeKind, update.QuarantineMode, actorID,
		).Scan(&policy.SpaceRef, &policy.OrgID, &policy.MaxGrantScope, &policy.DefaultAddresseeKind, &policy.QuarantineMode, &policy.Revision, &policy.UpdatedBy)
	} else {
		err = r.db.Pool.QueryRow(ctx, `
			UPDATE space_approval_policies
			SET max_grant_scope=$4, default_addressee_kind=$5, quarantine_mode=$6, revision=revision+1,
			    updated_by=$7, updated_at=NOW()
			WHERE space_ref=$1 AND org_id=$2 AND revision=$3
			RETURNING space_ref, org_id, max_grant_scope, default_addressee_kind, quarantine_mode, revision, updated_by`,
			spaceRef, orgID, update.ExpectedRevision, update.MaxGrantScope, update.DefaultAddresseeKind, update.QuarantineMode, actorID,
		).Scan(&policy.SpaceRef, &policy.OrgID, &policy.MaxGrantScope, &policy.DefaultAddresseeKind, &policy.QuarantineMode, &policy.Revision, &policy.UpdatedBy)
	}
	if errors.Is(err, pgx.ErrNoRows) {
		return ApprovalPolicy{}, ErrApprovalPolicyConflict
	}
	if err != nil {
		return ApprovalPolicy{}, fmt.Errorf("write Space approval policy: %w", err)
	}
	return policy, nil
}

// CreateApprovalRule records one standing rule. evidence is the manager's
// freshly resolved authority (ValidateForApprovalRuleManage already passed).
// It is idempotent on (org, idempotency_key): a replay returns the stored rule.
// A live rule for the same agent action with another schema hash is superseded
// in the same transaction.
func (r *Repository) CreateApprovalRule(ctx context.Context, evidence PersonalThreadDecisionEvidence, request ApprovalRuleRequest, now time.Time) (ApprovalRule, bool, error) {
	if r == nil || r.db == nil {
		return ApprovalRule{}, false, fmt.Errorf("space authority repository unavailable")
	}
	if err := evidence.ValidateForApprovalRuleManage(); err != nil {
		return ApprovalRule{}, false, err
	}
	if err := request.Validate(); err != nil {
		return ApprovalRule{}, false, err
	}
	if strings.TrimSpace(request.SpaceRef) != evidence.Membership.SpaceRef {
		return ApprovalRule{}, false, fmt.Errorf("approval rule Space does not match the resolved authority")
	}
	orgID := evidence.Membership.OrgID
	spaceRef := evidence.Membership.SpaceRef
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return ApprovalRule{}, false, fmt.Errorf("begin approval rule creation: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	// Serialise writers of this Space's rules and read the ceiling inside the lock.
	if _, err := tx.Exec(ctx, `SELECT 1 FROM space_authority_revisions WHERE space_ref=$1 FOR UPDATE`, spaceRef); err != nil {
		return ApprovalRule{}, false, fmt.Errorf("lock Space for approval rule: %w", err)
	}
	if existing, err := scanApprovalRule(tx.QueryRow(ctx,
		`SELECT `+approvalRuleColumns+` FROM space_approval_grants WHERE org_id=$1 AND idempotency_key=$2`,
		orgID, strings.TrimSpace(request.IdempotencyKey))); err == nil {
		if existing.SpaceRef != spaceRef || existing.AgentSubjectID != strings.TrimSpace(request.AgentSubjectID) ||
			existing.ActionKey != strings.TrimSpace(request.ActionKey) || existing.ActionSchemaHash != strings.TrimSpace(request.ActionSchemaHash) {
			return ApprovalRule{}, false, ErrApprovalIdempotencyReused
		}
		return existing, false, nil
	} else if !errors.Is(err, pgx.ErrNoRows) {
		return ApprovalRule{}, false, fmt.Errorf("read approval rule by idempotency key: %w", err)
	}

	var ceiling string
	err = tx.QueryRow(ctx, `SELECT max_grant_scope FROM space_approval_policies WHERE space_ref=$1`, spaceRef).Scan(&ceiling)
	if errors.Is(err, pgx.ErrNoRows) {
		ceiling = string(ApprovalScopeOnce)
	} else if err != nil {
		return ApprovalRule{}, false, fmt.Errorf("read Space approval ceiling: %w", err)
	}
	if !ApprovalScopeAlways.Within(ApprovalGrantScope(ceiling)) {
		return ApprovalRule{}, false, ErrApprovalScopeNotPermitted
	}

	var agentActive bool
	if err := tx.QueryRow(ctx, `
		SELECT EXISTS (SELECT 1 FROM space_memberships
		WHERE space_ref=$1 AND subject_type='service' AND subject_id=$2 AND active=TRUE)`,
		spaceRef, strings.TrimSpace(request.AgentSubjectID)).Scan(&agentActive); err != nil {
		return ApprovalRule{}, false, fmt.Errorf("check agent membership: %w", err)
	}
	if !agentActive {
		return ApprovalRule{}, false, ErrApprovalAgentNotInSpace
	}

	// A changed schema hash replaces the old rule; an identical live rule is returned as is.
	if live, err := scanApprovalRule(tx.QueryRow(ctx, `
		SELECT `+approvalRuleColumns+` FROM space_approval_grants
		WHERE space_ref=$1 AND agent_subject_id=$2 AND action_key=$3 AND revoked_at IS NULL FOR UPDATE`,
		spaceRef, strings.TrimSpace(request.AgentSubjectID), strings.TrimSpace(request.ActionKey))); err == nil {
		if live.ActionSchemaHash == strings.TrimSpace(request.ActionSchemaHash) && now.Before(live.ReviewBy) {
			return live, false, nil
		}
		if _, err := tx.Exec(ctx, `
			UPDATE space_approval_grants SET revoked_at=$2, revoked_by=$3, revoke_reason='superseded'
			WHERE rule_id=$1`, live.RuleID, now.UTC(), evidence.Membership.SubjectID); err != nil {
			return ApprovalRule{}, false, fmt.Errorf("supersede approval rule: %w", err)
		}
		if err := insertApprovalEvent(ctx, tx, live.RuleID, orgID, spaceRef, approvalEventRevoked, evidence.Membership.SubjectID, "", "superseded"); err != nil {
			return ApprovalRule{}, false, err
		}
	} else if !errors.Is(err, pgx.ErrNoRows) {
		return ApprovalRule{}, false, fmt.Errorf("read live approval rule: %w", err)
	}

	ruleID := "apr_" + uuid.NewString()
	rule, err := scanApprovalRule(tx.QueryRow(ctx, `
		INSERT INTO space_approval_grants (rule_id, org_id, space_ref, scope, agent_subject_id, tool_key, action_key,
			action_schema_hash, source_approval_id, granted_by_subject_id, granted_by_role, authority_revision,
			idempotency_key, created_at, review_by)
		VALUES ($1,$2,$3,'always',$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
		RETURNING `+approvalRuleColumns,
		ruleID, orgID, spaceRef, strings.TrimSpace(request.AgentSubjectID), strings.TrimSpace(request.ToolKey),
		strings.TrimSpace(request.ActionKey), strings.TrimSpace(request.ActionSchemaHash),
		strings.TrimSpace(request.SourceApprovalID), evidence.Membership.SubjectID, evidence.Membership.Role,
		evidence.Membership.Revisions.Authority, strings.TrimSpace(request.IdempotencyKey),
		now.UTC(), now.UTC().Add(ApprovalRuleReviewWindow)))
	if err != nil {
		return ApprovalRule{}, false, fmt.Errorf("insert approval rule: %w", err)
	}
	if err := insertApprovalEvent(ctx, tx, rule.RuleID, orgID, spaceRef, approvalEventCreated, evidence.Membership.SubjectID, "", ""); err != nil {
		return ApprovalRule{}, false, err
	}
	if err := tx.Commit(ctx); err != nil {
		return ApprovalRule{}, false, fmt.Errorf("commit approval rule creation: %w", err)
	}
	return rule, true, nil
}

// ListApprovalRules lists a Space's rules, live ones first. Revoked rules are
// included only when asked for.
func (r *Repository) ListApprovalRules(ctx context.Context, spaceRef, orgID string, includeRevoked bool) ([]ApprovalRule, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("space authority repository unavailable")
	}
	rows, err := r.db.Pool.Query(ctx, `
		SELECT `+approvalRuleColumns+` FROM space_approval_grants
		WHERE space_ref=$1 AND org_id=$2 AND ($3 OR revoked_at IS NULL)
		ORDER BY (revoked_at IS NOT NULL), created_at DESC LIMIT 500`,
		strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), includeRevoked)
	if err != nil {
		return nil, fmt.Errorf("list approval rules: %w", err)
	}
	defer rows.Close()
	out := []ApprovalRule{}
	for rows.Next() {
		rule, err := scanApprovalRule(rows)
		if err != nil {
			return nil, fmt.Errorf("scan approval rule: %w", err)
		}
		out = append(out, rule)
	}
	return out, rows.Err()
}

// RevokeApprovalRule revokes one rule. It is idempotent: revoking a revoked
// rule returns it unchanged. The caller has proven manager/owner (or creator).
func (r *Repository) RevokeApprovalRule(ctx context.Context, spaceRef, orgID, ruleID, actorID, reason string, now time.Time) (ApprovalRule, error) {
	if r == nil || r.db == nil {
		return ApprovalRule{}, fmt.Errorf("space authority repository unavailable")
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return ApprovalRule{}, fmt.Errorf("begin approval rule revoke: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	rule, err := scanApprovalRule(tx.QueryRow(ctx, `
		SELECT `+approvalRuleColumns+` FROM space_approval_grants
		WHERE rule_id=$1 AND space_ref=$2 AND org_id=$3 FOR UPDATE`,
		strings.TrimSpace(ruleID), strings.TrimSpace(spaceRef), strings.TrimSpace(orgID)))
	if errors.Is(err, pgx.ErrNoRows) {
		return ApprovalRule{}, ErrApprovalRuleNotFound
	}
	if err != nil {
		return ApprovalRule{}, fmt.Errorf("read approval rule: %w", err)
	}
	if rule.RevokedAt != nil {
		return rule, nil
	}
	if strings.TrimSpace(reason) == "" {
		reason = "revoked"
	}
	if _, err := tx.Exec(ctx, `
		UPDATE space_approval_grants SET revoked_at=$2, revoked_by=$3, revoke_reason=$4 WHERE rule_id=$1`,
		rule.RuleID, now.UTC(), actorID, reason); err != nil {
		return ApprovalRule{}, fmt.Errorf("revoke approval rule: %w", err)
	}
	if err := insertApprovalEvent(ctx, tx, rule.RuleID, rule.OrgID, rule.SpaceRef, approvalEventRevoked, actorID, "", reason); err != nil {
		return ApprovalRule{}, err
	}
	if err := tx.Commit(ctx); err != nil {
		return ApprovalRule{}, fmt.Errorf("commit approval rule revoke: %w", err)
	}
	revoked := now.UTC()
	rule.RevokedAt, rule.RevokedBy, rule.RevokeReason = &revoked, actorID, reason
	return rule, nil
}

// UseApprovalRule finds the live rule that covers one call and records one
// `used` event. The Space ceiling must currently be `always`; the rule must be
// unrevoked, inside its review window and bound to the exact schema hash. Any
// other outcome is ErrNoApplicableApprovalRule (a rule that exists but does not
// match also leaves a `use_denied` event with an allowlisted reason). evidence is
// the requester's fresh authority (ValidateForApprovalRuleUse already passed).
func (r *Repository) UseApprovalRule(ctx context.Context, evidence PersonalThreadDecisionEvidence, request ApprovalRuleUseRequest, now time.Time) (ApprovalRule, error) {
	if r == nil || r.db == nil {
		return ApprovalRule{}, fmt.Errorf("space authority repository unavailable")
	}
	if err := evidence.ValidateForApprovalRuleUse(); err != nil {
		return ApprovalRule{}, err
	}
	if err := request.Validate(); err != nil {
		return ApprovalRule{}, err
	}
	if !StandingRuleEligibleAction(request.ToolKey, request.ActionKey) {
		return ApprovalRule{}, ErrNoApplicableApprovalRule
	}
	orgID, spaceRef := evidence.Membership.OrgID, evidence.Membership.SpaceRef
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return ApprovalRule{}, fmt.Errorf("begin approval rule use: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	var ceiling string
	err = tx.QueryRow(ctx, `SELECT max_grant_scope FROM space_approval_policies WHERE space_ref=$1`, spaceRef).Scan(&ceiling)
	if errors.Is(err, pgx.ErrNoRows) {
		ceiling = string(ApprovalScopeOnce)
	} else if err != nil {
		return ApprovalRule{}, fmt.Errorf("read Space approval ceiling: %w", err)
	}
	if !ApprovalScopeAlways.Within(ApprovalGrantScope(ceiling)) {
		return ApprovalRule{}, ErrNoApplicableApprovalRule
	}
	rule, err := scanApprovalRule(tx.QueryRow(ctx, `
		SELECT `+approvalRuleColumns+` FROM space_approval_grants
		WHERE space_ref=$1 AND org_id=$2 AND agent_subject_id=$3 AND action_key=$4 AND revoked_at IS NULL FOR UPDATE`,
		spaceRef, orgID, strings.TrimSpace(request.AgentSubjectID), strings.TrimSpace(request.ActionKey)))
	if errors.Is(err, pgx.ErrNoRows) {
		return ApprovalRule{}, ErrNoApplicableApprovalRule
	}
	if err != nil {
		return ApprovalRule{}, fmt.Errorf("read approval rule for use: %w", err)
	}
	deny := func(reason string) (ApprovalRule, error) {
		if err := insertApprovalEvent(ctx, tx, rule.RuleID, orgID, spaceRef, approvalEventUseDenied, evidence.Membership.SubjectID, request.RunID, reason); err != nil {
			return ApprovalRule{}, err
		}
		if err := tx.Commit(ctx); err != nil {
			return ApprovalRule{}, fmt.Errorf("commit approval rule denial: %w", err)
		}
		return ApprovalRule{}, ErrNoApplicableApprovalRule
	}
	if !now.Before(rule.ReviewBy) {
		if _, err := tx.Exec(ctx, `
			INSERT INTO space_approval_grant_events (rule_id, org_id, space_ref, kind, reason_code)
			SELECT $1,$2,$3,'expired','review_by' WHERE NOT EXISTS (
				SELECT 1 FROM space_approval_grant_events WHERE rule_id=$1 AND kind='expired')`,
			rule.RuleID, orgID, spaceRef); err != nil {
			return ApprovalRule{}, fmt.Errorf("record approval rule expiry: %w", err)
		}
		return deny("review_expired")
	}
	if rule.ActionSchemaHash != strings.TrimSpace(request.SchemaHash) || rule.ToolKey != strings.TrimSpace(request.ToolKey) {
		return deny("schema_changed")
	}
	if err := tx.QueryRow(ctx, `
		UPDATE space_approval_grants SET use_count=use_count+1, last_used_at=$2 WHERE rule_id=$1
		RETURNING use_count, last_used_at`, rule.RuleID, now.UTC()).Scan(&rule.UseCount, &rule.LastUsedAt); err != nil {
		return ApprovalRule{}, fmt.Errorf("count approval rule use: %w", err)
	}
	if err := insertApprovalEvent(ctx, tx, rule.RuleID, orgID, spaceRef, approvalEventUsed, evidence.Membership.SubjectID, request.RunID, ""); err != nil {
		return ApprovalRule{}, err
	}
	if err := tx.Commit(ctx); err != nil {
		return ApprovalRule{}, fmt.Errorf("commit approval rule use: %w", err)
	}
	return rule, nil
}

// GetApprovalRule reads one rule of a Space, revoked or not.
func (r *Repository) GetApprovalRule(ctx context.Context, spaceRef, orgID, ruleID string) (ApprovalRule, error) {
	if r == nil || r.db == nil {
		return ApprovalRule{}, fmt.Errorf("space authority repository unavailable")
	}
	rule, err := scanApprovalRule(r.db.Pool.QueryRow(ctx, `
		SELECT `+approvalRuleColumns+` FROM space_approval_grants
		WHERE rule_id=$1 AND space_ref=$2 AND org_id=$3`,
		strings.TrimSpace(ruleID), strings.TrimSpace(spaceRef), strings.TrimSpace(orgID)))
	if errors.Is(err, pgx.ErrNoRows) {
		return ApprovalRule{}, ErrApprovalRuleNotFound
	}
	if err != nil {
		return ApprovalRule{}, fmt.Errorf("read approval rule: %w", err)
	}
	return rule, nil
}
