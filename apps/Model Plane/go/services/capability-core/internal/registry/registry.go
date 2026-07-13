// Package registry is an in-memory capability registry seeded with a static
// catalog. It is safe for concurrent reads and supports simple filtering and
// cursor-based pagination via the AfterID/Limit parameters.
package registry

import (
	"strings"
	"sync"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// Source provides the authoritative list of capabilities backing a Registry.
// Implementations may be static seeds, filesystem loaders, or remote config
// sources. Load is invoked on construction and on every Reload.
type Source interface {
	Load() ([]*models.Capability, error)
}

// staticSeedSource returns the default in-memory catalog.
type staticSeedSource struct{}

// Load returns a fresh copy of the default seeded capability catalog.
func (staticSeedSource) Load() ([]*models.Capability, error) {
	return []*models.Capability{
		{ID: "cap.browser.open", Name: "Open Browser Session", Kind: models.KindBrowser, Version: "1.0.0", Description: "Start an isolated headless browser session.", RiskLevel: models.RiskMedium, LazyLoad: true, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.browser.open", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.sandbox.exec", Name: "Execute In Sandbox", Kind: models.KindSandbox, Version: "1.0.0", Description: "Run arbitrary code in an ephemeral sandbox.", RiskLevel: models.RiskHigh, LazyLoad: true, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.sandbox.exec", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.memory.search", Name: "Search Memory", Kind: models.KindMemory, Version: "1.1.0", Description: "Semantic search over long-term memory.", RiskLevel: models.RiskLow, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.memory.search", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.memory.index", Name: "Index Memory", Kind: models.KindMemory, Version: "1.1.0", Description: "Persist a document into long-term memory.", RiskLevel: models.RiskLow, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.memory.index", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.retrieval.query", Name: "Retrieval Query", Kind: models.KindRetrieval, Version: "2.0.0", Description: "Hybrid retrieval over enterprise corpora.", RiskLevel: models.RiskLow, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.retrieval.query", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "operating_map.generate", Name: "Generate Operating Map", Kind: models.KindInference, Version: "1.0.0", Description: "Synthesize evidence-grounded AI Operating Map proposals from Data Plane retrieval, graph, wiki, and source traces.", RiskLevel: models.RiskMedium, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: idempotencyPrefix + "operating_map.generate", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.tool.http", Name: "HTTP Fetch", Kind: models.KindTool, Version: "1.0.0", Description: "Perform an outbound HTTP request.", RiskLevel: models.RiskMedium, LazyLoad: true, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.tool.http", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.inference.chat", Name: "Chat Completion", Kind: models.KindInference, Version: "1.0.0", Description: "Route a chat completion to the inference plane.", RiskLevel: models.RiskLow, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.inference.chat", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.inference.embedding", Name: "Embedding Generation", Kind: models.KindInference, Version: "1.0.0", Description: "Generate provider-backed embeddings; Data Plane owns vector storage and retrieval.", RiskLevel: models.RiskLow, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.inference.embedding", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.inference.model-catalog", Name: "Model Catalogue", Kind: models.KindInference, Version: "1.0.0", Description: "List active provider models and deployments available to inference-core.", RiskLevel: models.RiskLow, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.inference.model-catalog", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.inference.speech", Name: "Speech Synthesis and Transcription", Kind: models.KindInference, Version: "1.0.0", Description: "Provider-backed TTS/STT through inference-core with HTTP/gRPC gateway routes and voice catalogue.", RiskLevel: models.RiskMedium, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.inference.speech", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.inference.image", Name: "Image Generation and OCR", Kind: models.KindInference, Version: "1.0.0", Description: "Provider-backed image generation, image analysis, and OCR through inference-core.", RiskLevel: models.RiskMedium, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.inference.image", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.inference.translation", Name: "Translation", Kind: models.KindInference, Version: "1.0.0", Description: "Provider-backed translate, batch translate, language detection, and language catalogue calls through inference-core.", RiskLevel: models.RiskLow, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.inference.translation", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.inference.document-intel", Name: "Document Intelligence", Kind: models.KindInference, Version: "1.0.0", Description: "Provider-backed document analysis, layout, forms, receipt, and invoice extraction through inference-core.", RiskLevel: models.RiskMedium, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.inference.document-intel", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.inference.language-analytics", Name: "Language Analytics", Kind: models.KindInference, Version: "1.0.0", Description: "Provider-backed sentiment, entities, key phrases, PII, language detection, and text summary through inference-core.", RiskLevel: models.RiskMedium, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.inference.language-analytics", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.inference.realtime", Name: "Realtime Session Broker", Kind: models.KindInference, Version: "1.0.0", Description: "Provider-backed realtime client-secret brokering through inference-core.", RiskLevel: models.RiskMedium, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.inference.realtime", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.inference.video", Name: "Video Generation Jobs", Kind: models.KindInference, Version: "1.0.0", Description: "Provider-backed async video generation job submission and status through inference-core.", RiskLevel: models.RiskMedium, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.inference.video", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.skill.summarize", Name: "Summarize Skill Bundle", Kind: models.KindSkill, Version: "1.0.0", Description: "Summarize and promote long-form reasoning assets.", RiskLevel: models.RiskLow, LazyLoad: true, Scope: "agent", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.skill.summarize", OrgID: "triodelab", EnabledForScopes: []string{"agent"}},
		{ID: "cap.plugin.code-exec", Name: "Code Execution Plugin", Kind: models.KindPlugin, Version: "1.0.0", Description: "Plugin wrapper for language-specific code execution runtimes.", RiskLevel: models.RiskHigh, LazyLoad: true, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.plugin.code-exec", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.mcp.filesystem", Name: "Filesystem MCP Server", Kind: models.KindMCPServer, Version: "1.0.0", Description: "MCP server exposing local filesystem tools to the model plane.", RiskLevel: models.RiskMedium, LazyLoad: true, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.mcp.filesystem", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.policy.round-robin", Name: "Round-Robin Routing Policy", Kind: models.KindRoutingPolicy, Version: "1.0.0", Description: "Distribute inference requests evenly across available models.", RiskLevel: models.RiskLow, LazyLoad: false, Scope: "global", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.policy.round-robin", OrgID: "triodelab", EnabledForScopes: []string{"global"}},
		{ID: "cap.memory-adapter.redis", Name: "Redis Memory Adapter", Kind: models.KindMemoryAdapter, Version: "1.0.0", Description: "Adapter that persists and retrieves memory entries via Redis.", RiskLevel: models.RiskLow, LazyLoad: false, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.memory-adapter.redis", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		{ID: "cap.safety.pii-filter", Name: "PII Safety Filter", Kind: models.KindSafetyPolicy, Version: "1.0.0", Description: "Redact personally identifiable information from model I/O.", RiskLevel: models.RiskLow, LazyLoad: false, Scope: "global", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.safety.pii-filter", OrgID: "triodelab", EnabledForScopes: []string{"global"}},
		{ID: "cap.command.shell", Name: "Shell Command", Kind: models.KindCommand, Version: "1.0.0", Description: "Execute an allowlisted shell command via the command plane.", RiskLevel: models.RiskHigh, LazyLoad: true, Scope: "workspace", Enabled: true, IdempotencyKey: "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:cap.command.shell", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
	}, nil
}

// Registry holds capabilities in memory.
type Registry struct {
	mu    sync.RWMutex
	src   Source
	items []*models.Capability
	index map[string]*models.Capability
}

// NewRegistry returns a registry pre-seeded with the default static catalog.
func NewRegistry() *Registry {
	r, err := NewFromSource(staticSeedSource{})
	if err != nil {
		// staticSeedSource never errors; a panic here indicates a programmer
		// mistake (e.g. corrupt seed) and must surface loudly.
		panic(err)
	}
	return r
}

// NewFromSource constructs a Registry backed by the provided Source and
// performs the initial load.
func NewFromSource(s Source) (*Registry, error) {
	if s == nil {
		return nil, domain.ErrInvalidArgument
	}
	r := &Registry{src: s}
	if err := r.Reload(); err != nil {
		return nil, err
	}
	return r, nil
}

// Reload re-reads the backing Source and atomically swaps the in-memory
// catalog. Readers observe either the old or new catalog consistently.
func (r *Registry) Reload() error {
	if r.src == nil {
		return domain.ErrInvalidArgument
	}
	caps, err := r.src.Load()
	if err != nil {
		return err
	}
	idx := make(map[string]*models.Capability, len(caps))
	for _, c := range caps {
		if c == nil || !models.IsSupportedRiskLevel(c.RiskLevel) {
			return domain.ErrInvalidArgument
		}
		idx[c.ID] = c
	}
	r.mu.Lock()
	r.items = caps
	r.index = idx
	r.mu.Unlock()
	return nil
}

var validScopes = map[string]struct{}{
	"agent":     {},
	"workspace": {},
	"user":      {},
	"global":    {},
}

func containsAll(checks []string, required ...string) bool {
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

// List returns a slice of capabilities matching the provided filters. A
// pagination cursor (afterID) advances past the matching element. When limit
// is zero, a default of 50 is used.
func (r *Registry) List(kindFilter, query, afterID string, limit uint32) ([]*models.Capability, bool) {
	r.mu.RLock()
	defer r.mu.RUnlock()

	if limit == 0 || limit > 200 {
		limit = 50
	}

	q := strings.ToLower(strings.TrimSpace(query))
	filtered := make([]*models.Capability, 0, len(r.items))
	for _, c := range r.items {
		if !c.Enabled {
			continue
		}
		if kindFilter != "" && c.Kind != kindFilter {
			continue
		}
		if q != "" && !strings.Contains(strings.ToLower(c.Name), q) && !strings.Contains(strings.ToLower(c.Description), q) {
			continue
		}
		filtered = append(filtered, c)
	}

	start := 0
	if afterID != "" {
		for i, c := range filtered {
			if c.ID == afterID {
				start = i + 1
				break
			}
		}
	}

	end := start + int(limit)
	hasMore := false
	if end < len(filtered) {
		hasMore = true
	} else {
		end = len(filtered)
	}
	if start > len(filtered) {
		start = len(filtered)
	}
	return filtered[start:end], hasMore
}

// ListForOrg returns only capabilities owned by the verified tenant or the
// explicit global catalog. Tenant entries override a global entry with the
// same ID so a mixed process-wide registry cannot leak or ambiguously resolve
// another tenant's capability.
func (r *Registry) ListForOrg(orgID, kindFilter, query, afterID string, limit uint32) ([]*models.Capability, bool) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, false
	}

	r.mu.RLock()
	defer r.mu.RUnlock()

	if limit == 0 || limit > 200 {
		limit = 50
	}

	selected := make([]*models.Capability, 0, len(r.items))
	positions := make(map[string]int, len(r.items))
	for _, capability := range r.items {
		if !capabilityVisibleToOrg(capability, orgID) {
			continue
		}
		if position, exists := positions[capability.ID]; exists {
			if capability.OrgID == orgID {
				selected[position] = capability
			}
			continue
		}
		positions[capability.ID] = len(selected)
		selected = append(selected, capability)
	}

	q := strings.ToLower(strings.TrimSpace(query))
	filtered := make([]*models.Capability, 0, len(selected))
	for _, capability := range selected {
		if !capability.Enabled || (kindFilter != "" && capability.Kind != kindFilter) {
			continue
		}
		if q != "" && !strings.Contains(strings.ToLower(capability.Name), q) &&
			!strings.Contains(strings.ToLower(capability.Description), q) {
			continue
		}
		filtered = append(filtered, capability)
	}

	start := 0
	if afterID != "" {
		for index, capability := range filtered {
			if capability.ID == afterID {
				start = index + 1
				break
			}
		}
	}
	if start > len(filtered) {
		start = len(filtered)
	}
	end := start + int(limit)
	hasMore := end < len(filtered)
	if end > len(filtered) {
		end = len(filtered)
	}
	return filtered[start:end], hasMore
}

// Get returns a capability by ID, optionally enforcing a version constraint.
// The constraint syntax is a simple exact match (e.g. "1.0.0").
func (r *Registry) Get(id, versionConstraint string) (*models.Capability, error) {
	if id == "" {
		return nil, domain.ErrInvalidArgument
	}
	r.mu.RLock()
	defer r.mu.RUnlock()
	c, ok := r.index[id]
	if !ok {
		return nil, domain.ErrCapabilityNotFound
	}
	if versionConstraint != "" && versionConstraint != c.Version {
		return nil, domain.ErrVersionMismatch
	}
	return c, nil
}

// GetForOrg resolves a capability only when it belongs to the verified tenant
// or the explicit global catalog. A tenant-owned entry wins over a global
// entry with the same ID. Foreign and missing IDs are intentionally
// indistinguishable.
func (r *Registry) GetForOrg(id, versionConstraint, orgID string) (*models.Capability, error) {
	if strings.TrimSpace(id) == "" || strings.TrimSpace(orgID) == "" {
		return nil, domain.ErrInvalidArgument
	}
	r.mu.RLock()
	defer r.mu.RUnlock()
	return r.getForOrgLocked(id, versionConstraint, strings.TrimSpace(orgID))
}

func (r *Registry) getForOrgLocked(id, versionConstraint, orgID string) (*models.Capability, error) {
	var global *models.Capability
	for _, capability := range r.items {
		if capability == nil || capability.ID != id {
			continue
		}
		if capability.OrgID == orgID {
			if versionConstraint != "" && versionConstraint != capability.Version {
				return nil, domain.ErrVersionMismatch
			}
			return capability, nil
		}
		if capability.OrgID == "global" {
			global = capability
		}
	}
	if global == nil {
		return nil, domain.ErrCapabilityNotFound
	}
	if versionConstraint != "" && versionConstraint != global.Version {
		return nil, domain.ErrVersionMismatch
	}
	return global, nil
}

func capabilityVisibleToOrg(capability *models.Capability, orgID string) bool {
	return capability != nil && (capability.OrgID == orgID || capability.OrgID == "global")
}

// ValidateSkill checks that the requested capability exists and is a skill.
// Semantic validation failures are reported in the returned error list rather
// than as transport errors so callers can surface them directly to users.
func (r *Registry) ValidateSkill(id string) (*models.Capability, []string, error) {
	capability, err := r.Get(id, "")
	if err != nil {
		return nil, nil, err
	}

	errorsOut := make([]string, 0)
	if capability.Kind != models.KindSkill {
		errorsOut = append(errorsOut, "capability is not a skill")
	}
	if capability.Name == "" {
		errorsOut = append(errorsOut, "skill name is required")
	}
	if capability.Version == "" {
		errorsOut = append(errorsOut, "skill version is required")
	}
	if capability.Scope == "" {
		errorsOut = append(errorsOut, "skill scope is required")
	}

	return capability, errorsOut, nil
}

// ValidateSkillForOrg is the tenant-pinned variant used at authenticated gRPC
// boundaries.
func (r *Registry) ValidateSkillForOrg(id, orgID string) (*models.Capability, []string, error) {
	if strings.TrimSpace(id) == "" || strings.TrimSpace(orgID) == "" {
		return nil, nil, domain.ErrInvalidArgument
	}
	r.mu.RLock()
	defer r.mu.RUnlock()
	capability, err := r.getForOrgLocked(id, "", strings.TrimSpace(orgID))
	if err != nil {
		return nil, nil, err
	}
	return validateSkill(capability)
}

func validateSkill(capability *models.Capability) (*models.Capability, []string, error) {
	if capability == nil {
		return nil, nil, domain.ErrCapabilityNotFound
	}
	errorsOut := make([]string, 0)
	if capability.Kind != models.KindSkill {
		errorsOut = append(errorsOut, "capability is not a skill")
	}
	if capability.Name == "" {
		errorsOut = append(errorsOut, "skill name is required")
	}
	if capability.Version == "" {
		errorsOut = append(errorsOut, "skill version is required")
	}
	if capability.Scope == "" {
		errorsOut = append(errorsOut, "skill scope is required")
	}
	return capability, errorsOut, nil
}

// CheckPromotion validates whether a skill can be promoted between scopes.
func (r *Registry) CheckPromotion(id, fromScope, toScope string) (*models.Capability, []string, error) {
	if fromScope == "" || toScope == "" {
		return nil, nil, domain.ErrInvalidArgument
	}
	capability, validationErrors, err := r.ValidateSkill(id)
	if err != nil {
		return nil, nil, err
	}

	checks := make([]string, 0, 4)
	checks = append(checks, "skill_exists")
	if len(validationErrors) == 0 {
		checks = append(checks, "skill_valid")
	}
	if capability.Scope == fromScope {
		checks = append(checks, "source_scope_matches")
	}
	if _, ok := validScopes[toScope]; ok {
		checks = append(checks, "target_scope_valid")
	}
	if fromScope != toScope {
		checks = append(checks, "scope_changes")
	}

	if len(validationErrors) > 0 {
		return capability, append(checks, validationErrors...), nil
	}
	if capability.Scope != fromScope {
		return capability, append(checks, "source_scope_mismatch"), nil
	}
	if _, ok := validScopes[toScope]; !ok {
		return capability, append(checks, "target_scope_invalid"), nil
	}
	if fromScope == toScope {
		return capability, append(checks, "target_scope_unchanged"), nil
	}

	return capability, checks, nil
}

// CheckPromotionForOrg performs promotion validation only after tenant-pinned
// lookup of the skill.
func (r *Registry) CheckPromotionForOrg(id, fromScope, toScope, orgID string) (*models.Capability, []string, error) {
	if fromScope == "" || toScope == "" || strings.TrimSpace(orgID) == "" {
		return nil, nil, domain.ErrInvalidArgument
	}
	r.mu.RLock()
	defer r.mu.RUnlock()
	capability, err := r.getForOrgLocked(id, "", strings.TrimSpace(orgID))
	if err != nil {
		return nil, nil, err
	}
	return checkPromotion(capability, fromScope, toScope)
}

func checkPromotion(capability *models.Capability, fromScope, toScope string) (*models.Capability, []string, error) {
	if fromScope == "" || toScope == "" {
		return nil, nil, domain.ErrInvalidArgument
	}
	capability, validationErrors, err := validateSkill(capability)
	if err != nil {
		return nil, nil, err
	}

	checks := make([]string, 0, 5)
	checks = append(checks, "skill_exists")
	if len(validationErrors) == 0 {
		checks = append(checks, "skill_valid")
	}
	if capability.Scope == fromScope {
		checks = append(checks, "source_scope_matches")
	}
	if _, ok := validScopes[toScope]; ok {
		checks = append(checks, "target_scope_valid")
	}
	if fromScope != toScope {
		checks = append(checks, "scope_changes")
	}

	if len(validationErrors) > 0 {
		return capability, append(checks, validationErrors...), nil
	}
	if capability.Scope != fromScope {
		return capability, append(checks, "source_scope_mismatch"), nil
	}
	if _, ok := validScopes[toScope]; !ok {
		return capability, append(checks, "target_scope_invalid"), nil
	}
	if fromScope == toScope {
		return capability, append(checks, "target_scope_unchanged"), nil
	}
	return capability, checks, nil
}

// PromoteSkill updates a skill's scope after promotion checks have passed.
func (r *Registry) PromoteSkill(id, fromScope, toScope string) (*models.Capability, []string, error) {
	capability, checks, err := r.CheckPromotion(id, fromScope, toScope)
	if err != nil {
		return nil, nil, err
	}
	if !containsAll(checks,
		"skill_exists",
		"skill_valid",
		"source_scope_matches",
		"target_scope_valid",
		"scope_changes",
	) {
		return capability, checks, nil
	}

	r.mu.Lock()
	defer r.mu.Unlock()

	current, ok := r.index[id]
	if !ok {
		return nil, nil, domain.ErrCapabilityNotFound
	}
	updated := *current
	updated.Scope = toScope
	r.index[id] = &updated
	for idx, entry := range r.items {
		if entry.ID == id {
			r.items[idx] = &updated
			break
		}
	}

	checks = append(checks, "registry_updated")
	return &updated, checks, nil
}

// PromoteSkillForOrg atomically rechecks ownership and promotion invariants
// under the registry lock before changing the tenant/global entry. A reload
// cannot swap in a foreign entry between authorization and mutation.
func (r *Registry) PromoteSkillForOrg(id, fromScope, toScope, orgID string) (*models.Capability, []string, error) {
	if strings.TrimSpace(orgID) == "" {
		return nil, nil, domain.ErrInvalidArgument
	}
	r.mu.Lock()
	defer r.mu.Unlock()

	capability, err := r.getForOrgLocked(id, "", strings.TrimSpace(orgID))
	if err != nil {
		return nil, nil, err
	}
	capability, checks, err := checkPromotion(capability, fromScope, toScope)
	if err != nil {
		return nil, nil, err
	}
	if !containsAll(checks,
		"skill_exists",
		"skill_valid",
		"source_scope_matches",
		"target_scope_valid",
		"scope_changes",
	) {
		return capability, checks, nil
	}

	updated := *capability
	updated.Scope = toScope
	for index, entry := range r.items {
		if entry == capability {
			r.items[index] = &updated
			break
		}
	}
	if indexed := r.index[id]; indexed == capability {
		r.index[id] = &updated
	}
	checks = append(checks, "registry_updated")
	return &updated, checks, nil
}
