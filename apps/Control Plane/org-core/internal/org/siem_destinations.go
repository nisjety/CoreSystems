package org

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net/netip"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

const (
	SIEMKindNDJSONWebhook = "ndjson_webhook"
	SIEMKindSplunkHEC     = "splunk_hec"

	// MaxSIEMDestinationsPerOrg is the cap the table trigger backs up.
	MaxSIEMDestinationsPerOrg = 3

	// SIEMExportEntitlementKey is a COMPUTED (never stored) entitlement, like
	// InteractiveZdrEntitlementKey: shipping audit events to a customer SIEM is
	// an enterprise feature (DEC-35), so the plan allowlist lives here.
	SIEMExportEntitlementKey = "feature.siem_export"

	// SIEMAuditEvent and SIEMAuditSubject are the durable audit row written for
	// every destination change; audit-core ingests the subject as a Control
	// producer event.
	SIEMAuditEvent   = "org_siem_destination_changed"
	SIEMAuditSubject = "verevon.audit.v2.control.org-core." + SIEMAuditEvent

	siemSecretMinBytes = 16
	siemSecretMaxBytes = 512
	siemURLMaxBytes    = 2048
	siemFilterMaxItems = 64
)

var (
	// ErrSIEMDestinationNotFound is returned when the destination does not exist
	// in the organisation. Another organisation's id is indistinguishable.
	ErrSIEMDestinationNotFound = errors.New("siem destination not found")
	// ErrSIEMDestinationLimit is returned when the organisation already has
	// MaxSIEMDestinationsPerOrg destinations.
	ErrSIEMDestinationLimit = errors.New("siem destination limit reached")

	siemExportPlans = map[string]bool{"enterprise": true}

	siemFilterItemPattern = regexp.MustCompile(`^[A-Za-z0-9._:-]{1,128}$`)

	// Ranges net/netip does not already classify. The exporter's dial-time guard
	// (S8) is the authoritative SSRF check; this only keeps an obviously internal
	// literal from being stored.
	siemBlockedPrefixes = []netip.Prefix{
		netip.MustParsePrefix("0.0.0.0/8"),
		netip.MustParsePrefix("100.64.0.0/10"),
		netip.MustParsePrefix("192.0.0.0/24"),
		netip.MustParsePrefix("198.18.0.0/15"),
		netip.MustParsePrefix("240.0.0.0/4"),
		netip.MustParsePrefix("::/96"),
		netip.MustParsePrefix("64:ff9b::/96"),
	}
)

// PlanAllowsSIEMExport reports whether a plan may configure SIEM destinations.
func PlanAllowsSIEMExport(plan string) bool {
	return siemExportPlans[strings.ToLower(strings.TrimSpace(plan))]
}

// SIEMEventFilter narrows what the exporter sends; empty means everything.
type SIEMEventFilter struct {
	Events []string `json:"events,omitempty"`
	Planes []string `json:"planes,omitempty"`
}

// SIEMDestination is the API view of a destination. The secret is write-only:
// SecretSet says one exists and nothing here can carry it.
type SIEMDestination struct {
	ID          string          `json:"id"`
	OrgID       string          `json:"org_id"`
	Kind        string          `json:"kind"`
	URL         string          `json:"url"`
	SecretSet   bool            `json:"secret_set"`
	Enabled     bool            `json:"enabled"`
	EventFilter SIEMEventFilter `json:"event_filter"`
	Revision    int64           `json:"revision"`
	CreatedBy   string          `json:"created_by"`
	CreatedAt   time.Time       `json:"created_at"`
	UpdatedAt   time.Time       `json:"updated_at"`
}

// SIEMExporterDestination is what the exporter reads: the one place the
// unsealed secret leaves org-core, behind org:siem:read:any.
type SIEMExporterDestination struct {
	ID          string          `json:"id"`
	OrgID       string          `json:"org_id"`
	Kind        string          `json:"kind"`
	URL         string          `json:"url"`
	Secret      string          `json:"secret"`
	EventFilter SIEMEventFilter `json:"event_filter"`
	Revision    int64           `json:"revision"`
	UpdatedAt   time.Time       `json:"updated_at"`
}

