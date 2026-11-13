package api

import (
	"context"
	"errors"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/config"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/store"
)

// Credential references (W4-CC-4, ADR-NEXT-sandbox-credential-injection sections 2, 6 and 7).
//
// Two internal routes. Each admits one principal with that principal's own key from
// CREDENTIAL_REF_SEAL_KEYS, never the shared internal key:
//   - seal (model-gateway) turns an active binding into a reference for one call. The reference
//     holds no secret and releases nothing.
//   - redeem (execution-core) opens a reference for its host and call and returns the plaintext
//     secret and header name to that principal only. Nothing is cached. A redemption that names a
//     binding writes its connection.credential.redeemed row before the secret leaves, and a row
//     that cannot be written refuses the redemption with 503 credential_audit_failed.
//
// Both routes are mounted only with CREDENTIAL_BINDINGS_ENABLED. ZDR and waived evidence is refused
// at seal, so no reference, and no redemption row, exists for such a run.

const (
	credentialRefSealPath   = "/internal/credential-refs/seal"
	credentialRefRedeemPath = "/internal/credential-refs/redeem"

	auditEventCredentialRedeemed = "connection.credential.redeemed"

	// A reference lasts until the call deadline plus credentialRefDeadlineGrace, and no longer than
	// credentialRefMaxLifetime from the moment it is issued.
	credentialRefDeadlineGrace = 30 * time.Second
	credentialRefMaxLifetime   = 15 * time.Minute

	credentialRefMaxIDLength   = 128
	credentialRefMaxSlotLength = 32
	credentialRefMaxRefLength  = 8192
)

// Redemption outcomes and reason codes. They name why a reference was refused and never carry a
// value, a path or a header.
const (
	credentialRefOutcomeAllowed = "allowed"
	credentialRefOutcomeRefused = "refused"

	credentialRefReasonOK                    = "ok"
	credentialRefReasonZDROrWaived           = "zdr_or_waived"
	credentialRefReasonHostMismatch          = "host_mismatch"
	credentialRefReasonCallMismatch          = "call_mismatch"
	credentialRefReasonExpired               = "expired"
	credentialRefReasonBindingNotFound       = "binding_not_found"
	credentialRefReasonBindingUnavailable    = "binding_unavailable"
	credentialRefReasonBindingRevoked        = "binding_revoked"
	credentialRefReasonBindingHostMismatch   = "binding_host_mismatch"
	credentialRefReasonCredentialUnavailable = "credential_unavailable"
)

// registerCredentialRefRoutes mounts the seal and redeem routes. Nothing is mounted unless
// CREDENTIAL_BINDINGS_ENABLED is exactly "true" and a vault is present. Each route admits its own
// principal's key; a principal with no configured key gets 401 on its route.
func registerCredentialRefRoutes(app *fiber.App, cfg ServerConfig, rateLimited []fiber.Handler) {
	if !cfg.Config.CredentialBindingsEnabled || cfg.CredentialSealer == nil {
		return
	}
	refs, err := secretcrypto.NewCredentialRefs(cfg.Config.EncryptionKey)
	if err != nil {
		if cfg.Logger != nil {
			cfg.Logger.Error().Err(err).Msg("credential reference routes are not mounted")
		}
		return
	}
	keys := cfg.Config.CredentialRefPrincipalKeys()
	sealAuth := auth.InternalOnly(auth.Config{
		APIKey:       keys[config.CredentialRefPrincipalModelGateway],
		APIKeyHeader: cfg.Config.InternalAPIKeyHeader,
	})
	redeemAuth := auth.InternalOnly(auth.Config{
		APIKey:       keys[config.CredentialRefPrincipalExecutionCore],
		APIKeyHeader: cfg.Config.InternalAPIKeyHeader,
	})

	app.Post(credentialRefSealPath, chainHandlers(rateLimited, sealAuth, func(c *fiber.Ctx) error {
		return credentialRefSeal(c, cfg, refs)
	})...)
	app.Post(credentialRefRedeemPath, chainHandlers(rateLimited, redeemAuth, func(c *fiber.Ctx) error {
		return credentialRefRedeem(c, cfg, refs)
	})...)
}

