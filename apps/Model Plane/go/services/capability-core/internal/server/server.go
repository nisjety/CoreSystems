package server

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/api"
	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/lettatools"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/procedures"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillbundle"
	"github.com/triodelab/model-plane/services/capability-core/internal/telemetry"

	"github.com/jackc/pgx/v5"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

// Server is the default CapabilityCore implementation backed by the registry,
// policy engine, and (optionally) the durable capabilities store used for
// score-ranked listing.
type Server struct {
	mpv1.UnimplementedCapabilityCoreServer
	registry            *registry.Registry
	modelReg            modelCapabilitySource
	policy              *policy.Engine
	store               capabilityStore // optional: enables score-ranked List
	toolRank            toolDefinitionSearcher
	decisionProofSigner *DecisionProofSigner
	toolDB              toolBindingDatabase // optional: enables tool-level binding
	skillBundles        skillBundleReader
	skillBundlesEnabled bool

	// savedProcedures reads the saved browser procedures GetProcedureVersion serves (plan 3.4 E4.2
	// step 3). proceduresEnabled is PROCEDURES_API, parsed by the api package; off, or with no store,
	// the read answers Unavailable procedures_disabled.
	savedProcedures   procedureReader
	proceduresEnabled bool

	// deckTemplatesEnabled reports which bundles are deck templates (SKILL_DECK_TEMPLATES_ENABLED).
	deckTemplatesEnabled bool

	// skillBundleRetention is SKILL_BUNDLE_RETENTION (EP-11 S5): on, GetSkillBundleManifest
	// honours manifest_sha256 and reads the bundle a pinned version named; off, the field is
	// ignored and the live bundle is read, as before.
	skillBundleRetention bool

	// permissionRules reads the org permission rules (W5-074). The gate consults it only while
	// permissionRulesEnabled is true.
	permissionRules permissionRuleMatcher

	// permissionRulesEnabled is CAPABILITY_PERMISSION_RULES_ENABLED, parsed by the caller.
	permissionRulesEnabled bool

	// ruleUses records the use of a stored allow that relaxed a call. It is read only while ruleAudit is true.
	ruleUses RuleUseRecorder

	// ruleAudit is TOOL_POLICY_RULE_AUDIT_ENABLED, parsed by the caller (policy.ToolPolicyRuleAuditFromLookup).
	ruleAudit bool
}

// RuleUseRecorder writes the content-free audit row of a call a stored allow relaxed. *api.ToolPoliciesHandler
// satisfies it. A failed write fails the decision closed (WS13 S7, K3-2).
type RuleUseRecorder interface {
	RecordRuleUse(ctx context.Context, use api.RuleUse) error
}

// modelCapabilitySource is the narrow slice of the models registry the
// server needs. Satisfied by both *registry.ModelsRegistry (direct Postgres
// reads) and *registry.ModelsCache (the startup-loaded, hot-path-safe
// projection — see PROVIDER_AND_PRIVACY_STRATEGY.md §4.5); production wiring
// (cmd/main.go) uses the cache.
type modelCapabilitySource interface {
	ListAsCapabilitiesForOrg(ctx context.Context, orgID string) ([]*models.Capability, error)
	GetByCapabilityIDForOrg(ctx context.Context, id, orgID string) (*registry.Model, error)
}

// skillBundleReader is the read side of the skill bundle store. *skillbundle.Store
// satisfies it; the RPCs take the narrow slice so tests can stand a fake in.
type skillBundleReader interface {
	List(ctx context.Context, orgID string, skillIDs []string) ([]skillbundle.Summary, error)
	Manifest(ctx context.Context, orgID, skillID string) (skillbundle.Manifest, error)
	Blob(ctx context.Context, orgID, skillID, digest string) ([]byte, error)
}

// skillBundleVersionReader reads a skill's bundle by its manifest digest (EP-11 S5).
// *skillbundle.Store satisfies it; a reader without it serves the live bundle only.
type skillBundleVersionReader interface {
	ManifestAt(ctx context.Context, orgID, skillID, manifestSHA256 string) (skillbundle.Manifest, error)
}

// procedureReader is the read side of the saved-procedure store. *procedures.Store satisfies it; the RPC
// takes the narrow slice so tests can stand a fake in.
type procedureReader interface {
	Get(ctx context.Context, who procedures.Owner, id string) (*procedures.Procedure, error)
	GetVersion(ctx context.Context, who procedures.Owner, id string, ref procedures.Ref) (*procedures.Version, error)
}

type toolDefinitionSearcher interface {
	Search(context.Context, string, int) ([]lettatools.Match, error)
}

type capabilityStore interface {
	GetForOrg(context.Context, string, string) (*registry.CapabilityRow, error)
	RankedList(context.Context, string, string, []string, int) ([]registry.ScoredCapability, error)
}

// permissionRuleMatcher is the org permission rule read the gate applies: the decision the rules of the verified org
// make for one call, and the id of the rule that made it. An error is a failed read. *api.PermissionRuleStore satisfies
// it.
type permissionRuleMatcher interface {
	Match(ctx context.Context, orgID, tool, kind, value string) (string, string, error)
}

// toDetail converts a domain Capability into a wire CapabilityDetail.
func toDetail(c *models.Capability) *mpv1.CapabilityDetail {
	if c == nil {
		return nil
	}
	availability := models.DeriveAvailability(c)
	return &mpv1.CapabilityDetail{
		CapabilityId:     c.ID,
		Name:             c.Name,
		Kind:             c.Kind,
		Version:          c.Version,
		Description:      c.Description,
		RiskLevel:        c.RiskLevel,
		LazyLoad:         c.LazyLoad,
		Scope:            c.Scope,
		State:            string(availability.State),
		ReasonCode:       availability.ReasonCode,
		Reason:           availability.Reason,
		RequiresApproval: availability.RequiresApproval,
		ExecutionMode:    availability.ExecutionMode,
		CostClass:        availability.CostClass,
		HealthCheckedAt:  availability.HealthCheckedAt,
		PrivacyTier:      privacyTierToProto(c.PrivacyTier),
		Residency:        c.Residency,
		SideEffectClass:  models.NormalizeSideEffectClass(c.SideEffectClass),
		ApprovalFloor:    models.EffectiveApprovalFloor(c.SideEffectClass, c.ApprovalFloor),
	}
}

// privacyTierToProto maps the persisted snake_case label to the wire enum.
// An unrecognized/empty label degrades to UNSPECIFIED rather than fabricating
// a stronger claim — disclosure must never overclaim.
func privacyTierToProto(label string) mpv1.PrivacyTier {
	switch label {
	case models.PrivacyTierGlobal:
		return mpv1.PrivacyTier_PRIVACY_TIER_GLOBAL
	case models.PrivacyTierEUResident:
		return mpv1.PrivacyTier_PRIVACY_TIER_EU_RESIDENT
	case models.PrivacyTierZDRContractual:
		return mpv1.PrivacyTier_PRIVACY_TIER_ZDR_CONTRACTUAL
	case models.PrivacyTierSovereign:
		return mpv1.PrivacyTier_PRIVACY_TIER_SOVEREIGN
	default:
		return mpv1.PrivacyTier_PRIVACY_TIER_UNSPECIFIED
	}
}

func hasAllChecks(checks []string, required ...string) bool {
	set := make(map[string]struct{}, len(checks))
	for _, check := range checks {
		set[check] = struct{}{}
	}
	for _, requiredCheck := range required {
		if _, ok := set[requiredCheck]; !ok {
			return false
		}
	}
	return true
}