type CreateSIEMDestinationRequest struct {
	Kind        string           `json:"kind"`
	URL         string           `json:"url"`
	Secret      string           `json:"secret"`
	Enabled     *bool            `json:"enabled,omitempty"`
	EventFilter *SIEMEventFilter `json:"event_filter,omitempty"`
}

// UpdateSIEMDestinationRequest changes only the fields that are present. The
// kind cannot change: a webhook secret is not a Splunk token.
type UpdateSIEMDestinationRequest struct {
	ExpectedRevision int64            `json:"expected_revision"`
	URL              *string          `json:"url,omitempty"`
	Secret           *string          `json:"secret,omitempty"`
	Enabled          *bool            `json:"enabled,omitempty"`
	EventFilter      *SIEMEventFilter `json:"event_filter,omitempty"`
}

// SIEMValidationError lists every problem found, never a value that was sent.
type SIEMValidationError struct {
	Errors []string
}

func (e *SIEMValidationError) Error() string {
	return "invalid siem destination: " + strings.Join(e.Errors, "; ")
}

// SIEMRevisionConflictError means the write named a revision that is no longer
// current; Current is the revision to re-read and retry from.
type SIEMRevisionConflictError struct {
	Current int64
}

func (e *SIEMRevisionConflictError) Error() string {
	return fmt.Sprintf("siem destination revision conflict: current revision is %d", e.Current)
}

// EnableSIEMDestinations switches the feature on by giving the service the key
// that seals destination secrets. Until it is called every SIEM method refuses
// and the computed entitlement is not reported, so a service without it behaves
// exactly as before.
func (s *Service) EnableSIEMDestinations(sealer *SIEMSealer) {
	s.siemSealer = sealer
}

// SIEMDestinationsEnabled reports whether EnableSIEMDestinations was called.
func (s *Service) SIEMDestinationsEnabled() bool {
	return s.siemSealer != nil
}

func (s *Service) requireSIEM() error {
	if s.siemSealer == nil {
		return ErrSIEMSealUnavailable
	}
	return nil
}

func (s *Service) ListSIEMDestinations(ctx context.Context, orgID string) ([]SIEMDestination, error) {
	if err := s.requireSIEM(); err != nil {
		return nil, err
	}
	return s.repo.listSIEMDestinations(ctx, orgID)
}

// CreateSIEMDestination validates, seals the secret and stores the destination
// with its audit intent in one transaction.
func (s *Service) CreateSIEMDestination(ctx context.Context, orgID, actorID, actorRole string, req CreateSIEMDestinationRequest) (*SIEMDestination, error) {
	if err := s.requireSIEM(); err != nil {
		return nil, err
	}
	var problems []string
	if req.Kind != SIEMKindNDJSONWebhook && req.Kind != SIEMKindSplunkHEC {
		problems = append(problems, "kind must be ndjson_webhook or splunk_hec")
	}
	problems = append(problems, validateSIEMURL(req.URL)...)
	problems = append(problems, validateSIEMSecret(req.Secret)...)
	filter := SIEMEventFilter{}
	if req.EventFilter != nil {
		filter = *req.EventFilter
		problems = append(problems, validateSIEMFilter(filter)...)
	}
	if len(problems) > 0 {
		return nil, &SIEMValidationError{Errors: problems}
	}

	id, err := newSIEMDestinationID()
	if err != nil {
		return nil, err
	}
	sealed, err := s.siemSealer.Seal(orgID, id, []byte(req.Secret))
	if err != nil {
		return nil, err
	}
	enabled := true
	if req.Enabled != nil {
		enabled = *req.Enabled
	}
	return s.repo.createSIEMDestination(ctx, siemInsert{
		ID: id, OrgID: orgID, Kind: req.Kind, URL: req.URL, Sealed: sealed, KeyID: s.siemSealer.KeyID(),
		Enabled: enabled, Filter: normalizeSIEMFilter(filter), ActorID: actorID, ActorRole: actorRole,
	})
}

