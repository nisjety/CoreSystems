package users

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// GDPR erasure + DSAR for users.
//
// The gdpr_hard_delete_user / gdpr_anonymize_user stored procedures live in the
// auth-core migrations and run against the auth_service database (tables
// "user", session, account, member, ...). user-core's primary pool connects to
// user_service, so erasure needs a SECOND pool to the auth DB
// (AUTH_DATABASE_URL). When that pool is not configured the hard/anonymize
// operations fail loudly rather than silently skipping auth-side data.
//
// Subjects (mirror auth-core's publishVelionAudit + the cross-plane contract):
//   - velion.audit.v1.control.erasure       (durable audit)
//   - velion.audit.v1.control.dsar_export    (durable audit)
//   - velion.gdpr.erasure.requested          (cross-plane fan-out)
const (
	ErasureAuditSubject      = "velion.audit.v1.control.erasure"
	DSARExportAuditSubject   = "velion.audit.v1.control.dsar_export"
	GDPRErasureFanoutSubject = "velion.gdpr.erasure.requested"
)

// SetAuthPool wires the secondary pgx pool used to invoke the auth-DB GDPR
// stored procedures. Pass nil to leave it unconfigured (hard erasure / anonymize
// will then return an error explaining AUTH_DATABASE_URL is required).
func (s *Service) SetAuthPool(pool *pgxpool.Pool) {
	s.authPool = pool
}

// ErasureAvailable reports whether the auth-DB pool is wired, i.e. whether
// hard-erase / anonymize can actually run. When false (AUTH_DATABASE_URL unset),
// callers should refuse the erasure routes with an explicit 503 rather than
// attempting the operation and surfacing an opaque 500.
func (s *Service) ErasureAvailable() bool {
	return s.authPool != nil
}

// ErasureReceipt aggregates the auth-DB proc receipt and the local cleanup.
type ErasureReceipt struct {
	Success      bool            `json:"success"`
	UserID       string          `json:"user_id"`
	Mode         string          `json:"mode"` // "hard_delete" | "anonymize"
	AuthDB       json.RawMessage `json:"auth_db_receipt,omitempty"`
	LocalDeleted bool            `json:"local_user_deleted"`
	ErasedAt     time.Time       `json:"erased_at"`
}

// HardEraseUser performs an irreversible GDPR erasure of a user:
//  1. invokes gdpr_hard_delete_user($1) on the auth_service DB (cascades
//     session/account/two_factor/passkey/apikey/member/... and the "user" row);
//  2. hard-deletes the user's local rows in user_service (memberships + users).
//
// Both calls are parameterized. Returns the combined receipt.
func (s *Service) HardEraseUser(ctx context.Context, userID string) (*ErasureReceipt, error) {
	if strings.TrimSpace(userID) == "" {
		return nil, fmt.Errorf("user ID is required")
	}
	if s.authPool == nil {
		return nil, fmt.Errorf("auth database not configured (AUTH_DATABASE_URL); cannot run gdpr_hard_delete_user")
	}

	var authReceipt []byte
	if err := s.authPool.QueryRow(ctx, `SELECT gdpr_hard_delete_user($1)`, userID).Scan(&authReceipt); err != nil {
		return nil, fmt.Errorf("gdpr_hard_delete_user: %w", err)
	}

	// Local cleanup in user_service. Memberships first (FK-free but explicit),
	// then the canonical users row.
	if err := s.repo.DeleteUserOrgMemberships(ctx, userID); err != nil {
		return nil, err
	}
	localDeleted := true
	if err := s.repo.Delete(ctx, userID); err != nil {
		// A missing local row is acceptable (auth-side erasure still happened).
		if !strings.Contains(strings.ToLower(err.Error()), "not found") {
			return nil, err
		}
		localDeleted = false
	}

	if s.cache != nil {
		_ = s.cache.Del(ctx, userIDKeyPrefix+userID)
	}

	return &ErasureReceipt{
		Success:      true,
		UserID:       userID,
		Mode:         "hard_delete",
		AuthDB:       json.RawMessage(authReceipt),
		LocalDeleted: localDeleted,
		ErasedAt:     time.Now().UTC(),
	}, nil
}