// NewServer constructs a Server wired to the provided registry and policy engine.
func NewServer(reg *registry.Registry, modelReg modelCapabilitySource, pol *policy.Engine) *Server {
	return &Server{registry: reg, modelReg: modelReg, policy: pol}
}

// WithStore attaches the durable capabilities store. When present,
// ListCapabilities ranks results by composite score (descending) instead of the
// registry's kind/name order. Returns the same Server for chaining; a nil store
// leaves the in-memory ordering in place.
func (s *Server) WithStore(store *registry.CapabilitiesStore) *Server {
	s.store = store
	return s
}

// WithLettaToolSearcher attaches optional, non-authoritative semantic ranking
// for tool definitions. The searcher can only reorder/intersect rows already
// returned by the tenant-scoped durable store; it is never consulted by
// EvaluatePolicy or execution dispatch.
func (s *Server) WithLettaToolSearcher(searcher toolDefinitionSearcher) *Server {
	s.toolRank = searcher
	return s
}

// WithDecisionProofSigner enables short-lived per-call Ed25519 evidence on
// EvaluatePolicy responses. Keeping this explicit prevents tests and local
// read-only deployments from silently inventing a signing key.
func (s *Server) WithDecisionProofSigner(signer *DecisionProofSigner) *Server {
	s.decisionProofSigner = signer
	return s
}

// WithToolBindingDatabase attaches the read-only handle the tool-binding lookup
// queries capability_tools through. Without it ResolveToolBindings is
// unavailable, an `mcp__` tool name never resolves (so it is denied), and a
// built-in tool is evaluated at its capability's ceiling, which is never weaker
// than the tool's own declaration.
func (s *Server) WithToolBindingDatabase(db toolBindingDatabase) *Server {
	s.toolDB = db
	return s
}

// WithSkillBundles attaches the skill bundle store the bundle read RPCs serve from. The
// switch arrives already parsed, so only the literal "true" of SKILL_BUNDLES_ENABLED turns
// the reads on; with it off, or with no store, they answer an empty list or NotFound.
func (s *Server) WithSkillBundles(store skillBundleReader, enabled bool) *Server {
	s.skillBundles = store
	s.skillBundlesEnabled = enabled && store != nil
	return s
}

// WithProcedures attaches the saved-procedure store GetProcedureVersion reads from. The switch arrives
// already parsed by api.ProceduresAPIFromLookup; with it off, or with no store, the read answers
// Unavailable procedures_disabled.
func (s *Server) WithProcedures(store procedureReader, enabled bool) *Server {
	s.savedProcedures = store
	s.proceduresEnabled = enabled && store != nil
	return s
}

// WithSkillBundleRetention lets GetSkillBundleManifest read a kept bundle by its manifest digest
// (EP-11 S5). The switch arrives already parsed by skillbundle.RetentionFromLookup.
func (s *Server) WithSkillBundleRetention(enabled bool) *Server {
	s.skillBundleRetention = enabled
	return s
}

// WithDeckTemplates turns on the deck_template flag of ListSkillBundles (EP-5 S6). The switch
// arrives already parsed by skillbundle.DeckTemplatesFromLookup; off, every summary reports false.
func (s *Server) WithDeckTemplates(enabled bool) *Server {
	s.deckTemplatesEnabled = enabled
	return s
}

// WithPermissionRules attaches the org permission rules and the switch that lets the gate read them. The switch arrives
// already parsed (api.PermissionRulesEnabledFromLookup), so only the literal "true" enables it; off, no rule is read and
// EvaluatePolicy decides as it did before.
func (s *Server) WithPermissionRules(rules permissionRuleMatcher, enabled bool) *Server {
	s.permissionRules = rules
	s.permissionRulesEnabled = enabled && rules != nil
	return s
}

// WithRuleAudit attaches the recorder of rule-relaxed calls and the switch that turns the audit on. The switch arrives
// already parsed (policy.ToolPolicyRuleAuditFromLookup), so only the literal "true" enables it. Off, EvaluatePolicy
// writes no row and treats an unstated run posture as it did before; an explicit retention_bound run is still never
// relaxed by a person's allow.
func (s *Server) WithRuleAudit(recorder RuleUseRecorder, enabled bool) *Server {
	s.ruleUses = recorder
	s.ruleAudit = enabled
	return s
}

// ListCapabilities returns a filtered, paginated list of capabilities. When a
// durable store is attached it returns results ranked by composite score
// (descending) so the agentic loop sees the healthiest/safest capabilities
// first; otherwise it falls back to the in-memory registry's kind/name order.
func (s *Server) ListCapabilities(ctx context.Context, req *mpv1.ListCapabilitiesRequest) (*mpv1.ListCapabilitiesResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ListCapabilities")))
	principal, err := verifiedPrincipal(ctx)
	if err != nil {
		return nil, err
	}
	if s.store != nil {
		return s.listRanked(ctx, req, principal)
	}
	orgID := principal.OrganizationID
	items, hasMore := s.registry.ListForOrg(orgID, req.KindFilter, req.Query, req.AfterId, req.Limit)
	details := make([]*mpv1.CapabilityDetail, 0, len(items))
	for _, c := range items {
		details = append(details, toDetail(c))
	}
	if s.modelReg != nil {
		modelCaps, err := s.modelReg.ListAsCapabilitiesForOrg(ctx, orgID)
		if err != nil {
			return nil, mapErr(err)
		}
		for _, c := range modelCaps {
			details = append(details, toDetail(c))
		}
	}
	return &mpv1.ListCapabilitiesResponse{Capabilities: details, HasMore: hasMore}, nil
}

// listRanked serves ListCapabilities from the durable store, ordered by
// descending composite score. The text query filters by name/description; the
// kind filter narrows by kind; AfterId is an id cursor over the ranked order.
func (s *Server) listRanked(ctx context.Context, req *mpv1.ListCapabilitiesRequest, principal authctx.Principal) (*mpv1.ListCapabilitiesResponse, error) {
	limit := int(req.Limit)
	if limit <= 0 || limit > 200 {
		limit = 50
	}
	// Pull a generous tenant-filtered window (including global entries).
	// Over-fetch so the query filter + cursor can still fill a page.
	scored, err := s.store.RankedList(ctx, principal.OrganizationID, req.KindFilter, nil, limit*4+200)
	if err != nil {
		return nil, mapErr(err)
	}

	q := strings.TrimSpace(req.Query)
	localRows := make([]*registry.CapabilityRow, 0, len(scored))
	for _, sc := range scored {
		if sc.Row != nil {
			localRows = append(localRows, sc.Row)
		}
	}
	filtered, source, reason := s.rankLocalCapabilities(ctx, principal, q, localRows)
	recordCapabilityRanking(ctx, source, reason)

	start := 0
	if req.AfterId != "" {
		for i, r := range filtered {
			if r.ID == req.AfterId {
				start = i + 1
				break
			}
		}
	}
	end := start + limit
	hasMore := false
	if end < len(filtered) {
		hasMore = true
	} else {
		end = len(filtered)
	}
	if start > len(filtered) {
		start = len(filtered)
	}

	details := make([]*mpv1.CapabilityDetail, 0, end-start)
	for _, r := range filtered[start:end] {
		details = append(details, rowToDetail(r))
	}
	return &mpv1.ListCapabilitiesResponse{Capabilities: details, HasMore: hasMore}, nil
}

