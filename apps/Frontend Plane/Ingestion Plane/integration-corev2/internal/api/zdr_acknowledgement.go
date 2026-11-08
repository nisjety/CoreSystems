package api

import (
	"context"
	"errors"
	"strconv"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/zdr"
)

// The ZDR acknowledgement API (Z-13). A member who uses a connection that cannot give zero data
// retention acknowledges that once, against the exact assessment and the exact text they were
// shown. The routes are bearer-only and act on the owner's own connection: the internal API key
// can never name the user, and an organisation admin sees the ledger as ids and counts only.
//
// The routes exist only while ZDR_ACKNOWLEDGEMENT_ENABLED is on (dark by default). Nothing here
// decides what a turn may do: the decision is Auth Core's, and it reads the posture through
// GET /api/v1/model-connections/:id/zdr-posture.

// ZDROrgPosture reads whether an organisation's ZDR is in force: the stored intent AND the plan
// entitlement, from org-core. Any error means the posture is unknown and the caller must refuse.
type ZDROrgPosture interface {
	EffectiveZDR(ctx context.Context, organizationID string) (bool, error)
}

// Audit and integration event names written by the acknowledgement API. Content-free.
const (
	auditEventZDRAcknowledged = "subscription_connection.zdr_acknowledged"
	integrationEventConnUpd   = "verevon.ingestion.integration.connection_updated"
)

// zdrAcknowledgementBase is the path every acknowledgement route lives under.
const zdrAcknowledgementBase = "/api/v1/model-connections"

// registerZDRAcknowledgementRoutes mounts the acknowledgement routes when the switch is on. The
// bearer guard is passed in: it must be auth.BearerOnly, never a guard that accepts the internal key.
func registerZDRAcknowledgementRoutes(app *fiber.App, cfg ServerConfig, bearerOnly fiber.Handler, rateLimited []fiber.Handler) {
	if !cfg.Config.ZDRAcknowledgementEnabled {
		return
	}

	app.Get(zdrAcknowledgementBase+"/zdr-acknowledgements", bearerOnly, func(c *fiber.Ctx) error {
		return zdrAdminList(c, cfg)
	})
	app.Get(zdrAcknowledgementBase+"/:id/zdr", bearerOnly, func(c *fiber.Ctx) error {
		return zdrAssessmentRead(c, cfg)
	})
	app.Post(zdrAcknowledgementBase+"/:id/zdr-acknowledgement", chainHandlers(rateLimited, bearerOnly, func(c *fiber.Ctx) error {
		return zdrAcknowledge(c, cfg)
	})...)
	app.Delete(zdrAcknowledgementBase+"/:id/zdr-acknowledgement", bearerOnly, func(c *fiber.Ctx) error {
		return zdrRevokeOwn(c, cfg)
	})
	app.Get(zdrAcknowledgementBase+"/:id/zdr-posture", bearerOnly, func(c *fiber.Ctx) error {
		return zdrPostureRead(c, cfg)
	})
}

// zdrHTTPError is a refusal with its status, code and message; details, when set, are merged into
// the error object (for example the current assessment on a stale acknowledgement).
type zdrHTTPError struct {
	status  int
	code    string
	message string
	details fiber.Map
}

func (e zdrHTTPError) Error() string { return e.message }

func zdrRefuse(status int, code, message string) error {
	return zdrHTTPError{status: status, code: code, message: message}
}

func writeZDRError(c *fiber.Ctx, err error) error {
	var refusal zdrHTTPError
	if errors.As(err, &refusal) {
		if refusal.details != nil {
			return apiErrorWithDetails(c, refusal.status, refusal.code, refusal.message, refusal.details)
		}
		return apiError(c, refusal.status, refusal.code, refusal.message)
	}
	return apiError(c, fiber.StatusInternalServerError, "internal_error", "The request could not be completed.")
}

