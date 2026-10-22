// Package api — per-tool registry rows for MCP servers (WS05-S2).
//
// A capability row describes one thing, but an MCP server is many tools, each
// able to do something different, and each defined by text the server controls.
// This file keeps one registry row per discovered tool: its class and approval
// floor, and a digest of the definition a person accepted.
//
//   - The gateway reports what it discovered over an org-scoped HMAC route
//     (observeToolsInternal). The first digest seen for a tool is pinned
//     (trust on first use). A later digest that differs drifts the tool: it is
//     switched off, its class is left as it was, and it stays off until a person
//     approves the new definition (approveTool). Observation can create and
//     refresh a row and can only ever tighten a class; it cannot approve a
//     drifted tool, lower a class or touch a policy.
//   - A tool's class is derived from the annotations the server declared, and
//     those are a third party's claims: unless the server's registrant chose to
//     trust them, a read-only claim derives as an external write.
//   - Nothing stored here is the definition itself. A row holds digests and the
//     server's boolean hints, never a description, a schema, an argument or an
//     output, so a tenant's tool text is not retained by the registry.
package api

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"regexp"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

const (
	// maxObservationBodyBytes and maxObservationsPerBatch bound one observation
	// request; the gateway splits a larger discovery into batches of the second.
	maxObservationBodyBytes = 512 << 10
	maxObservationsPerBatch = 200
	// observationRetries is how often a row that changed under us is re-read and
	// re-decided before the observation is reported as contended.
	observationRetries = 3
	// reservedGlobalOrg is the process-wide registry tenant; observation can
	// never write into it.
	reservedGlobalOrg = "global"

	auditActionToolObserved = "mcp_tool_observed"
	auditActionToolDrifted  = "mcp_tool_drifted"
	auditActionToolApproved = "mcp_tool_approved"
)

// Review states of a tool row (the capability_tools.review_state values).
const (
	reviewApproved = "approved"
	reviewPending  = "pending"
	reviewDrifted  = "drifted"
)

var sha256HexPattern = regexp.MustCompile(`^[0-9a-f]{64}$`)

// The server's declared side effect, as the gateway interprets the MCP hints.
func validDeclaredSideEffect(value string) bool {
	switch value {
	case models.DeclaredReadOnly, models.DeclaredNonDestructiveWrite, models.DeclaredDestructive, models.DeclaredUndeclared:
		return true
	default:
		return false
	}
}

// observedHints is the four boolean annotation hints a server may send. `title`
// is free text and is deliberately not accepted. Null means "not stated".
type observedHints struct {
	ReadOnlyHint    *bool `json:"read_only_hint"`
	DestructiveHint *bool `json:"destructive_hint"`
	IdempotentHint  *bool `json:"idempotent_hint"`
	OpenWorldHint   *bool `json:"open_world_hint"`
}

// toolObservation is one discovered tool as the gateway reports it.
type toolObservation struct {
	ServerID           string        `json:"server_id"`
	ToolName           string        `json:"tool_name"`
	QualifiedName      string        `json:"qualified_name"`
	Digest             string        `json:"digest"`
	DescriptionSHA256  string        `json:"description_sha256"`
	SchemaSHA256       string        `json:"schema_sha256"`
	DeclaredSideEffect string        `json:"declared_side_effect"`
	Annotations        observedHints `json:"annotations"`
}

type toolObservationBatch struct {
	Observations []toolObservation `json:"observations"`
}

// observationResult is the per-tool outcome returned to the gateway. Reasons are
// short codes; nothing from the request is echoed back.
type observationResult struct {
	QualifiedName   string `json:"qualified_name"`
	Outcome         string `json:"outcome"`
	Reason          string `json:"reason,omitempty"`
	ReviewState     string `json:"review_state,omitempty"`
	SideEffectClass string `json:"side_effect_class,omitempty"`
}

const (
	outcomeRecorded  = "recorded"
	outcomeRejected  = "rejected"
	outcomeContended = "contended"
)

// qualifiedMCPToolName is the name a tool is offered under, mcp__<server>__<tool>.
func qualifiedMCPToolName(serverID, toolName string) string {
	return "mcp__" + serverID + "__" + toolName
}

// mcpToolCapabilityID is the capability id of a tool's row: a function of the
// org and the qualified name, so the same tool always lands on the same row and
// two orgs never share one.
func mcpToolCapabilityID(orgID, qualifiedName string) string {
	sum := sha256.Sum256([]byte(orgID + "\x00" + qualifiedName))
	return "cap.mcp_tool." + hex.EncodeToString(sum[:16])
}

// mcpToolVersion changes whenever the pinned definition does, so a decision
// evidence bound to one version cannot be replayed against a re-approved tool.
func mcpToolVersion(pinnedDigest string) string {
	if len(pinnedDigest) < 12 {
		return "1.0.0"
	}
	return "1.0.0-" + pinnedDigest[:12]
}

// riskLevelForClass maps a derived class onto the coarse risk level the
// registry has always carried: classes that only read, compute or write to the
// run's own context are low, anything that reaches outside is high.
func riskLevelForClass(class string) string {
	switch class {
	case models.SideEffectRead, models.SideEffectSandboxCompute, models.SideEffectContextWrite:
		return models.RiskLow
	default:
		return models.RiskHigh
	}
}

