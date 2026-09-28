package users

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"strings"
	"time"
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
// Subjects (mirror auth-core's publishVerevonAudit + the cross-plane contract):
//   - verevon.audit.v1.control.erasure       (durable audit)
//   - verevon.audit.v1.control.dsar_export    (durable audit)
//   - verevon.gdpr.erasure.requested          (cross-plane fan-out)
const (
	ErasureAuditSubject      = "verevon.audit.v2.control.user-core.erasure"
	DSARExportAuditSubject   = "verevon.audit.v2.control.user-core.dsar_export"
	GDPRErasureFanoutSubject = "verevon.gdpr.erasure.requested"
)

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
	"Audit events for this subject are retained by audit-core (verevon.audit.v2.control.user-core.*).",
	"Model Plane run history / conversations and Data Plane documents are purged/exported via the verevon.gdpr.erasure.requested fan-out (follow-up subscribers).",
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
	if profile, profileErr := s.repo.GetProfile(ctx, userID); profileErr == nil && profile != nil {
		export.Profile["bio"] = profile.Bio
		export.Profile["phone"] = profile.Phone
		export.Profile["location"] = profile.Location
		export.Profile["timezone"] = profile.Timezone
		export.Profile["language"] = profile.Language
		export.Profile["metadata"] = profile.Metadata
	}

	memberships, membershipsErr := s.repo.ListUserOrgMembershipsForSubject(ctx, userID)
	if membershipsErr != nil {
		return nil, membershipsErr
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

// PublishDSARAudit emits a durable audit record for a DSAR export on the local
// control-plane bus (controlplane-nats). DSAR is a read, so it does NOT emit the
// erasure fan-out. Best-effort; no-ops when the local audit publisher is unset.
func (s *Service) PublishDSARAudit(ctx context.Context, orgID, subjectID, actorID, actorRole, outcome string) error {
	now := time.Now().UTC().Format(time.RFC3339Nano)
	eventID := fmt.Sprintf("dsar:user-core:%x", sha256.Sum256([]byte(subjectID+"\x00"+actorID+"\x00"+outcome+"\x00"+now)))
	payload := map[string]any{
		"event_id":    eventID,
		"occurred_at": now,
		"org_id":      orgID,
		"user_id":     actorID,
		"actor_role":  actorRole,
		"plane":       "control",
		"producer":    "user-core",
		"event":       "dsar_export",
		"subject":     "user:" + subjectID,
		"resource_id": subjectID,
		"outcome":     outcome,
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("encode DSAR audit event: %w", err)
	}
	err = s.auditOutbox.EnqueueAndDispatch(ctx, AuditOutboxRow{EventID: eventID, Subject: DSARExportAuditSubject, Payload: encoded})
	if deferred, ok := errors.AsType[*auditDispatchDeferredError](err); ok {
		log.Printf("user-core audit event %s retained for retry: %v", eventID, deferred)
		return nil
	}
	return err
}