func (s *Server) rankLocalCapabilities(
	ctx context.Context,
	principal authctx.Principal,
	query string,
	rows []*registry.CapabilityRow,
) ([]*registry.CapabilityRow, string, string) {
	local := localTextFilter(rows, query)
	if query == "" {
		return local, "local", "query_empty"
	}
	if s.toolRank == nil {
		return local, "local", "external_not_configured"
	}
	// External calls require an explicit, cryptographically verified non-ZDR
	// posture. Missing retention policy is not permission to disclose a query.
	if !principal.RetentionPolicyPresent {
		return local, "local", "retention_unspecified"
	}
	if principal.ZeroDataRetention {
		return local, "local", "zdr_external_disabled"
	}

	searchLimit := len(rows)
	if searchLimit < 1 {
		return local, "local", "local_catalog_empty"
	}
	if searchLimit > 100 {
		searchLimit = 100
	}
	matches, err := s.toolRank.Search(ctx, query, searchLimit)
	if err != nil {
		return local, "local", "external_unavailable"
	}
	ranked := exactLocalToolIntersection(rows, matches)
	if len(ranked) == 0 {
		return local, "local", "no_exact_intersection"
	}

	selected := make(map[string]struct{}, len(ranked))
	result := make([]*registry.CapabilityRow, 0, len(ranked)+len(local))
	for _, row := range ranked {
		selected[row.ID] = struct{}{}
		result = append(result, row)
	}
	// Preserve locally matching capabilities after the semantic intersection,
	// so an external outage or incomplete Letta catalog cannot hide durable
	// tenant-authorized entries.
	for _, row := range local {
		if _, exists := selected[row.ID]; !exists {
			result = append(result, row)
		}
	}
	return result, "letta_tool_intersection", "ranked_exact_intersection"
}

func localTextFilter(rows []*registry.CapabilityRow, query string) []*registry.CapabilityRow {
	q := strings.ToLower(strings.TrimSpace(query))
	filtered := make([]*registry.CapabilityRow, 0, len(rows))
	for _, row := range rows {
		if row == nil {
			continue
		}
		if q != "" &&
			!strings.Contains(strings.ToLower(row.Name), q) &&
			!strings.Contains(strings.ToLower(row.Description), q) {
			continue
		}
		filtered = append(filtered, row)
	}
	return filtered
}

func exactLocalToolIntersection(rows []*registry.CapabilityRow, matches []lettatools.Match) []*registry.CapabilityRow {
	const ambiguous = -1
	byName := make(map[string]int)
	for index, row := range rows {
		for _, name := range trustedToolNames(row) {
			if existing, present := byName[name]; present && existing != index {
				byName[name] = ambiguous
				continue
			}
			byName[name] = index
		}
	}
	result := make([]*registry.CapabilityRow, 0, len(matches))
	selected := make(map[int]struct{}, len(matches))
	for _, match := range matches {
		index, exists := byName[match.Name]
		if !exists || index == ambiguous {
			continue
		}
		if _, exists := selected[index]; exists {
			continue
		}
		selected[index] = struct{}{}
		result = append(result, rows[index])
	}
	return result
}

func trustedToolNames(row *registry.CapabilityRow) []string {
	if row == nil || row.Kind != models.KindTool {
		return nil
	}
	var config struct {
		DispatchName string `json:"dispatch_name"`
	}
	if len(row.ConfigJSON) > 0 {
		if json.Unmarshal(row.ConfigJSON, &config) != nil {
			return nil
		}
		if strings.TrimSpace(config.DispatchName) != "" {
			return exactDispatchNames(config.DispatchName)
		}
	}
	if validExactToolName(row.Name) {
		return []string{row.Name}
	}
	return nil
}

func exactDispatchNames(dispatch string) []string {
	parts := strings.Split(dispatch, "/")
	names := make([]string, 0, len(parts))
	for _, part := range parts {
		if validExactToolName(part) {
			names = append(names, part)
		}
	}
	return names
}

func validExactToolName(name string) bool {
	if name == "" || len(name) > 128 || strings.TrimSpace(name) != name {
		return false
	}
	for _, character := range name {
		if (character >= 'a' && character <= 'z') ||
			(character >= 'A' && character <= 'Z') ||
			(character >= '0' && character <= '9') ||
			character == '_' || character == '-' || character == '.' || character == ':' {
			continue
		}
		return false
	}
	return true
}

func recordCapabilityRanking(ctx context.Context, source, reason string) {
	telemetry.CapabilityRankingTotal.Add(
		ctx,
		1,
		metric.WithAttributes(attribute.String("source", source), attribute.String("reason", reason)),
	)
	_ = grpc.SetTrailer(ctx, metadata.Pairs(
		"x-capability-ranking-source", source,
		"x-capability-ranking-reason", reason,
	))
}

// rowToDetail converts a durable CapabilityRow into a wire CapabilityDetail.
func rowToDetail(r *registry.CapabilityRow) *mpv1.CapabilityDetail {
	return toDetail(rowToCapability(r))
}

func rowToCapability(r *registry.CapabilityRow) *models.Capability {
	if r == nil {
		return nil
	}
	return &models.Capability{
		ID:                r.ID,
		Name:              r.Name,
		Kind:              r.Kind,
		Version:           r.Version,
		Description:       r.Description,
		RiskLevel:         r.RiskLevel,
		LazyLoad:          r.LazyLoad,
		Scope:             r.Scope,
		Enabled:           r.Enabled,
		IdempotencyKey:    r.IdempotencyKey,
		OrgID:             r.OrgID,
		EnabledForScopes:  append([]string(nil), r.EnabledForScopes...),
		RolloutState:      r.RolloutState,
		AvailabilityState: r.AvailabilityState,
		ReasonCode:        r.ReasonCode,
		Reason:            r.Reason,
		ExecutionMode:     r.ExecutionMode,
		CostClass:         r.CostClass,
		HealthCheckedAt:   r.HealthCheckedAt,
		SideEffectClass:   r.SideEffectClass,
		ApprovalFloor:     r.ApprovalFloor,
	}
}

// GetCapability returns a single capability by ID, honouring an optional version constraint.
func (s *Server) GetCapability(ctx context.Context, req *mpv1.GetCapabilityRequest) (*mpv1.CapabilityDetail, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "GetCapability")))
	orgID, err := verifiedOrganizationID(ctx)
	if err != nil {
		return nil, err
	}
	if s.modelReg != nil && strings.HasPrefix(req.CapabilityId, "cap.model.") {
		m, err := s.modelReg.GetByCapabilityIDForOrg(ctx, req.CapabilityId, orgID)
		if err != nil {
			return nil, mapErr(err)
		}
		return toDetail(registry.ToCapability(m)), nil
	}
	if s.store != nil {
		row, err := s.store.GetForOrg(ctx, req.CapabilityId, orgID)
		if err != nil {
			return nil, mapErr(err)
		}
		if req.VersionConstraint != "" && row.Version != req.VersionConstraint {
			return nil, mapErr(domain.ErrVersionMismatch)
		}
		return rowToDetail(row), nil
	}
	c, err := s.registry.GetForOrg(req.CapabilityId, req.VersionConstraint, orgID)
	if err != nil {
		return nil, mapErr(err)
	}
	return toDetail(c), nil
}