// UpdateSIEMDestination applies the present fields if ExpectedRevision is still
// current. Turning a destination on is plan-gated like creating one; turning it
// off, editing it and deleting it never are, so a lapsed plan can always be
// wound down.
func (s *Service) UpdateSIEMDestination(ctx context.Context, orgID, destinationID, actorID, actorRole string, req UpdateSIEMDestinationRequest) (*SIEMDestination, error) {
	if err := s.requireSIEM(); err != nil {
		return nil, err
	}
	var problems []string
	if req.ExpectedRevision < 1 {
		problems = append(problems, "expected_revision is required")
	}
	if req.URL == nil && req.Secret == nil && req.Enabled == nil && req.EventFilter == nil {
		problems = append(problems, "at least one of url, secret, enabled or event_filter is required")
	}
	update := siemUpdate{
		OrgID: orgID, ID: destinationID, ExpectedRevision: req.ExpectedRevision,
		Enabled: req.Enabled, ActorID: actorID, ActorRole: actorRole,
	}
	if req.URL != nil {
		problems = append(problems, validateSIEMURL(*req.URL)...)
		update.URL = req.URL
	}
	if req.Secret != nil {
		problems = append(problems, validateSIEMSecret(*req.Secret)...)
	}
	if req.EventFilter != nil {
		problems = append(problems, validateSIEMFilter(*req.EventFilter)...)
		normalized := normalizeSIEMFilter(*req.EventFilter)
		update.Filter = &normalized
	}
	if len(problems) > 0 {
		return nil, &SIEMValidationError{Errors: problems}
	}
	if req.Secret != nil {
		sealed, err := s.siemSealer.Seal(orgID, destinationID, []byte(*req.Secret))
		if err != nil {
			return nil, err
		}
		update.Sealed, update.KeyID = sealed, s.siemSealer.KeyID()
	}
	return s.repo.updateSIEMDestination(ctx, update)
}

func (s *Service) DeleteSIEMDestination(ctx context.Context, orgID, destinationID, actorID, actorRole string) error {
	if err := s.requireSIEM(); err != nil {
		return err
	}
	return s.repo.deleteSIEMDestination(ctx, orgID, destinationID, actorID, actorRole)
}

// ListSIEMDestinationsForExport returns every enabled destination of every
// organisation whose plan still allows SIEM export, with the secret unsealed.
// A row whose secret does not open (a rotated or lost key) is left out and
// logged by id: handing the exporter a destination it cannot authenticate to
// would deliver unsigned.
func (s *Service) ListSIEMDestinationsForExport(ctx context.Context, updatedSince *time.Time) ([]SIEMExporterDestination, error) {
	if err := s.requireSIEM(); err != nil {
		return nil, err
	}
	rows, err := s.repo.listSIEMDestinationsForExport(ctx, updatedSince)
	if err != nil {
		return nil, err
	}
	out := make([]SIEMExporterDestination, 0, len(rows))
	for _, row := range rows {
		secret, err := s.siemSealer.Open(row.keyID, row.OrgID, row.ID, row.sealed)
		if err != nil {
			log.Printf("siem export: skipping destination %s of org %s: %v", row.ID, row.OrgID, err)
			continue
		}
		row.Secret = string(secret)
		out = append(out, row.SIEMExporterDestination)
	}
	return out, nil
}

// FlushSIEMAuditOutbox publishes pending destination-change audit intents, the
// same loop FlushGDPRAuditOutbox runs for erasure.
func (s *Service) FlushSIEMAuditOutbox(ctx context.Context, limit int) (GDPRAuditFlushResult, error) {
	if limit < 1 || limit > 1000 {
		return GDPRAuditFlushResult{}, fmt.Errorf("SIEM audit outbox limit must be between 1 and 1000")
	}
	if s.auditPublisher == nil {
		return GDPRAuditFlushResult{}, fmt.Errorf("SIEM audit publisher is unavailable")
	}
	if s.repo == nil || s.repo.pool == nil {
		return GDPRAuditFlushResult{}, fmt.Errorf("SIEM audit outbox repository is unavailable")
	}
	rows, err := s.repo.claimSIEMAuditOutbox(ctx, limit)
	if err != nil {
		return GDPRAuditFlushResult{}, err
	}
	result := GDPRAuditFlushResult{}
	var publishErrors error
	for _, row := range rows {
		if err := s.auditPublisher.PublishAudit(ctx, row.Subject, row.EventID, row.Payload); err != nil {
			deadLettered, markErr := s.repo.markSIEMAuditPublishFailed(ctx, row.EventID, err)
			if deadLettered {
				result.DeadLettered++
			}
			publishErrors = errors.Join(publishErrors, err, markErr)
			continue
		}
		if err := s.repo.markSIEMAuditPublished(ctx, row.EventID); err != nil {
			publishErrors = errors.Join(publishErrors, err)
			continue
		}
		result.Published++
	}
	return result, publishErrors
}

