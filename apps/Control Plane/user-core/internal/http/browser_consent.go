package http

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"net/http"
	"os"
	"sort"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// W3 E3.2-e3: the browser_consent settings category (OD13 / DEC-19).
//
// Control is the policy of record for a person's per-origin browser consent;
// the broker enforces at grant mint and Quarry stays the final enforcer. This
// file only stores and validates the person's decisions:
//
//   - one decision per (startHost, scope, conversationId, turnId);
//   - scope turn | conversation | always, for origin consent only (kind
//     "origin" is the only kind this category accepts);
//   - a decision fences the start host plus at most 8 user-listed hosts
//     (the broker/Quarry cap of 32 is theirs, not ours);
//   - every host is a canonical DNS name; IP literals and private/loopback
//     names are never grantable;
//   - ZDR orgs: the turn-only rule is enforced in Quarry and the broker, not
//     here (Control does not know the run's ZDR posture at this route).
//
// Off by default: USER_CORE_BROWSER_CONSENT=true turns the routes on; off, they
// answer 404 browser_consent_disabled.

const (
	browserConsentFlagEnv = "USER_CORE_BROWSER_CONSENT"

	browserConsentSchemaVersion = 1
	// browserConsentMaxListedHosts is the OD13 fence: start host + at most 8.
	browserConsentMaxListedHosts = 8
	// browserConsentMaxDecisions bounds one person's stored document. When full,
	// the oldest turn-scoped then conversation-scoped decisions are evicted;
	// "always" decisions are never evicted silently.
	browserConsentMaxDecisions = 200
	browserConsentMaxIDLength  = 128

	browserConsentKindOrigin = "origin"

	browserConsentDecisionAllow = "allow"
	browserConsentDecisionDeny  = "deny"

	browserConsentScopeTurn         = "turn"
	browserConsentScopeConversation = "conversation"
	browserConsentScopeAlways       = "always"

	browserConsentAuditDecided = "browser_consent.origin_decided"
	browserConsentAuditRevoked = "browser_consent.origin_revoked"
)

// BrowserConsentStore is the user-scoped storage the routes need.
// *users.Repository satisfies it.
type BrowserConsentStore interface {
	GetBrowserConsentSettings(ctx context.Context, userID string) (*users.UserSettings, error)
	MutateBrowserConsentSettings(ctx context.Context, userID string, mutate users.BrowserConsentMutation) (*users.UserSettings, error)
}

// browserConsentDecision is one stored per-origin decision.
type browserConsentDecision struct {
	Kind           string    `json:"kind"`
	StartHost      string    `json:"startHost"`
	Hosts          []string  `json:"hosts"`
	Decision       string    `json:"decision"`
	Scope          string    `json:"scope"`
	ConversationID string    `json:"conversationId,omitempty"`
	TurnID         string    `json:"turnId,omitempty"`
	DecidedAt      time.Time `json:"decidedAt"`
}

// browserConsentDocument is the whole browser_consent settings value.
type browserConsentDocument struct {
	SchemaVersion int                      `json:"schemaVersion"`
	Origins       []browserConsentDecision `json:"origins"`
}

// browserConsentDecisionRequest is the PUT body.
type browserConsentDecisionRequest struct {
	Kind           string   `json:"kind"`
	StartHost      string   `json:"startHost"`
	Hosts          []string `json:"hosts"`
	Decision       string   `json:"decision"`
	Scope          string   `json:"scope"`
	ConversationID string   `json:"conversationId"`
	TurnID         string   `json:"turnId"`
}

// browserConsentError is a typed refusal that crosses the store transaction
// unwrapped and becomes {"error":{code,message,details}}.
type browserConsentError struct {
	status  int
	code    string
	message string
	details map[string]any
}

func (e *browserConsentError) Error() string { return e.code + ": " + e.message }

