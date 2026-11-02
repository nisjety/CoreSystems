package spaces

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// Typed error codes of the org-admin policy routes (WS03-S3). The HTTP layer
// maps each code to a status; this package only names them.
const (
	OrgPolicyCodeInvalidRequest       = "space_policy_invalid_request"
	OrgPolicyCodeNotProvisioned       = "space_policy_not_provisioned"
	OrgPolicyCodeRevisionConflict     = "space_policy_revision_conflict"
	OrgPolicyCodeIdempotencyConflict  = "space_policy_idempotency_conflict"
	OrgPolicyCodeZDRConflict          = "space_policy_zdr_conflict"
	OrgPolicyCodeUnknownPreset        = "space_policy_unknown_preset"
	OrgPolicyReasonZDRInert           = "zdr_inert"
	OrgPolicyReasonZDRUnproven        = "zdr_unproven"
	OrgPolicyStateProvisioned         = "provisioned"
	OrgPolicyStateNotProvisioned      = "not_provisioned"
	OrgPolicyEventsDefaultLimit       = 20
	OrgPolicyEventsMaxLimit           = 100
	OrgPolicyIdempotencyKeyMinLength  = 8
	OrgPolicyIdempotencyKeyMaxLength  = 128
	orgPolicyWriteDeniedAuditSubject  = "verevon.audit.v2.control.user-core.space_effect_policy_write_denied"
	orgPolicyWriteDeniedAuditEventTag = "space_effect_policy_write_denied"
)

// OrgPolicyError is a refusal the caller can act on. Details never carry
// attestation values or content: key names, a revision number, a preset id.
type OrgPolicyError struct {
	Code    string
	Message string
	Details map[string]any
}

func (e *OrgPolicyError) Error() string { return e.Message }

func orgPolicyErr(code, message string, details map[string]any) *OrgPolicyError {
	return &OrgPolicyError{Code: code, Message: message, Details: details}
}

// ValidateEntitlementChange checks a full desired entitlement state against
// the current policy. The target must name every catalogue key and no other
// key. Under Space ZDR (the stored attestation), a key the phase-1 rule blocks
// (class zdr_inert or zdr_unproven) cannot be newly enabled; switching a bit
// off, or leaving an already-enabled one alone, is always allowed.
func ValidateEntitlementChange(current EffectPolicy, target map[string]bool) error {
	for key := range target {
		if _, ok := EntitlementSpecByKey(key); !ok {
			return orgPolicyErr(OrgPolicyCodeInvalidRequest, "unknown entitlement key",
				map[string]any{"field": "entitlements." + key})
		}
	}
	for _, spec := range entitlementCatalog {
		if _, ok := target[spec.Key]; !ok {
			return orgPolicyErr(OrgPolicyCodeInvalidRequest, "every entitlement key must be stated",
				map[string]any{"field": "entitlements." + spec.Key})
		}
	}
	if !current.ZeroDataRetention {
		return nil
	}
	var blocked []string
	reason := ""
	for _, spec := range entitlementCatalog {
		if !target[spec.Key] || spec.Get(current) || spec.ZDR == ZDRCompatible {
			continue
		}
		blocked = append(blocked, spec.Key)
		if spec.ZDR == ZDRInert {
			reason = OrgPolicyReasonZDRInert
		} else if reason == "" {
			reason = OrgPolicyReasonZDRUnproven
		}
	}
	if len(blocked) > 0 {
		return orgPolicyErr(OrgPolicyCodeZDRConflict,
			"entitlement cannot be enabled while Space Zero Data Retention is on",
			map[string]any{"keys": blocked, "reason": reason})
	}
	return nil
}

// OrgPolicyAttestation is the read-only privacy attestation of the org row.
type OrgPolicyAttestation struct {
	PrivacyPolicyRef            string `json:"privacy_policy_ref"`
	Purpose                     string `json:"purpose"`
	LawfulBasis                 string `json:"lawful_basis"`
	PrivacyClass                string `json:"privacy_class"`
	ThirdPartyProcessingAllowed bool   `json:"third_party_processing_allowed"`
	RetentionClass              string `json:"retention_class"`
	Residency                   string `json:"residency"`
	DeletionScope               string `json:"deletion_scope"`
	ZeroDataRetention           bool   `json:"zero_data_retention"`
}