// zdrOwnedModelConnection loads the connection the request names and refuses unless the verified
// caller is its owner in its own organisation. The internal key never reaches here: the guard in
// front of every route admits only a verified bearer token.
func zdrOwnedModelConnection(c *fiber.Ctx, cfg ServerConfig) (store.Connection, error) {
	principal, ok := auth.PrincipalFromContext(c)
	if !ok || auth.IsInternalCall(c) {
		return store.Connection{}, zdrRefuse(fiber.StatusUnauthorized, "unauthorized", "A verified user is required.")
	}
	if cfg.Repo == nil {
		return store.Connection{}, zdrRefuse(fiber.StatusServiceUnavailable, "repository_unavailable", "The connection store is unavailable.")
	}
	connection, err := cfg.Repo.GetConnection(c.UserContext(), c.Params("id"))
	if errors.Is(err, store.ErrNotFound) || (err == nil && connection.DeletedAt != nil) {
		return store.Connection{}, zdrRefuse(fiber.StatusNotFound, "connection_not_found", "Connection was not found.")
	}
	if err != nil {
		return store.Connection{}, err
	}
	if strings.TrimSpace(principal.OrganizationID) == "" || principal.OrganizationID != connection.OrganizationID {
		return store.Connection{}, zdrRefuse(fiber.StatusForbidden, "forbidden", "Access to this organization is not allowed.")
	}
	if owner := principal.EffectiveUserID(false); owner == "" || owner != connection.UserID {
		return store.Connection{}, zdrRefuse(fiber.StatusForbidden, "not_connection_owner", "Only the member who connected this subscription can act on it.")
	}
	if _, model := modelConnectionAuthMode(connection); !model {
		return store.Connection{}, zdrRefuse(fiber.StatusNotFound, "not_a_model_connection", "This connection is not a model connection.")
	}
	return connection, nil
}

// zdrWording is the loaded acknowledgement wording, or a refusal when it cannot be read.
func zdrWording() (*zdr.Wording, error) {
	wording, err := zdr.DefaultWording()
	if err != nil {
		return nil, zdrRefuse(fiber.StatusServiceUnavailable, "zdr_wording_unavailable", "The acknowledgement text is unavailable.")
	}
	return wording, nil
}

// zdrWordingKey is the wording key of a recorded assessment (the contract row it was taken from).
func zdrWordingKey(record store.ConnectionZDR) (string, error) {
	assessor, err := zdr.Default()
	if err != nil {
		return "", err
	}
	assessment, err := assessor.Assess(record.ProviderKey, record.AuthMode)
	if err != nil {
		return "", err
	}
	return assessment.WordingKey, nil
}

// zdrAssessmentRead answers what the member sees before acknowledging: the capability record, whether
// an acknowledgement is required, the acknowledgement in force and the text for a locale and variant.
func zdrAssessmentRead(c *fiber.Ctx, cfg ServerConfig) error {
	connection, err := zdrOwnedModelConnection(c, cfg)
	if err != nil {
		return writeZDRError(c, err)
	}
	wording, err := zdrWording()
	if err != nil {
		return writeZDRError(c, err)
	}
	record, err := cfg.Repo.GetConnectionZDR(c.UserContext(), connection.ID)
	if errors.Is(err, store.ErrNotFound) {
		return success(c, fiber.Map{
			"connectionId": connection.ID, "providerKey": connection.ProviderKey, "assessed": false,
			"capability": string(zdr.CapabilityCannotVerify), "basis": string(zdr.BasisNotAssessed), "digest": "",
			"requiresAcknowledgement": true, "wordingVersion": wording.Version(), "wordingStatus": wording.Status(),
			"acknowledgement": nil, "text": nil,
		})
	}
	if err != nil {
		return writeZDRError(c, err)
	}
	locale := strings.TrimSpace(c.Query("locale", "no"))
	variant := strings.TrimSpace(c.Query("variant", "org_zdr_on"))
	if !zdrLocaleKnown(locale) {
		return writeZDRError(c, zdrRefuse(fiber.StatusBadRequest, "invalid_locale", "Locale must be no or en."))
	}
	if !zdrVariantKnown(variant) {
		return writeZDRError(c, zdrRefuse(fiber.StatusBadRequest, "invalid_variant", "Variant must be org_zdr_on, org_zdr_off or first_use."))
	}
	key, err := zdrWordingKey(record)
	if err != nil {
		return writeZDRError(c, err)
	}
	text, err := wording.Lookup(key, variant, locale)
	if err != nil {
		return writeZDRError(c, zdrRefuse(fiber.StatusBadRequest, "text_not_available", "There is no text for this connection in that variant."))
	}
	acknowledgement, err := zdrAckInForce(c, cfg, connection.ID, record.Digest)
	if err != nil {
		return writeZDRError(c, err)
	}
	return success(c, fiber.Map{
		"connectionId": connection.ID, "providerKey": connection.ProviderKey, "assessed": true,
		"capability": string(record.Capability), "basis": string(record.Basis), "digest": record.Digest,
		"catalogVersion": record.CatalogVersion, "requiresAcknowledgement": record.Capability.RequiresAcknowledgement(),
		"wordingVersion": wording.Version(), "wordingStatus": wording.Status(),
		"acknowledgement": acknowledgement,
		"text": fiber.Map{
			"key": key, "variant": variant, "locale": locale, "title": text.Title, "body": text.Body,
			"continueLabel": text.ContinueLabel, "cancelLabel": text.CancelLabel, "sha256": text.SHA256,
		},
	})
}