// --- validation ---

// validateSIEMURL enforces the stored-URL rules: https only, no credentials, no
// fragment, no internal literal. Returned strings never echo the URL.
func validateSIEMURL(raw string) []string {
	if raw == "" {
		return []string{"url is required"}
	}
	if len(raw) > siemURLMaxBytes {
		return []string{"url is longer than 2048 bytes"}
	}
	if raw != strings.TrimSpace(raw) || strings.ContainsAny(raw, " \t\r\n\\") {
		return []string{"url must not contain whitespace or backslashes"}
	}
	parsed, err := url.Parse(raw)
	if err != nil {
		return []string{"url is not a valid URL"}
	}
	var problems []string
	if parsed.Scheme != "https" {
		problems = append(problems, "url must use https")
	}
	if parsed.User != nil {
		problems = append(problems, "url must not contain credentials")
	}
	if parsed.Fragment != "" || strings.Contains(raw, "#") {
		problems = append(problems, "url must not contain a fragment")
	}
	if port := parsed.Port(); port != "" {
		if n, err := strconv.Atoi(port); err != nil || n < 1 || n > 65535 {
			problems = append(problems, "url port is out of range")
		}
	}
	host := strings.TrimSuffix(strings.ToLower(parsed.Hostname()), ".")
	if host == "" {
		problems = append(problems, "url must have a host")
		return problems
	}
	if siemInternalHostname(host) {
		problems = append(problems, "url host must not be an internal name")
		return problems
	}
	if addr, err := netip.ParseAddr(host); err == nil {
		if addr.Zone() != "" || siemBlockedAddr(addr) {
			problems = append(problems, "url host must not be a private, loopback or link-local address")
		}
		return problems
	}
	// A last label that is all digits is an IPv4 spelling the resolver may read
	// (2130706433, 0x7f.1) rather than a name; no public suffix is numeric.
	labels := strings.Split(host, ".")
	last := labels[len(labels)-1]
	if strings.HasPrefix(last, "0x") || strings.Trim(last, "0123456789") == "" {
		problems = append(problems, "url host must be a hostname or a public IP address")
	}
	return problems
}

func siemInternalHostname(host string) bool {
	if host == "localhost" {
		return true
	}
	for _, suffix := range []string{".localhost", ".local", ".internal", ".localdomain"} {
		if strings.HasSuffix(host, suffix) {
			return true
		}
	}
	return false
}

func siemBlockedAddr(addr netip.Addr) bool {
	addr = addr.Unmap()
	if addr.IsLoopback() || addr.IsPrivate() || addr.IsLinkLocalUnicast() || addr.IsLinkLocalMulticast() ||
		addr.IsMulticast() || addr.IsUnspecified() || addr.IsInterfaceLocalMulticast() {
		return true
	}
	for _, prefix := range siemBlockedPrefixes {
		if prefix.Contains(addr) {
			return true
		}
	}
	return false
}

func validateSIEMSecret(secret string) []string {
	if len(secret) < siemSecretMinBytes || len(secret) > siemSecretMaxBytes {
		return []string{fmt.Sprintf("secret must be %d to %d bytes", siemSecretMinBytes, siemSecretMaxBytes)}
	}
	if secret != strings.TrimSpace(secret) {
		return []string{"secret must not start or end with whitespace"}
	}
	for _, b := range []byte(secret) {
		if b < 0x20 || b == 0x7f {
			return []string{"secret must not contain control characters"}
		}
	}
	return nil
}

func validateSIEMFilter(filter SIEMEventFilter) []string {
	var problems []string
	for name, items := range map[string][]string{"events": filter.Events, "planes": filter.Planes} {
		if len(items) > siemFilterMaxItems {
			problems = append(problems, fmt.Sprintf("event_filter.%s has more than %d entries", name, siemFilterMaxItems))
			continue
		}
		for _, item := range items {
			if !siemFilterItemPattern.MatchString(item) {
				problems = append(problems, fmt.Sprintf("event_filter.%s has an entry that is not 1 to 128 of letters, digits, '.', '_', ':' or '-'", name))
				break
			}
		}
	}
	return problems
}

