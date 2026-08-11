// Package api — HTTP handlers for skills, MCP servers, plugins, routing, and safety.
package api

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"os"
	"regexp"
	"strconv"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/pkg/publisher"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/crypto"
	"github.com/triodelab/model-plane/services/capability-core/internal/reconcile"
)

type registryDatabase interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
	QueryRow(context.Context, string, ...any) pgx.Row
}

// ---------------------------------------------------------------------------
// SkillsHandler  /api/v1/skills
// ---------------------------------------------------------------------------

// SkillsHandler handles CRUD for agent_skills.
type SkillsHandler struct {
	pool registryDatabase
	pub  publisher.EventPublisher
}

// NewSkillsHandler constructs the handler.
func NewSkillsHandler(pool registryDatabase) *SkillsHandler {
	return &SkillsHandler{pool: pool}
}

// WithPublisher wires reconcile-event emission (matrix §4.3). Optional, nil-safe.
func (h *SkillsHandler) WithPublisher(pub publisher.EventPublisher) *SkillsHandler {
	h.pub = pub
	return h
}

// Register mounts routes.
func (h *SkillsHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/skills", h.listOrCreate)
	mux.HandleFunc("/api/v1/skills/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/skills/"):]
		switch r.Method {
		case http.MethodGet:
			h.get(w, r, id)
		case http.MethodPatch:
			h.update(w, r, id)
		case http.MethodDelete:
			h.delete(w, r, id)
		default:
			http.NotFound(w, r)
		}
	})
}

type skillRow struct {
	ID                  string    `json:"id"`
	OrgID               string    `json:"org_id"`
	Name                string    `json:"name"`
	Description         string    `json:"description"`
	Content             string    `json:"content"`
	TriggerKeywords     []string  `json:"trigger_keywords"`
	TriggerFilePatterns []string  `json:"trigger_file_patterns"`
	ToolRestrictions    []string  `json:"tool_restrictions"`
	Enabled             bool      `json:"enabled"`
	CreatedAt           time.Time `json:"created_at"`
	UpdatedAt           time.Time `json:"updated_at"`
}

func (h *SkillsHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID := verifiedOrganizationID(r)
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, description, content,
			       trigger_keywords, trigger_file_patterns, tool_restrictions,
			       enabled, created_at, updated_at
			FROM agent_skills WHERE org_id = $1 ORDER BY name
		`, orgID)
		if err != nil {
			slog.Error("list skills failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		var skills []skillRow
		for rows.Next() {
			var s skillRow
			if err := rows.Scan(&s.ID, &s.OrgID, &s.Name, &s.Description, &s.Content,
				&s.TriggerKeywords, &s.TriggerFilePatterns, &s.ToolRestrictions,
				&s.Enabled, &s.CreatedAt, &s.UpdatedAt); err != nil {
				jsonErr(w, err.Error(), http.StatusInternalServerError)
				return
			}
			skills = append(skills, s)
		}
		writeJSON(w, map[string]any{"skills": skills})
	case http.MethodPost:
		var s skillRow
		if err := json.NewDecoder(r.Body).Decode(&s); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		s.OrgID = verifiedOrganizationID(r)
		s.ID = "skill_" + uuid.New().String()
		now := time.Now().UTC()
		if s.TriggerKeywords == nil {
			s.TriggerKeywords = []string{}
		}
		if s.TriggerFilePatterns == nil {
			s.TriggerFilePatterns = []string{}
		}
		if s.ToolRestrictions == nil {
			s.ToolRestrictions = []string{}
		}
		kw, _ := json.Marshal(s.TriggerKeywords)
		fp, _ := json.Marshal(s.TriggerFilePatterns)
		tr, _ := json.Marshal(s.ToolRestrictions)
		_, err := h.pool.Exec(r.Context(), `
			INSERT INTO agent_skills (id, org_id, name, description, content,
			    trigger_keywords, trigger_file_patterns, tool_restrictions,
			    enabled, created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
		`, s.ID, s.OrgID, s.Name, s.Description, s.Content, kw, fp, tr, s.Enabled, now, now)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindSkill,
			reconcile.ActionRegistered, s.ID, s.OrgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", reconcile.KindSkill, "id", s.ID, "error", eerr)
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": s.ID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *SkillsHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	var s skillRow
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, name, description, content,
		       trigger_keywords, trigger_file_patterns, tool_restrictions,
		       enabled, created_at, updated_at
		FROM agent_skills WHERE id = $1 AND org_id = $2
	`, id, verifiedOrganizationID(r)).Scan(&s.ID, &s.OrgID, &s.Name, &s.Description, &s.Content,
		&s.TriggerKeywords, &s.TriggerFilePatterns, &s.ToolRestrictions,
		&s.Enabled, &s.CreatedAt, &s.UpdatedAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, s)
}