// OrgPolicyEntitlementView is one entitlement bit as an org admin sees it.
type OrgPolicyEntitlementView struct {
	Key       string   `json:"key"`
	Enabled   bool     `json:"enabled"`
	Effective bool     `json:"effective"`
	ZDR       ZDRClass `json:"zdr"`
	// ZDRBlocked says the bit cannot be newly enabled while the org's Space
	// ZDR attestation is on (phase-1 write rule).
	ZDRBlocked bool     `json:"zdr_blocked"`
	Decisions  []string `json:"decisions"`
}

// OrgPolicyPresetView is a preset with its availability for this org.
type OrgPolicyPresetView struct {
	ID           string          `json:"id"`
	Entitlements map[string]bool `json:"entitlements"`
	Available    bool            `json:"available"`
	BlockedBy    []string        `json:"blocked_by"`
}

// OrgPolicyRevisions are the two optimistic-concurrency counters.
type OrgPolicyRevisions struct {
	Policy      int64 `json:"policy"`
	Entitlement int64 `json:"entitlement"`
}

// OrgPolicyCapabilities is filled by the HTTP layer (it knows the write
// switch); the store leaves it zero.
type OrgPolicyCapabilities struct {
	Write             bool    `json:"write"`
	WriteDeniedReason *string `json:"write_denied_reason"`
}

// OrgPolicyEventActor names who made a change.
type OrgPolicyEventActor struct {
	Type string `json:"type"`
	ID   string `json:"id"`
	Name string `json:"name,omitempty"`
	Role string `json:"role,omitempty"`
}

// OrgPolicyEvent is one row of the immutable change history.
type OrgPolicyEvent struct {
	EventID                  int64               `json:"event_id"`
	At                       time.Time           `json:"at"`
	Source                   string              `json:"source"`
	Actor                    OrgPolicyEventActor `json:"actor"`
	ChangeKind               string              `json:"change_kind"`
	PresetID                 string              `json:"preset_id,omitempty"`
	Changed                  []EntitlementChange `json:"changed"`
	EntitlementRevisionAfter int64               `json:"entitlement_revision_after"`
	SpacesFenced             int                 `json:"spaces_fenced"`
}

// OrgPolicyLastChange is the compact form of the newest event.
type OrgPolicyLastChange struct {
	EventID     int64               `json:"event_id"`
	At          time.Time           `json:"at"`
	Source      string              `json:"source"`
	Actor       OrgPolicyEventActor `json:"actor"`
	ChangeKind  string              `json:"change_kind"`
	PresetID    string              `json:"preset_id,omitempty"`
	ChangedKeys []string            `json:"changed_keys"`
}

// OrgPolicyView is what GET /org-policy returns. A missing row is
// `not_provisioned`: attestation null, every bit off, write not possible.
type OrgPolicyView struct {
	OrgID         string                     `json:"org_id"`
	State         string                     `json:"state"`
	Capabilities  OrgPolicyCapabilities      `json:"capabilities"`
	Revisions     OrgPolicyRevisions         `json:"revisions"`
	UpdatedAt     *time.Time                 `json:"updated_at"`
	Attestation   *OrgPolicyAttestation      `json:"attestation"`
	Entitlements  []OrgPolicyEntitlementView `json:"entitlements"`
	Presets       []OrgPolicyPresetView      `json:"presets"`
	MatchesPreset *string                    `json:"matches_preset"`
	ActiveSpaces  int64                      `json:"active_spaces"`
	LastChange    *OrgPolicyLastChange       `json:"last_change"`
}