// AnonymizeUser performs the softer GDPR variant: it invokes
// gdpr_anonymize_user($1) on the auth_service DB (scrubs PII + bans the
// account, drops sessions/accounts/2fa/passkeys/apikeys) and leaves the local
// user_service row in place (the user id survives for referential integrity).
func (s *Service) AnonymizeUser(ctx context.Context, userID string) (*ErasureReceipt, error) {
	if strings.TrimSpace(userID) == "" {
		return nil, fmt.Errorf("user ID is required")
	}
	if s.authPool == nil {
		return nil, fmt.Errorf("auth database not configured (AUTH_DATABASE_URL); cannot run gdpr_anonymize_user")
	}

	var authReceipt []byte
	if err := s.authPool.QueryRow(ctx, `SELECT gdpr_anonymize_user($1)`, userID).Scan(&authReceipt); err != nil {
		return nil, fmt.Errorf("gdpr_anonymize_user: %w", err)
	}

	if s.cache != nil {
		_ = s.cache.Del(ctx, userIDKeyPrefix+userID)
	}

	return &ErasureReceipt{
		Success:      true,
		UserID:       userID,
		Mode:         "anonymize",
		AuthDB:       json.RawMessage(authReceipt),
		LocalDeleted: false,
		ErasedAt:     time.Now().UTC(),
	}, nil
}

// DSARExport assembles a GDPR Art. 15 data-subject export from the Control
// Plane data reachable from user-core: the user profile + org memberships.
// audit_events for the subject join via the cross-plane fan-out and Model-Plane
// run-history/conversations are NOT assembled here (documented follow-up).
type DSARExport struct {
	Subject     string           `json:"subject"`
	SubjectID   string           `json:"subject_id"`
	GeneratedAt time.Time        `json:"generated_at"`
	Profile     map[string]any   `json:"profile"`
	Memberships []map[string]any `json:"org_memberships"`
	APIKeys     []map[string]any `json:"api_keys"`
	Notes       []string         `json:"notes"`
}

// DSARControlPlaneDisclosure is the verbatim Art. 15 scope notice attached to
// every DSAR export. It states plainly that the export covers Control-Plane data
// ONLY — Model-Plane run history/conversations and Data-Plane documents are
// handled via the cross-plane erasure fan-out, not assembled here. This honesty
// boundary (never imply "exported/erased everywhere") is pinned verbatim by a
// test; do not weaken it without updating that test.
var DSARControlPlaneDisclosure = []string{
	"Control Plane export: profile + org memberships + API key metadata.",
	"Audit events for this subject are retained by audit-core (velion.audit.v1.control.*).",
	"Model Plane run history / conversations and Data Plane documents are purged/exported via the velion.gdpr.erasure.requested fan-out (follow-up subscribers).",
}