// zdrAckInForce returns the active acknowledgement bound to the current digest, or nil.
func zdrAckInForce(c *fiber.Ctx, cfg ServerConfig, connectionID, digest string) (fiber.Map, error) {
	rows, err := cfg.Repo.ListConnectionZDRAcknowledgements(c.UserContext(), connectionID, true)
	if err != nil {
		return nil, err
	}
	for _, ack := range rows {
		if ack.Digest == digest && ack.Active() {
			return zdrAckSummary(ack), nil
		}
	}
	return nil, nil
}

func zdrAckSummary(ack store.ConnectionZDRAcknowledgement) fiber.Map {
	return fiber.Map{
		"id": ack.ID, "state": zdrAckState(ack), "digest": ack.Digest, "textVersion": ack.TextVersion,
		"locale": ack.Locale, "acknowledgedVia": ack.AcknowledgedVia, "acknowledgedAt": ack.AcknowledgedAt,
	}
}

func zdrAckState(ack store.ConnectionZDRAcknowledgement) string {
	if ack.Active() {
		return "active"
	}
	return "revoked"
}

// zdrAcknowledge records the member's acknowledgement. The digest and text version must be the
// ones the member was shown; a change in either is a 409 with the current assessment. The text is
// composed on the server from the versioned wording and stored with its hash, and the acknowledgement,
// its audit row and the outbox event commit together. A replay returns the stored acknowledgement.
func zdrAcknowledge(c *fiber.Ctx, cfg ServerConfig) error {
	connection, err := zdrOwnedModelConnection(c, cfg)
	if err != nil {
		return writeZDRError(c, err)
	}
	var body struct {
		Digest          string `json:"digest"`
		TextVersion     string `json:"textVersion"`
		Locale          string `json:"locale"`
		Variant         string `json:"variant"`
		AcknowledgedVia string `json:"acknowledgedVia"`
	}
	if err := c.BodyParser(&body); err != nil {
		return writeZDRError(c, zdrRefuse(fiber.StatusBadRequest, "invalid_body", "Request body is invalid."))
	}
	if !zdr.IsDigest(body.Digest) {
		return writeZDRError(c, zdrRefuse(fiber.StatusBadRequest, "invalid_body", "digest must be 64 lowercase hex characters."))
	}
	if !zdrLocaleKnown(body.Locale) {
		return writeZDRError(c, zdrRefuse(fiber.StatusBadRequest, "invalid_locale", "Locale must be no or en."))
	}
	if !zdrViaKnown(body.AcknowledgedVia) {
		return writeZDRError(c, zdrRefuse(fiber.StatusBadRequest, "invalid_acknowledged_via", "acknowledgedVia must be connect, first_use or settings."))
	}
	if !zdrAckVariantKnown(body.Variant) {
		return writeZDRError(c, zdrRefuse(fiber.StatusBadRequest, "invalid_variant", "variant must be org_zdr_on or first_use, the text the member was shown."))
	}
	wording, err := zdrWording()
	if err != nil {
		return writeZDRError(c, err)
	}
	record, err := cfg.Repo.GetConnectionZDR(c.UserContext(), connection.ID)
	if errors.Is(err, store.ErrNotFound) {
		return writeZDRError(c, zdrRefuse(fiber.StatusConflict, "zdr_unassessed", "This connection has not been assessed yet. Reload the connection and try again."))
	}
	if err != nil {
		return writeZDRError(c, err)
	}
	if !record.Capability.RequiresAcknowledgement() {
		return writeZDRError(c, zdrRefuse(fiber.StatusConflict, "zdr_acknowledgement_not_required", "This connection can give ZDR, so there is nothing to acknowledge."))
	}
	if status := strings.ToLower(strings.TrimSpace(connection.Status)); status != "active" && status != "connected" {
		return writeZDRError(c, zdrRefuse(fiber.StatusConflict, "connection_not_active", "Finish connecting the subscription before acknowledging it."))
	}
	current, err := zdrCurrentAssessment(record, wording.Version())
	if err != nil {
		return writeZDRError(c, err)
	}
	if body.Digest != record.Digest || body.TextVersion != wording.Version() {
		return writeZDRError(c, zdrHTTPError{
			status: fiber.StatusConflict, code: "zdr_assessment_changed",
			message: "The assessment or the text changed. Review the current text and acknowledge again.",
			details: fiber.Map{"current": current},
		})
	}
	if cfg.OrgZDR == nil {
		return writeZDRError(c, zdrRefuse(fiber.StatusServiceUnavailable, "zdr_org_state_unavailable", "The organisation's ZDR setting cannot be read right now. Nothing was recorded."))
	}
	orgOn, err := cfg.OrgZDR.EffectiveZDR(c.UserContext(), connection.OrganizationID)
	if err != nil {
		return writeZDRError(c, zdrRefuse(fiber.StatusServiceUnavailable, "zdr_org_state_unavailable", "The organisation's ZDR setting cannot be read right now. Nothing was recorded."))
	}
	if !orgOn {
		return writeZDRError(c, zdrRefuse(fiber.StatusConflict, "zdr_org_not_on", "Your organisation does not use ZDR, so there is nothing to acknowledge."))
	}

	// The text is the variant the member was shown, not one implied by how they got here: the ledger
	// holds exactly the text they accepted.
	variant := body.Variant
	key, err := zdrWordingKey(record)
	if err != nil {
		return writeZDRError(c, err)
	}
	text, err := wording.Lookup(key, variant, body.Locale)
	if err != nil {
		return writeZDRError(c, zdrRefuse(fiber.StatusServiceUnavailable, "zdr_wording_unavailable", "The acknowledgement text is unavailable."))
	}

	var saved store.ConnectionZDRAcknowledgement
	var created bool
	err = withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		var insertErr error
		saved, created, insertErr = tx.InsertConnectionZDRAcknowledgement(c.UserContext(), store.NewConnectionZDRAcknowledgement{
			OrganizationID: connection.OrganizationID, ConnectionID: connection.ID, UserID: connection.UserID,
			Digest: body.Digest, TextVersion: text.Version, Locale: body.Locale,
			TextSHA256: text.SHA256, TextSnapshot: text.Snapshot,
			OrgZDRAtAck: true, AcknowledgedVia: body.AcknowledgedVia,
			RequestID: requestIDFromContext(c.UserContext()),
		})
		if insertErr != nil {
			return insertErr
		}
		if !created {
			return nil
		}
		return persist(store.AuditEvent{
			ID:             auditMutationID(c.UserContext(), "zdr-acknowledged", saved.ID),
			OrganizationID: connection.OrganizationID,
			UserID:         connection.UserID,
			ConnectionID:   connection.ID,
			EventType:      auditEventZDRAcknowledged,
			ProviderKey:    connection.ProviderKey,
			Metadata: map[string]any{
				"acknowledgementId": saved.ID,
				"digest":            saved.Digest,
				"textVersion":       saved.TextVersion,
				"locale":            saved.Locale,
				"acknowledgedVia":   saved.AcknowledgedVia,
				"capability":        string(saved.Capability),
				"basis":             string(saved.Basis),
				"orgZdrAtAck":       saved.OrgZDRAtAck,
			},
		})
	})
	if err != nil {
		return writeZDRError(c, zdrStoreRefusal(err))
	}
	if created {
		publishIntegrationEvent(c.UserContext(), cfg, integrationEventConnUpd, connection, map[string]any{
			"change": "zdr_acknowledged", "acknowledgementId": saved.ID,
		})
	}
	status := fiber.StatusOK
	if created {
		status = fiber.StatusCreated
	}
	return c.Status(status).JSON(fiber.Map{
		"success": true,
		"data":    fiber.Map{"created": created, "acknowledgement": zdrAckSummary(saved)},
	})
}