// validateObservation rejects an observation that is malformed or that names a
// tool the registry could not address unambiguously. The qualified name is
// never taken from the caller: if one is supplied it must be the one the
// server and tool names produce.
func validateObservation(o toolObservation) string {
	switch {
	case !mcpIdentifier.MatchString(o.ServerID):
		return "invalid_server_id"
	case strings.Contains(o.ServerID, "__"):
		// mcp__<server>__<tool> is parsed on its first `__`, so a server id that
		// contains one makes two different tools share a name.
		return "ambiguous_server_id"
	case !mcpToolNamePattern.MatchString(o.ToolName):
		return "invalid_tool_name"
	case o.QualifiedName != "" && o.QualifiedName != qualifiedMCPToolName(o.ServerID, o.ToolName):
		return "qualified_name_mismatch"
	case !sha256HexPattern.MatchString(o.Digest):
		return "invalid_digest"
	case !sha256HexPattern.MatchString(o.DescriptionSHA256), !sha256HexPattern.MatchString(o.SchemaSHA256):
		return "invalid_hash"
	case !validDeclaredSideEffect(o.DeclaredSideEffect):
		return "invalid_declared_side_effect"
	default:
		return ""
	}
}

// observedServer is what the registry knows about the server a tool belongs to.
type observedServer struct {
	registered bool
	enabled    bool
	trusted    bool
	allowlist  map[string]struct{}
}

func (s observedServer) allowlists(tool string) bool {
	_, ok := s.allowlist[tool]
	return ok
}

// callable is whether the server's own state lets any of its tools run.
func (s observedServer) callable(tool string) bool {
	return s.registered && s.enabled && s.allowlists(tool)
}

// storedTool is the part of an existing capability_tools row a decision reads.
type storedTool struct {
	exists   bool
	class    string
	floor    string
	pinned   string
	observed string
	state    string
}

// toolDecision is everything an observation writes for one tool.
type toolDecision struct {
	class      string
	floor      string
	pinned     string
	state      string
	enabled    bool
	reasonCode string
	// audit is the registry_audit_log action to append, or empty for a plain
	// refresh that changed nothing worth a row.
	audit string
	// unchanged means the observation must not write at all: the row is left
	// exactly as it is. Only a drifted row with a digest that is not new evidence
	// takes this path (see decideObservation).
	unchanged bool
}

// decideObservation applies the observation rules to one tool. It is pure: the
// same stored row, report and server state always give the same decision.
//
//   - First sight pins the reported digest (trust on first use). The tool is
//     approved at once only if its server is registered and the tool is on the
//     server's allowlist; otherwise it waits as pending.
//   - A digest that differs from the pin drifts the tool. Its class is left as
//     it was and it stays off until a person approves the new definition.
//   - Otherwise the class can only rise: the stricter of what is stored and what
//     the declared side effect derives to. Observation never lowers a class.
//   - A pending tool whose server and allowlist now qualify is approved on the
//     digest it was pinned at. It never takes a new digest on trust.
//   - Observation never moves a row out of drifted. A drifted tool whose
//     reported digest equals the pin is not restored: a replica with a stale
//     tool cache reports the old digest too, so it is not evidence the server
//     is back to what a person approved. The row is left untouched (not even its
//     observed digest, which is the one a person is about to review), and only
//     approveTool, a person looking at the observed digest, changes it.
func decideObservation(stored storedTool, o toolObservation, server observedServer) toolDecision {
	derived := models.DeriveMCPClass(o.DeclaredSideEffect, server.trusted)
	qualifies := server.registered && server.allowlists(o.ToolName)
	d := toolDecision{}

	switch {
	case !stored.exists:
		d.class = derived
		d.pinned = o.Digest
		d.state = reviewPending
		if qualifies {
			d.state = reviewApproved
		}
		d.audit = auditActionToolObserved
	case stored.pinned != "" && o.Digest != stored.pinned:
		d.class = models.NormalizeSideEffectClass(stored.class)
		d.pinned = stored.pinned
		d.state = reviewDrifted
		if stored.state != reviewDrifted || stored.observed != o.Digest {
			d.audit = auditActionToolDrifted
		}
	case stored.state == reviewDrifted:
		d.class = models.NormalizeSideEffectClass(stored.class)
		d.pinned = stored.pinned
		d.state = reviewDrifted
		d.unchanged = true
	default:
		d.class = models.StricterSideEffectClass(stored.class, derived)
		d.pinned = stored.pinned
		if d.pinned == "" {
			d.pinned = o.Digest
		}
		d.state = reviewPending
		if stored.state == reviewApproved || qualifies {
			d.state = reviewApproved
		}
		if d.state != stored.state || d.class != stored.class {
			d.audit = auditActionToolObserved
		}
	}

	d.floor = models.StricterApprovalFloor(models.MinimumApprovalFloor(d.class), storedFloor(stored))
	d.enabled = d.state == reviewApproved && server.callable(o.ToolName)
	d.reasonCode = unavailableReasonFor(d.state, d.enabled)
	return d
}