// EvaluatePolicy produces an allow/deny/constrained decision for a capability
// invocation. When the request names a tool the decision is made for that tool's
// registry binding: a tool the registry marks not approved is denied, a class or
// approval floor of `ask` or above turns an allow into an ask, and an `mcp__`
// name the registry does not hold is denied, as is a name the registry binds to
// a different capability (`tool_not_bound`); a built-in name the registry holds
// nowhere is decided against the capability's own class and floor, its ceiling.
// With TOOL_POLICY_ENFORCED on, the stored tool policies of the org and of the
// named person apply as well (policy.EvaluateTool).
// With CAPABILITY_PERMISSION_RULES_ENABLED on, the org's permission rules tighten
// the decision last (applyPermissionRules).
// The tool, the user, the class and floor, the policy source and the pinned
// digest are all signed into the decision evidence.
func (s *Server) EvaluatePolicy(ctx context.Context, req *mpv1.EvaluatePolicyRequest) (*mpv1.EvaluatePolicyResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "EvaluatePolicy")))
	principal, err := verifiedPrincipal(ctx)
	if err != nil {
		return nil, err
	}
	orgID := principal.OrganizationID
	if req == nil || !policy.IsSupportedScope(req.Scope) {
		return nil, mapErr(domain.ErrInvalidArgument)
	}
	// A service (execution-core) asserts the person its run acts for; a person may
	// only name themselves. The named person's stored policies decide the call, so a
	// person naming someone else could be decided under that person's own allow.
	if principal.PrincipalType != "service" && req.GetUserId() != "" && req.GetUserId() != principal.ActorID {
		return nil, status.Error(codes.PermissionDenied, "a caller may only evaluate policies for themselves")
	}
	capability, rolloutState, err := s.capabilityForPolicy(ctx, orgID, req.CapabilityId)
	if err != nil {
		return nil, mapErr(err)
	}
	var binding *policy.ToolBinding
	if toolName := req.GetToolName(); toolName != "" {
		var boundElsewhere bool
		binding, boundElsewhere, err = s.lookupToolBinding(ctx, orgID, capability.ID, toolName)
		if err != nil {
			return nil, err
		}
		if binding == nil {
			// A tool bound to an MCP capability, or an `mcp__` name, has no
			// capability-level fallback: the registry row is the only authority.
			// Neither has a built-in name the registry binds to a different
			// capability: evaluating it here would sign "tool X under capability
			// Y" for a pairing the registry never made. The ceiling is for a name
			// the registry holds nowhere.
			if strings.HasPrefix(toolName, mcpToolPrefix) || capability.Kind == models.KindMCPTool || boundElsewhere {
				return s.policyResponse(req, orgID, capability, &policy.Result{
					Decision:     policy.DecisionDeny,
					Reason:       "tool_not_bound",
					PolicySource: policy.PolicySourceToolReview,
				}, nil), nil
			}
			// A name nobody declared is held to the capability's ceiling, so an
			// owner_effect or durable_owner capability is never signed as a plain
			// allow with a floor that nothing enforces.
			binding = policy.CeilingBinding(capability, toolName)
		}
	}
	if reason := capabilityPolicyBlockReason(capability, rolloutState); reason != "" {
		return s.policyResponse(req, orgID, capability, &policy.Result{
			Decision:     policy.DecisionDeny,
			Reason:       reason,
			PolicySource: policy.PolicySourceCapabilityGate,
		}, binding), nil
	}
	// EvaluateTool is the capability decision tightened by the binding, and by the
	// stored policies when TOOL_POLICY_ENFORCED is on. With the switch off the
	// decision and its signed evidence are the ones this path produced before.
	// A run that stores no content never has a person's allow applied to it (D-WS13-1); the run's posture is the one
	// execution-core stated, and an unstated one counts as bound while the rule audit is on.
	retentionBound := policy.RetentionBoundFor(req.GetRunRetention(), s.ruleAudit)
	result, err := s.policy.EvaluateToolFor(ctx, capability, binding, policy.ToolCall{
		UserID:         req.GetUserId(),
		RunID:          req.RunId,
		AgentID:        req.AgentId,
		OrgID:          orgID,
		Scope:          req.Scope,
		RetentionBound: retentionBound,
	})
	if err != nil {
		return nil, mapErr(err)
	}
	if s.permissionRulesEnabled {
		result = s.applyPermissionRules(ctx, orgID, req, capability, binding, result)
	}
	result = s.auditRuleUse(ctx, principal, orgID, req, capability, binding, retentionBound, result)
	return s.policyResponse(req, orgID, capability, result, binding), nil
}

// auditRuleUse writes the tool_policy_rule_used row for a call a stored allow relaxed (WS13 S7). It writes nothing
// while the audit is off, for a call that no allow relaxed, for a retention-bound run (an org allow still applied; a
// ZDR or waived run gets no row, DEC-32) and for a caller that is not a service (a preview). When the write fails it
// logs the capability id and the error only, and demotes the decision to an ask: a rule is never used without its row.
func (s *Server) auditRuleUse(
	ctx context.Context,
	principal authctx.Principal,
	orgID string,
	req *mpv1.EvaluatePolicyRequest,
	capability *models.Capability,
	binding *policy.ToolBinding,
	retentionBound bool,
	result *policy.Result,
) *policy.Result {
	if !s.ruleAudit || result == nil || result.PolicySource != policy.PolicySourceToolPolicyAllow || result.RulePolicyID == "" {
		return result
	}
	if retentionBound || principal.PrincipalType != "service" || req.GetRunId() == "" {
		return result
	}
	use := api.RuleUse{
		OrgID:        orgID,
		PolicyID:     result.RulePolicyID,
		CapabilityID: capability.ID,
		ScopeKind:    result.RuleScope,
		RunID:        req.GetRunId(),
		Actor:        principal.ActorID,
	}
	if binding != nil {
		use.ToolDigest = binding.ToolDigest
	}
	var err error
	if s.ruleUses == nil {
		err = errors.New("rule use recorder unavailable")
	} else {
		err = s.ruleUses.RecordRuleUse(ctx, use)
	}
	if err != nil {
		slog.Error("tool policy rule use not recorded", "capability_id", capability.ID, "error", err)
		return &policy.Result{
			Decision:          policy.DecisionAsk,
			Reason:            "tool_policy_rule_unaudited",
			PolicySource:      policy.PolicySourceRiskLevel,
			ToolPolicyChecked: result.ToolPolicyChecked,
		}
	}
	return result
}

// orgPermissionRuleReasonPrefix starts the reason of a decision an org permission rule made. The reason names the rule
// and nothing else: no argument, target or address of the call reaches the signed decision.
const orgPermissionRuleReasonPrefix = "org_rule:"

// orgPermissionRulesUnavailableReason is the reason of the ask that a failed read of the rule table makes. It names no
// rule, because none was read.
const orgPermissionRulesUnavailableReason = "org_rules_unavailable"