func (h *SkillsHandler) update(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Enabled     *bool  `json:"enabled"`
		Description string `json:"description"`
		Content     string `json:"content"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	if update.Enabled == nil && update.Description == "" && update.Content == "" {
		jsonErr(w, "at least one skill field is required", http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	orgID := verifiedOrganizationID(r)
	var enabled any
	if update.Enabled != nil {
		enabled = *update.Enabled
	}
	result, err := h.pool.Exec(r.Context(), `
		UPDATE agent_skills
		SET enabled=COALESCE($1, enabled), description=COALESCE(NULLIF($2, ''), description),
			content=COALESCE(NULLIF($3, ''), content), updated_at=$4
		WHERE id=$5 AND org_id=$6
	`, enabled, update.Description, update.Content, now, id, orgID)
	if !writeSingleScopedMutation(w, "agent skill", result, err) {
		return
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *SkillsHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	result, err := h.pool.Exec(r.Context(), `DELETE FROM agent_skills WHERE id=$1 AND org_id=$2`, id, verifiedOrganizationID(r))
	if !writeSingleScopedMutation(w, "agent skill", result, err) {
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// ---------------------------------------------------------------------------
// MCPHandler  /api/v1/mcp
// ---------------------------------------------------------------------------

// MCPHandler handles CRUD for mcp_servers.
type MCPHandler struct {
	pool     registryDatabase
	pub      publisher.EventPublisher
	resolver mcpHostResolver
	// vault encrypts/decrypts mcp_oauth_tokens at rest. Nil (unset) makes the
	// oauth-token(s) endpoints fail closed with 503 rather than silently
	// storing or returning plaintext.
	vault *crypto.Vault
	// mcpServiceToken authenticates model-gateway's internal oauth-token
	// resolve and refresh-writeback when it has no live per-user bearer to
	// forward (the execution-core-triggered tools/call path). Empty (unset)
	// disables those paths entirely — the per-user JWT endpoints under
	// /api/v1/mcp are unaffected either way.
	//
	// This is the ROOT secret and is never accepted on the wire: callers
	// present HMAC-SHA256(root, org_id) instead, so a credential proves
	// "trusted internal caller, for THIS org" — see
	// expectedInternalServiceToken for the exact property that buys and what
	// it does not. The caller-asserted org_id additionally has to match a
	// real stored (server_id, org_id) row, so this cannot widen access beyond
	// servers that genuinely belong to the org it asserts.
	mcpServiceToken string
}

type mcpHostResolver interface {
	LookupNetIP(context.Context, string, string) ([]netip.Addr, error)
}

const (
	maxMCPRegistrationBodyBytes = 32 << 10
	maxMCPToolAllowlistEntries  = 64
)

var (
	mcpToolNamePattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$`)
	mcpIdentifier      = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_.:-]{0,199}$`)
	mcpForbiddenRanges = []netip.Prefix{
		netip.MustParsePrefix("0.0.0.0/8"),
		netip.MustParsePrefix("10.0.0.0/8"),
		netip.MustParsePrefix("100.64.0.0/10"),
		netip.MustParsePrefix("127.0.0.0/8"),
		netip.MustParsePrefix("169.254.0.0/16"),
		netip.MustParsePrefix("172.16.0.0/12"),
		netip.MustParsePrefix("192.0.0.0/24"),
		netip.MustParsePrefix("192.0.2.0/24"),
		netip.MustParsePrefix("192.168.0.0/16"),
		netip.MustParsePrefix("198.18.0.0/15"),
		netip.MustParsePrefix("198.51.100.0/24"),
		netip.MustParsePrefix("203.0.113.0/24"),
		netip.MustParsePrefix("224.0.0.0/4"),
		netip.MustParsePrefix("240.0.0.0/4"),
		netip.MustParsePrefix("::/128"),
		netip.MustParsePrefix("::1/128"),
		netip.MustParsePrefix("64:ff9b::/96"),
		netip.MustParsePrefix("100::/64"),
		netip.MustParsePrefix("2001:db8::/32"),
		netip.MustParsePrefix("fc00::/7"),
		netip.MustParsePrefix("fe80::/10"),
		netip.MustParsePrefix("ff00::/8"),
	}
)

// NewMCPHandler constructs the handler.
func NewMCPHandler(pool *pgxpool.Pool) *MCPHandler {
	return &MCPHandler{pool: pool, resolver: net.DefaultResolver}
}

// WithPublisher wires reconcile-event emission (matrix §4.3). Optional and
// nil-safe: without it, mutations simply don't emit and the gateway falls back
// to its cache TTL. Chainable: NewMCPHandler(pool).WithPublisher(pub).Register(mux).
func (h *MCPHandler) WithPublisher(pub publisher.EventPublisher) *MCPHandler {
	h.pub = pub
	return h
}

// WithVault wires MCP_TOKEN_ENCRYPTION_KEY-backed encryption for the
// oauth-token(s) endpoints. Optional and nil-safe at construction time — but
// those two endpoints refuse to run without it (503, never plaintext).
func (h *MCPHandler) WithVault(vault *crypto.Vault) *MCPHandler {
	h.vault = vault
	return h
}

// WithMCPServiceToken enables the Model-Plane-local service-to-service
// oauth-token routes (see mcpServiceToken doc comment). Passing an empty
// string leaves them disabled, same as never calling this at all.
func (h *MCPHandler) WithMCPServiceToken(token string) *MCPHandler {
	h.mcpServiceToken = token
	return h
}

type mcpServerRow struct {
	ID                  string    `json:"id"`
	OrgID               string    `json:"org_id"`
	Name                string    `json:"name"`
	Description         string    `json:"description"`
	EndpointURL         string    `json:"endpoint_url"`
	Transport           string    `json:"transport"`
	AuthKind            string    `json:"auth_kind"`
	ConfigJSON          any       `json:"-"`
	ToolAllowlist       []string  `json:"tool_allowlist"`
	SecretConfigured    bool      `json:"secret_configured"`
	ConfigurationState  string    `json:"configuration_state"`
	ConfigurationReason string    `json:"configuration_reason,omitempty"`
	Scope               string    `json:"scope"`
	Enabled             bool      `json:"enabled"`
	RolloutState        string    `json:"rollout_state"`
	RiskLevel           string    `json:"risk_level"`
	CreatedAt           time.Time `json:"created_at"`
	UpdatedAt           time.Time `json:"updated_at"`
}

type mcpServerRegistration struct {
	ID           string          `json:"id"`
	OrgID        string          `json:"org_id"`
	Name         string          `json:"name"`
	Description  string          `json:"description"`
	EndpointURL  string          `json:"endpoint_url"`
	Transport    string          `json:"transport"`
	AuthKind     string          `json:"auth_kind"`
	ConfigJSON   mcpServerConfig `json:"config_json"`
	Scope        string          `json:"scope"`
	Enabled      bool            `json:"enabled"`
	RolloutState string          `json:"rollout_state"`
	RiskLevel    string          `json:"risk_level"`
}

// mcpServerConfig deliberately enumerates every durable configuration field.
// Executable commands, arguments, headers, tokens, and provider configuration
// are not part of the registry contract and strict JSON decoding rejects them.
type mcpServerConfig struct {
	ToolAllowlist []string `json:"tool_allowlist"`
	SecretRef     string   `json:"secret_ref,omitempty"`
	OwnerUserID   string   `json:"owner_user_id,omitempty"`
	SharedWith    []string `json:"shared_with,omitempty"`
}

func mcpVerifiedOrganization(request *http.Request, requireWrite bool) (string, bool) {
	principal, ok := authctx.PrincipalFromContext(request.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		return "", false
	}
	if requireWrite && !principal.HasScope(authz.WriteScope) {
		return "", false
	}
	return principal.OrganizationID, true
}

// secureTokenEqual compares two shared-secret candidates in constant time.
// Hashing first (mirroring auth-core's own secureEqual in
// plane-service-principal.ts) means even the length comparison inside
// subtle.ConstantTimeCompare never depends on the raw secret's length.
func secureTokenEqual(expected, received string) bool {
	expectedDigest := sha256.Sum256([]byte(expected))
	receivedDigest := sha256.Sum256([]byte(received))
	return subtle.ConstantTimeCompare(expectedDigest[:], receivedDigest[:]) == 1
}

const mcpServiceTokenHeader = "X-Mcp-Service-Token"

// expectedInternalServiceToken derives the value X-Mcp-Service-Token must carry
// for a call targeting orgID:
//
//	presented = lowercase_hex( HMAC-SHA256( key = MCP_OAUTH_SERVICE_TOKEN,
//	                                       message = org_id ) )
//
// Honest security property: this bounds the blast radius of an INTERCEPTED
// credential. A header captured off one request — a proxy access log, a
// tcpdump on the Model-Plane network, a leaked trace — is replayable only
// against the single org it was minted for and is worthless for every other
// tenant. It does NOT mitigate compromise of the root
// MCP_OAUTH_SERVICE_TOKEN itself: whoever holds that value can derive the
// token for any org at will, exactly as the raw shared secret allowed. The
// root secret remains a tenant-wide credential and must be handled as one.
//
// orgID must be the SAME string this request is authorized against (the
// TrimSpace'd query parameter internalTokenTarget returns), or the gate would
// verify one org's token while the handler body reads another's row.
// model-gateway's twin is derive_org_scoped_service_token in
// rust/services/model-gateway/src/mcp_oauth.rs; the known-answer vector in
// this package's tests is duplicated in that module's tests so any drift
// between the two implementations fails a test instead of breaking auth in
// production.
func expectedInternalServiceToken(rootSecret, orgID string) string {
	mac := hmac.New(sha256.New, []byte(rootSecret))
	mac.Write([]byte(orgID))
	return hex.EncodeToString(mac.Sum(nil))
}

// authorizedInternalServiceCall is the sole gate on every RegisterInternal
// route: those are mounted outside the per-user JWT middleware, so nothing
// else stands between the network and the handler body. Takes the already
// parsed orgID because the credential is bound to it — see
// expectedInternalServiceToken. An unconfigured root secret (or an empty
// orgID, which internalTokenTarget already rejects) keeps the route closed
// rather than degrading into "any caller is trusted".
func (h *MCPHandler) authorizedInternalServiceCall(r *http.Request, orgID string) bool {
	received := r.Header.Get(mcpServiceTokenHeader)
	if received == "" || h.mcpServiceToken == "" || orgID == "" {
		return false
	}
	return secureTokenEqual(expectedInternalServiceToken(h.mcpServiceToken, orgID), received)
}

// internalTokenTarget reads the caller-asserted (server_id, org_id) pair the
// internal routes are scoped by, and must run BEFORE
// authorizedInternalServiceCall — the presented credential is derived from
// this exact trimmed org_id. Asserted, not proven — see the mcpServiceToken
// doc comment for why that is still containing.
func internalTokenTarget(r *http.Request) (id, orgID string, ok bool) {
	id = strings.TrimSpace(r.URL.Query().Get("server_id"))
	orgID = strings.TrimSpace(r.URL.Query().Get("org_id"))
	return id, orgID, id != "" && orgID != ""
}

func decodeMCPRegistration(w http.ResponseWriter, request *http.Request) (mcpServerRegistration, error) {
	request.Body = http.MaxBytesReader(w, request.Body, maxMCPRegistrationBodyBytes)
	decoder := json.NewDecoder(request.Body)
	decoder.DisallowUnknownFields()
	var input mcpServerRegistration
	if err := decoder.Decode(&input); err != nil {
		return mcpServerRegistration{}, err
	}
	if err := ensureJSONEOF(decoder); err != nil {
		return mcpServerRegistration{}, err
	}
	return input, nil
}

func (h *MCPHandler) normalizeMCPRegistration(
	ctx context.Context,
	input mcpServerRegistration,
	organizationID string,
) (mcpServerRow, mcpServerConfig, error) {
	input.ID = strings.TrimSpace(input.ID)
	input.Name = strings.TrimSpace(input.Name)
	input.Description = strings.TrimSpace(input.Description)
	input.Transport = strings.ToLower(strings.TrimSpace(input.Transport))
	input.AuthKind = strings.ToLower(strings.TrimSpace(input.AuthKind))
	input.Scope = strings.ToLower(strings.TrimSpace(input.Scope))
	input.RolloutState = strings.ToLower(strings.TrimSpace(input.RolloutState))
	input.RiskLevel = strings.ToLower(strings.TrimSpace(input.RiskLevel))
	if input.ID == "" {
		input.ID = "mcp_" + uuid.New().String()
	}
	if input.Transport == "" {
		input.Transport = "http"
	}
	if input.AuthKind == "" {
		input.AuthKind = "none"
	}
	if input.Scope == "" {
		input.Scope = "workspace"
	}
	if input.RolloutState == "" {
		input.RolloutState = "stable"
	}
	if input.RiskLevel == "" {
		input.RiskLevel = "medium"
	}
	if !mcpIdentifier.MatchString(input.ID) || !validMCPDisplayName(input.Name) || len(input.Description) > 2_000 {
		return mcpServerRow{}, mcpServerConfig{}, errors.New("invalid identity fields")
	}
	if input.Transport != "http" {
		return mcpServerRow{}, mcpServerConfig{}, errors.New("only remote HTTP transport is supported")
	}
	endpoint, host, err := parseMCPEndpoint(input.EndpointURL)
	if err != nil {
		return mcpServerRow{}, mcpServerConfig{}, err
	}
	config, err := normalizeMCPConfig(input.ConfigJSON, input.AuthKind, input.Scope)
	if err != nil {
		return mcpServerRow{}, mcpServerConfig{}, err
	}
	if !validMCPAuthKind(input.AuthKind) || !validMCPScope(input.Scope) ||
		!validMCPRolloutState(input.RolloutState) || !validMCPRiskLevel(input.RiskLevel) {
		return mcpServerRow{}, mcpServerConfig{}, errors.New("invalid MCP policy fields")
	}
	if h.resolver == nil {
		return mcpServerRow{}, mcpServerConfig{}, errors.New("DNS resolver unavailable")
	}
	lookupContext, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	addresses, err := h.resolver.LookupNetIP(lookupContext, "ip", host)
	if err != nil || len(addresses) == 0 {
		return mcpServerRow{}, mcpServerConfig{}, fmt.Errorf("MCP endpoint DNS lookup failed")
	}
	for _, address := range addresses {
		if !mcpAddressIsPublic(address) {
			return mcpServerRow{}, mcpServerConfig{}, errors.New("MCP endpoint resolved to a forbidden address")
		}
	}
	return mcpServerRow{
		ID: input.ID, OrgID: organizationID, Name: input.Name, Description: input.Description,
		EndpointURL: endpoint, Transport: input.Transport, AuthKind: input.AuthKind,
		Scope: input.Scope, Enabled: input.Enabled, RolloutState: input.RolloutState, RiskLevel: input.RiskLevel,
	}, config, nil
}

func parseMCPEndpoint(raw string) (string, string, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" || len(raw) > 2_048 || !allASCII(raw) || strings.ContainsAny(raw, "\\#\r\n\t") {
		return "", "", errors.New("invalid MCP endpoint")
	}
	endpoint, err := url.ParseRequestURI(raw)
	if err != nil || endpoint.Opaque != "" || endpoint.Host == "" ||
		endpoint.User != nil || endpoint.RawQuery != "" || endpoint.Fragment != "" {
		return "", "", errors.New("MCP endpoint must be a credential-free URL")
	}
	authority := endpoint.Host
	if strings.Contains(authority, "%") {
		return "", "", errors.New("encoded MCP endpoint hosts are forbidden")
	}
	host := strings.ToLower(endpoint.Hostname())
	// A trusted internal host (opt-in via MCP_INTERNAL_ALLOWED_HOSTS) may use
	// plain HTTP on a private address — the bundled bridge sidecar. Every other
	// server stays public-HTTPS-only + SSRF-guarded. Credential/query/fragment
	// hygiene is enforced for all.
	hostAllowed := mcpHostInInternalAllowlist(host)
	if endpoint.Scheme != "https" && !(hostAllowed && endpoint.Scheme == "http") {
		return "", "", errors.New("MCP endpoint must be a credential-free HTTPS URL (or an allowlisted internal host)")
	}
	if host == "" || strings.HasSuffix(host, ".") || (!hostAllowed && mcpHostIsForbidden(host)) {
		return "", "", errors.New("MCP endpoint host is forbidden")
	}
	if _, err := netip.ParseAddr(host); err == nil && !hostAllowed {
		return "", "", errors.New("literal MCP endpoint addresses are forbidden")
	}
	// Allowlisted internal hosts are commonly single-label docker service names
	// (e.g. "mcp-bridge") which the public-FQDN validator rejects; the operator
	// allowlist is itself the trust decision, so skip the FQDN shape check.
	if !hostAllowed && !validMCPHostname(host) {
		return "", "", errors.New("invalid MCP endpoint hostname")
	}
	port := endpoint.Port()
	if port != "" {
		value, err := strconv.Atoi(port)
		if err != nil || value < 1 || value > 65_535 {
			return "", "", errors.New("invalid MCP endpoint port")
		}
		endpoint.Host = net.JoinHostPort(host, port)
	} else {
		endpoint.Host = host
	}
	if !(hostAllowed && endpoint.Scheme == "http") {
		endpoint.Scheme = "https"
	}
	return endpoint.String(), host, nil
}

// mcpHostInInternalAllowlist reports whether host is an operator-trusted internal
// MCP host (comma-separated MCP_INTERNAL_ALLOWED_HOSTS). Empty env => nothing is
// allowed, so the public-HTTPS-only + SSRF guard is unchanged by default.
func mcpHostInInternalAllowlist(host string) bool {
	csv := os.Getenv("MCP_INTERNAL_ALLOWED_HOSTS")
	if csv == "" {
		return false
	}
	host = strings.TrimSuffix(strings.ToLower(host), ".")
	for entry := range strings.SplitSeq(csv, ",") {
		if e := strings.TrimSpace(strings.ToLower(entry)); e != "" && e == host {
			return true
		}
	}
	return false
}

func mcpHostIsForbidden(host string) bool {
	if host == "localhost" || host == "metadata" || host == "instance-data" ||
		host == "metadata.google.internal" || strings.HasSuffix(host, ".localhost") ||
		strings.HasSuffix(host, ".local") || strings.HasSuffix(host, ".internal") ||
		strings.HasSuffix(host, ".home") || strings.HasSuffix(host, ".lan") ||
		strings.HasSuffix(host, ".svc") || strings.HasSuffix(host, ".cluster.local") ||
		strings.HasSuffix(host, ".arpa") {
		return true
	}
	compact := strings.ReplaceAll(host, ".", "")
	if compact == "" {
		return true
	}
	allNumeric := true
	for _, character := range compact {
		if character < '0' || character > '9' {
			allNumeric = false
			break
		}
	}
	return allNumeric || strings.HasPrefix(host, "0x")
}

func validMCPHostname(host string) bool {
	if len(host) > 253 || !allASCII(host) {
		return false
	}
	labels := strings.Split(host, ".")
	if len(labels) < 2 {
		return false
	}
	for _, label := range labels {
		if label == "" || len(label) > 63 || label[0] == '-' || label[len(label)-1] == '-' ||
			strings.HasPrefix(label, "xn--") {
			return false
		}
		for _, character := range label {
			if !(character >= 'a' && character <= 'z') && !(character >= '0' && character <= '9') && character != '-' {
				return false
			}
		}
	}
	return true
}

func mcpAddressIsPublic(address netip.Addr) bool {
	if !address.IsValid() {
		return false
	}
	address = address.Unmap()
	for _, prefix := range mcpForbiddenRanges {
		if prefix.Contains(address) {
			return false
		}
	}
	return address.IsGlobalUnicast()
}

func normalizeMCPConfig(config mcpServerConfig, authKind, scope string) (mcpServerConfig, error) {
	if len(config.ToolAllowlist) == 0 || len(config.ToolAllowlist) > maxMCPToolAllowlistEntries {
		return mcpServerConfig{}, errors.New("an exact MCP tool allowlist is required")
	}
	seen := make(map[string]struct{}, len(config.ToolAllowlist))
	tools := make([]string, 0, len(config.ToolAllowlist))
	for _, raw := range config.ToolAllowlist {
		tool := strings.TrimSpace(raw)
		if !mcpToolNamePattern.MatchString(tool) {
			return mcpServerConfig{}, errors.New("invalid MCP tool allowlist")
		}
		if _, exists := seen[tool]; exists {
			return mcpServerConfig{}, errors.New("duplicate MCP tool allowlist entry")
		}
		seen[tool] = struct{}{}
		tools = append(tools, tool)
	}
	config.ToolAllowlist = tools
	config.SecretRef = strings.TrimSpace(config.SecretRef)
	if authKind == "none" && config.SecretRef != "" {
		return mcpServerConfig{}, errors.New("unauthenticated MCP server cannot have a secret reference")
	}
	if authKind != "none" && !validMCPSecretReference(config.SecretRef) {
		return mcpServerConfig{}, errors.New("managed secret reference required")
	}
	if len(config.OwnerUserID) > 200 || len(config.SharedWith) > 64 {
		return mcpServerConfig{}, errors.New("invalid MCP ownership metadata")
	}
	if scope == "user" && !mcpIdentifier.MatchString(config.OwnerUserID) {
		return mcpServerConfig{}, errors.New("user-scoped MCP server requires an owner")
	}
	for _, sharedUser := range config.SharedWith {
		if !mcpIdentifier.MatchString(sharedUser) {
			return mcpServerConfig{}, errors.New("invalid MCP sharing metadata")
		}
	}
	return config, nil
}

func validMCPSecretReference(reference string) bool {
	if reference == "" || len(reference) > 512 || !allASCII(reference) {
		return false
	}
	parsed, err := url.Parse(reference)
	if err != nil || parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" || parsed.Host == "" || parsed.Path == "" {
		return false
	}
	switch parsed.Scheme {
	case "secret", "vault", "aws-secretsmanager", "azure-keyvault", "gcp-secretmanager":
		return true
	default:
		return false
	}
}

func allASCII(value string) bool {
	for _, character := range value {
		if character > 127 || character < 32 {
			return false
		}
	}
	return true
}

func validMCPDisplayName(value string) bool {
	if value == "" || len(value) > 200 {
		return false
	}
	for _, character := range value {
		if character < 32 || character == 127 {
			return false
		}
	}
	return true
}

func validMCPAuthKind(value string) bool {
	return value == "none" || value == "bearer" || value == "api_key" || value == "oauth"
}

func validMCPScope(value string) bool {
	return value == "workspace" || value == "org" || value == "user"
}

func validMCPRolloutState(value string) bool {
	return value == "stable" || value == "canary" || value == "quarantine" || value == "deprecated"
}

func validMCPRiskLevel(value string) bool {
	return value == "low" || value == "medium" || value == "high"
}

func hydrateMCPServerView(server *mcpServerRow) {
	server.ToolAllowlist = []string{}
	server.ConfigurationState = "invalid"
	server.ConfigurationReason = "invalid_registry_record"
	config, err := decodeStoredMCPConfig(server.ConfigJSON)
	if err != nil || server.Transport != "http" {
		quarantineInvalidMCPServer(server)
		return
	}
	if _, _, err := parseMCPEndpoint(server.EndpointURL); err != nil {
		quarantineInvalidMCPServer(server)
		return
	}
	config, err = normalizeMCPConfig(config, server.AuthKind, server.Scope)
	if err != nil || !validMCPAuthKind(server.AuthKind) {
		quarantineInvalidMCPServer(server)
		return
	}
	server.ToolAllowlist = append([]string(nil), config.ToolAllowlist...)
	server.SecretConfigured = config.SecretRef != ""
	server.ConfigurationState = "valid"
	server.ConfigurationReason = ""
}

func quarantineInvalidMCPServer(server *mcpServerRow) {
	// Legacy rows may have embedded credentials in endpoint_url. Once any part
	// of the durable record fails the current contract, do not echo that URL.
	server.EndpointURL = ""
	server.Enabled = false
	server.RolloutState = "quarantine"
	server.SecretConfigured = false
}

func decodeStoredMCPConfig(raw any) (mcpServerConfig, error) {
	var payload []byte
	switch value := raw.(type) {
	case nil:
		payload = []byte("{}")
	case []byte:
		payload = append([]byte(nil), value...)
	case string:
		payload = []byte(value)
	case json.RawMessage:
		payload = append([]byte(nil), value...)
	default:
		var err error
		payload, err = json.Marshal(value)
		if err != nil {
			return mcpServerConfig{}, err
		}
	}
	if len(payload) > maxMCPRegistrationBodyBytes {
		return mcpServerConfig{}, errors.New("stored MCP config exceeds limit")
	}
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	var config mcpServerConfig
	if err := decoder.Decode(&config); err != nil {
		return mcpServerConfig{}, err
	}
	if err := ensureJSONEOF(decoder); err != nil {
		return mcpServerConfig{}, err
	}
	return config, nil
}

// Register mounts routes.
func (h *MCPHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/mcp", h.listOrCreate)
	mux.HandleFunc("/api/v1/mcp/", func(w http.ResponseWriter, r *http.Request) {
		rest := r.URL.Path[len("/api/v1/mcp/"):]
		// OAuth token storage/resolution is sub-routed under the server id
		// before falling into the plain get/patch/delete-by-id dispatch below,
		// which expects `rest` to be a bare id.
		if id := strings.TrimSuffix(rest, "/oauth-tokens"); id != rest {
			switch r.Method {
			case http.MethodPut:
				h.oauthTokensUpsert(w, r, id)
			default:
				http.NotFound(w, r)
			}
			return
		}
		if id := strings.TrimSuffix(rest, "/oauth-token"); id != rest {
			switch r.Method {
			case http.MethodGet:
				h.oauthTokenResolve(w, r, id)
			default:
				http.NotFound(w, r)
			}
			return
		}
		id := rest
		switch r.Method {
		case http.MethodGet:
			h.get(w, r, id)
		case http.MethodPatch:
			h.patch(w, r, id)
		case http.MethodDelete:
			h.delete(w, r, id)
		default:
			http.NotFound(w, r)
		}
	})
}

// RegisterInternal mounts the routes that must NOT sit behind this
// service's blanket per-user JWT middleware (cmd/main.go wraps Register's
// mux with verifier.HTTPMiddleware before anything in it runs) — the
// execution-core-triggered tools/call dispatch path has no per-user bearer
// to present, only the org-derived X-Mcp-Service-Token (see
// expectedInternalServiceToken), which that middleware doesn't understand and
// would reject before these handlers' own (sole) gate ever ran. Callers must
// mount this directly on the UNWRAPPED mux (main.go's publicMux), never on
// the one passed to Register.
func (h *MCPHandler) RegisterInternal(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/internal/mcp/oauth-token", h.oauthTokenResolveInternal)
	mux.HandleFunc("/api/v1/internal/mcp/oauth-tokens", h.oauthTokensUpsertInternal)
}

// mcpOAuthTokensUpsertBody is what model-gateway POSTs right after a
// successful authorization-code exchange, and again after every refresh.
type mcpOAuthTokensUpsertBody struct {
	AccessToken   string `json:"access_token"`
	RefreshToken  string `json:"refresh_token"`
	TokenType     string `json:"token_type"`
	Scope         string `json:"scope"`
	ExpiresInSecs *int64 `json:"expires_in_seconds"`
	TokenEndpoint string `json:"token_endpoint"`
	ClientID      string `json:"client_id"`
}

// oauthTokensUpsert stores (encrypted) OAuth tokens for an MCP server.
func (h *MCPHandler) oauthTokensUpsert(w http.ResponseWriter, r *http.Request, id string) {
	orgID, ok := mcpVerifiedOrganization(r, true)
	if !ok {
		jsonErr(w, "capability write scope required", http.StatusForbidden)
		return
	}
	h.writeEncryptedOAuthTokens(w, r, id, orgID)
}

// oauthTokensUpsertInternal is the mirror of oauthTokenResolveInternal for
// writes: model-gateway refreshes an expired access token itself (it owns the
// OAuth protocol; capability-core only encrypts and stores) and has to persist
// the result on a path with no per-user bearer, so the same shared service
// token is this route's sole gate. Both paths funnel into the one
// writeEncryptedOAuthTokens body below, which keeps the vault AAD binding
// identical — a row written through either path decrypts through either.
func (h *MCPHandler) oauthTokensUpsertInternal(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPut {
		http.NotFound(w, r)
		return
	}
	// Params first, then the gate: the service token is bound to the org this
	// call targets, so the gate cannot be evaluated before org_id is known.
	id, orgID, ok := internalTokenTarget(r)
	if !ok {
		jsonErr(w, "server_id and org_id query parameters are required", http.StatusBadRequest)
		return
	}
	if !h.authorizedInternalServiceCall(r, orgID) {
		jsonErr(w, "invalid service token", http.StatusForbidden)
		return
	}
	h.writeEncryptedOAuthTokens(w, r, id, orgID)
}

func (h *MCPHandler) writeEncryptedOAuthTokens(w http.ResponseWriter, r *http.Request, id, orgID string) {
	if h.vault == nil {
		jsonErr(w, "token encryption is not configured", http.StatusServiceUnavailable)
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, maxMCPRegistrationBodyBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	var body mcpOAuthTokensUpsertBody
	if err := decoder.Decode(&body); err != nil {
		jsonErr(w, "invalid oauth token payload", http.StatusBadRequest)
		return
	}
	if body.AccessToken == "" {
		jsonErr(w, "access_token is required", http.StatusBadRequest)
		return
	}
	// Confirm the server exists and belongs to this org before attaching
	// tokens to it — also guards against a bogus/foreign server id.
	var exists bool
	if err := h.pool.QueryRow(r.Context(),
		`SELECT EXISTS(SELECT 1 FROM mcp_servers WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL)`,
		id, orgID,
	).Scan(&exists); err != nil {
		slog.Error("mcp oauth token server lookup failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	if !exists {
		jsonErr(w, "mcp server not found", http.StatusNotFound)
		return
	}

	// AAD binds ciphertext to this exact server+org pair, mirroring
	// integration-corev2's vault usage — ciphertext copied to another row
	// (or another org's row of the same server id, if that were ever
	// possible) fails to decrypt rather than silently decrypting wrong.
	aad := []byte(id + ":" + orgID)
	encAccess, err := h.vault.Encrypt(body.AccessToken, aad)
	if err != nil {
		slog.Error("encrypt mcp access token failed", "error", err)
		jsonErr(w, "token encryption failed", http.StatusInternalServerError)
		return
	}
	encRefresh := ""
	if body.RefreshToken != "" {
		encRefresh, err = h.vault.Encrypt(body.RefreshToken, aad)
		if err != nil {
			slog.Error("encrypt mcp refresh token failed", "error", err)
			jsonErr(w, "token encryption failed", http.StatusInternalServerError)
			return
		}
	}
	tokenType := body.TokenType
	if tokenType == "" {
		tokenType = "Bearer"
	}
	now := time.Now().UTC()
	var expiresAt *time.Time
	if body.ExpiresInSecs != nil {
		t := now.Add(time.Duration(*body.ExpiresInSecs) * time.Second)
		expiresAt = &t
	}
	rowID := id + ":" + orgID
	_, err = h.pool.Exec(r.Context(), `
		INSERT INTO mcp_oauth_tokens (id, server_id, org_id, access_token, refresh_token,
		    token_type, scope, expires_at, token_endpoint, client_id, created_at, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11)
		ON CONFLICT (server_id, org_id) DO UPDATE SET
		    access_token=$4, refresh_token=$5, token_type=$6, scope=$7,
		    expires_at=$8, token_endpoint=$9, client_id=$10, updated_at=$11
	`, rowID, id, orgID, encAccess, encRefresh, tokenType, body.Scope, expiresAt,
		body.TokenEndpoint, body.ClientID, now)
	if err != nil {
		slog.Error("store mcp oauth tokens failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"stored": true})
}

// oauthTokenResolve returns the DECRYPTED current access token plus enough
// metadata (token_endpoint, client_id) for the caller to refresh it itself —
// capability-core stores and encrypts, it never speaks OAuth to the
// authorization server. Requires the same write-level trust as storing: this
// is a live credential, not a public projection.
func (h *MCPHandler) oauthTokenResolve(w http.ResponseWriter, r *http.Request, id string) {
	orgID, ok := mcpVerifiedOrganization(r, true)
	if !ok {
		jsonErr(w, "capability write scope required", http.StatusForbidden)
		return
	}
	h.writeDecryptedOAuthToken(w, r, id, orgID)
}

// oauthTokenResolveInternal is the execution-core-triggered tools/call
// dispatch's path: that call has no live per-user bearer to present (see
// mcpServiceToken doc comment), so it cannot use oauthTokenResolve above,
// which sits behind this service's blanket per-user JWT middleware
// (cmd/main.go's verifier.HTTPMiddleware wrapping protectedMux) — a request
// carrying only X-Mcp-Service-Token would be rejected by that middleware
// before ever reaching this handler's body. Registered directly on the
// unwrapped publicMux instead (see RegisterInternal), with the service
// token as its OWN, sole gate — there is no per-user fallback here, unlike
// oauthTokenResolve, since nothing but that one caller ever reaches this path.
func (h *MCPHandler) oauthTokenResolveInternal(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.NotFound(w, r)
		return
	}
	// Params first, then the gate — see oauthTokensUpsertInternal.
	id, orgID, ok := internalTokenTarget(r)
	if !ok {
		jsonErr(w, "server_id and org_id query parameters are required", http.StatusBadRequest)
		return
	}
	if !h.authorizedInternalServiceCall(r, orgID) {
		jsonErr(w, "invalid service token", http.StatusForbidden)
		return
	}
	h.writeDecryptedOAuthToken(w, r, id, orgID)
}

func (h *MCPHandler) writeDecryptedOAuthToken(w http.ResponseWriter, r *http.Request, id, orgID string) {
	if h.vault == nil {
		jsonErr(w, "token encryption is not configured", http.StatusServiceUnavailable)
		return
	}
	var encAccess, encRefresh, tokenType, scope, tokenEndpoint, clientID string
	var expiresAt *time.Time
	err := h.pool.QueryRow(r.Context(), `
		SELECT access_token, refresh_token, token_type, scope, expires_at, token_endpoint, client_id
		FROM mcp_oauth_tokens WHERE server_id=$1 AND org_id=$2
	`, id, orgID).Scan(&encAccess, &encRefresh, &tokenType, &scope, &expiresAt, &tokenEndpoint, &clientID)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, "no oauth tokens stored for this server", http.StatusNotFound)
		return
	}
	if err != nil {
		slog.Error("resolve mcp oauth token failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	aad := []byte(id + ":" + orgID)
	accessToken, err := h.vault.Decrypt(encAccess, aad)
	if err != nil {
		slog.Error("decrypt mcp access token failed", "error", err)
		jsonErr(w, "token decryption failed", http.StatusInternalServerError)
		return
	}
	refreshToken := ""
	if encRefresh != "" {
		refreshToken, err = h.vault.Decrypt(encRefresh, aad)
		if err != nil {
			slog.Error("decrypt mcp refresh token failed", "error", err)
			jsonErr(w, "token decryption failed", http.StatusInternalServerError)
			return
		}
	}
	resp := map[string]any{
		"access_token":   accessToken,
		"refresh_token":  refreshToken,
		"token_type":     tokenType,
		"scope":          scope,
		"token_endpoint": tokenEndpoint,
		"client_id":      clientID,
	}
	if expiresAt != nil {
		resp["expires_at"] = expiresAt.Format(time.RFC3339)
	}
	writeJSON(w, resp)
}

func (h *MCPHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID, ok := mcpVerifiedOrganization(r, false)
		if !ok {
			jsonErr(w, "authentication required", http.StatusUnauthorized)
			return
		}
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, description, endpoint_url, transport, auth_kind,
			       config_json, scope, enabled, rollout_state, risk_level, created_at, updated_at
			FROM mcp_servers WHERE (org_id=$1 OR org_id='global') AND deleted_at IS NULL ORDER BY name
		`, orgID)
		if err != nil {
			slog.Error("list MCP servers failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		var servers []mcpServerRow
		for rows.Next() {
			var s mcpServerRow
			if err := rows.Scan(&s.ID, &s.OrgID, &s.Name, &s.Description, &s.EndpointURL,
				&s.Transport, &s.AuthKind, &s.ConfigJSON, &s.Scope, &s.Enabled,
				&s.RolloutState, &s.RiskLevel, &s.CreatedAt, &s.UpdatedAt); err != nil {
				slog.Error("scan MCP server failed", "error", err)
				jsonErr(w, "database unavailable", http.StatusInternalServerError)
				return
			}
			hydrateMCPServerView(&s)
			servers = append(servers, s)
		}
		writeJSON(w, map[string]any{"servers": servers})
	case http.MethodPost, http.MethodPut:
		orgID, ok := mcpVerifiedOrganization(r, true)
		if !ok {
			jsonErr(w, "capability write scope required", http.StatusForbidden)
			return
		}
		input, err := decodeMCPRegistration(w, r)
		if err != nil {
			jsonErr(w, "invalid MCP registration", http.StatusBadRequest)
			return
		}
		s, config, err := h.normalizeMCPRegistration(r.Context(), input, orgID)
		if err != nil {
			jsonErr(w, "invalid MCP registration", http.StatusUnprocessableEntity)
			return
		}
		now := time.Now().UTC()
		cfgJSON, err := json.Marshal(config)
		if err != nil {
			jsonErr(w, "invalid MCP registration", http.StatusBadRequest)
			return
		}
		// ON CONFLICT (org_id, name) intentionally never touches `id` — a
		// second registration under the same name (e.g. reconnecting after
		// model-gateway's ephemeral cache was wiped by a restart, so it
		// proposes a brand-new id for what is durably the same server) must
		// keep the ORIGINAL id, since every other durable record scoped to
		// this server (mcp_oauth_tokens' FK, ownership) points at it.
		// RETURNING id is required, not cosmetic: without it this handler
		// echoed back the caller-supplied id even when the conflict path
		// silently kept a different one, so a caller's own follow-up call
		// (e.g. the OAuth callback's token-storage PUT) would address a row
		// that never existed under that id and 404.
		var persistedID string
		err = h.pool.QueryRow(r.Context(), `
			INSERT INTO mcp_servers (id, org_id, name, description, endpoint_url, transport, auth_kind,
			    config_json, scope, enabled, rollout_state, risk_level, created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
			ON CONFLICT (org_id, name) WHERE deleted_at IS NULL DO UPDATE SET
			    description=$4, endpoint_url=$5, transport=$6, auth_kind=$7,
			    config_json=$8, scope=$9, enabled=$10, rollout_state=$11,
			    risk_level=$12, updated_at=$14
			RETURNING id
		`, s.ID, s.OrgID, s.Name, s.Description, s.EndpointURL, s.Transport, s.AuthKind,
			cfgJSON, s.Scope, s.Enabled, s.RolloutState, s.RiskLevel, now, now).Scan(&persistedID)
		if err != nil {
			slog.Error("persist MCP server failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		// Reconcile (matrix §4.3): notify cache holders an MCP server changed.
		// Best-effort — never block the mutation on event emission.
		if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindMCPServer,
			reconcile.ActionRegistered, persistedID, s.OrgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", reconcile.KindMCPServer, "id", persistedID, "error", eerr)
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": persistedID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *MCPHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	orgID, ok := mcpVerifiedOrganization(r, false)
	if !ok {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	var s mcpServerRow
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, name, description, endpoint_url, transport, auth_kind,
		       config_json, scope, enabled, rollout_state, risk_level, created_at, updated_at
		FROM mcp_servers
		WHERE id=$1 AND (org_id=$2 OR org_id='global') AND deleted_at IS NULL
	`, id, orgID).Scan(&s.ID, &s.OrgID, &s.Name, &s.Description, &s.EndpointURL,
		&s.Transport, &s.AuthKind, &s.ConfigJSON, &s.Scope, &s.Enabled,
		&s.RolloutState, &s.RiskLevel, &s.CreatedAt, &s.UpdatedAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	hydrateMCPServerView(&s)
	writeJSON(w, s)
}

func (h *MCPHandler) patch(w http.ResponseWriter, r *http.Request, id string) {
	orgID, ok := mcpVerifiedOrganization(r, true)
	if !ok {
		jsonErr(w, "capability write scope required", http.StatusForbidden)
		return
	}
	var update struct {
		Enabled      *bool     `json:"enabled"`
		RolloutState string    `json:"rollout_state"`
		SharedWith   *[]string `json:"shared_with"`
	}
	r.Body = http.MaxBytesReader(w, r.Body, maxMCPRegistrationBodyBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&update); err != nil || ensureJSONEOF(decoder) != nil {
		jsonErr(w, "invalid MCP update", http.StatusBadRequest)
		return
	}
	if update.Enabled == nil && update.RolloutState == "" && update.SharedWith == nil {
		jsonErr(w, "invalid MCP update", http.StatusUnprocessableEntity)
		return
	}
	// Enabling or promoting a record must go through PUT so the endpoint,
	// DNS answer, auth reference, and exact tool allowlist are all revalidated.
	if (update.Enabled != nil && *update.Enabled) || update.RolloutState == "stable" || update.RolloutState == "canary" {
		jsonErr(w, "full MCP registration required", http.StatusUnprocessableEntity)
		return
	}
	if update.RolloutState != "" && !validMCPRolloutState(update.RolloutState) {
		jsonErr(w, "invalid MCP update", http.StatusUnprocessableEntity)
		return
	}
	var configJSON any
	if update.SharedWith != nil {
		var stored any
		var scope, authKind string
		if err := h.pool.QueryRow(r.Context(), `
			SELECT config_json, scope, auth_kind
			FROM mcp_servers WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL
		`, id, orgID).Scan(&stored, &scope, &authKind); err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				jsonErr(w, "MCP server not found", http.StatusNotFound)
			} else {
				slog.Error("read MCP sharing config failed", "error", err)
				jsonErr(w, "database unavailable", http.StatusInternalServerError)
			}
			return
		}
		config, err := decodeStoredMCPConfig(stored)
		if err != nil {
			jsonErr(w, "MCP server configuration is invalid", http.StatusUnprocessableEntity)
			return
		}
		// A user-owned resource may only be shared by its durable owner. The
		// gateway performs the same check, but capability-core must enforce it
		// independently because it is the system of record.
		principal, hasPrincipal := authctx.PrincipalFromContext(r.Context())
		if config.OwnerUserID != "" && (!hasPrincipal || principal.ActorID != config.OwnerUserID) {
			jsonErr(w, "only the MCP owner may change sharing", http.StatusForbidden)
			return
		}
		config.SharedWith = append([]string(nil), (*update.SharedWith)...)
		config, err = normalizeMCPConfig(config, authKind, scope)
		if err != nil {
			jsonErr(w, "invalid MCP sharing configuration", http.StatusUnprocessableEntity)
			return
		}
		configJSON, err = json.Marshal(config)
		if err != nil {
			jsonErr(w, "invalid MCP sharing configuration", http.StatusUnprocessableEntity)
			return
		}
	}
	now := time.Now().UTC()
	var enabled any
	if update.Enabled != nil {
		enabled = *update.Enabled
	}
	result, err := h.pool.Exec(r.Context(), `
		UPDATE mcp_servers
		SET enabled=COALESCE($1, enabled), rollout_state=COALESCE(NULLIF($2, ''), rollout_state),
		    config_json=COALESCE($3, config_json), updated_at=$4
		WHERE id=$5 AND org_id=$6 AND deleted_at IS NULL
	`, enabled, update.RolloutState, configJSON, now, id, orgID)
	if !writeSingleScopedMutation(w, "MCP server", result, err) {
		return
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *MCPHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	orgID, ok := mcpVerifiedOrganization(r, true)
	if !ok {
		jsonErr(w, "capability write scope required", http.StatusForbidden)
		return
	}
	now := time.Now().UTC()
	result, err := h.pool.Exec(r.Context(), `UPDATE mcp_servers SET deleted_at=$1, updated_at=$1 WHERE id=$2 AND org_id=$3 AND deleted_at IS NULL`, now, id, orgID)
	if !writeSingleScopedMutation(w, "MCP server", result, err) {
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// ---------------------------------------------------------------------------
// RoutingHandler  /api/v1/routing
// ---------------------------------------------------------------------------

// RoutingHandler handles CRUD for routing_policies.
type RoutingHandler struct {
	pool registryDatabase
	pub  publisher.EventPublisher
}

// NewRoutingHandler constructs the handler.
func NewRoutingHandler(pool registryDatabase) *RoutingHandler {
	return &RoutingHandler{pool: pool}
}

// WithPublisher wires reconcile-event emission (matrix §4.3). Optional, nil-safe.
func (h *RoutingHandler) WithPublisher(pub publisher.EventPublisher) *RoutingHandler {
	h.pub = pub
	return h
}

// Register mounts routes.
func (h *RoutingHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/routing", h.listOrCreate)
	mux.HandleFunc("/api/v1/routing/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/routing/"):]
		switch r.Method {
		case http.MethodGet:
			h.get(w, r, id)
		case http.MethodPatch:
			h.patch(w, r, id)
		case http.MethodDelete:
			h.delete(w, r, id)
		default:
			http.NotFound(w, r)
		}
	})
}

type routingPolicyRow struct {
	ID          string    `json:"id"`
	OrgID       string    `json:"org_id"`
	Name        string    `json:"name"`
	Description string    `json:"description"`
	Strategy    string    `json:"strategy"`
	ConfigJSON  any       `json:"config_json"`
	ModelIDs    []string  `json:"model_ids"`
	Priority    int       `json:"priority"`
	Enabled     bool      `json:"enabled"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

func (h *RoutingHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID := verifiedOrganizationID(r)
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, description, strategy, config_json, model_ids,
			       priority, enabled, created_at, updated_at
			FROM routing_policies WHERE (org_id=$1 OR org_id='global') AND deleted_at IS NULL
			ORDER BY priority DESC, name
		`, orgID)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		var policies []routingPolicyRow
		for rows.Next() {
			var p routingPolicyRow
			if err := rows.Scan(&p.ID, &p.OrgID, &p.Name, &p.Description, &p.Strategy,
				&p.ConfigJSON, &p.ModelIDs, &p.Priority, &p.Enabled, &p.CreatedAt, &p.UpdatedAt); err != nil {
				jsonErr(w, err.Error(), http.StatusInternalServerError)
				return
			}
			policies = append(policies, p)
		}
		writeJSON(w, map[string]any{"policies": policies})
	case http.MethodPost:
		var p routingPolicyRow
		if err := json.NewDecoder(r.Body).Decode(&p); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		p.OrgID = verifiedOrganizationID(r)
		if p.ID == "" {
			p.ID = "rp_" + uuid.New().String()
		}
		now := time.Now().UTC()
		cfgJSON, _ := json.Marshal(p.ConfigJSON)
		if p.ModelIDs == nil {
			p.ModelIDs = []string{}
		}
		_, err := h.pool.Exec(r.Context(), `
			INSERT INTO routing_policies (id, org_id, name, description, strategy, config_json, model_ids,
			    priority, enabled, created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
		`, p.ID, p.OrgID, p.Name, p.Description, p.Strategy, cfgJSON, p.ModelIDs,
			p.Priority, p.Enabled, now, now)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindRoutingPolicy,
			reconcile.ActionRegistered, p.ID, p.OrgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", reconcile.KindRoutingPolicy, "id", p.ID, "error", eerr)
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": p.ID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *RoutingHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	var p routingPolicyRow
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, name, description, strategy, config_json, model_ids,
		       priority, enabled, created_at, updated_at
		FROM routing_policies
		WHERE id=$1 AND (org_id=$2 OR org_id='global') AND deleted_at IS NULL
	`, id, verifiedOrganizationID(r)).Scan(&p.ID, &p.OrgID, &p.Name, &p.Description, &p.Strategy,
		&p.ConfigJSON, &p.ModelIDs, &p.Priority, &p.Enabled, &p.CreatedAt, &p.UpdatedAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, p)
}

func (h *RoutingHandler) patch(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Enabled  *bool `json:"enabled"`
		Priority *int  `json:"priority"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	if update.Enabled == nil && update.Priority == nil {
		jsonErr(w, "at least one routing policy field is required", http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	orgID := verifiedOrganizationID(r)
	var enabled any
	if update.Enabled != nil {
		enabled = *update.Enabled
	}
	var priority any
	if update.Priority != nil {
		priority = *update.Priority
	}
	result, err := h.pool.Exec(r.Context(), `
		UPDATE routing_policies
		SET enabled=COALESCE($1, enabled), priority=COALESCE($2, priority), updated_at=$3
		WHERE id=$4 AND org_id=$5 AND deleted_at IS NULL
	`, enabled, priority, now, id, orgID)
	if !writeSingleScopedMutation(w, "routing policy", result, err) {
		return
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *RoutingHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	now := time.Now().UTC()
	result, err := h.pool.Exec(r.Context(), `UPDATE routing_policies SET deleted_at=$1, updated_at=$1 WHERE id=$2 AND org_id=$3 AND deleted_at IS NULL`, now, id, verifiedOrganizationID(r))
	if !writeSingleScopedMutation(w, "routing policy", result, err) {
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// ---------------------------------------------------------------------------
// SafetyHandler  /api/v1/safety
// ---------------------------------------------------------------------------

// SafetyHandler handles CRUD for safety_policies.
type SafetyHandler struct {
	pool registryDatabase
	pub  publisher.EventPublisher
}

// NewSafetyHandler constructs the handler.
func NewSafetyHandler(pool registryDatabase) *SafetyHandler {
	return &SafetyHandler{pool: pool}
}

// WithPublisher wires reconcile-event emission (matrix §4.3). Optional, nil-safe.
func (h *SafetyHandler) WithPublisher(pub publisher.EventPublisher) *SafetyHandler {
	h.pub = pub
	return h
}

// Register mounts routes.
func (h *SafetyHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/safety", h.listOrCreate)
	mux.HandleFunc("/api/v1/safety/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/safety/"):]
		switch r.Method {
		case http.MethodGet:
			h.get(w, r, id)
		case http.MethodPatch:
			h.patch(w, r, id)
		case http.MethodDelete:
			h.delete(w, r, id)
		default:
			http.NotFound(w, r)
		}
	})
}

type safetyPolicyRow struct {
	ID          string    `json:"id"`
	OrgID       string    `json:"org_id"`
	Name        string    `json:"name"`
	Description string    `json:"description"`
	Kind        string    `json:"kind"`
	ConfigJSON  any       `json:"config_json"`
	AppliesTo   []string  `json:"applies_to"`
	Priority    int       `json:"priority"`
	Enabled     bool      `json:"enabled"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

func (h *SafetyHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID := verifiedOrganizationID(r)
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, description, kind, config_json, applies_to,
			       priority, enabled, created_at, updated_at
			FROM safety_policies WHERE (org_id=$1 OR org_id='global') AND deleted_at IS NULL
			ORDER BY priority DESC, name
		`, orgID)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		var policies []safetyPolicyRow
		for rows.Next() {
			var p safetyPolicyRow
			if err := rows.Scan(&p.ID, &p.OrgID, &p.Name, &p.Description, &p.Kind,
				&p.ConfigJSON, &p.AppliesTo, &p.Priority, &p.Enabled, &p.CreatedAt, &p.UpdatedAt); err != nil {
				jsonErr(w, err.Error(), http.StatusInternalServerError)
				return
			}
			policies = append(policies, p)
		}
		writeJSON(w, map[string]any{"policies": policies})
	case http.MethodPost:
		var p safetyPolicyRow
		if err := json.NewDecoder(r.Body).Decode(&p); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		p.OrgID = verifiedOrganizationID(r)
		if p.ID == "" {
			p.ID = "sp_" + uuid.New().String()
		}
		now := time.Now().UTC()
		cfgJSON, _ := json.Marshal(p.ConfigJSON)
		if p.AppliesTo == nil {
			p.AppliesTo = []string{}
		}
		_, err := h.pool.Exec(r.Context(), `
			INSERT INTO safety_policies (id, org_id, name, description, kind, config_json, applies_to,
			    priority, enabled, created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
		`, p.ID, p.OrgID, p.Name, p.Description, p.Kind, cfgJSON, p.AppliesTo,
			p.Priority, p.Enabled, now, now)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindSafetyPolicy,
			reconcile.ActionRegistered, p.ID, p.OrgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", reconcile.KindSafetyPolicy, "id", p.ID, "error", eerr)
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": p.ID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *SafetyHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	var p safetyPolicyRow
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, name, description, kind, config_json, applies_to,
		       priority, enabled, created_at, updated_at
		FROM safety_policies
		WHERE id=$1 AND (org_id=$2 OR org_id='global') AND deleted_at IS NULL
	`, id, verifiedOrganizationID(r)).Scan(&p.ID, &p.OrgID, &p.Name, &p.Description, &p.Kind,
		&p.ConfigJSON, &p.AppliesTo, &p.Priority, &p.Enabled, &p.CreatedAt, &p.UpdatedAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, p)
}

func (h *SafetyHandler) patch(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Enabled  *bool `json:"enabled"`
		Priority *int  `json:"priority"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	if update.Enabled == nil && update.Priority == nil {
		jsonErr(w, "at least one safety policy field is required", http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	orgID := verifiedOrganizationID(r)
	var enabled any
	if update.Enabled != nil {
		enabled = *update.Enabled
	}
	var priority any
	if update.Priority != nil {
		priority = *update.Priority
	}
	result, err := h.pool.Exec(r.Context(), `
		UPDATE safety_policies
		SET enabled=COALESCE($1, enabled), priority=COALESCE($2, priority), updated_at=$3
		WHERE id=$4 AND org_id=$5 AND deleted_at IS NULL
	`, enabled, priority, now, id, orgID)
	if !writeSingleScopedMutation(w, "safety policy", result, err) {
		return
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *SafetyHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	now := time.Now().UTC()
	result, err := h.pool.Exec(r.Context(), `UPDATE safety_policies SET deleted_at=$1, updated_at=$1 WHERE id=$2 AND org_id=$3 AND deleted_at IS NULL`, now, id, verifiedOrganizationID(r))
	if !writeSingleScopedMutation(w, "safety policy", result, err) {
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

func writeSingleScopedMutation(w http.ResponseWriter, resource string, result pgconn.CommandTag, err error) bool {
	if err != nil {
		slog.Error("scoped mutation failed", "resource", resource, "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return false
	}
	if result.RowsAffected() != 1 {
		jsonErr(w, "not found", http.StatusNotFound)
		return false
	}
	return true
}