// normalizeSIEMFilter drops duplicates and empty lists so equal filters store
// as equal JSON.
func normalizeSIEMFilter(filter SIEMEventFilter) SIEMEventFilter {
	return SIEMEventFilter{Events: dedupeStrings(filter.Events), Planes: dedupeStrings(filter.Planes)}
}

func dedupeStrings(items []string) []string {
	if len(items) == 0 {
		return nil
	}
	seen := make(map[string]struct{}, len(items))
	out := make([]string, 0, len(items))
	for _, item := range items {
		if _, dup := seen[item]; dup {
			continue
		}
		seen[item] = struct{}{}
		out = append(out, item)
	}
	return out
}

func newSIEMDestinationID() (string, error) {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "", fmt.Errorf("siem destination id: %w", err)
	}
	b[6] = b[6]&0x0f | 0x40
	b[8] = b[8]&0x3f | 0x80
	h := hex.EncodeToString(b[:])
	return h[0:8] + "-" + h[8:12] + "-" + h[12:16] + "-" + h[16:20] + "-" + h[20:], nil
}

// siemURLHost is the only part of a destination URL that reaches the audit
// trail: no path, no query, no port.
func siemURLHost(raw string) string {
	parsed, err := url.Parse(raw)
	if err != nil {
		return ""
	}
	return strings.ToLower(parsed.Hostname())
}

// --- repository ---

type siemInsert struct {
	ID, OrgID, Kind, URL string
	Sealed               []byte
	KeyID                string
	Enabled              bool
	Filter               SIEMEventFilter
	ActorID, ActorRole   string
}

type siemUpdate struct {
	OrgID, ID          string
	ExpectedRevision   int64
	URL                *string
	Sealed             []byte
	KeyID              string
	Enabled            *bool
	Filter             *SIEMEventFilter
	ActorID, ActorRole string
}

// siemExportRow carries the ciphertext next to the exporter view so the
// service can open it; the ciphertext never serialises.
type siemExportRow struct {
	SIEMExporterDestination
	sealed []byte
	keyID  string
}

const siemPublicColumns = `id, org_id, kind, url, enabled, event_filter, revision, created_by, created_at, updated_at`

func scanSIEMDestination(row orgScanner) (SIEMDestination, error) {
	var d SIEMDestination
	var filter []byte
	if err := row.Scan(&d.ID, &d.OrgID, &d.Kind, &d.URL, &d.Enabled, &filter, &d.Revision, &d.CreatedBy, &d.CreatedAt, &d.UpdatedAt); err != nil {
		return SIEMDestination{}, err
	}
	d.SecretSet = true
	if len(filter) > 0 {
		if err := json.Unmarshal(filter, &d.EventFilter); err != nil {
			return SIEMDestination{}, fmt.Errorf("decode siem event filter: %w", err)
		}
	}
	return d, nil
}

func marshalSIEMFilter(filter SIEMEventFilter) ([]byte, error) {
	encoded, err := json.Marshal(filter)
	if err != nil {
		return nil, fmt.Errorf("encode siem event filter: %w", err)
	}
	return encoded, nil
}