// BuildDSARExport gathers the data Control Plane owns for a subject. It never
// includes secrets (password hashes, raw API key material) — only metadata.
func (s *Service) BuildDSARExport(ctx context.Context, userID string) (*DSARExport, error) {
	if strings.TrimSpace(userID) == "" {
		return nil, fmt.Errorf("user ID is required")
	}

	user, err := s.repo.GetByID(ctx, userID)
	if err != nil {
		return nil, err
	}

	export := &DSARExport{
		Subject:     "user",
		SubjectID:   userID,
		GeneratedAt: time.Now().UTC(),
		Profile: map[string]any{
			"id":                  user.ID,
			"email":               user.Email,
			"name":                user.Name,
			"avatar":              user.Avatar,
			"status":              string(user.Status),
			"email_verified":      user.EmailVerified,
			"onboarding_complete": user.OnboardingComplete,
			"created_at":          user.CreatedAt,
			"updated_at":          user.UpdatedAt,
			"last_login_at":       user.LastLoginAt,
		},
		Memberships: []map[string]any{},
		APIKeys:     []map[string]any{},
		// Copy the pinned disclosure so a caller mutating export.Notes can never
		// alter the shared package-level contract.
		Notes: append([]string(nil), DSARControlPlaneDisclosure...),
	}

	// Extended profile (bio/phone/location/...), best-effort.
	if profile, perr := s.repo.GetProfile(ctx, userID); perr == nil && profile != nil {
		export.Profile["bio"] = profile.Bio
		export.Profile["phone"] = profile.Phone
		export.Profile["location"] = profile.Location
		export.Profile["timezone"] = profile.Timezone
		export.Profile["language"] = profile.Language
		export.Profile["metadata"] = profile.Metadata
	}

	memberships, merr := s.repo.ListUserOrgMembershipsForSubject(ctx, userID)
	if merr != nil {
		return nil, merr
	}
	for _, m := range memberships {
		export.Memberships = append(export.Memberships, map[string]any{
			"org_id":     m.OrgID,
			"role":       m.Role,
			"status":     m.Status,
			"created_at": m.CreatedAt,
			"updated_at": m.UpdatedAt,
		})
	}

	if keys, kerr := s.repo.ListAPIKeys(ctx, userID); kerr == nil {
		for _, k := range keys {
			export.APIKeys = append(export.APIKeys, map[string]any{
				"id":           k.ID,
				"name":         k.Name,
				"key_prefix":   k.KeyPrefix,
				"scopes":       k.Scopes,
				"created_at":   k.CreatedAt,
				"expires_at":   k.ExpiresAt,
				"revoked_at":   k.RevokedAt,
				"last_used_at": k.LastUsedAt,
			})
		}
	}

	return export, nil
}

// PublishErasureAudit emits a durable audit record on the local control-plane
// bus (controlplane-nats, where audit-core listens) plus the cross-plane erasure
// fan-out on the shared velion-nats bus. The two transports are independent: a
// disabled local audit publisher does not suppress the fan-out, and vice versa.
// Both are best-effort and silently no-op when their connection is unavailable.
func (s *Service) PublishErasureAudit(orgID, subjectType, subjectID, actorID, actorRole, outcome string, receipt any) {
	now := time.Now().UTC().Format(time.RFC3339Nano)

	var details map[string]any
	if receipt != nil {
		if b, err := json.Marshal(receipt); err == nil {
			_ = json.Unmarshal(b, &details)
		}
	}

	// Durable audit event → LOCAL control-plane bus via CORE publish, matching
	// audit-core's core QueueSubscribe on velion.audit.v1.>.
	if ap := s.auditPublisher; ap != nil {
		_ = ap.Publish(ErasureAuditSubject, map[string]any{
			"occurred_at": now,
			"org_id":      orgID,
			"user_id":     actorID,
			"actor_role":  actorRole,
			"plane":       "control",
			"event":       "erasure",
			"subject":     subjectType + ":" + subjectID,
			"resource_id": subjectID,
			"outcome":     outcome,
			"details":     details,
		})
	}

	// Cross-plane erasure fan-out → SHARED velion-nats bus (Model/Data plane
	// purge their side). Fires only on erasure success.
	if outcome == "ok" {
		if sp := s.sharedPublisher; sp != nil {
			sp.PublishPlain(GDPRErasureFanoutSubject, map[string]any{
				"subject_type": subjectType,
				"subject_id":   subjectID,
				"org_id":       orgID,
				"requested_by": actorID,
				"ts":           now,
			})
		}
	}
}

// PublishDSARAudit emits a durable audit record for a DSAR export on the local
// control-plane bus (controlplane-nats). DSAR is a read, so it does NOT emit the
// erasure fan-out. Best-effort; no-ops when the local audit publisher is unset.
func (s *Service) PublishDSARAudit(orgID, subjectID, actorID, actorRole, outcome string) {
	ap := s.auditPublisher
	if ap == nil {
		return
	}
	_ = ap.Publish(DSARExportAuditSubject, map[string]any{
		"occurred_at": time.Now().UTC().Format(time.RFC3339Nano),
		"org_id":      orgID,
		"user_id":     actorID,
		"actor_role":  actorRole,
		"plane":       "control",
		"event":       "dsar_export",
		"subject":     "user:" + subjectID,
		"resource_id": subjectID,
		"outcome":     outcome,
	})
}