func newBrowserConsentError(status int, code, message string, details map[string]any) *browserConsentError {
	return &browserConsentError{status: status, code: code, message: message, details: details}
}

func invalidBrowserConsent(code, message string, details map[string]any) *browserConsentError {
	return newBrowserConsentError(http.StatusUnprocessableEntity, code, message, details)
}

// browserConsentHandlers carries the flag and store captured at route
// registration, so the feature needs no new Server field.
type browserConsentHandlers struct {
	enabled bool
	store   BrowserConsentStore
	now     func() time.Time
}

func browserConsentEnabledFromEnv() bool {
	return strings.EqualFold(strings.TrimSpace(os.Getenv(browserConsentFlagEnv)), "true")
}

// browserConsentStoreFor returns the user-scoped repository over the server's
// database handle, or nil (routes then answer 503) when it is not wired.
func browserConsentStoreFor(s *Server) BrowserConsentStore {
	if s == nil || s.aclRepo == nil {
		return nil
	}
	repo := s.aclRepo.BrowserConsentRepository()
	if repo == nil {
		return nil
	}
	return repo
}

func init() {
	registerFeatureRoute(func(s *Server, v1 *gin.RouterGroup, _ *gin.RouterGroup) {
		h := &browserConsentHandlers{
			enabled: browserConsentEnabledFromEnv(),
			store:   browserConsentStoreFor(s),
			now:     time.Now,
		}
		h.register(v1)
	})
}

func (h *browserConsentHandlers) register(v1 *gin.RouterGroup) {
	group := v1.Group("/settings/browser-consent")
	// GET /api/v1/settings/browser-consent - the person's stored decisions
	group.GET("", h.get)
	// PUT /api/v1/settings/browser-consent/origins - record one decision
	group.PUT("/origins", h.put)
	// DELETE /api/v1/settings/browser-consent/origins?startHost=&scope=&conversationId=&turnId=
	group.DELETE("/origins", h.revoke)
}

// guard enforces flag, store and an authenticated person, in that order.
func (h *browserConsentHandlers) guard(c *gin.Context) (string, bool) {
	if !h.enabled {
		abortTypedError(c, http.StatusNotFound, "browser_consent_disabled", "browser consent settings are not enabled", nil)
		return "", false
	}
	if h.store == nil {
		abortTypedError(c, http.StatusServiceUnavailable, "browser_consent_unavailable", "browser consent storage is not configured", nil)
		return "", false
	}
	userID, ok := getUserIDFromContext(c)
	if !ok || strings.TrimSpace(userID) == "" {
		abortTypedError(c, http.StatusUnauthorized, "unauthenticated", "authentication required", nil)
		return "", false
	}
	return userID, true
}

func (h *browserConsentHandlers) clock() time.Time {
	if h.now != nil {
		return h.now().UTC()
	}
	return time.Now().UTC()
}

// GET /api/v1/settings/browser-consent
func (h *browserConsentHandlers) get(c *gin.Context) {
	userID, ok := h.guard(c)
	if !ok {
		return
	}
	stored, err := h.store.GetBrowserConsentSettings(c.Request.Context(), userID)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to get browser consent settings")
		abortTypedError(c, http.StatusInternalServerError, "browser_consent_read_failed", "failed to retrieve browser consent settings", nil)
		return
	}
	var settings map[string]any
	if stored != nil {
		settings = stored.Settings
	}
	doc, err := decodeBrowserConsentDocument(settings)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("stored browser consent settings no longer decode")
		abortTypedError(c, http.StatusInternalServerError, "browser_consent_read_failed", "failed to decode browser consent settings", nil)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": doc})
}