func (r *Repository) listSIEMDestinations(ctx context.Context, orgID string) ([]SIEMDestination, error) {
	out := []SIEMDestination{}
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, `SELECT `+siemPublicColumns+` FROM siem_destinations WHERE org_id = $1 ORDER BY created_at, id`, orgID)
		if err != nil {
			return fmt.Errorf("list siem destinations: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			d, err := scanSIEMDestination(rows)
			if err != nil {
				return fmt.Errorf("scan siem destination: %w", err)
			}
			out = append(out, d)
		}
		return rows.Err()
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

func (r *Repository) createSIEMDestination(ctx context.Context, in siemInsert) (*SIEMDestination, error) {
	filterJSON, err := marshalSIEMFilter(in.Filter)
	if err != nil {
		return nil, err
	}
	var created SIEMDestination
	err = r.db.WithOrgScope(ctx, in.OrgID, func(tx pgx.Tx) error {
		if err := lockOrganizationLifecycle(ctx, tx, in.OrgID); err != nil {
			return err
		}
		var plan string
		if err := tx.QueryRow(ctx, `SELECT plan FROM organizations WHERE id = $1 AND deleted_at IS NULL`, in.OrgID).Scan(&plan); err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrNotFound
			}
			return fmt.Errorf("read organization plan: %w", err)
		}
		if !PlanAllowsSIEMExport(plan) {
			return ErrPlanUpgradeRequired
		}
		var count int
		if err := tx.QueryRow(ctx, `SELECT COUNT(*) FROM siem_destinations WHERE org_id = $1`, in.OrgID).Scan(&count); err != nil {
			return fmt.Errorf("count siem destinations: %w", err)
		}
		if count >= MaxSIEMDestinationsPerOrg {
			return ErrSIEMDestinationLimit
		}
		row := tx.QueryRow(ctx, `
INSERT INTO siem_destinations (id, org_id, kind, url, secret_ciphertext, secret_key_id, enabled, event_filter, created_by)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
RETURNING `+siemPublicColumns,
			in.ID, in.OrgID, in.Kind, in.URL, in.Sealed, in.KeyID, in.Enabled, filterJSON, in.ActorID)
		created, err = scanSIEMDestination(row)
		if err != nil {
			return fmt.Errorf("insert siem destination: %w", err)
		}
		return enqueueSIEMAudit(ctx, tx, siemAuditChange{
			Action: "created", Destination: created, ActorID: in.ActorID, ActorRole: in.ActorRole,
			SecretRotated: true, EventRevision: created.Revision,
		})
	})
	if err != nil {
		return nil, err
	}
	return &created, nil
}

func (r *Repository) updateSIEMDestination(ctx context.Context, in siemUpdate) (*SIEMDestination, error) {
	var updated SIEMDestination
	err := r.db.WithOrgScope(ctx, in.OrgID, func(tx pgx.Tx) error {
		current, err := scanSIEMDestination(tx.QueryRow(ctx,
			`SELECT `+siemPublicColumns+` FROM siem_destinations WHERE id = $1 AND org_id = $2 FOR UPDATE`, in.ID, in.OrgID))
		if err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrSIEMDestinationNotFound
			}
			return fmt.Errorf("read siem destination: %w", err)
		}
		if current.Revision != in.ExpectedRevision {
			return &SIEMRevisionConflictError{Current: current.Revision}
		}
		if in.Enabled != nil && *in.Enabled && !current.Enabled {
			var plan string
			if err := tx.QueryRow(ctx, `SELECT plan FROM organizations WHERE id = $1 AND deleted_at IS NULL`, in.OrgID).Scan(&plan); err != nil {
				if errors.Is(err, pgx.ErrNoRows) {
					return ErrNotFound
				}
				return fmt.Errorf("read organization plan: %w", err)
			}
			if !PlanAllowsSIEMExport(plan) {
				return ErrPlanUpgradeRequired
			}
		}

		target, enabled, filter := current.URL, current.Enabled, current.EventFilter
		if in.URL != nil {
			target = *in.URL
		}
		if in.Enabled != nil {
			enabled = *in.Enabled
		}
		if in.Filter != nil {
			filter = *in.Filter
		}
		filterJSON, err := marshalSIEMFilter(filter)
		if err != nil {
			return err
		}
		// COALESCE keeps the stored ciphertext when no new secret was sent.
		row := tx.QueryRow(ctx, `
UPDATE siem_destinations
SET url = $3, enabled = $4, event_filter = $5,
    secret_ciphertext = COALESCE($6, secret_ciphertext),
    secret_key_id = COALESCE($7, secret_key_id),
    revision = revision + 1, updated_at = NOW()
WHERE id = $1 AND org_id = $2
RETURNING `+siemPublicColumns,
			in.ID, in.OrgID, target, enabled, filterJSON, nullableBytes(in.Sealed), nullableString(in.KeyID))
		updated, err = scanSIEMDestination(row)
		if err != nil {
			return fmt.Errorf("update siem destination: %w", err)
		}
		return enqueueSIEMAudit(ctx, tx, siemAuditChange{
			Action: "updated", Destination: updated, ActorID: in.ActorID, ActorRole: in.ActorRole,
			SecretRotated: in.Sealed != nil, EventRevision: updated.Revision,
		})
	})
	if err != nil {
		return nil, err
	}
	return &updated, nil
}