// zdrStoreRefusal maps the ledger's refusals to the API's codes. Anything else is an internal error.
func zdrStoreRefusal(err error) error {
	switch {
	case errors.Is(err, store.ErrConflict):
		return zdrRefuse(fiber.StatusConflict, "zdr_assessment_changed", "The assessment changed while you were acknowledging it. Review the current text and try again.")
	case errors.Is(err, store.ErrNotFound):
		return zdrRefuse(fiber.StatusNotFound, "connection_not_found", "Connection was not found.")
	case errors.Is(err, store.ErrInvalidZDR):
		return zdrRefuse(fiber.StatusConflict, "zdr_acknowledgement_not_required", "This connection needs no acknowledgement.")
	default:
		return zdrRefuse(fiber.StatusServiceUnavailable, "zdr_acknowledgement_failed", "The acknowledgement could not be recorded. Nothing was saved; try again.")
	}
}

// zdrCurrentAssessment is what a 409 hands back so the client can show the current state again.
func zdrCurrentAssessment(record store.ConnectionZDR, textVersion string) (fiber.Map, error) {
	key, err := zdrWordingKey(record)
	if err != nil {
		return nil, err
	}
	return fiber.Map{
		"digest": record.Digest, "capability": string(record.Capability), "basis": string(record.Basis),
		"textVersion": textVersion, "wordingKey": key, "requiresAcknowledgement": record.Capability.RequiresAcknowledgement(),
	}, nil
}