// PUT /api/v1/settings/browser-consent/origins
func (h *browserConsentHandlers) put(c *gin.Context) {
	userID, ok := h.guard(c)
	if !ok {
		return
	}
	var req browserConsentDecisionRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		abortTypedError(c, http.StatusBadRequest, "invalid_request", "invalid request body", nil)
		return
	}
	decision, verr := validateBrowserConsentDecision(req, h.clock())
	if verr != nil {
		abortTypedError(c, verr.status, verr.code, verr.message, verr.details)
		return
	}
	updated, err := h.store.MutateBrowserConsentSettings(c.Request.Context(), userID, func(current map[string]any) (map[string]any, *users.BrowserConsentAudit, error) {
		doc, err := decodeBrowserConsentDocument(current)
		if err != nil {
			return nil, nil, err
		}
		next, replaced, err := upsertBrowserConsentDecision(doc, decision)
		if err != nil {
			return nil, nil, err
		}
		encoded, err := encodeBrowserConsentDocument(next)
		if err != nil {
			return nil, nil, err
		}
		return encoded, browserConsentDecidedAudit(decision, replaced), nil
	})
	h.writeMutationResult(c, userID, updated, err)
}

// DELETE /api/v1/settings/browser-consent/origins
func (h *browserConsentHandlers) revoke(c *gin.Context) {
	userID, ok := h.guard(c)
	if !ok {
		return
	}
	key, verr := validateBrowserConsentKey(
		c.Query("startHost"), c.Query("scope"), c.Query("conversationId"), c.Query("turnId"),
	)
	if verr != nil {
		abortTypedError(c, verr.status, verr.code, verr.message, verr.details)
		return
	}
	updated, err := h.store.MutateBrowserConsentSettings(c.Request.Context(), userID, func(current map[string]any) (map[string]any, *users.BrowserConsentAudit, error) {
		doc, err := decodeBrowserConsentDocument(current)
		if err != nil {
			return nil, nil, err
		}
		next, removed := removeBrowserConsentDecision(doc, key)
		if !removed {
			return nil, nil, newBrowserConsentError(http.StatusNotFound, "browser_consent_not_found", "no stored decision matches", nil)
		}
		encoded, err := encodeBrowserConsentDocument(next)
		if err != nil {
			return nil, nil, err
		}
		return encoded, browserConsentRevokedAudit(key), nil
	})
	h.writeMutationResult(c, userID, updated, err)
}