// BuildOrgPolicyView renders the admin view from a stored row. policy nil is
// the not-provisioned view. It is pure so it can be unit tested.
func BuildOrgPolicyView(orgID string, policy *EffectPolicy, revisions OrgPolicyRevisions, updatedAt *time.Time, activeSpaces int64, last *OrgPolicyLastChange) OrgPolicyView {
	view := OrgPolicyView{
		OrgID: orgID, State: OrgPolicyStateNotProvisioned, Revisions: revisions, UpdatedAt: updatedAt,
		ActiveSpaces: activeSpaces, LastChange: last,
	}
	var current EffectPolicy
	zdr := false
	if policy != nil {
		current = *policy
		zdr = policy.ZeroDataRetention
		view.State = OrgPolicyStateProvisioned
		view.Attestation = &OrgPolicyAttestation{
			PrivacyPolicyRef: policy.PrivacyPolicyRef, Purpose: policy.Purpose, LawfulBasis: policy.LawfulBasis,
			PrivacyClass: policy.PrivacyClass, ThirdPartyProcessingAllowed: policy.ThirdPartyProcessingAllowed,
			RetentionClass: policy.RetentionClass, Residency: policy.Residency, DeletionScope: policy.DeletionScope,
			ZeroDataRetention: policy.ZeroDataRetention,
		}
	}
	view.Entitlements = make([]OrgPolicyEntitlementView, 0, len(entitlementCatalog))
	for _, spec := range entitlementCatalog {
		enabled := spec.Get(current)
		// Control refuses a ZDR-inert decision outright under Space ZDR, so an
		// enabled inert bit is not effective. An unproven bit is still issued by
		// Control (the downstream plane's handling is what is unproven), so it
		// stays effective when enabled; it only cannot be newly enabled.
		view.Entitlements = append(view.Entitlements, OrgPolicyEntitlementView{
			Key: spec.Key, Enabled: enabled,
			Effective:  enabled && !(zdr && spec.ZDR == ZDRInert),
			ZDR:        spec.ZDR,
			ZDRBlocked: zdr && spec.ZDR != ZDRCompatible,
			Decisions:  append([]string(nil), spec.Decisions...),
		})
	}
	values := EntitlementValues(current)
	view.Presets = make([]OrgPolicyPresetView, 0, len(orgPolicyPresets))
	for _, preset := range orgPolicyPresets {
		expanded, _ := PresetEntitlements(preset.ID)
		blockedBy := []string{}
		if zdr {
			for _, key := range preset.Keys {
				if spec, ok := EntitlementSpecByKey(key); ok && spec.ZDR != ZDRCompatible {
					blockedBy = []string{"zero_data_retention"}
					break
				}
			}
		}
		view.Presets = append(view.Presets, OrgPolicyPresetView{
			ID: preset.ID, Entitlements: expanded, Available: len(blockedBy) == 0, BlockedBy: blockedBy,
		})
		if policy != nil && view.MatchesPreset == nil && boolMapsEqual(values, expanded) {
			id := preset.ID
			view.MatchesPreset = &id
		}
	}
	return view
}

func boolMapsEqual(a, b map[string]bool) bool {
	if len(a) != len(b) {
		return false
	}
	for key, value := range a {
		if other, ok := b[key]; !ok || other != value {
			return false
		}
	}
	return true
}