func (r *Repository) deleteSIEMDestination(ctx context.Context, orgID, id, actorID, actorRole string) error {
	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		current, err := scanSIEMDestination(tx.QueryRow(ctx,
			`SELECT `+siemPublicColumns+` FROM siem_destinations WHERE id = $1 AND org_id = $2 FOR UPDATE`, id, orgID))
		if err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrSIEMDestinationNotFound
			}
			return fmt.Errorf("read siem destination: %w", err)
		}
		if _, err := tx.Exec(ctx, `DELETE FROM siem_destinations WHERE id = $1 AND org_id = $2`, id, orgID); err != nil {
			return fmt.Errorf("delete siem destination: %w", err)
		}
		return enqueueSIEMAudit(ctx, tx, siemAuditChange{
			Action: "deleted", Destination: current, ActorID: actorID, ActorRole: actorRole,
			EventRevision: current.Revision + 1,
		})
	})
}

// listSIEMDestinationsForExport runs unscoped on the pool: the exporter serves
// every tenant and has no acting user, like the other /internal cross-org
// reads. The join leaves out organisations that were deleted or whose plan no
// longer allows export.
func (r *Repository) listSIEMDestinationsForExport(ctx context.Context, updatedSince *time.Time) ([]siemExportRow, error) {
	plans := make([]string, 0, len(siemExportPlans))
	for plan := range siemExportPlans {
		plans = append(plans, plan)
	}
	rows, err := r.pool.Query(ctx, `
SELECT d.id, d.org_id, d.kind, d.url, d.secret_ciphertext, d.secret_key_id, d.event_filter, d.revision, d.updated_at
FROM siem_destinations d
JOIN organizations o ON o.id = d.org_id
WHERE d.enabled
  AND o.deleted_at IS NULL
  AND lower(o.plan) = ANY($1)
  AND ($2::timestamptz IS NULL OR d.updated_at > $2)
ORDER BY d.updated_at, d.id`, plans, updatedSince)
	if err != nil {
		return nil, fmt.Errorf("list siem destinations for export: %w", err)
	}
	defer rows.Close()
	out := []siemExportRow{}
	for rows.Next() {
		var row siemExportRow
		var filter []byte
		if err := rows.Scan(&row.ID, &row.OrgID, &row.Kind, &row.URL, &row.sealed, &row.keyID, &filter, &row.Revision, &row.UpdatedAt); err != nil {
			return nil, fmt.Errorf("scan siem export row: %w", err)
		}
		if len(filter) > 0 {
			if err := json.Unmarshal(filter, &row.EventFilter); err != nil {
				return nil, fmt.Errorf("decode siem event filter: %w", err)
			}
		}
		out = append(out, row)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("list siem destinations for export: %w", err)
	}
	return out, nil
}

func nullableBytes(b []byte) any {
	if b == nil {
		return nil
	}
	return b
}

func nullableString(s string) any {
	if s == "" {
		return nil
	}
	return s
}

// --- audit outbox ---

type siemAuditChange struct {
	Action        string
	Destination   SIEMDestination
	ActorID       string
	ActorRole     string
	SecretRotated bool
	EventRevision int64
}

// siemAuditPayload is content-free by construction: the destination's host (no
// port, path or query), its kind and state. Neither the secret nor the rest of
// the URL has a path into it.
func siemAuditPayload(change siemAuditChange, occurredAt time.Time) (eventID string, payload map[string]any) {
	d := change.Destination
	eventID = fmt.Sprintf("siem:org-core:%s:%d", d.ID, change.EventRevision)
	return eventID, map[string]any{
		"event_id": eventID, "occurred_at": occurredAt.UTC().Format(time.RFC3339Nano),
		"org_id": d.OrgID, "user_id": change.ActorID, "actor_role": change.ActorRole,
		"plane": "control", "producer": "org-core", "event": SIEMAuditEvent,
		"subject": "siem_destination:" + d.ID, "resource_id": d.ID, "outcome": "ok",
		"details": map[string]any{
			"action": change.Action, "kind": d.Kind, "host": siemURLHost(d.URL),
			"enabled": d.Enabled, "revision": change.EventRevision, "secret_rotated": change.SecretRotated,
		},
	}
}