// credentialRefSealBody is the model-gateway's request for one reference. ZDR and Waived are the
// signed decision's evidence. Both are required, so an absent flag is refused and never read as
// false.
type credentialRefSealBody struct {
	BindingID    string    `json:"binding_id"`
	OrgID        string    `json:"org_id"`
	ConnectionID string    `json:"connection_id"`
	CallID       string    `json:"call_id"`
	Slot         string    `json:"slot"`
	Host         string    `json:"host"`
	Deadline     time.Time `json:"deadline"`
	ZDR          *bool     `json:"zdr"`
	Waived       *bool     `json:"waived"`
}

// credentialRefSeal issues a reference for one active binding and one call. It refuses ZDR or
// waived evidence, an inactive binding and a host the binding does not name. The reference lasts
// until min(deadline + 30 s, now + 15 min). Sealing writes no row: it stores nothing and releases
// no secret.
func credentialRefSeal(c *fiber.Ctx, cfg ServerConfig, refs *secretcrypto.CredentialRefs) error {
	c.Set("Cache-Control", "no-store")
	if cfg.Repo == nil {
		return apiError(c, fiber.StatusServiceUnavailable, "repository_unavailable", "The binding store is unavailable.")
	}
	var body credentialRefSealBody
	if err := c.BodyParser(&body); err != nil {
		return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
	}
	bindingID := strings.TrimSpace(body.BindingID)
	orgID := strings.TrimSpace(body.OrgID)
	connectionID := strings.TrimSpace(body.ConnectionID)
	callID := strings.TrimSpace(body.CallID)
	slot := strings.TrimSpace(body.Slot)
	if !strings.HasPrefix(bindingID, credentialBindingIDPrefix) || !credentialRefTokenValid(bindingID) {
		return apiError(c, fiber.StatusBadRequest, "invalid_binding_id", "binding_id must name a credential binding.")
	}
	if !credentialRefTokenValid(orgID) {
		return apiError(c, fiber.StatusBadRequest, "invalid_org_id", "org_id is required and must be a plain identifier.")
	}
	if connectionID != "" && !credentialRefTokenValid(connectionID) {
		return apiError(c, fiber.StatusBadRequest, "invalid_connection_id", "connection_id must be a plain identifier when it is given.")
	}
	if !credentialRefTokenValid(callID) {
		return apiError(c, fiber.StatusBadRequest, "invalid_call_id", "call_id is required and must be a plain identifier.")
	}
	if !credentialRefSlotValid(slot) {
		return apiError(c, fiber.StatusBadRequest, "invalid_slot", "slot must be 1 to 32 letters, digits, underscores or hyphens.")
	}
	host, ok := normalizeCredentialBindingHost(body.Host)
	if !ok {
		return apiError(c, fiber.StatusBadRequest, "invalid_host", "host must be a lowercase DNS name on port 443, with no IP literal, wildcard, userinfo or path.")
	}
	if body.Deadline.IsZero() {
		return apiError(c, fiber.StatusBadRequest, "invalid_deadline", "deadline is required.")
	}
	if body.ZDR == nil || body.Waived == nil {
		return apiError(c, fiber.StatusBadRequest, "invalid_evidence", "The zdr and waived evidence flags are both required.")
	}
	// Refused before the binding is read, so a ZDR or waived run never reaches the store.
	if *body.ZDR || *body.Waived {
		return credentialRefRefused(c, "A credential reference is not issued for a zero-data-retention or waived run.", credentialRefReasonZDROrWaived)
	}
	now := time.Now().UTC()
	expiresAt := body.Deadline.UTC().Add(credentialRefDeadlineGrace)
	if latest := now.Add(credentialRefMaxLifetime); expiresAt.After(latest) {
		expiresAt = latest
	}
	if !expiresAt.After(now) {
		return apiError(c, fiber.StatusConflict, "call_deadline_passed", "The call deadline has passed, so no reference was issued.")
	}
	binding, err := cfg.Repo.GetCredentialBinding(c.UserContext(), orgID, bindingID)
	if err != nil {
		if errors.Is(err, store.ErrNotFound) {
			return apiError(c, fiber.StatusNotFound, "credential_binding_not_found", "The credential binding was not found.")
		}
		return apiError(c, fiber.StatusServiceUnavailable, "credential_binding_unavailable", "The credential binding could not be read right now.")
	}
	if binding.Status != store.CredentialBindingStatusActive {
		return apiError(c, fiber.StatusConflict, "credential_binding_inactive", "The credential binding is not active.")
	}
	if binding.Host != host {
		return credentialRefRefused(c, "The host is not the one this credential binding names.", credentialRefReasonHostMismatch)
	}
	ref, err := refs.Seal(bindingID, orgID, connectionID, callID, slot, host, expiresAt)
	if err != nil {
		return apiError(c, fiber.StatusServiceUnavailable, "credential_ref_unavailable", "The credential reference could not be issued safely.")
	}
	return success(c, fiber.Map{"ref": ref, "expires_at": expiresAt})
}