// storedFloor is the floor of an existing row, or the weakest floor for a new
// one so the class alone decides it.
func storedFloor(stored storedTool) string {
	if !stored.exists {
		return models.ApprovalFloorNone
	}
	return models.NormalizeApprovalFloor(stored.floor)
}

// unavailableReasonFor names why a tool row is off, for the reason code carried
// on its capability row; empty when it is on.
func unavailableReasonFor(state string, enabled bool) string {
	switch {
	case enabled:
		return ""
	case state == reviewDrifted:
		return models.ReasonToolDefinitionChanged
	case state == reviewPending:
		return models.ReasonToolNotApproved
	default:
		return "capability_disabled"
	}
}

// availabilityFor is the capability availability columns of a tool row. A tool
// that is on has been seen just now (observedAt), which is the whole health
// attestation an MCP tool has; one that is off carries its reason.
func availabilityFor(enabled bool, reasonCode string) (state, reason, mode, cost string) {
	if enabled {
		return string(models.AvailabilityAvailable), "", models.ExecutionAgentic, models.CostVariable
	}
	reasonText := "The tool is not callable."
	switch reasonCode {
	case models.ReasonToolDefinitionChanged:
		reasonText = "The tool's definition changed since it was approved."
	case models.ReasonToolNotApproved:
		reasonText = "The tool has not been approved."
	case "capability_disabled":
		reasonText = "The tool server is disabled or no longer lists it."
	}
	return string(models.AvailabilityUnavailable), reasonText, models.ExecutionUnavailable, models.CostUnknown
}

// withMCPToolCascade makes a server mutation also switch off the capability rows
// of that server's tools, in the same statement. orgParam and serverParam are
// the placeholders (`$6`, `$5`) the wrapped update already binds.
func withMCPToolCascade(update, orgParam, serverParam string) string {
	return "WITH updated AS (" + update + " RETURNING id), cascade AS (" +
		"UPDATE capabilities SET enabled=false, availability_state='unavailable', " +
		"availability_reason_code='capability_disabled', " +
		"availability_reason='The MCP server is disabled or was removed.', " +
		"execution_mode='unavailable', updated_at=now() " +
		"WHERE kind='mcp_tool' AND org_id=" + orgParam + " AND deleted_at IS NULL " +
		"AND EXISTS (SELECT 1 FROM updated) " +
		"AND id IN (SELECT capability_id FROM capability_tools WHERE org_id=" + orgParam + " AND server_id=" + serverParam + ")" +
		") SELECT id FROM updated"
}