// applyPermissionRules tightens a decision with the org's permission rules (W5-074). It never loosens. A deny stands.
// A rule that denies makes a deny; a rule that asks makes an ask, whether the decision was an allow or an ask already.
// A failed read of the table makes an ask for an effect class and leaves a read class as it was. The target is the one
// execution-core derived for the call. A ZDR or waived run sends the kind alone, and a rule of that kind then covers
// the call whatever its value (targetPatternMatches), so the rules decide such a run as they decide a persistent one,
// and never more loosely.
func (s *Server) applyPermissionRules(
	ctx context.Context,
	orgID string,
	req *mpv1.EvaluatePolicyRequest,
	capability *models.Capability,
	binding *policy.ToolBinding,
	result *policy.Result,
) *policy.Result {
	if result.Decision == policy.DecisionDeny {
		return result
	}
	decision, ruleID, err := s.permissionRules.Match(ctx, orgID, req.GetToolName(), req.GetTargetKind(), req.GetTargetValue())
	if err != nil {
		slog.Error("org permission rules unavailable", "error", err)
		class, _, _ := evaluatedClass(capability, binding)
		if result.Decision == policy.DecisionAllow && class != models.SideEffectRead {
			return &policy.Result{
				Decision:          policy.DecisionAsk,
				Reason:            orgPermissionRulesUnavailableReason,
				PolicySource:      policy.PolicySourceOrgPermissionRule,
				ToolPolicyChecked: result.ToolPolicyChecked,
			}
		}
		return result
	}
	if decision == "" {
		return result
	}
	return &policy.Result{
		Decision:          decision,
		Reason:            orgPermissionRuleReasonPrefix + ruleID,
		PolicySource:      policy.PolicySourceOrgPermissionRule,
		ToolPolicyChecked: result.ToolPolicyChecked,
	}
}

// capabilityForPolicy resolves the capability snapshot a decision is made
// against, through the tenant-aware lookups, with the rollout state the durable
// store carries.
func (s *Server) capabilityForPolicy(ctx context.Context, orgID, capabilityID string) (*models.Capability, string, error) {
	if s.store != nil {
		row, err := s.store.GetForOrg(ctx, capabilityID, orgID)
		if err != nil {
			return nil, "", err
		}
		if row == nil || (row.OrgID != orgID && row.OrgID != "global") {
			return nil, "", domain.ErrCapabilityNotFound
		}
		return rowToCapability(row), row.RolloutState, nil
	}
	capability, err := s.registry.GetForOrg(capabilityID, "", orgID)
	if err != nil {
		return nil, "", err
	}
	return capability, "", nil
}

// policyResponse binds the caller-visible decision to the exact capability
// snapshot and authenticated tenant used during evaluation. The decision ID
// is an audit correlation value; decision evidence is the separately signed
// execution authorization artifact.
func (s *Server) policyResponse(req *mpv1.EvaluatePolicyRequest, orgID string, capability *models.Capability, result *policy.Result, binding *policy.ToolBinding) *mpv1.EvaluatePolicyResponse {
	version := ""
	capabilityID := ""
	if capability != nil {
		version = capability.Version
		capabilityID = capability.ID
	}
	class, floor, digest := evaluatedClass(capability, binding)
	digestInput := fmt.Sprintf("policy-v1\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s", capabilityID, version, orgID, req.GetRunId(), req.GetAgentId(), req.GetScope(), result.Decision, result.Reason, result.BudgetContext)
	if req.GetToolName() != "" || req.GetUserId() != "" {
		// A different tool of the same capability, or a different person, is a
		// different decision; the audit id must say so.
		digestInput = fmt.Sprintf("policy-v2\n%s\n%s\n%s\n%s", digestInput, req.GetToolName(), req.GetUserId(), digest)
	}
	hash := sha256.Sum256([]byte(digestInput))
	response := &mpv1.EvaluatePolicyResponse{
		Decision:          result.Decision,
		Reason:            result.Reason,
		BudgetContext:     result.BudgetContext,
		DecisionId:        "pdec_" + hex.EncodeToString(hash[:16]),
		CapabilityVersion: version,
		SideEffectClass:   class,
		ApprovalFloor:     floor,
		PolicySource:      result.PolicySource,
		ToolDigest:        digest,
	}
	if s.decisionProofSigner != nil {
		proof, err := s.decisionProofSigner.Sign(decisionProofClaimsFor(
			decisionProofRequest{
				capabilityID: capabilityID,
				runID:        req.GetRunId(),
				agentID:      req.GetAgentId(),
				orgID:        orgID,
				scope:        req.GetScope(),
				userID:       req.GetUserId(),
				toolName:     req.GetToolName(),
			},
			&decisionProofResponse{
				decision:          result.Decision,
				reason:            result.Reason,
				budgetContext:     result.BudgetContext,
				decisionID:        response.DecisionId,
				capabilityVersion: version,
				sideEffectClass:   class,
				approvalFloor:     floor,
				policySource:      result.PolicySource,
				toolDigest:        digest,
				toolPolicyChecked: result.ToolPolicyChecked,
			},
		))
		if err == nil {
			response.DecisionEvidence = proof
		}
	}
	return response
}

// evaluatedClass is the class, approval floor and pinned digest a decision was
// made for: the tool's own declaration when it was bound, otherwise the
// capability's ceiling (the strictest of its tools), which a tool-less
// evaluation and an unregistered built-in name are held to.
func evaluatedClass(capability *models.Capability, binding *policy.ToolBinding) (class, floor, digest string) {
	switch {
	case binding != nil:
		return models.NormalizeSideEffectClass(binding.SideEffectClass),
			models.EffectiveApprovalFloor(binding.SideEffectClass, binding.ApprovalFloor),
			binding.ToolDigest
	case capability != nil:
		return models.NormalizeSideEffectClass(capability.SideEffectClass),
			models.EffectiveApprovalFloor(capability.SideEffectClass, capability.ApprovalFloor),
			""
	default:
		return models.SideEffectUnclassified, models.EffectiveApprovalFloor(models.SideEffectUnclassified, ""), ""
	}
}

// ---------------------------------------------------------------------------
// Tool bindings
// ---------------------------------------------------------------------------

const (
	// mcpToolPrefix starts every qualified MCP tool name. The registry row, never
	// a parse of the name, decides what an `mcp__` name is bound to.
	mcpToolPrefix = "mcp__"
	// maxToolBindingNames bounds one ResolveToolBindings request. A run offers a
	// few dozen tools; the bound only keeps the lookup from becoming a scan.
	maxToolBindingNames = 256
	maxToolNameLength   = 256
)

// toolBindingDatabase is the one read the binding lookup needs; *pgxpool.Pool
// satisfies it.
type toolBindingDatabase interface {
	Query(context.Context, string, ...any) (pgx.Rows, error)
}

// toolBindingSQL reads the tool rows a set of names resolves to, with the
// version of the capability each belongs to. A built-in name matches only a row
// with no qualified name, in the tenant or the global seed; an `mcp__` name
// matches only the tenant's own row with that exact qualified name, so a
// built-in name can never resolve to an MCP row and the other way round.
const toolBindingSQL = `
SELECT ct.capability_id, ct.tool_name, COALESCE(ct.qualified_name, ''), ct.side_effect_class,
       ct.approval_floor, COALESCE(ct.pinned_digest, ''), COALESCE(ct.review_state, ''), c.version,
       COALESCE(ct.server_id, '')
FROM capability_tools ct
JOIN capabilities c
  ON c.id = ct.capability_id
 AND c.deleted_at IS NULL
 AND (c.org_id = $1 OR c.org_id = 'global')
WHERE (ct.qualified_name IS NULL AND ct.tool_name = ANY($2::text[]) AND (ct.org_id = $1 OR ct.org_id = 'global'))
   OR (ct.qualified_name = ANY($2::text[]) AND ct.org_id = $1)
`

// toolBindings holds what a lookup found: name, then capability id, then the
// binding. A name can match more than one capability only by registry error.
type toolBindings map[string]map[string]*policy.ToolBinding