// credentialRefRedeemBody is the execution-core proxy's request to open one reference for one call.
type credentialRefRedeemBody struct {
	Ref    string `json:"ref"`
	Host   string `json:"host"`
	CallID string `json:"call_id"`
}

// credentialRefAudit names one redemption for its audit row: ids, the host and the call. It never
// holds the secret, the reference, a path, a query or a header value.
type credentialRefAudit struct {
	OrganizationID string
	ConnectionID   string
	BindingID      string
	CallID         string
	Host           string
}

// credentialRefRedeem releases one binding's secret to the execution-core proxy for one reference,
// host and call. The redemption row is written before the secret leaves. Nothing is cached.
func credentialRefRedeem(c *fiber.Ctx, cfg ServerConfig, refs *secretcrypto.CredentialRefs) error {
	c.Set("Cache-Control", "no-store")
	if cfg.Repo == nil || cfg.CredentialSealer == nil {
		return apiError(c, fiber.StatusServiceUnavailable, "repository_unavailable", "The binding store is unavailable.")
	}
	var body credentialRefRedeemBody
	if err := c.BodyParser(&body); err != nil {
		return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
	}
	ref := strings.TrimSpace(body.Ref)
	if ref == "" || len(ref) > credentialRefMaxRefLength {
		return apiError(c, fiber.StatusBadRequest, "invalid_ref", "ref is required.")
	}
	host, ok := normalizeCredentialBindingHost(body.Host)
	if !ok {
		return apiError(c, fiber.StatusBadRequest, "invalid_host", "host must be a lowercase DNS name on port 443, with no IP literal, wildcard, userinfo or path.")
	}
	callID := strings.TrimSpace(body.CallID)
	if !credentialRefTokenValid(callID) {
		return apiError(c, fiber.StatusBadRequest, "invalid_call_id", "call_id is required and must be a plain identifier.")
	}

	claims, err := refs.Open(ref, host, callID)
	audit := credentialRefAudit{
		OrganizationID: claims.OrgID,
		ConnectionID:   claims.ConnectionID,
		BindingID:      claims.BindingID,
		CallID:         callID,
		Host:           host,
	}
	switch {
	case errors.Is(err, secretcrypto.ErrCredentialRefHostMismatch):
		return credentialRefRedeemRefused(c, cfg, audit, credentialRefReasonHostMismatch, "The credential reference is not valid for this host.")
	case errors.Is(err, secretcrypto.ErrCredentialRefCallMismatch):
		return credentialRefRedeemRefused(c, cfg, audit, credentialRefReasonCallMismatch, "The credential reference is not valid for this call.")
	case err != nil:
		// Nothing authenticated, so no organisation or binding can be named for an audit row.
		return apiError(c, fiber.StatusForbidden, "credential_ref_invalid", "The credential reference is not valid.")
	}
	if !time.Now().UTC().Before(claims.ExpiresAt) {
		return credentialRefRedeemRefused(c, cfg, audit, credentialRefReasonExpired, "The credential reference has expired.")
	}
	binding, err := cfg.Repo.GetCredentialBindingForRedeem(c.UserContext(), claims.OrgID, claims.BindingID)
	if err != nil {
		if errors.Is(err, store.ErrNotFound) {
			return credentialRefRedeemRefused(c, cfg, audit, credentialRefReasonBindingNotFound, "The credential binding is not available.")
		}
		if auditErr := auditCredentialRedemption(c.UserContext(), cfg, audit, credentialRefOutcomeRefused, credentialRefReasonBindingUnavailable); auditErr != nil {
			return credentialRefAuditFailed(c)
		}
		return apiError(c, fiber.StatusServiceUnavailable, "credential_binding_unavailable", "The credential binding could not be read right now.")
	}
	if binding.Status != store.CredentialBindingStatusActive {
		return credentialRefRedeemRefused(c, cfg, audit, credentialRefReasonBindingRevoked, "The credential binding is not active.")
	}
	if binding.Host != host {
		return credentialRefRedeemRefused(c, cfg, audit, credentialRefReasonBindingHostMismatch, "The credential binding does not name this host.")
	}
	secret, err := cfg.CredentialSealer.Decrypt(string(binding.SecretCiphertext), []byte(binding.ID))
	if err != nil {
		// A reference sealed under a retired key lands here and fails closed.
		return credentialRefRedeemRefused(c, cfg, audit, credentialRefReasonCredentialUnavailable, "The credential is not available under the current key.")
	}
	if err := auditCredentialRedemption(c.UserContext(), cfg, audit, credentialRefOutcomeAllowed, credentialRefReasonOK); err != nil {
		return credentialRefAuditFailed(c)
	}
	return success(c, fiber.Map{"secret": secret, "header_name": binding.HeaderName})
}