// zdrRevokeOwn ends the owner's acknowledgements of the connection. It is idempotent: with nothing
// active it revokes nothing and writes nothing.
func zdrRevokeOwn(c *fiber.Ctx, cfg ServerConfig) error {
	connection, err := zdrOwnedModelConnection(c, cfg)
	if err != nil {
		return writeZDRError(c, err)
	}
	at := time.Now().UTC()
	var revoked int
	err = withAuditTransaction(c.UserContext(), cfg, func(tx store.AuditTransaction, persist func(store.AuditEvent) error) error {
		rows, revokeErr := tx.RevokeConnectionZDRAcknowledgements(c.UserContext(), connection.ID, store.ZDRRevokeOwnerRevoked, at)
		if revokeErr != nil {
			return revokeErr
		}
		for _, ack := range rows {
			if err := persist(store.ZDRAcknowledgementRevokedAuditEvent(ack, store.ZDRRevokeOwnerRevoked, at)); err != nil {
				return err
			}
		}
		revoked = len(rows)
		return nil
	})
	if err != nil {
		return writeZDRError(c, zdrStoreRefusal(err))
	}
	if revoked > 0 {
		publishIntegrationEvent(c.UserContext(), cfg, integrationEventConnUpd, connection, map[string]any{
			"change": "zdr_acknowledgement_revoked", "revoked": revoked,
		})
	}
	return success(c, fiber.Map{"connectionId": connection.ID, "revoked": revoked})
}

// zdrPostureRead is the user-bound posture Auth Core reads, as the session user, when it decides a
// turn. It carries only the opaque digest, the text version and the acknowledgement state (and the
// acknowledgement's id, which the decision binds to). It never carries the capability's basis, the
// organisation or any text.
func zdrPostureRead(c *fiber.Ctx, cfg ServerConfig) error {
	connection, err := zdrOwnedModelConnection(c, cfg)
	if err != nil {
		return writeZDRError(c, err)
	}
	wording, err := zdrWording()
	if err != nil {
		return writeZDRError(c, err)
	}
	// A posture read decides whether a turn's content is retained: it is never cached.
	c.Set("Cache-Control", "no-store")
	posture := fiber.Map{"connectionId": connection.ID, "ackState": "unassessed", "ackId": "", "digest": "", "textVersion": ""}
	record, err := cfg.Repo.GetConnectionZDR(c.UserContext(), connection.ID)
	if errors.Is(err, store.ErrNotFound) {
		return success(c, posture)
	}
	if err != nil {
		return writeZDRError(c, err)
	}
	posture["digest"] = record.Digest
	posture["textVersion"] = wording.Version()
	if record.Capability.KeepsZDR() {
		posture["ackState"] = "not_required"
		return success(c, posture)
	}
	posture["ackState"] = "required"
	rows, err := cfg.Repo.ListConnectionZDRAcknowledgements(c.UserContext(), connection.ID, true)
	if err != nil {
		return writeZDRError(c, err)
	}
	for _, ack := range rows {
		if ack.Digest == record.Digest && ack.Active() {
			posture["ackState"] = "acknowledged"
			posture["ackId"] = ack.ID
			posture["textVersion"] = ack.TextVersion
			break
		}
	}
	return success(c, posture)
}