// forCapability returns the binding of name under exactly that capability.
func (b toolBindings) forCapability(name, capabilityID string) *policy.ToolBinding {
	return b[name][capabilityID]
}

// unique returns the binding of name when exactly one capability holds it. A
// name two capabilities claim resolves to nothing: an ambiguous binding is not a
// binding.
func (b toolBindings) unique(name string) *policy.ToolBinding {
	if len(b[name]) != 1 {
		return nil
	}
	for _, binding := range b[name] {
		return binding
	}
	return nil
}

func validToolName(name string) bool {
	return name != "" && len(name) <= maxToolNameLength
}

// lookupToolBindings resolves names against capability_tools for the verified
// tenant. The same tool declared by the tenant and by the global seed resolves
// to the stricter class and floor, so a tenant row can never loosen the seed.
func (s *Server) lookupToolBindings(ctx context.Context, orgID string, names []string) (toolBindings, error) {
	rows, err := s.toolDB.Query(ctx, toolBindingSQL, orgID, names)
	if err != nil {
		return nil, fmt.Errorf("resolve tool bindings: %w", err)
	}
	defer rows.Close()
	found := toolBindings{}
	for rows.Next() {
		var capabilityID, toolName, qualifiedName, class, floor, digest, reviewState, version, serverID string
		if err := rows.Scan(&capabilityID, &toolName, &qualifiedName, &class, &floor, &digest, &reviewState, &version, &serverID); err != nil {
			return nil, fmt.Errorf("scan tool binding: %w", err)
		}
		name := toolName
		if qualifiedName != "" {
			name = qualifiedName
		}
		if found[name] == nil {
			found[name] = map[string]*policy.ToolBinding{}
		}
		if existing := found[name][capabilityID]; existing != nil {
			existing.SideEffectClass = models.StricterSideEffectClass(existing.SideEffectClass, class)
			existing.ApprovalFloor = models.StricterApprovalFloor(existing.ApprovalFloor, floor)
			continue
		}
		found[name][capabilityID] = &policy.ToolBinding{
			ToolName:          name,
			CapabilityID:      capabilityID,
			CapabilityVersion: version,
			SideEffectClass:   models.NormalizeSideEffectClass(class),
			ApprovalFloor:     models.NormalizeApprovalFloor(floor),
			ToolDigest:        digest,
			ReviewState:       reviewState,
			// WS25-S9: the org extension allow-list matches MCP tools by server; empty for a built-in tool.
			ServerID: serverID,
		}
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read tool bindings: %w", err)
	}
	return found, nil
}

// lookupToolBinding resolves one tool name under the capability being
// evaluated. It returns nil when the registry holds no such binding or no
// database is attached; the caller decides whether that is a denial.
// boundElsewhere reports that the name is not bound under this capability but
// the registry does hold it under another one, which the caller must not paper
// over with this capability's ceiling.
func (s *Server) lookupToolBinding(ctx context.Context, orgID, capabilityID, toolName string) (binding *policy.ToolBinding, boundElsewhere bool, err error) {
	if s.toolDB == nil || !validToolName(toolName) {
		return nil, false, nil
	}
	found, err := s.lookupToolBindings(ctx, orgID, []string{toolName})
	if err != nil {
		return nil, false, status.Error(codes.Unavailable, "tool binding lookup unavailable")
	}
	binding = found.forCapability(toolName, capabilityID)
	return binding, binding == nil && len(found[toolName]) > 0, nil
}

// ResolveToolBindings returns the registry-owned binding of each requested tool
// the verified tenant can see. It is the only way an `mcp__` name acquires a
// capability, class and digest: a name the registry does not hold is absent from
// the response, and the caller must not dispatch it. The response carries
// classes, floors, digests and review states, never a description or a schema.
func (s *Server) ResolveToolBindings(ctx context.Context, req *mpv1.ResolveToolBindingsRequest) (*mpv1.ResolveToolBindingsResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ResolveToolBindings")))
	principal, err := verifiedPrincipal(ctx)
	if err != nil {
		return nil, err
	}
	orgID := principal.OrganizationID
	if req == nil || (req.GetOrgId() != "" && req.GetOrgId() != orgID) {
		return nil, mapErr(domain.ErrInvalidArgument)
	}
	// A service (execution-core) asserts the person its run acts for; a person
	// may only ask on their own behalf.
	if principal.PrincipalType != "service" && req.GetUserId() != "" && req.GetUserId() != principal.ActorID {
		return nil, status.Error(codes.PermissionDenied, "a caller may only resolve bindings for themselves")
	}
	if len(req.GetToolNames()) > maxToolBindingNames {
		return nil, mapErr(domain.ErrInvalidArgument)
	}
	names := make([]string, 0, len(req.GetToolNames()))
	seen := make(map[string]struct{}, len(req.GetToolNames()))
	for _, name := range req.GetToolNames() {
		if _, dup := seen[name]; dup || !validToolName(name) {
			continue
		}
		seen[name] = struct{}{}
		names = append(names, name)
	}
	if s.toolDB == nil {
		return nil, status.Error(codes.Unavailable, "tool binding lookup unavailable")
	}
	found := toolBindings{}
	if len(names) > 0 {
		found, err = s.lookupToolBindings(ctx, orgID, names)
		if err != nil {
			return nil, status.Error(codes.Unavailable, "tool binding lookup unavailable")
		}
	}
	blocked := map[string]*models.Capability{}
	response := &mpv1.ResolveToolBindingsResponse{Bindings: make([]*mpv1.ToolBinding, 0, len(names))}
	for _, name := range names {
		binding := found.unique(name)
		if binding == nil {
			continue
		}
		effective, err := s.effectiveToolPolicy(ctx, orgID, binding, blocked)
		if err != nil {
			return nil, err
		}
		response.Bindings = append(response.Bindings, &mpv1.ToolBinding{
			ToolName:          name,
			CapabilityId:      binding.CapabilityID,
			CapabilityVersion: binding.CapabilityVersion,
			SideEffectClass:   models.NormalizeSideEffectClass(binding.SideEffectClass),
			ApprovalFloor:     models.EffectiveApprovalFloor(binding.SideEffectClass, binding.ApprovalFloor),
			ToolDigest:        binding.ToolDigest,
			ReviewState:       binding.ReviewState,
			EffectivePolicy:   effective,
		})
	}
	return response, nil
}

// effectiveToolPolicy is the tool's registry-level answer: deny when its
// capability is gone, out of rollout or unavailable, otherwise the capability's
// risk decision tightened by the tool's own declaration. seen memoises the
// capability snapshot per id so a request resolving many tools of one
// capability reads it once; a nil entry records "not usable".
func (s *Server) effectiveToolPolicy(ctx context.Context, orgID string, binding *policy.ToolBinding, seen map[string]*models.Capability) (string, error) {
	capability, known := seen[binding.CapabilityID]
	if !known {
		resolved, rolloutState, err := s.capabilityForPolicy(ctx, orgID, binding.CapabilityID)
		switch {
		case errors.Is(err, domain.ErrCapabilityNotFound):
			resolved = nil
		case err != nil:
			return "", mapErr(err)
		case capabilityPolicyBlockReason(resolved, rolloutState) != "":
			resolved = nil
		}
		seen[binding.CapabilityID] = resolved
		capability = resolved
	}
	if capability == nil {
		return policy.DecisionDeny, nil
	}
	return policy.EffectivePolicy(capability, binding), nil
}