// credentialRefRedeemRefused writes the refused redemption row, then answers the refusal. A row that
// cannot be written turns the answer into 503 credential_audit_failed, so a refusal is never silent.
func credentialRefRedeemRefused(c *fiber.Ctx, cfg ServerConfig, audit credentialRefAudit, reason, message string) error {
	if err := auditCredentialRedemption(c.UserContext(), cfg, audit, credentialRefOutcomeRefused, reason); err != nil {
		return credentialRefAuditFailed(c)
	}
	return credentialRefRefused(c, message, reason)
}

// credentialRefRefused answers a refusal with the typed envelope; details carries the reason code.
func credentialRefRefused(c *fiber.Ctx, message, reason string) error {
	return apiErrorWithDetails(c, fiber.StatusForbidden, "credential_ref_refused", message, fiber.Map{"details": fiber.Map{"reason_code": reason}})
}

// credentialRefAuditFailed is the answer when a redemption row cannot be persisted. No secret leaves.
func credentialRefAuditFailed(c *fiber.Ctx) error {
	return apiError(c, fiber.StatusServiceUnavailable, "credential_audit_failed", "The credential could not be released safely.")
}

// auditCredentialRedemption writes one connection.credential.redeemed row. It carries ids, the
// outcome, the reason code, the host and the call id, and never the secret, the reference, a path,
// a query or a header value. The caller fails closed on the error.
func auditCredentialRedemption(ctx context.Context, cfg ServerConfig, audit credentialRefAudit, outcome, reason string) error {
	return withAuditTransaction(ctx, cfg, func(_ store.AuditTransaction, persist func(store.AuditEvent) error) error {
		return persist(store.AuditEvent{
			ID:             auditMutationID(ctx, "credential-redeemed", audit.BindingID),
			OrganizationID: audit.OrganizationID,
			ConnectionID:   audit.ConnectionID,
			EventType:      auditEventCredentialRedeemed,
			Metadata: map[string]any{
				"outcome":    outcome,
				"reasonCode": reason,
				"bindingId":  audit.BindingID,
				"callId":     audit.CallID,
				"host":       audit.Host,
			},
		})
	})
}

// credentialRefTokenValid admits an identifier a reference carries: 1 to 128 letters, digits and
// the separators _ - . :. It excludes the scope separator and anything a path or header could misread.
func credentialRefTokenValid(value string) bool {
	if value == "" || len(value) > credentialRefMaxIDLength {
		return false
	}
	for index := 0; index < len(value); index++ {
		ch := value[index]
		if !credentialRefAlnum(ch) && ch != '_' && ch != '-' && ch != '.' && ch != ':' {
			return false
		}
	}
	return true
}

// credentialRefSlotValid admits a slot name. A slot becomes one path segment in the proxy, so it
// takes the strict alphabet and never a dot.
func credentialRefSlotValid(value string) bool {
	if value == "" || len(value) > credentialRefMaxSlotLength {
		return false
	}
	for index := 0; index < len(value); index++ {
		ch := value[index]
		if !credentialRefAlnum(ch) && ch != '_' && ch != '-' {
			return false
		}
	}
	return true
}

func credentialRefAlnum(ch byte) bool {
	return (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || (ch >= '0' && ch <= '9')
}