// zdrAdminList is the organisation's ledger for its admins: ids, states and counts, never the text
// the member accepted and never a request id.
func zdrAdminList(c *fiber.Ctx, cfg ServerConfig) error {
	principal, ok := auth.PrincipalFromContext(c)
	if !ok || auth.IsInternalCall(c) || strings.TrimSpace(principal.OrganizationID) == "" {
		return writeZDRError(c, zdrRefuse(fiber.StatusUnauthorized, "unauthorized", "A verified user is required."))
	}
	if !zdrHasOrgAdminRole(principal.Role) {
		return writeZDRError(c, zdrRefuse(fiber.StatusForbidden, "org_admin_required", "Only an organisation owner or admin can see the acknowledgement record."))
	}
	limit := 200
	if raw := strings.TrimSpace(c.Query("limit")); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil || parsed < 1 || parsed > 500 {
			return writeZDRError(c, zdrRefuse(fiber.StatusBadRequest, "invalid_limit", "limit must be between 1 and 500."))
		}
		limit = parsed
	}
	if cfg.Repo == nil {
		return writeZDRError(c, zdrRefuse(fiber.StatusServiceUnavailable, "repository_unavailable", "The connection store is unavailable."))
	}
	list, err := cfg.Repo.ListOrganizationConnectionZDRAcknowledgements(c.UserContext(), principal.OrganizationID, limit)
	if err != nil {
		return writeZDRError(c, err)
	}
	items := make([]fiber.Map, 0, len(list.Items))
	for _, ack := range list.Items {
		items = append(items, fiber.Map{
			"id": ack.ID, "connectionId": ack.ConnectionID, "userId": ack.UserID, "providerKey": ack.ProviderKey,
			"capability": string(ack.Capability), "textVersion": ack.TextVersion, "locale": ack.Locale,
			"acknowledgedVia": ack.AcknowledgedVia, "acknowledgedAt": ack.AcknowledgedAt,
			"revokedAt": ack.RevokedAt, "revokeReason": ack.RevokeReason, "status": zdrAckState(ack),
		})
	}
	return success(c, fiber.Map{
		"items":     items,
		"counts":    fiber.Map{"active": list.Active, "revoked": list.Revoked},
		"truncated": list.Active+list.Revoked > len(list.Items),
	})
}

// zdrHasOrgAdminRole reports whether an organisation role grants the admin view. Roles are the
// comma-separated list Auth Core issues; owner and admin are the two that administer an organisation.
func zdrHasOrgAdminRole(role string) bool {
	for _, part := range strings.Split(role, ",") {
		switch strings.ToLower(strings.TrimSpace(part)) {
		case "owner", "admin":
			return true
		}
	}
	return false
}

func zdrLocaleKnown(locale string) bool { return locale == "no" || locale == "en" }

func zdrVariantKnown(variant string) bool {
	return variant == "org_zdr_on" || variant == "org_zdr_off" || variant == "first_use"
}

// zdrAckVariantKnown reports whether a member can acknowledge a text variant: the text shown while the
// organisation has ZDR on, or the first-use text. Nothing else is acknowledged.
func zdrAckVariantKnown(variant string) bool {
	return variant == "org_zdr_on" || variant == "first_use"
}

func zdrViaKnown(via string) bool {
	return via == store.ZDRAcknowledgedViaConnect || via == store.ZDRAcknowledgedViaFirstUse || via == store.ZDRAcknowledgedViaSettings
}
