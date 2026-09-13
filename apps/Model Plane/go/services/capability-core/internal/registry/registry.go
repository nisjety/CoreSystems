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
		// cap.command.sandbox is deliberately RiskLow while cap.command.shell
		// directly above stays RiskHigh. They are not two settings of one tool.
		//
		// Shell runs an arbitrary operator-allowlisted command with the run's
		// own filesystem and network reach, so policy.EvaluateCapability must
		// keep returning `ask` for it — arbitrary command execution stays
		// human-approved.
		//
		// The sandbox executor accepts only a code body, never a host command
		// name, and runs it under constraints neither the caller nor the model
		// can select, widen, or opt out of: read-only root filesystem (the code
		// cannot mutate or persist into the image), networking disabled (no
		// egress, no lateral reach, and therefore no exfiltration path), a
		// wall-clock timeout that kills the process so worst-case spend is
		// bounded by that ceiling, a per-call throwaway workspace discarded
		// afterwards so nothing carries across invocations, runs, or tenants,
		// and secret-scrubbed stdout/stderr so captured output cannot ferry
		// credentials back into the transcript. With no egress, no writable
		// image, no cross-call persistence, and no path from the code body to a
		// named host command, the residual blast radius is bounded CPU and
		// wall-clock time inside a container that is destroyed either way —
		// which is what RiskLow means here. Routing it through the high-risk
		// branch instead would make a calculator step `awaiting_approval` and
		// train operators to rubber-stamp prompts, degrading the gate that
		// cap.command.shell genuinely needs.
		//
		// Kind and scope mirror cap.command.shell's declaration in this seed.
		// The durable row is seeded by
		// migrations/0010_sandbox_code_execution_capability.up.sql, which
		// mirrors the live cap.command.shell row instead (kind 'tool', global
		// scope) — that divergence between seed and migration is pre-existing
		// and deliberate on both sides.
		{ID: "cap.command.sandbox", Name: "Sandboxed Code Execution", Kind: models.KindCommand, Version: "1.0.0", Description: "Run a code body in a hermetic per-call sandbox: read-only root filesystem, networking disabled, wall-clock timeout, throwaway workspace discarded after the call, and secret-scrubbed output. Cannot execute a named host command and cannot persist between calls.", RiskLevel: models.RiskLow, LazyLoad: true, Scope: "workspace", Enabled: true, IdempotencyKey: idempotencyPrefix + "cap.command.sandbox", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
		// cap.process.background is the S4.2 background-process family
		// (process_start/read/stdin/signal/list), and it is RiskLow for the
		// same reasons the row directly above is — not as a separate judgement
		// call. A background process runs the same code body, never a named
		// host command, under the same bubblewrap argv: read-only root
		// filesystem, networking disabled, secret-scrubbed output, no path to
		// a host command.
		//
		// What it adds over cap.command.sandbox is bounded on both axes it
		// widens: duration (a TTL of at most an hour, and never past the
		// owning lease's own expiry) and concurrency (a registry-enforced
		// count limit per Space). Its workspace is the Space's hydrated
		// workspace rather than a per-call throwaway, which is more
		// persistence but strictly less reach — it still cannot touch the
		// image.
		//
		// The capability is deliberately NOT the only gate, which is what
		// makes RiskLow safe to state plainly: a call also needs a Space lease
		// whose processes_permitted column is true, and sandbox-manager sets
		// that only from a Control decision carrying `space:processes`, which
		// Control grants only to a Space explicitly entitled to it. This row
		// governs whether the tool loop can reach the family at all; per-Space
		// authority is a separate deny-by-default decision.
		//
		// One row for five dispatch names on purpose: they are one authority
		// over one object. "May start but may not stop" is not a posture
		// anyone wants, and five rows is what makes it reachable by accident.
		//
		// Kind and scope mirror cap.command.sandbox's declaration here. The
		// durable row is seeded by
		// migrations/0015_background_process_capability.up.sql, which mirrors
		// the live cap.command.sandbox row instead (kind 'tool', global scope)
		// — that divergence between seed and migration is pre-existing and
		// deliberate on both sides.
		{ID: "cap.process.background", Name: "Background Process", Kind: models.KindCommand, Version: "1.0.0", Description: "Start and manage a background process in a Space's hydrated sandbox workspace: read-only root filesystem, networking disabled, TTL-bounded lifetime that never outlives the lease, count-limited concurrency, and secret-scrubbed durable output. Accepts a code body, never a named host command, and requires a Space lease Control separately granted background-process authority.", RiskLevel: models.RiskLow, LazyLoad: true, Scope: "workspace", Enabled: true, IdempotencyKey: idempotencyPrefix + "cap.process.background", OrgID: "triodelab", EnabledForScopes: []string{"workspace"}},
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