func capabilityPolicyBlockReason(capability *models.Capability, rolloutState string) string {
	switch strings.TrimSpace(rolloutState) {
	case "", "stable", "canary":
	case "quarantine":
		return "rollout_quarantine"
	case "deprecated":
		return "rollout_deprecated"
	default:
		return "invalid_rollout_state"
	}

	availability := models.DeriveAvailability(capability)
	if availability.State == models.AvailabilityAvailable {
		return ""
	}
	if availability.State == models.AvailabilityApprovalRequired {
		// Availability says this route is healthy but gated. Continue through
		// scope/grant policy so the engine can return the first-class `ask`
		// decision; treating it as unavailable would collapse HITL into deny.
		return ""
	}
	if availability.ReasonCode != "" {
		return availability.ReasonCode
	}
	return "runtime_unavailable"
}

// ValidateSkillBundle validates that a skill exists and is structurally promotable.
func (s *Server) ValidateSkillBundle(ctx context.Context, req *mpv1.ValidateSkillBundleRequest) (*mpv1.ValidateSkillBundleResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ValidateSkillBundle")))
	orgID, err := verifiedOrganizationID(ctx)
	if err != nil {
		return nil, err
	}
	capability, validationErrors, err := s.registry.ValidateSkillForOrg(req.SkillId, orgID)
	if err != nil {
		return nil, mapErr(err)
	}
	detail := toDetail(capability)
	return &mpv1.ValidateSkillBundleResponse{Valid: len(validationErrors) == 0, Errors: validationErrors, Capability: detail}, nil
}

// ListSkillBundles returns the org's skill bundle summaries, narrowed to the requested skill
// ids when any are given. While SKILL_BUNDLES_ENABLED is off it answers an empty list.
func (s *Server) ListSkillBundles(ctx context.Context, req *mpv1.ListSkillBundlesRequest) (*mpv1.ListSkillBundlesResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ListSkillBundles")))
	orgID, err := verifiedOrganizationID(ctx)
	if err != nil {
		return nil, err
	}
	if !s.skillBundlesEnabled {
		return &mpv1.ListSkillBundlesResponse{}, nil
	}
	summaries, err := s.skillBundles.List(ctx, orgID, req.SkillIds)
	if err != nil {
		return nil, skillBundleErr(err)
	}
	// DeckTemplate stays false while SKILL_DECK_TEMPLATES_ENABLED is off, so the response is the
	// same bytes as before the field existed.
	bundles := make([]*mpv1.SkillBundleSummary, 0, len(summaries))
	for _, summary := range summaries {
		bundles = append(bundles, &mpv1.SkillBundleSummary{
			SkillId:        summary.SkillID,
			ManifestSha256: summary.ManifestSHA256,
			FileCount:      int32(summary.FileCount),
			TotalBytes:     summary.TotalBytes,
			CreatedBy:      summary.CreatedBy,
			CreatedAt:      summary.CreatedAt.UTC().Format(time.RFC3339),
			DeckTemplate:   s.deckTemplatesEnabled && summary.DeckTemplate,
		})
	}
	return &mpv1.ListSkillBundlesResponse{Bundles: bundles}, nil
}

// GetSkillBundleManifest returns the digest and file metadata of the org's bundle for a skill,
// never a file body. While SKILL_BUNDLES_ENABLED is off every skill reads as having no bundle.
// With SKILL_BUNDLE_RETENTION on, a manifest_sha256 in the request names the kept bundle to read.
func (s *Server) GetSkillBundleManifest(ctx context.Context, req *mpv1.GetSkillBundleManifestRequest) (*mpv1.SkillBundleManifest, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "GetSkillBundleManifest")))
	orgID, err := verifiedOrganizationID(ctx)
	if err != nil {
		return nil, err
	}
	if !s.skillBundlesEnabled {
		return nil, skillBundleErr(skillbundle.ErrNotFound)
	}
	manifest, err := s.readSkillBundleManifest(ctx, orgID, req.SkillId, req.ManifestSha256)
	if err != nil {
		return nil, skillBundleErr(err)
	}
	files := make([]*mpv1.SkillBundleFile, 0, len(manifest.Files))
	for _, file := range manifest.Files {
		files = append(files, &mpv1.SkillBundleFile{
			Path:       file.Path,
			Sha256:     file.SHA256,
			SizeBytes:  file.Size,
			Executable: file.Executable,
			Binary:     file.Binary,
		})
	}
	return &mpv1.SkillBundleManifest{SkillId: req.SkillId, ManifestSha256: manifest.ManifestSHA256, Files: files}, nil
}

// readSkillBundleManifest reads the live bundle, or with retention on and a manifest digest named,
// the skill's bundle with that digest (a run pinned to an older version reads its own files). A
// digest that is not lowercase hex SHA-256 is NotFound before any read.
func (s *Server) readSkillBundleManifest(ctx context.Context, orgID, skillID, manifestSHA256 string) (skillbundle.Manifest, error) {
	if !s.skillBundleRetention || manifestSHA256 == "" {
		return s.skillBundles.Manifest(ctx, orgID, skillID)
	}
	versions, ok := s.skillBundles.(skillBundleVersionReader)
	if !ok || !isSHA256Hex(manifestSHA256) {
		return skillbundle.Manifest{}, skillbundle.ErrNotFound
	}
	return versions.ManifestAt(ctx, orgID, skillID, manifestSHA256)
}

// isSHA256Hex reports whether value is the lowercase hex spelling every bundle digest uses.
func isSHA256Hex(value string) bool {
	if len(value) != 64 {
		return false
	}
	for _, character := range value {
		if (character < '0' || character > '9') && (character < 'a' || character > 'f') {
			return false
		}
	}
	return true
}

// GetSkillBlob returns one file body of the org's bundle for a skill, addressed by digest. A
// digest the skill's own bundle does not name is NotFound, even when another skill stores the
// same bytes.
func (s *Server) GetSkillBlob(ctx context.Context, req *mpv1.GetSkillBlobRequest) (*mpv1.GetSkillBlobResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "GetSkillBlob")))
	orgID, err := verifiedOrganizationID(ctx)
	if err != nil {
		return nil, err
	}
	if !s.skillBundlesEnabled {
		return nil, skillBundleErr(skillbundle.ErrNotFound)
	}
	content, err := s.skillBundles.Blob(ctx, orgID, req.SkillId, req.Sha256)
	if err != nil {
		return nil, skillBundleErr(err)
	}
	return &mpv1.GetSkillBlobResponse{Content: content}, nil
}

// skillBundleErr maps a skill bundle store error to a status. The store's own messages name SQL
// steps, so any failure other than a missing bundle answers a fixed Internal message.
func skillBundleErr(err error) error {
	if errors.Is(err, skillbundle.ErrNotFound) {
		return status.Error(codes.NotFound, "skill bundle not found")
	}
	return status.Error(codes.Internal, "skill bundle store unavailable")
}