func (h *browserConsentHandlers) writeMutationResult(c *gin.Context, userID string, updated *users.UserSettings, err error) {
	if err != nil {
		var typed *browserConsentError
		if errors.As(err, &typed) {
			abortTypedError(c, typed.status, typed.code, typed.message, typed.details)
			return
		}
		log.Error().Err(err).Str("user_id", userID).Msg("failed to save browser consent settings")
		abortTypedError(c, http.StatusInternalServerError, "browser_consent_write_failed", "failed to save browser consent settings", nil)
		return
	}
	var settings map[string]any
	if updated != nil {
		settings = updated.Settings
	}
	doc, err := decodeBrowserConsentDocument(settings)
	if err != nil {
		abortTypedError(c, http.StatusInternalServerError, "browser_consent_read_failed", "failed to decode browser consent settings", nil)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": doc})
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

// browserConsentReservedSuffixes are private/loopback-style names that are
// never grantable (DEC-19: private/loopback never grantable).
var browserConsentReservedSuffixes = []string{
	"localhost", "local", "internal", "localdomain", "lan", "home", "intranet",
	"corp", "private", "invalid", "test", "example", "onion", "home.arpa",
	"in-addr.arpa", "ip6.arpa",
}

// canonicalBrowserConsentHost returns the canonical form of a host (lower
// case, no trailing dot) or a typed refusal. Input must already be a bare
// ASCII host name: no scheme, port, path, user info, wildcard or IP literal;
// internationalised names must arrive in punycode (xn--) form.
func canonicalBrowserConsentHost(raw string) (string, *browserConsentError) {
	host := strings.ToLower(strings.TrimSpace(raw))
	host = strings.TrimSuffix(host, ".")
	refuse := func(reason string) (string, *browserConsentError) {
		return "", invalidBrowserConsent("invalid_host", "host is not a canonical, grantable host name", map[string]any{"reason": reason})
	}
	if host == "" {
		return refuse("empty")
	}
	if len(host) > 253 {
		return refuse("too_long")
	}
	trimmed := strings.Trim(host, "[]")
	if net.ParseIP(trimmed) != nil {
		return refuse("ip_literal")
	}
	labels := strings.Split(host, ".")
	if len(labels) < 2 {
		return refuse("not_fully_qualified")
	}
	for _, label := range labels {
		if len(label) == 0 || len(label) > 63 {
			return refuse("bad_label_length")
		}
		if label[0] == '-' || label[len(label)-1] == '-' {
			return refuse("bad_label_hyphen")
		}
		for i := 0; i < len(label); i++ {
			ch := label[i]
			isAlnum := (ch >= 'a' && ch <= 'z') || (ch >= '0' && ch <= '9')
			if !isAlnum && ch != '-' {
				return refuse("bad_character")
			}
		}
	}
	tld := labels[len(labels)-1]
	allDigits := true
	for i := 0; i < len(tld); i++ {
		if tld[i] < '0' || tld[i] > '9' {
			allDigits = false
			break
		}
	}
	if allDigits {
		return refuse("numeric_tld")
	}
	for _, suffix := range browserConsentReservedSuffixes {
		if host == suffix || strings.HasSuffix(host, "."+suffix) {
			return refuse("reserved_name")
		}
	}
	return host, nil
}

func validBrowserConsentID(id string) bool {
	if id == "" || len(id) > browserConsentMaxIDLength {
		return false
	}
	for i := 0; i < len(id); i++ {
		ch := id[i]
		switch {
		case ch >= 'a' && ch <= 'z', ch >= 'A' && ch <= 'Z', ch >= '0' && ch <= '9':
		case ch == '-', ch == '_', ch == '.', ch == ':':
		default:
			return false
		}
	}
	return true
}

// browserConsentKey identifies one stored decision.
type browserConsentKey struct {
	StartHost      string
	Scope          string
	ConversationID string
	TurnID         string
}

func (d browserConsentDecision) key() browserConsentKey {
	return browserConsentKey{StartHost: d.StartHost, Scope: d.Scope, ConversationID: d.ConversationID, TurnID: d.TurnID}
}

// validateBrowserConsentKey checks the identifying fields: canonical start
// host, a known scope, and exactly the binding id the scope needs (turn ->
// turnId, conversation -> conversationId, always -> neither).
func validateBrowserConsentKey(startHost, scope, conversationID, turnID string) (browserConsentKey, *browserConsentError) {
	host, verr := canonicalBrowserConsentHost(startHost)
	if verr != nil {
		verr.details["field"] = "startHost"
		return browserConsentKey{}, verr
	}
	scope = strings.ToLower(strings.TrimSpace(scope))
	conversationID = strings.TrimSpace(conversationID)
	turnID = strings.TrimSpace(turnID)
	switch scope {
	case browserConsentScopeTurn:
		if !validBrowserConsentID(turnID) {
			return browserConsentKey{}, invalidBrowserConsent("invalid_scope_binding", "turn scope requires a valid turnId", map[string]any{"scope": scope})
		}
		if conversationID != "" && !validBrowserConsentID(conversationID) {
			return browserConsentKey{}, invalidBrowserConsent("invalid_scope_binding", "conversationId is malformed", map[string]any{"scope": scope})
		}
	case browserConsentScopeConversation:
		if !validBrowserConsentID(conversationID) {
			return browserConsentKey{}, invalidBrowserConsent("invalid_scope_binding", "conversation scope requires a valid conversationId", map[string]any{"scope": scope})
		}
		if turnID != "" {
			return browserConsentKey{}, invalidBrowserConsent("invalid_scope_binding", "conversation scope takes no turnId", map[string]any{"scope": scope})
		}
	case browserConsentScopeAlways:
		if conversationID != "" || turnID != "" {
			return browserConsentKey{}, invalidBrowserConsent("invalid_scope_binding", "always scope takes no conversationId or turnId", map[string]any{"scope": scope})
		}
	default:
		return browserConsentKey{}, invalidBrowserConsent("invalid_scope", "scope must be turn, conversation or always", nil)
	}
	return browserConsentKey{StartHost: host, Scope: scope, ConversationID: conversationID, TurnID: turnID}, nil
}

// validateBrowserConsentDecision turns a PUT body into a canonical decision.
func validateBrowserConsentDecision(req browserConsentDecisionRequest, now time.Time) (browserConsentDecision, *browserConsentError) {
	kind := strings.ToLower(strings.TrimSpace(req.Kind))
	if kind == "" {
		kind = browserConsentKindOrigin
	}
	if kind != browserConsentKindOrigin {
		return browserConsentDecision{}, invalidBrowserConsent("invalid_kind", "only origin consent is stored in this category", map[string]any{"kind": kind})
	}
	key, verr := validateBrowserConsentKey(req.StartHost, req.Scope, req.ConversationID, req.TurnID)
	if verr != nil {
		return browserConsentDecision{}, verr
	}
	decision := strings.ToLower(strings.TrimSpace(req.Decision))
	if decision != browserConsentDecisionAllow && decision != browserConsentDecisionDeny {
		return browserConsentDecision{}, invalidBrowserConsent("invalid_decision", "decision must be allow or deny", nil)
	}
	hosts := make([]string, 0, len(req.Hosts))
	seen := map[string]bool{key.StartHost: true}
	for i, raw := range req.Hosts {
		host, herr := canonicalBrowserConsentHost(raw)
		if herr != nil {
			herr.details["field"] = "hosts"
			herr.details["index"] = i
			return browserConsentDecision{}, herr
		}
		if seen[host] {
			continue
		}
		seen[host] = true
		hosts = append(hosts, host)
	}
	if decision == browserConsentDecisionDeny && len(hosts) > 0 {
		return browserConsentDecision{}, invalidBrowserConsent("invalid_hosts", "a deny decision names only its start host", nil)
	}
	if len(hosts) > browserConsentMaxListedHosts {
		return browserConsentDecision{}, invalidBrowserConsent("too_many_hosts", "a decision may list at most 8 hosts besides the start host", map[string]any{"max": browserConsentMaxListedHosts, "count": len(hosts)})
	}
	sort.Strings(hosts)
	return browserConsentDecision{
		Kind:           browserConsentKindOrigin,
		StartHost:      key.StartHost,
		Hosts:          hosts,
		Decision:       decision,
		Scope:          key.Scope,
		ConversationID: key.ConversationID,
		TurnID:         key.TurnID,
		DecidedAt:      now.UTC(),
	}, nil
}

// ---------------------------------------------------------------------------
// Document operations
// ---------------------------------------------------------------------------

func decodeBrowserConsentDocument(settings map[string]any) (browserConsentDocument, error) {
	doc := browserConsentDocument{SchemaVersion: browserConsentSchemaVersion, Origins: []browserConsentDecision{}}
	if len(settings) == 0 {
		return doc, nil
	}
	raw, err := json.Marshal(settings)
	if err != nil {
		return doc, fmt.Errorf("encode stored browser consent: %w", err)
	}
	if err := json.Unmarshal(raw, &doc); err != nil {
		return doc, fmt.Errorf("decode stored browser consent: %w", err)
	}
	if doc.SchemaVersion == 0 {
		doc.SchemaVersion = browserConsentSchemaVersion
	}
	if doc.SchemaVersion != browserConsentSchemaVersion {
		return doc, fmt.Errorf("unsupported browser consent schema version %d", doc.SchemaVersion)
	}
	if doc.Origins == nil {
		doc.Origins = []browserConsentDecision{}
	}
	for i := range doc.Origins {
		if doc.Origins[i].Hosts == nil {
			doc.Origins[i].Hosts = []string{}
		}
	}
	return doc, nil
}

func encodeBrowserConsentDocument(doc browserConsentDocument) (map[string]any, error) {
	doc.SchemaVersion = browserConsentSchemaVersion
	if doc.Origins == nil {
		doc.Origins = []browserConsentDecision{}
	}
	return structToMap(doc)
}

// upsertBrowserConsentDecision replaces the decision with the same key or
// appends it, evicting the oldest turn- then conversation-scoped decisions
// when the document is full. It reports whether an existing decision was
// replaced. A full document of "always" decisions is refused.
func upsertBrowserConsentDecision(doc browserConsentDocument, decision browserConsentDecision) (browserConsentDocument, bool, error) {
	key := decision.key()
	out := make([]browserConsentDecision, 0, len(doc.Origins)+1)
	replaced := false
	for _, existing := range doc.Origins {
		if existing.key() == key {
			replaced = true
			continue
		}
		out = append(out, existing)
	}
	for len(out) >= browserConsentMaxDecisions {
		victim := oldestEvictableBrowserConsent(out)
		if victim < 0 {
			return doc, false, newBrowserConsentError(http.StatusConflict, "browser_consent_limit", "too many stored decisions; revoke some first", map[string]any{"max": browserConsentMaxDecisions})
		}
		out = append(out[:victim], out[victim+1:]...)
	}
	out = append(out, decision)
	doc.Origins = out
	return doc, replaced, nil
}

// oldestEvictableBrowserConsent picks the oldest turn-scoped decision, else
// the oldest conversation-scoped one, else -1.
func oldestEvictableBrowserConsent(origins []browserConsentDecision) int {
	for _, scope := range []string{browserConsentScopeTurn, browserConsentScopeConversation} {
		victim := -1
		for i, d := range origins {
			if d.Scope != scope {
				continue
			}
			if victim < 0 || d.DecidedAt.Before(origins[victim].DecidedAt) {
				victim = i
			}
		}
		if victim >= 0 {
			return victim
		}
	}
	return -1
}

func removeBrowserConsentDecision(doc browserConsentDocument, key browserConsentKey) (browserConsentDocument, bool) {
	out := make([]browserConsentDecision, 0, len(doc.Origins))
	removed := false
	for _, existing := range doc.Origins {
		if existing.key() == key {
			removed = true
			continue
		}
		out = append(out, existing)
	}
	doc.Origins = out
	return doc, removed
}

// ---------------------------------------------------------------------------
// Audit (content-free: host digests, never host names)
// ---------------------------------------------------------------------------

// browserConsentHostDigest keeps host names out of user_activity_log, which
// outlives an anonymize erasure; the digest still lets an operator match a
// known host.
func browserConsentHostDigest(host string) string {
	sum := sha256.Sum256([]byte(host))
	return hex.EncodeToString(sum[:8])
}

func browserConsentDecidedAudit(d browserConsentDecision, replaced bool) *users.BrowserConsentAudit {
	return &users.BrowserConsentAudit{
		Action: browserConsentAuditDecided,
		Details: map[string]any{
			"kind":            d.Kind,
			"decision":        d.Decision,
			"scope":           d.Scope,
			"startHostDigest": browserConsentHostDigest(d.StartHost),
			"listedHosts":     len(d.Hosts),
			"replaced":        replaced,
		},
	}
}

func browserConsentRevokedAudit(key browserConsentKey) *users.BrowserConsentAudit {
	return &users.BrowserConsentAudit{
		Action: browserConsentAuditRevoked,
		Details: map[string]any{
			"kind":            browserConsentKindOrigin,
			"scope":           key.Scope,
			"startHostDigest": browserConsentHostDigest(key.StartHost),
		},
	}
}