func enqueueSIEMAudit(ctx context.Context, tx pgx.Tx, change siemAuditChange) error {
	if strings.TrimSpace(change.ActorID) == "" || strings.TrimSpace(change.ActorRole) == "" {
		return fmt.Errorf("siem audit actor and role are required")
	}
	eventID, payload := siemAuditPayload(change, time.Now())
	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("encode siem audit payload: %w", err)
	}
	if _, err := tx.Exec(ctx, `
INSERT INTO organization_siem_audit_outbox (event_id, org_id, subject, payload)
VALUES ($1, $2, $3, $4)`, eventID, change.Destination.OrgID, SIEMAuditSubject, body); err != nil {
		return fmt.Errorf("record siem audit outbox: %w", err)
	}
	return nil
}

func (r *Repository) claimSIEMAuditOutbox(ctx context.Context, limit int) ([]GDPRAuditOutboxRow, error) {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin SIEM audit outbox claim: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	rows, err := tx.Query(ctx, `
WITH claimed AS (
  SELECT event_id
  FROM organization_siem_audit_outbox
  WHERE published_at IS NULL
    AND dead_lettered_at IS NULL
    AND attempts < $2
    AND next_attempt_at <= NOW()
    AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '1 minute')
  ORDER BY next_attempt_at, created_at, event_id
  LIMIT $1
  FOR UPDATE SKIP LOCKED
)
UPDATE organization_siem_audit_outbox o
SET processing_at = NOW(), updated_at = NOW()
FROM claimed
WHERE o.event_id = claimed.event_id
RETURNING o.event_id, o.org_id, o.subject, o.payload, o.attempts`, limit, GDPRAuditMaxAttempts)
	if err != nil {
		return nil, fmt.Errorf("claim SIEM audit outbox: %w", err)
	}
	defer rows.Close()
	claimed := make([]GDPRAuditOutboxRow, 0)
	for rows.Next() {
		var row GDPRAuditOutboxRow
		var body []byte
		if err := rows.Scan(&row.EventID, &row.OrgID, &row.Subject, &body, &row.Attempts); err != nil {
			return nil, fmt.Errorf("scan SIEM audit outbox: %w", err)
		}
		if err := json.Unmarshal(body, &row.Payload); err != nil {
			return nil, fmt.Errorf("decode SIEM audit outbox payload: %w", err)
		}
		claimed = append(claimed, row)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate SIEM audit outbox: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit SIEM audit outbox claim: %w", err)
	}
	return claimed, nil
}

func (r *Repository) markSIEMAuditPublished(ctx context.Context, eventID string) error {
	result, err := r.pool.Exec(ctx, `
UPDATE organization_siem_audit_outbox
SET published_at = NOW(), processing_at = NULL, attempts = attempts + 1,
    last_error = NULL, updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL`, eventID)
	if err != nil {
		return fmt.Errorf("mark SIEM audit published: %w", err)
	}
	if result.RowsAffected() != 1 {
		return fmt.Errorf("SIEM audit acknowledgement did not match a pending event")
	}
	return nil
}

func (r *Repository) markSIEMAuditPublishFailed(ctx context.Context, eventID string, publishErr error) (bool, error) {
	message := "unknown audit publish failure"
	if publishErr != nil {
		message = publishErr.Error()
	}
	if len(message) > 2048 {
		message = message[:2048]
	}
	var deadLettered bool
	err := r.pool.QueryRow(ctx, `
UPDATE organization_siem_audit_outbox
SET attempts = attempts + 1,
    processing_at = NULL,
    next_attempt_at = NOW() + LEAST(
      INTERVAL '5 minutes',
      INTERVAL '1 second' * power(2, LEAST(attempts, 8))
    ),
    dead_lettered_at = CASE WHEN attempts + 1 >= $2 THEN NOW() ELSE NULL END,
    last_error = $3,
    updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL
RETURNING dead_lettered_at IS NOT NULL`, eventID, GDPRAuditMaxAttempts, message).Scan(&deadLettered)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return false, fmt.Errorf("SIEM audit failure did not match a pending event")
		}
		return false, fmt.Errorf("record SIEM audit publish failure: %w", err)
	}
	return deadLettered, nil
}