// GetProcedureVersion returns one saved browser procedure version: its canonical body, which hashes to the digest
// it was stored under, and its name and tags, so the caller can verify the body before it runs. A user caller
// reads its own procedures. A service caller reads for the person it names in for_user_id, which the authorizer
// lets only execution-core assert. The org is always the verified one, never a request field. The read writes
// nothing, and a saved procedure is org configuration rather than conversation content, so a zero-retention
// caller may read it.
func (s *Server) GetProcedureVersion(ctx context.Context, req *mpv1.GetProcedureVersionRequest) (*mpv1.ProcedureVersion, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "GetProcedureVersion")))
	principal, err := verifiedPrincipal(ctx)
	if err != nil {
		return nil, err
	}
	if !s.proceduresEnabled {
		return nil, status.Error(codes.Unavailable, "procedures_disabled")
	}
	owner, err := procedureReaderFor(principal, req.GetForUserId())
	if err != nil {
		return nil, err
	}
	id := req.GetProcedureId()
	procedure, err := s.savedProcedures.Get(ctx, owner, id)
	if err != nil {
		return nil, procedureStoreErr(err)
	}
	version, err := s.savedProcedures.GetVersion(ctx, owner, id, procedures.Ref{Version: int(req.GetVersion()), Tag: req.GetTag()})
	if err != nil {
		return nil, procedureStoreErr(err)
	}
	// The store reads a body back from a JSONB column, which respells it (key order and spacing), so the body is
	// served in the canonical form the digest was taken over, as the HTTP route serves it. A body that no longer
	// hashes to the digest it was recorded under is refused, not served.
	canonical, digest, err := procedures.Canonicalize(version.Body)
	if err != nil || digest != version.Digest {
		return nil, status.Error(codes.Internal, "procedure_digest_mismatch")
	}
	return &mpv1.ProcedureVersion{
		ProcedureId: version.ProcedureID,
		Version:     uint32(version.Version),
		Digest:      version.Digest,
		Name:        procedure.Name,
		BodyJson:    canonical,
		Tags:        tagsAtVersion(procedure.Tags, version.Version),
	}, nil
}

// procedureReaderFor is the person a saved-procedure read is for. A user caller reads as itself and
// for_user_id is ignored, so it cannot reach another person's procedure. A service caller reads as the
// person it names, and must name one.
func procedureReaderFor(principal authctx.Principal, forUserID string) (procedures.Owner, error) {
	switch principal.PrincipalType {
	case "user":
		if principal.ActorID == "" {
			return procedures.Owner{}, status.Error(codes.PermissionDenied, "a saved procedure belongs to a person")
		}
		return procedures.Owner{OrgID: principal.OrganizationID, UserID: principal.ActorID}, nil
	case "service":
		user := strings.TrimSpace(forUserID)
		if user == "" {
			return procedures.Owner{}, status.Error(codes.PermissionDenied, "a service caller must name the person a saved procedure belongs to")
		}
		return procedures.Owner{OrgID: principal.OrganizationID, UserID: user}, nil
	}
	return procedures.Owner{}, status.Error(codes.PermissionDenied, "a saved procedure belongs to a person")
}

// procedureStoreErr maps a saved-procedure store error to a status. A missing procedure, version or tag, and a
// procedure that belongs to someone else, all answer NotFound. Any other failure answers a fixed message, never
// the store's own text.
func procedureStoreErr(err error) error {
	switch {
	case errors.Is(err, procedures.ErrNotFound):
		return status.Error(codes.NotFound, "procedure_not_found")
	case errors.Is(err, procedures.ErrInvalidRef):
		return status.Error(codes.InvalidArgument, "invalid_version_reference")
	default:
		return status.Error(codes.Internal, "saved procedure store unavailable")
	}
}

// tagsAtVersion names the tags that point at one version, in the store's tag order.
func tagsAtVersion(tags []procedures.Tag, version int) []string {
	var names []string
	for _, tag := range tags {
		if tag.Version == version {
			names = append(names, tag.Tag)
		}
	}
	return names
}

// CheckSkillPromotion validates whether a skill may move between scopes.
func (s *Server) CheckSkillPromotion(ctx context.Context, req *mpv1.CheckSkillPromotionRequest) (*mpv1.CheckSkillPromotionResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "CheckSkillPromotion")))
	orgID, err := verifiedOrganizationID(ctx)
	if err != nil {
		return nil, err
	}
	_, checks, err := s.registry.CheckPromotionForOrg(req.SkillId, req.FromScope, req.ToScope, orgID)
	if err != nil {
		return nil, mapErr(err)
	}
	passed := hasAllChecks(checks,
		"skill_exists",
		"skill_valid",
		"source_scope_matches",
		"target_scope_valid",
		"scope_changes",
	)
	reason := "promotion checks passed"
	if !passed {
		reason = "promotion checks failed"
	}
	return &mpv1.CheckSkillPromotionResponse{Passed: passed, Checks: checks, Reason: reason}, nil
}

// PromoteSkill is deliberately not implemented here. Calls fall through to
// the embedded mpv1.UnimplementedCapabilityCoreServer.PromoteSkill, which
// returns a plain codes.Unimplemented — the honest answer, in place of the
// permanently-failing codes.FailedPrecondition this method used to return.
//
// Removed per QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md SKILL-2. Findings:
//
//   - This RPC could never succeed in production: cmd/main.go always builds
//     a durable capabilities store and attaches it via WithStore, so the
//     `if s.store != nil` guard this method used to have fired on every
//     real call and returned FailedPrecondition. Only tests, which build a
//     Server with no store, ever reached the in-memory branch below it.
//
//   - That in-memory branch was not "real" durable promotion regardless:
//     the models.Capability.Scope field it mutated (via the registry's
//     since-removed PromoteSkillForOrg) is a rollout/routing label — agent
//     -> workspace -> org -> global — not an ownership or access-control
//     field, and nothing reads it back to decide runtime availability.
//     policy.Engine.EvaluateCapability, the one function that actually
//     decides allow/deny/ask, never reads Capability.Scope: it reads
//     EnabledForScopes (the static scope *kinds* a capability supports)
//     and, when a ScopeResolver is attached, the durable capability_scopes
//     grant table via registry.ScopeStore — the real "who is granted it"
//     authority. Implementing durable promotion against .Scope would have
//     built a convincing no-op, not a working feature.
//
//   - The real mechanism for broadening a capability's reach already
//     exists and works: registry.ScopeStore.Grant, exposed through
//     api.NewCapabilitiesHandler(...).WithScopeStore(...) in cmd/main.go.
//
// ValidateSkillBundle and CheckSkillPromotion are unaffected and stay: they
// are read-only dry-run checks over the same Scope field/checkPromotion()
// invariants, still useful for inspecting promotion eligibility even though
// nothing can durably act on the result via this RPC.
//
// orchestrator-core's SkillPromotionWorkflow (which called this RPC via
// UpdateRegistryActivity) now fails fast and explicitly at that step
// instead of retrying a call that could never succeed — see
// services/orchestrator-core/cmd/workflows/skill_promotion.go.

func verifiedOrganizationID(ctx context.Context) (string, error) {
	principal, err := verifiedPrincipal(ctx)
	if err != nil {
		return "", err
	}
	return principal.OrganizationID, nil
}

func verifiedPrincipal(ctx context.Context) (authctx.Principal, error) {
	principal, ok := authctx.PrincipalFromContext(ctx)
	if !ok || strings.TrimSpace(principal.OrganizationID) == "" {
		return authctx.Principal{}, status.Error(codes.Unauthenticated, "verified identity required")
	}
	return principal, nil
}

// Register wires the CapabilityCore service onto the provided gRPC server.
func Register(g grpc.ServiceRegistrar, impl mpv1.CapabilityCoreServer) {
	mpv1.RegisterCapabilityCoreServer(g, impl)
}