// GetOrgPolicy reads the org's policy for its admins. A missing row is a
// valid, not-provisioned answer (never an error, never an insert).
func (r *Repository) GetOrgPolicy(ctx context.Context, orgID string) (OrgPolicyView, error) {
	if r == nil || r.db == nil {
		return OrgPolicyView{}, fmt.Errorf("space authority repository unavailable")
	}
	orgID = strings.TrimSpace(orgID)
	var policy EffectPolicy
	var revisions OrgPolicyRevisions
	var updatedAt time.Time
	err := scanEffectPolicy(r.db.Pool.QueryRow(ctx, `
		SELECT `+effectPolicyColumnList()+`, policy_revision, entitlement_revision, updated_at
		FROM space_effect_policies WHERE org_id=$1`, orgID),
		&policy, &revisions.Policy, &revisions.Entitlement, &updatedAt)
	provisioned := true
	if errors.Is(err, pgx.ErrNoRows) {
		provisioned = false
	} else if err != nil {
		return OrgPolicyView{}, fmt.Errorf("read org Space policy: %w", err)
	}
	var activeSpaces int64
	if err := r.db.Pool.QueryRow(ctx, `
		SELECT count(*) FROM registered_spaces WHERE org_id=$1 AND registration_state='active'`, orgID,
	).Scan(&activeSpaces); err != nil {
		return OrgPolicyView{}, fmt.Errorf("count active Spaces: %w", err)
	}
	events, _, err := r.ListOrgPolicyEvents(ctx, orgID, 1, 0)
	if err != nil {
		return OrgPolicyView{}, err
	}
	var last *OrgPolicyLastChange
	if len(events) > 0 {
		ev := events[0]
		keys := make([]string, 0, len(ev.Changed))
		for _, ch := range ev.Changed {
			keys = append(keys, ch.Key)
		}
		last = &OrgPolicyLastChange{
			EventID: ev.EventID, At: ev.At, Source: ev.Source, Actor: ev.Actor,
			ChangeKind: ev.ChangeKind, PresetID: ev.PresetID, ChangedKeys: keys,
		}
	}
	if !provisioned {
		return BuildOrgPolicyView(orgID, nil, OrgPolicyRevisions{}, nil, activeSpaces, last), nil
	}
	return BuildOrgPolicyView(orgID, &policy, revisions, &updatedAt, activeSpaces, last), nil
}

// ListOrgPolicyEvents returns the org's change history, newest first. cursor
// is the last event_id of the previous page (0 for the first); the returned
// next cursor is 0 when there is no further page. limit is clamped to
// [1, OrgPolicyEventsMaxLimit].
func (r *Repository) ListOrgPolicyEvents(ctx context.Context, orgID string, limit int, cursor int64) ([]OrgPolicyEvent, int64, error) {
	if r == nil || r.db == nil {
		return nil, 0, fmt.Errorf("space authority repository unavailable")
	}
	if limit < 1 {
		limit = OrgPolicyEventsDefaultLimit
	}
	if limit > OrgPolicyEventsMaxLimit {
		limit = OrgPolicyEventsMaxLimit
	}
	rows, err := r.db.Pool.Query(ctx, `
		SELECT e.event_id, e.occurred_at, e.source, e.actor_subject_type, e.actor_subject_id,
		       COALESCE(e.actor_org_role, ''), COALESCE(u.name, ''), e.change_kind, COALESCE(e.preset_id, ''),
		       e.before_entitlements, e.after_entitlements, e.entitlement_revision_after, e.spaces_fenced
		FROM space_effect_policy_events e
		LEFT JOIN users u ON e.actor_subject_type='user' AND u.id::text = e.actor_subject_id
		WHERE e.org_id=$1 AND ($2::bigint = 0 OR e.event_id < $2)
		ORDER BY e.event_id DESC
		LIMIT $3`, strings.TrimSpace(orgID), cursor, limit+1)
	if err != nil {
		return nil, 0, fmt.Errorf("list Space effect policy events: %w", err)
	}
	defer rows.Close()
	var events []OrgPolicyEvent
	for rows.Next() {
		var ev OrgPolicyEvent
		var beforeRaw, afterRaw []byte
		if err := rows.Scan(&ev.EventID, &ev.At, &ev.Source, &ev.Actor.Type, &ev.Actor.ID, &ev.Actor.Role,
			&ev.Actor.Name, &ev.ChangeKind, &ev.PresetID, &beforeRaw, &afterRaw,
			&ev.EntitlementRevisionAfter, &ev.SpacesFenced); err != nil {
			return nil, 0, fmt.Errorf("scan Space effect policy event: %w", err)
		}
		var before, after map[string]bool
		if err := json.Unmarshal(beforeRaw, &before); err != nil {
			return nil, 0, fmt.Errorf("decode event before entitlements: %w", err)
		}
		if err := json.Unmarshal(afterRaw, &after); err != nil {
			return nil, 0, fmt.Errorf("decode event after entitlements: %w", err)
		}
		ev.Changed = diffEntitlementMaps(before, after)
		events = append(events, ev)
	}
	if err := rows.Err(); err != nil {
		return nil, 0, fmt.Errorf("list Space effect policy events: %w", err)
	}
	var next int64
	if len(events) > limit {
		events = events[:limit]
		next = events[len(events)-1].EventID
	}
	return events, next, nil
}