// observeToolsInternal is POST /api/v1/internal/mcp/tools/observe.
//
// Authority, stated plainly: the sole gate is the org-derived X-Mcp-Service-Token
// (see expectedInternalServiceToken), so the caller proves it is the gateway
// acting for this org, and a captured credential is worthless against another
// org. Holding it permits one thing, reporting what the gateway discovered. It
// does not permit approving a changed tool, lowering a class or changing a
// policy, and the process-wide `global` org is refused outright. The route is
// not behind the per-user JWT middleware (see RegisterInternal), so a ZDR org's
// tools are kept out by the caller: the gateway never reports for a ZDR caller.
func (h *MCPHandler) observeToolsInternal(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.NotFound(w, r)
		return
	}
	// Params first, then the gate: the credential is bound to the org this call
	// names, so the gate cannot run before the org is read.
	orgID := strings.TrimSpace(r.URL.Query().Get("org_id"))
	if orgID == "" {
		jsonErr(w, "org_id query parameter is required", http.StatusBadRequest)
		return
	}
	if !h.authorizedInternalServiceCall(r, orgID) {
		jsonErr(w, "invalid service token", http.StatusForbidden)
		return
	}
	if orgID == reservedGlobalOrg {
		jsonErr(w, "the global registry cannot be written by observation", http.StatusForbidden)
		return
	}

	r.Body = http.MaxBytesReader(w, r.Body, maxObservationBodyBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	var batch toolObservationBatch
	if err := decoder.Decode(&batch); err != nil || ensureJSONEOF(decoder) != nil {
		jsonErr(w, "invalid observation batch", http.StatusBadRequest)
		return
	}
	if len(batch.Observations) == 0 {
		jsonErr(w, "invalid observation batch", http.StatusBadRequest)
		return
	}
	if len(batch.Observations) > maxObservationsPerBatch {
		jsonErr(w, "too many observations in one batch", http.StatusRequestEntityTooLarge)
		return
	}

	now := time.Now().UTC()
	servers := make(map[string]observedServer)
	results := make([]observationResult, 0, len(batch.Observations))
	for _, o := range batch.Observations {
		result := observationResult{QualifiedName: qualifiedMCPToolName(o.ServerID, o.ToolName)}
		if reason := validateObservation(o); reason != "" {
			result.QualifiedName = ""
			result.Outcome, result.Reason = outcomeRejected, reason
			results = append(results, result)
			continue
		}
		server, known := servers[o.ServerID]
		if !known {
			var err error
			server, err = h.lookupObservedServer(r.Context(), orgID, o.ServerID)
			if err != nil {
				slog.Error("mcp tool observation server lookup failed", "error", err)
				jsonErr(w, "database unavailable", http.StatusInternalServerError)
				return
			}
			servers[o.ServerID] = server
		}
		outcome, err := h.recordObservation(r.Context(), orgID, o, server, now)
		if err != nil {
			slog.Error("mcp tool observation write failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		results = append(results, outcome)
	}
	writeJSON(w, map[string]any{"results": results})
}

// lookupObservedServer reads the registry's view of one server for an org. A
// missing server, or a stored record that no longer parses, is "not registered":
// the tool is recorded pending and nothing is callable.
func (h *MCPHandler) lookupObservedServer(ctx context.Context, orgID, serverID string) (observedServer, error) {
	var (
		enabled      bool
		rolloutState string
		stored       any
	)
	err := h.pool.QueryRow(ctx, `
		SELECT enabled, rollout_state, config_json
		FROM mcp_servers
		WHERE id=$1 AND (org_id=$2 OR org_id='global') AND deleted_at IS NULL
	`, serverID, orgID).Scan(&enabled, &rolloutState, &stored)
	if errors.Is(err, pgx.ErrNoRows) {
		return observedServer{}, nil
	}
	if err != nil {
		return observedServer{}, err
	}
	config, err := decodeStoredMCPConfig(stored)
	if err != nil {
		return observedServer{}, nil
	}
	server := observedServer{
		registered: true,
		enabled:    enabled && (rolloutState == "stable" || rolloutState == "canary"),
		trusted:    config.annotationsTrusted(),
		allowlist:  make(map[string]struct{}, len(config.ToolAllowlist)),
	}
	for _, tool := range config.ToolAllowlist {
		server.allowlist[tool] = struct{}{}
	}
	return server, nil
}

func (h *MCPHandler) loadStoredTool(ctx context.Context, orgID, capabilityID, toolName string) (storedTool, error) {
	stored := storedTool{exists: true}
	err := h.pool.QueryRow(ctx, `
		SELECT side_effect_class, approval_floor, COALESCE(pinned_digest, ''), COALESCE(observed_digest, ''), COALESCE(review_state, '')
		FROM capability_tools
		WHERE org_id=$1 AND capability_id=$2 AND tool_name=$3
	`, orgID, capabilityID, toolName).Scan(&stored.class, &stored.floor, &stored.pinned, &stored.observed, &stored.state)
	if errors.Is(err, pgx.ErrNoRows) {
		return storedTool{}, nil
	}
	if err != nil {
		return storedTool{}, err
	}
	return stored, nil
}

// recordObservation reads the tool's row, decides, and writes the result with a
// compare-and-set on the fields the decision depended on, so two observations
// racing on one tool cannot overwrite each other's drift or class. A lost race
// re-reads and re-decides; after observationRetries it is reported as contended
// and the next observation settles it.
func (h *MCPHandler) recordObservation(ctx context.Context, orgID string, o toolObservation, server observedServer, observedAt time.Time) (observationResult, error) {
	qualified := qualifiedMCPToolName(o.ServerID, o.ToolName)
	capabilityID := mcpToolCapabilityID(orgID, qualified)
	result := observationResult{QualifiedName: qualified}

	annotations, err := json.Marshal(o.Annotations)
	if err != nil {
		return result, err
	}
	for attempt := 0; attempt < observationRetries; attempt++ {
		stored, err := h.loadStoredTool(ctx, orgID, capabilityID, o.ToolName)
		if err != nil {
			return result, err
		}
		d := decideObservation(stored, o, server)
		if err := models.ValidateClassAndFloor(d.class, d.floor); err != nil {
			return result, fmt.Errorf("derived an invalid class for %s: %w", qualified, err)
		}
		if d.unchanged {
			result.Outcome, result.ReviewState, result.SideEffectClass = outcomeRecorded, d.state, d.class
			return result, nil
		}
		availabilityState, availabilityReason, executionMode, costClass := availabilityFor(d.enabled, d.reasonCode)
		config, err := json.Marshal(map[string]string{"server_id": o.ServerID, "tool_name": o.ToolName})
		if err != nil {
			return result, err
		}
		auditID, auditDiff := "", []byte("{}")
		if d.audit != "" {
			auditID = "ral_" + uuid.NewString()
			auditDiff, err = json.Marshal(map[string]any{
				"server_id":             o.ServerID,
				"tool_name":             o.ToolName,
				"digest":                o.Digest,
				"pinned_digest":         d.pinned,
				"previous_pinned":       stored.pinned,
				"review_state":          d.state,
				"previous_review_state": stored.state,
				"side_effect_class":     d.class,
				"previous_class":        stored.class,
				"declared_side_effect":  o.DeclaredSideEffect,
				"annotations_trusted":   server.trusted,
			})
			if err != nil {
				return result, err
			}
		}
		var applied int64
		err = h.pool.QueryRow(ctx, applyObservationSQL,
			orgID, capabilityID, o.ToolName, d.class, d.floor, o.ServerID, qualified, d.pinned, o.Digest,
			o.DescriptionSHA256, o.SchemaSHA256, o.DeclaredSideEffect, string(annotations), d.state, observedAt,
			stored.pinned, stored.observed, stored.state, expectedClass(stored),
			mcpToolVersion(d.pinned), riskLevelForClass(d.class), d.enabled,
			availabilityState, d.reasonCode, availabilityReason, executionMode, costClass, config,
			auditID, d.audit, string(auditDiff),
		).Scan(&applied)
		if err != nil {
			return result, err
		}
		// The statement ends in `SELECT count(*) FROM tool`: 1 when the
		// compare-and-set held and the write happened, 0 when it lost a race.
		if applied == 1 {
			result.Outcome, result.ReviewState, result.SideEffectClass = outcomeRecorded, d.state, d.class
			return result, nil
		}
	}
	result.Outcome, result.Reason = outcomeContended, "row_changed_concurrently"
	return result, nil
}

// expectedClass is the class the compare-and-set expects the stored row to hold:
// the stored one, or the strictest class for a row that does not exist (the
// value no existing row can hold only if the insert wins, see applyObservationSQL).
func expectedClass(stored storedTool) string {
	if !stored.exists {
		return models.SideEffectUnclassified
	}
	return stored.class
}

// applyObservationSQL writes one tool's observation as a single statement: the
// per-tool row, the capability row that carries its availability, and the audit
// entry, all or none.
//
// The per-tool upsert is a compare-and-set. A new row inserts. An existing row
// updates only if it still holds the pin, observed digest, review state and
// class this decision was made from, and only if it is a derived row (a seeded
// or operator-overridden row is never rewritten by observation). When the guard
// fails nothing is written and the statement returns 0, so the caller re-reads.
//
// Whether the tool is on is decided here, against the server row as it is now,
// not taken from the caller: the handler read the server once for the whole batch,
// and a disable or delete that lands mid-batch must not be overwritten by a later
// write that still carries the stale `enabled`. The `srv` CTE selects the live
// server row FOR SHARE, which makes withMCPToolCascade's UPDATE of that row wait
// for this statement, so either this statement sees the disable (and writes the
// tool off) or the cascade runs after it and switches the tool off itself. A
// tool is callable only if the server is enabled, rolled out (stable or canary)
// and still lists the tool on its allowlist.
//
// Parameters: $1 org, $2 capability id, $3 tool name, $4 class, $5 floor,
// $6 server id, $7 qualified name, $8 pinned digest, $9 observed digest,
// $10 description sha256, $11 schema sha256, $12 declared side effect,
// $13 hints (json), $14 review state, $15 observed at; guard $16 pin, $17
// observed digest, $18 state, $19 class; capability row $20 version, $21 risk,
// $22 enabled, $23 availability state, $24 reason code, $25 reason, $26
// execution mode, $27 cost class, $28 config; audit $29 id, $30 action (empty
// for none), $31 diff.
const applyObservationSQL = `
WITH srv AS (
    SELECT 1 AS live FROM mcp_servers
    WHERE id = $6 AND (org_id = $1 OR org_id = 'global') AND deleted_at IS NULL
      AND enabled AND rollout_state IN ('stable', 'canary')
      AND config_json->'tool_allowlist' ? $3::text
    FOR SHARE
), eff AS (
    SELECT ($22::boolean AND EXISTS (SELECT 1 FROM srv)) AS callable,
           ($22::boolean AND NOT EXISTS (SELECT 1 FROM srv)) AS forced_off
), tool AS (
    INSERT INTO capability_tools AS ct (
        org_id, capability_id, tool_name, side_effect_class, approval_floor, source,
        server_id, qualified_name, pinned_digest, observed_digest, description_sha256, schema_sha256,
        declared_side_effect, declared_annotations, review_state, observed_at
    ) VALUES ($1, $2, $3, $4, $5, 'derived', $6, $7, $8, $9, $10, $11, $12, $13::jsonb, $14, $15)
    ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
        side_effect_class    = EXCLUDED.side_effect_class,
        approval_floor       = EXCLUDED.approval_floor,
        pinned_digest        = EXCLUDED.pinned_digest,
        observed_digest      = EXCLUDED.observed_digest,
        description_sha256   = EXCLUDED.description_sha256,
        schema_sha256        = EXCLUDED.schema_sha256,
        declared_side_effect = EXCLUDED.declared_side_effect,
        declared_annotations = EXCLUDED.declared_annotations,
        review_state         = EXCLUDED.review_state,
        observed_at          = EXCLUDED.observed_at,
        updated_at           = now()
    WHERE ct.source = 'derived'
      AND COALESCE(ct.pinned_digest, '') = $16
      AND COALESCE(ct.observed_digest, '') = $17
      AND COALESCE(ct.review_state, '') = $18
      AND ct.side_effect_class = $19
    RETURNING 1
), cap AS (
    INSERT INTO capabilities (
        id, org_id, kind, name, version, description, risk_level, scope, lazy_load, enabled,
        idempotency_key, schema_input, schema_output, config_json, tags, enabled_for_scopes,
        rollout_state, created_by, side_effect_class, approval_floor,
        availability_state, availability_reason_code, availability_reason, execution_mode, cost_class,
        health_checked_at
    )
    SELECT $2::text, $1::text, 'mcp_tool', $7::text, $20::text, '', $21::text, 'workspace', false,
           (SELECT callable FROM eff),
           '', '{}'::jsonb, '{}'::jsonb, $28::jsonb, '{}'::text[], ARRAY['global']::text[],
           'stable', 'model-gateway', $4::text, $5::text,
           CASE WHEN (SELECT forced_off FROM eff) THEN 'unavailable' ELSE $23::text END,
           CASE WHEN (SELECT forced_off FROM eff) THEN 'capability_disabled' ELSE $24::text END,
           CASE WHEN (SELECT forced_off FROM eff) THEN 'The tool server is disabled or no longer lists it.' ELSE $25::text END,
           CASE WHEN (SELECT forced_off FROM eff) THEN 'unavailable' ELSE $26::text END,
           CASE WHEN (SELECT forced_off FROM eff) THEN 'unknown' ELSE $27::text END,
           $15::timestamptz
    WHERE EXISTS (SELECT 1 FROM tool)
    ON CONFLICT (id) DO UPDATE SET
        version                  = EXCLUDED.version,
        risk_level               = EXCLUDED.risk_level,
        enabled                  = EXCLUDED.enabled,
        config_json              = EXCLUDED.config_json,
        side_effect_class        = EXCLUDED.side_effect_class,
        approval_floor           = EXCLUDED.approval_floor,
        availability_state       = EXCLUDED.availability_state,
        availability_reason_code = EXCLUDED.availability_reason_code,
        availability_reason      = EXCLUDED.availability_reason,
        execution_mode           = EXCLUDED.execution_mode,
        cost_class               = EXCLUDED.cost_class,
        health_checked_at        = EXCLUDED.health_checked_at,
        updated_at               = now()
    WHERE capabilities.deleted_at IS NULL AND capabilities.kind = 'mcp_tool' AND capabilities.org_id = $1
    RETURNING 1
), aud AS (
    INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
    SELECT $29::text, 'capability', $2::text, $30::text, 'model-gateway', $1::text, $31::jsonb
    WHERE $30::text <> '' AND EXISTS (SELECT 1 FROM tool)
    RETURNING 1
)
SELECT count(*) FROM tool
`

// ---------------------------------------------------------------------------
// Approve and list
// ---------------------------------------------------------------------------

// parseMCPToolApprovePath splits {server}/tools/{tool}/approve.
func parseMCPToolApprovePath(rest string) (serverID, tool string, ok bool) {
	parts := strings.Split(rest, "/")
	if len(parts) != 4 || parts[1] != "tools" || parts[3] != "approve" || parts[0] == "" || parts[2] == "" {
		return "", "", false
	}
	return parts[0], parts[2], true
}

type mcpToolView struct {
	ToolName           string          `json:"tool_name"`
	QualifiedName      string          `json:"qualified_name"`
	SideEffectClass    string          `json:"side_effect_class"`
	ApprovalFloor      string          `json:"approval_floor"`
	ReviewState        string          `json:"review_state"`
	DeclaredSideEffect string          `json:"declared_side_effect"`
	PinnedDigest       string          `json:"pinned_digest"`
	ObservedDigest     string          `json:"observed_digest"`
	DescriptionSHA256  string          `json:"description_sha256"`
	SchemaSHA256       string          `json:"schema_sha256"`
	Annotations        json.RawMessage `json:"annotations"`
	ObservedAt         *time.Time      `json:"observed_at"`
	ClassSource        string          `json:"class_source"`
}

// listTools is GET /api/v1/mcp/{id}/tools: the registry rows of one server's
// tools for the signed org. It returns digests, classes and states, never a
// description or a schema, because the registry holds none.
func (h *MCPHandler) listTools(w http.ResponseWriter, r *http.Request, serverID string) {
	orgID, ok := mcpVerifiedOrganization(r, false)
	if !ok {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	rows, err := h.pool.Query(r.Context(), `
		SELECT tool_name, qualified_name, side_effect_class, approval_floor, COALESCE(review_state, ''),
		       COALESCE(declared_side_effect, ''), COALESCE(pinned_digest, ''), COALESCE(observed_digest, ''),
		       COALESCE(description_sha256, ''), COALESCE(schema_sha256, ''),
		       COALESCE(declared_annotations, '{}'::jsonb), observed_at, source
		FROM capability_tools
		WHERE org_id=$1 AND server_id=$2
		ORDER BY tool_name
	`, orgID, serverID)
	if err != nil {
		slog.Error("list MCP tools failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	defer rows.Close()
	// Initialised, not `var`: an empty list must marshal as [] and not null.
	views := []mcpToolView{}
	for rows.Next() {
		var v mcpToolView
		var annotations []byte
		if err := rows.Scan(&v.ToolName, &v.QualifiedName, &v.SideEffectClass, &v.ApprovalFloor, &v.ReviewState,
			&v.DeclaredSideEffect, &v.PinnedDigest, &v.ObservedDigest, &v.DescriptionSHA256, &v.SchemaSHA256,
			&annotations, &v.ObservedAt, &v.ClassSource); err != nil {
			slog.Error("scan MCP tool failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		v.Annotations = json.RawMessage(annotations)
		v.SideEffectClass = models.NormalizeSideEffectClass(v.SideEffectClass)
		v.ApprovalFloor = models.EffectiveApprovalFloor(v.SideEffectClass, v.ApprovalFloor)
		views = append(views, v)
	}
	if err := rows.Err(); err != nil {
		slog.Error("list MCP tools failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, views)
}

// maxApprovalBodyBytes bounds the approval body, which is one digest.
const maxApprovalBodyBytes = 4 << 10

// approveToolRequest names the definition the person reviewed. The digest is
// what the list showed them, not something the server looks up at approval time:
// without it, a definition observed after they looked would be pinned unseen.
type approveToolRequest struct {
	ApprovedDigest string `json:"approved_digest"`
}

// approveTool is POST /api/v1/mcp/{id}/tools/{tool}/approve: a person accepts the
// definition the gateway last observed for a tool. It is the only way a drifted
// tool comes back, and it is the one place a derived class may be re-derived
// downward, because a person with authority over the server has just looked.
//
// The body carries the observed_digest the person reviewed (approved_digest). It
// is pinned only if it is still the observed digest, in the read and again in the
// write; otherwise the answer is 409 and they review the tool again.
//
// Who may: the owner of a user-scoped server; an admin (from the signed scopes)
// for anything shared wider. capability:write is required, and a ZDR principal
// never reaches this handler (the authz middleware refuses its writes).
func (h *MCPHandler) approveTool(w http.ResponseWriter, r *http.Request, serverID, tool string) {
	orgID, ok := mcpVerifiedOrganization(r, true)
	if !ok {
		jsonErr(w, "capability write scope required", http.StatusForbidden)
		return
	}
	principal, _ := authctx.PrincipalFromContext(r.Context())

	r.Body = http.MaxBytesReader(w, r.Body, maxApprovalBodyBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	var request approveToolRequest
	if err := decoder.Decode(&request); err != nil || ensureJSONEOF(decoder) != nil || !sha256HexPattern.MatchString(request.ApprovedDigest) {
		jsonErr(w, "approved_digest (the observed_digest that was reviewed, as sha256 hex) is required", http.StatusBadRequest)
		return
	}

	var (
		scope        string
		enabled      bool
		rolloutState string
		stored       any
	)
	err := h.pool.QueryRow(r.Context(), `
		SELECT scope, enabled, rollout_state, config_json
		FROM mcp_servers
		WHERE id=$1 AND (org_id=$2 OR org_id='global') AND deleted_at IS NULL
	`, serverID, orgID).Scan(&scope, &enabled, &rolloutState, &stored)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	if err != nil {
		slog.Error("read MCP server for tool approval failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	config, err := decodeStoredMCPConfig(stored)
	if err != nil {
		jsonErr(w, "MCP server configuration is invalid", http.StatusUnprocessableEntity)
		return
	}
	if scope == "user" {
		if config.OwnerUserID == "" || principal.ActorID != config.OwnerUserID {
			jsonErr(w, "only the server's owner may approve its tools", http.StatusForbidden)
			return
		}
	} else if !verifiedIsAdmin(r) {
		jsonErr(w, "only an admin may approve tools of a shared server", http.StatusForbidden)
		return
	}
	server := observedServer{
		registered: true,
		enabled:    enabled && (rolloutState == "stable" || rolloutState == "canary"),
		trusted:    config.annotationsTrusted(),
		allowlist:  make(map[string]struct{}, len(config.ToolAllowlist)),
	}
	for _, allowed := range config.ToolAllowlist {
		server.allowlist[allowed] = struct{}{}
	}

	var (
		capabilityID string
		current      storedTool
		declared     string
		source       string
	)
	current.exists = true
	err = h.pool.QueryRow(r.Context(), `
		SELECT capability_id, side_effect_class, approval_floor, COALESCE(pinned_digest, ''),
		       COALESCE(observed_digest, ''), COALESCE(review_state, ''), COALESCE(declared_side_effect, ''), source
		FROM capability_tools
		WHERE org_id=$1 AND server_id=$2 AND tool_name=$3 AND qualified_name IS NOT NULL
	`, orgID, serverID, tool).Scan(&capabilityID, &current.class, &current.floor, &current.pinned,
		&current.observed, &current.state, &declared, &source)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	if err != nil {
		slog.Error("read MCP tool for approval failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	if source != "derived" {
		jsonErr(w, "this tool's class is set by an operator and is not changed by approval", http.StatusConflict)
		return
	}
	if current.observed == "" {
		jsonErr(w, "the tool has not been observed", http.StatusConflict)
		return
	}
	if current.observed != request.ApprovedDigest {
		jsonErr(w, "the tool's observed definition is not the one that was reviewed; review it again", http.StatusConflict)
		return
	}
	if current.state == reviewApproved && current.pinned == current.observed {
		writeJSON(w, approvalResponse(serverID, tool, current.state, current.class, current.floor, current.pinned, server.callable(tool)))
		return
	}

	class := models.DeriveMCPClass(declared, server.trusted)
	floor := models.MinimumApprovalFloor(class)
	if err := models.ValidateClassAndFloor(class, floor); err != nil {
		slog.Error("approval derived an invalid class", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	callable := server.callable(tool)
	reasonCode := unavailableReasonFor(reviewApproved, callable)
	availabilityState, availabilityReason, executionMode, costClass := availabilityFor(callable, reasonCode)
	auditDiff, err := json.Marshal(map[string]any{
		"server_id":             serverID,
		"tool_name":             tool,
		"approved_digest":       request.ApprovedDigest,
		"previous_pinned":       current.pinned,
		"previous_review_state": current.state,
		"side_effect_class":     class,
		"previous_class":        current.class,
		"declared_side_effect":  declared,
		"annotations_trusted":   server.trusted,
	})
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	var applied int64
	err = h.pool.QueryRow(r.Context(), approveToolSQL,
		orgID, capabilityID, tool, class, floor,
		current.pinned, request.ApprovedDigest, current.state, current.class,
		mcpToolVersion(request.ApprovedDigest), riskLevelForClass(class), callable,
		availabilityState, reasonCode, availabilityReason, executionMode, costClass,
		"ral_"+uuid.NewString(), auditActionToolApproved, principal.ActorID, string(auditDiff),
		serverID,
	).Scan(&applied)
	if err != nil {
		slog.Error("approve MCP tool failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	if applied != 1 {
		// The row moved between the read and the write (a newer observation, or
		// another approval). Approving a digest the person did not see is the one
		// thing this must not do, so it asks them to look again.
		jsonErr(w, "the tool changed while it was being approved; review it again", http.StatusConflict)
		return
	}
	writeJSON(w, approvalResponse(serverID, tool, reviewApproved, class, floor, request.ApprovedDigest, callable))
}

func approvalResponse(serverID, tool, state, class, floor, pinned string, callable bool) map[string]any {
	return map[string]any{
		"qualified_name":    qualifiedMCPToolName(serverID, tool),
		"review_state":      state,
		"side_effect_class": models.NormalizeSideEffectClass(class),
		"approval_floor":    models.EffectiveApprovalFloor(class, floor),
		"pinned_digest":     pinned,
		"callable":          callable,
	}
}

// approveToolSQL re-pins a tool to its observed digest, re-derives its class,
// and records the approval, as one statement guarded on the state the person was
// shown ($6 pin, $7 the digest they approved, which must still be the observed
// one, $8 state, $9 class). Parameters: $1 org,
// $2 capability id, $3 tool, $4 class, $5 floor, $10 version, $11 risk, $12
// enabled, $13 availability state, $14 reason code, $15 reason, $16 execution
// mode, $17 cost class, $18 audit id, $19 audit action, $20 actor, $21 diff,
// $22 server id. As in applyObservationSQL, whether the tool ends up on is decided
// here against the server row as it is now (the `srv` CTE, FOR SHARE), so an
// approval cannot re-enable a tool whose server was disabled after the handler
// read it. health_checked_at is left alone: only an observation proves the tool
// is still on its server.
const approveToolSQL = `
WITH srv AS (
    SELECT 1 AS live FROM mcp_servers
    WHERE id = $22 AND (org_id = $1 OR org_id = 'global') AND deleted_at IS NULL
      AND enabled AND rollout_state IN ('stable', 'canary')
      AND config_json->'tool_allowlist' ? $3::text
    FOR SHARE
), eff AS (
    SELECT ($12::boolean AND EXISTS (SELECT 1 FROM srv)) AS callable,
           ($12::boolean AND NOT EXISTS (SELECT 1 FROM srv)) AS forced_off
), tool AS (
    UPDATE capability_tools
    SET side_effect_class = $4, approval_floor = $5, pinned_digest = observed_digest,
        review_state = 'approved', updated_at = now()
    WHERE org_id = $1 AND capability_id = $2 AND tool_name = $3 AND source = 'derived'
      AND COALESCE(pinned_digest, '') = $6 AND COALESCE(observed_digest, '') = $7
      AND COALESCE(review_state, '') = $8 AND side_effect_class = $9
    RETURNING 1
), cap AS (
    UPDATE capabilities
    SET version = $10, risk_level = $11, enabled = (SELECT callable FROM eff), side_effect_class = $4, approval_floor = $5,
        availability_state = CASE WHEN (SELECT forced_off FROM eff) THEN 'unavailable' ELSE $13::text END,
        availability_reason_code = CASE WHEN (SELECT forced_off FROM eff) THEN 'capability_disabled' ELSE $14::text END,
        availability_reason = CASE WHEN (SELECT forced_off FROM eff) THEN 'The tool server is disabled or no longer lists it.' ELSE $15::text END,
        execution_mode = CASE WHEN (SELECT forced_off FROM eff) THEN 'unavailable' ELSE $16::text END,
        cost_class = CASE WHEN (SELECT forced_off FROM eff) THEN 'unknown' ELSE $17::text END,
        updated_at = now()
    WHERE id = $2 AND org_id = $1 AND kind = 'mcp_tool' AND deleted_at IS NULL
      AND EXISTS (SELECT 1 FROM tool)
    RETURNING 1
), aud AS (
    INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
    SELECT $18::text, 'capability', $2::text, $19::text, $20::text, $1::text, $21::jsonb
    WHERE EXISTS (SELECT 1 FROM tool)
    RETURNING 1
)
SELECT count(*) FROM tool
`