// diffEntitlementMaps lists differing bits in catalogue order; a key missing
// from a map counts as false.
func diffEntitlementMaps(before, after map[string]bool) []EntitlementChange {
	changes := []EntitlementChange{}
	for _, spec := range entitlementCatalog {
		from, to := before[spec.Key], after[spec.Key]
		if from != to {
			changes = append(changes, EntitlementChange{Key: spec.Key, From: from, To: to})
		}
	}
	return changes
}

// ApplyOrgEntitlementsRequest is one org-admin write. The org and the actor
// come from the verified delegation, never from the request body.
type ApplyOrgEntitlementsRequest struct {
	OrgID                       string
	Actor                       EffectPolicyActor
	IdempotencyKey              string
	ExpectedEntitlementRevision int64
	// Exactly one of PresetID or Entitlements is set.
	PresetID     string
	Entitlements map[string]bool
}

// ApplyOrgEntitlementsResult is the outcome of an org-admin write.
type ApplyOrgEntitlementsResult struct {
	Changed      bool `json:"changed"`
	Replayed     bool `json:"replayed"`
	SpacesFenced int  `json:"spaces_fenced"`
	// Policy is the view after the write; the HTTP layer fills Capabilities.
	Policy OrgPolicyView `json:"policy"`
}

// orgPolicyRequestDigest binds an idempotency key to one exact request.
func orgPolicyRequestDigest(req ApplyOrgEntitlementsRequest) string {
	keys := make([]string, 0, len(req.Entitlements))
	for key := range req.Entitlements {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	canonical := struct {
		Preset   string          `json:"preset_id"`
		Expected int64           `json:"expected_entitlement_revision"`
		Keys     []string        `json:"keys"`
		Values   map[string]bool `json:"entitlements"`
	}{req.PresetID, req.ExpectedEntitlementRevision, keys, req.Entitlements}
	encoded, _ := json.Marshal(canonical)
	sum := sha256.Sum256(encoded)
	return base64.RawURLEncoding.EncodeToString(sum[:])
}

// ApplyOrgEntitlements is the org-admin writer. One transaction: lock the row
// (no row -> ErrNoOrgEffectPolicy, nothing inserted), replay check on
// (org, idempotency key), revision check, validation, UPDATE of the catalogue
// columns plus entitlement_revision+1 (privacy columns and policy_revision
// untouched), fence active Spaces, event row, audit outbox row. An unchanged
// request commits nothing and reports changed=false.
func (r *Repository) ApplyOrgEntitlements(ctx context.Context, req ApplyOrgEntitlementsRequest) (ApplyOrgEntitlementsResult, error) {
	if r == nil || r.db == nil {
		return ApplyOrgEntitlementsResult{}, fmt.Errorf("space authority repository unavailable")
	}
	req.OrgID = strings.TrimSpace(req.OrgID)
	if req.OrgID == "" {
		return ApplyOrgEntitlementsResult{}, fmt.Errorf("organization is required")
	}
	if err := req.Actor.Validate(); err != nil {
		return ApplyOrgEntitlementsResult{}, err
	}
	keyLen := len(req.IdempotencyKey)
	if keyLen < OrgPolicyIdempotencyKeyMinLength || keyLen > OrgPolicyIdempotencyKeyMaxLength {
		return ApplyOrgEntitlementsResult{}, orgPolicyErr(OrgPolicyCodeInvalidRequest, "idempotency_key must be 8 to 128 characters",
			map[string]any{"field": "idempotency_key"})
	}
	if req.ExpectedEntitlementRevision < 1 {
		return ApplyOrgEntitlementsResult{}, orgPolicyErr(OrgPolicyCodeInvalidRequest, "expected_entitlement_revision is required",
			map[string]any{"field": "expected_entitlement_revision"})
	}
	changeKind := PolicyChangeEntitlementsSet
	target := req.Entitlements
	switch {
	case req.PresetID != "" && req.Entitlements != nil:
		return ApplyOrgEntitlementsResult{}, orgPolicyErr(OrgPolicyCodeInvalidRequest, "send exactly one of preset_id or entitlements",
			map[string]any{"field": "preset_id"})
	case req.PresetID != "":
		expanded, ok := PresetEntitlements(req.PresetID)
		if !ok {
			return ApplyOrgEntitlementsResult{}, orgPolicyErr(OrgPolicyCodeUnknownPreset, "unknown preset",
				map[string]any{"preset_id": req.PresetID})
		}
		target = expanded
		changeKind = PolicyChangePresetApplied
	case req.Entitlements == nil:
		return ApplyOrgEntitlementsResult{}, orgPolicyErr(OrgPolicyCodeInvalidRequest, "send exactly one of preset_id or entitlements",
			map[string]any{"field": "entitlements"})
	}
	digest := orgPolicyRequestDigest(req)

	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return ApplyOrgEntitlementsResult{}, fmt.Errorf("begin org Space policy write: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	var current EffectPolicy
	var policyRev, entRev int64
	err = scanEffectPolicy(tx.QueryRow(ctx, `
		SELECT `+effectPolicyColumnList()+`, policy_revision, entitlement_revision
		FROM space_effect_policies WHERE org_id=$1 FOR UPDATE`, req.OrgID),
		&current, &policyRev, &entRev)
	if errors.Is(err, pgx.ErrNoRows) {
		return ApplyOrgEntitlementsResult{}, ErrNoOrgEffectPolicy
	}
	if err != nil {
		return ApplyOrgEntitlementsResult{}, fmt.Errorf("read org Space policy: %w", err)
	}

	var recordedDigest string
	var recordedFenced int32
	err = tx.QueryRow(ctx, `
		SELECT COALESCE(request_digest, ''), spaces_fenced FROM space_effect_policy_events
		WHERE org_id=$1 AND idempotency_key=$2`, req.OrgID, req.IdempotencyKey).Scan(&recordedDigest, &recordedFenced)
	if err == nil {
		if recordedDigest != digest {
			return ApplyOrgEntitlementsResult{}, orgPolicyErr(OrgPolicyCodeIdempotencyConflict,
				"idempotency_key was already used for a different request", nil)
		}
		if err := tx.Commit(ctx); err != nil {
			return ApplyOrgEntitlementsResult{}, fmt.Errorf("commit org Space policy replay: %w", err)
		}
		view, err := r.GetOrgPolicy(ctx, req.OrgID)
		if err != nil {
			return ApplyOrgEntitlementsResult{}, err
		}
		return ApplyOrgEntitlementsResult{Changed: true, Replayed: true, SpacesFenced: int(recordedFenced), Policy: view}, nil
	}
	if !errors.Is(err, pgx.ErrNoRows) {
		return ApplyOrgEntitlementsResult{}, fmt.Errorf("check org Space policy replay: %w", err)
	}

	if req.ExpectedEntitlementRevision != entRev {
		return ApplyOrgEntitlementsResult{}, orgPolicyErr(OrgPolicyCodeRevisionConflict,
			"entitlements changed since they were read",
			map[string]any{"current_entitlement_revision": entRev})
	}
	if err := ValidateEntitlementChange(current, target); err != nil {
		return ApplyOrgEntitlementsResult{}, err
	}
	next := current
	for _, spec := range entitlementCatalog {
		spec.Set(&next, target[spec.Key])
	}
	changes := DiffEntitlements(current, next)
	if len(changes) == 0 {
		if err := tx.Commit(ctx); err != nil {
			return ApplyOrgEntitlementsResult{}, fmt.Errorf("commit unchanged org Space policy: %w", err)
		}
		view, err := r.GetOrgPolicy(ctx, req.OrgID)
		if err != nil {
			return ApplyOrgEntitlementsResult{}, err
		}
		return ApplyOrgEntitlementsResult{Policy: view}, nil
	}

	setParts := make([]string, 0, len(entitlementCatalog))
	args := []any{req.OrgID}
	for i, spec := range entitlementCatalog {
		setParts = append(setParts, fmt.Sprintf("%s=$%d", spec.Column, i+2))
		args = append(args, spec.Get(next))
	}
	var newPolicyRev, newEntRev int64
	if err := tx.QueryRow(ctx, `
		UPDATE space_effect_policies SET `+strings.Join(setParts, ", ")+`,
		entitlement_revision=entitlement_revision + 1, updated_at=NOW()
		WHERE org_id=$1
		RETURNING policy_revision, entitlement_revision`, args...).Scan(&newPolicyRev, &newEntRev); err != nil {
		return ApplyOrgEntitlementsResult{}, fmt.Errorf("update org Space policy entitlements: %w", err)
	}
	fenced, err := fenceActiveSpaces(ctx, tx, req.OrgID, false, true)
	if err != nil {
		return ApplyOrgEntitlementsResult{}, err
	}
	if _, err := insertPolicyEventTx(ctx, tx, r.policyAudit, policyEvent{
		OrgID: req.OrgID, IdempotencyKey: req.IdempotencyKey, RequestDigest: digest,
		Source: PolicyEventSourceOrgAdmin, Actor: req.Actor, ChangeKind: changeKind, PresetID: req.PresetID,
		Before: EntitlementValues(current), After: EntitlementValues(next), Changes: changes,
		PrivacyChanged: false, PolicyRevAfter: newPolicyRev, EntRevBefore: entRev, EntRevAfter: newEntRev,
		SpacesFenced: fenced,
	}, time.Now().UTC()); err != nil {
		return ApplyOrgEntitlementsResult{}, err
	}
	if err := tx.Commit(ctx); err != nil {
		return ApplyOrgEntitlementsResult{}, fmt.Errorf("commit org Space policy write: %w", err)
	}
	view, err := r.GetOrgPolicy(ctx, req.OrgID)
	if err != nil {
		return ApplyOrgEntitlementsResult{}, err
	}
	return ApplyOrgEntitlementsResult{Changed: true, SpacesFenced: int(fenced), Policy: view}, nil
}

// RecordOrgPolicyWriteDenied enqueues the content-free audit event for an org
// admin write Control refused (outcome denied, reason code only, no body).
func (r *Repository) RecordOrgPolicyWriteDenied(ctx context.Context, orgID string, actor EffectPolicyActor, reason string) error {
	if r == nil || r.db == nil {
		return fmt.Errorf("space authority repository unavailable")
	}
	if r.policyAudit == nil {
		return fmt.Errorf("space effect policy audit outbox is not wired")
	}
	if err := actor.Validate(); err != nil {
		return err
	}
	suffix := make([]byte, 12)
	if _, err := rand.Read(suffix); err != nil {
		return fmt.Errorf("generate audit event id: %w", err)
	}
	eventID := "space-effect-policy-denied:" + hex.EncodeToString(suffix)
	payload, err := json.Marshal(map[string]any{
		"event_id":    eventID,
		"occurred_at": time.Now().UTC().Format(time.RFC3339Nano),
		"org_id":      orgID,
		"user_id":     actor.auditUserID(),
		"actor_role":  actor.auditRole(),
		"plane":       "control",
		"producer":    "user-core",
		"event":       orgPolicyWriteDeniedAuditEventTag,
		"subject":     "org:" + orgID,
		"resource_id": orgID,
		"outcome":     "denied",
		"details":     map[string]any{"reason": reason},
	})
	if err != nil {
		return fmt.Errorf("encode write-denied audit event: %w", err)
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin write-denied audit: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if err := r.policyAudit.EnqueueAuditTx(ctx, tx, eventID, orgPolicyWriteDeniedAuditSubject, payload); err != nil {
		return fmt.Errorf("enqueue write-denied audit: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit write-denied audit: %w", err)
	}
	return nil
}
