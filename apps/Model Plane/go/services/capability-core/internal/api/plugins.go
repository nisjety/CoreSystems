package api

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"net/url"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/pkg/publisher"
	pluginloader "github.com/triodelab/model-plane/services/capability-core/internal/plugins"
	"github.com/triodelab/model-plane/services/capability-core/internal/reconcile"
)

// PluginsHandler — HTTP CRUD for the durable per-org plugin registry
// (`plugin_packages`). A plugin package is a manifest that can contribute tools,
// skills, and hooks; this handler is the system-of-record. Registering a plugin
// is inert until a host loads its manifest — see the package doc in registry_apis.go.
//
// Mirrors SkillsHandler/MCPHandler: org-scoped, reconcile-event on mutation,
// soft-delete (plugin_packages carries deleted_at + a unique (org,name,version)
// index scoped to live rows). Plugins default disabled+unpinned (safe rollout).
type PluginsHandler struct {
	pool registryDatabase
	pub  publisher.EventPublisher
	// pins turns on the supply-chain pins behind PLUGIN_LOADER_ENABLED (see WithSupplyChainPins).
	// Off, the handler is the inert CRUD it has always been.
	pins bool
	// loader expands an installed plugin into its child capabilities (W4-PL-2). It is set by
	// WithChildStores; the install, uninstall and components routes are mounted only when it is set and
	// the supply-chain pins are on.
	loader *pluginloader.Loader
}

// NewPluginsHandler constructs the handler.
func NewPluginsHandler(pool registryDatabase) *PluginsHandler {
	return &PluginsHandler{pool: pool}
}

// WithPublisher wires reconcile-event emission (matrix §4.3). Optional, nil-safe.
func (h *PluginsHandler) WithPublisher(pub publisher.EventPublisher) *PluginsHandler {
	h.pub = pub
	return h
}

// WithSupplyChainPins turns the supply-chain pins on or off (PLUGIN_LOADER_ENABLED, read with
// PluginLoaderFromLookup). On: create needs a source pin and computes the manifest digest, a killed
// digest is refused, and enabling needs a pinned, installed plugin. Off: today's inert CRUD.
func (h *PluginsHandler) WithSupplyChainPins(on bool) *PluginsHandler {
	h.pins = on
	return h
}

// WithChildStores wires the loader's child stores (PLUGIN_LOADER_ENABLED): skills and commands go
// through the SkillsHandler's store and lint settings, remote MCP servers through the MCPHandler's
// registration checks. Either may be nil, in which case the loader refuses that kind of child.
func (h *PluginsHandler) WithChildStores(skills *SkillsHandler, mcp *MCPHandler) *PluginsHandler {
	h.loader = &pluginloader.Loader{Children: newPluginChildStore(skills, mcp)}
	return h
}

// Register mounts routes.
func (h *PluginsHandler) Register(mux *http.ServeMux) {
	// W4-PL-2: off (or without child stores), these paths fall to the subtree below and answer as before.
	if h.pins && h.loader != nil {
		mux.HandleFunc("GET /api/v1/plugins/{id}/components", h.components)
		mux.HandleFunc("POST /api/v1/plugins/{id}/install", h.install)
		mux.HandleFunc("POST /api/v1/plugins/{id}/uninstall", h.uninstall)
	}
	mux.HandleFunc("/api/v1/plugins", h.listOrCreate)
	mux.HandleFunc("/api/v1/plugins/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/plugins/"):]
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

type pluginRow struct {
	ID           string          `json:"id"`
	OrgID        string          `json:"org_id"`
	Name         string          `json:"name"`
	Version      string          `json:"version"`
	Description  string          `json:"description"`
	ManifestJSON json.RawMessage `json:"manifest_json"`
	RiskLevel    string          `json:"risk_level"`
	Enabled      bool            `json:"enabled"`
	Pinned       bool            `json:"pinned"`
	RolloutState string          `json:"rollout_state"`
	CreatedAt    time.Time       `json:"created_at"`
	UpdatedAt    time.Time       `json:"updated_at"`

	// The supply-chain pins, set only under PLUGIN_LOADER_ENABLED. A row created with the switch off
	// has NULL here, and the omitempty tags keep its JSON as it was.
	SourceKind     *string    `json:"source_kind,omitempty"`
	SourceRef      *string    `json:"source_ref,omitempty"`
	SourcePin      *string    `json:"source_pin,omitempty"`
	ManifestDigest *string    `json:"manifest_digest,omitempty"`
	InstalledAt    *time.Time `json:"installed_at,omitempty"`
}

// pluginPatch is the body of PATCH /api/v1/plugins/{id}. The source fields are decoded so that the
// switch-on path can refuse them: a source pin is set when the plugin is registered, never patched.
type pluginPatch struct {
	Enabled      *bool   `json:"enabled"`
	Pinned       *bool   `json:"pinned"`
	Description  string  `json:"description"`
	RolloutState string  `json:"rollout_state"`
	SourceKind   *string `json:"source_kind"`
	SourceRef    *string `json:"source_ref"`
	SourcePin    *string `json:"source_pin"`
}

// enablesPlugin reports whether the patch turns the plugin on.
func (u pluginPatch) enablesPlugin() bool { return u.Enabled != nil && *u.Enabled }

// pinsPlugin reports whether the patch pins the plugin.
func (u pluginPatch) pinsPlugin() bool { return u.Pinned != nil && *u.Pinned }

// The typed refusals of the supply-chain pins. Each is written in the {error:{code,message,details}}
// envelope, so a client can tell them apart without reading the sentence.
const (
	pluginKilledCode          = "plugin_killed"
	pluginSourceRequiredCode  = "plugin_source_required"
	pluginSourceInvalidCode   = "plugin_source_invalid"
	pluginSourceImmutableCode = "plugin_source_immutable"
	pluginNotInstalledCode    = "plugin_not_installed"
	pluginManifestInvalidCode = "plugin_manifest_invalid"
	pluginChangedCode         = "plugin_changed"
)

// pluginLoaderEnv is the switch behind the supply-chain pins.
const pluginLoaderEnv = "PLUGIN_LOADER_ENABLED"

// PluginLoaderFromLookup reads PLUGIN_LOADER_ENABLED strictly: only the literal "true" turns the
// supply-chain pins on, so a typo or a stray "1" leaves plugin registration as inert CRUD.
func PluginLoaderFromLookup(lookup func(string) string) bool {
	return lookup(pluginLoaderEnv) == "true"
}

var (
	gitCommitPinPattern = regexp.MustCompile(`^[0-9a-f]{40}$`)
	archivePinPattern   = regexp.MustCompile(`^sha256:[0-9a-f]{64}$`)
)

// maxPluginSourceRefBytes bounds a source URL: the API stores it and the loader fetches it.
const maxPluginSourceRefBytes = 2048

// pluginSource is a registered source in canonical form: the kind, the URL and the pin.
type pluginSource struct {
	kind string
	ref  string
	pin  string
}

// pluginRefusal is a typed 4xx from the supply-chain pins.
type pluginRefusal struct {
	status  int
	code    string
	message string
}

// write sends the refusal in the typed error envelope.
func (f *pluginRefusal) write(w http.ResponseWriter) {
	writeTypedError(w, f.status, f.code, f.message)
}

// checkPluginSource validates the source a plugin registers with and returns it in canonical form.
// Only a full commit SHA (git_commit) or an archive digest (archive) pins a source, so a branch or a
// tag is refused. The URL must be https with a host and no credentials, query or fragment, since it
// is stored and the loader fetches it. A nil refusal means the source is valid.
func checkPluginSource(kind, ref, pin string) (pluginSource, *pluginRefusal) {
	src := pluginSource{
		kind: strings.TrimSpace(kind),
		ref:  strings.TrimSpace(ref),
		pin:  strings.ToLower(strings.TrimSpace(pin)),
	}
	if src.kind == "" || src.ref == "" || src.pin == "" {
		return pluginSource{}, &pluginRefusal{http.StatusUnprocessableEntity, pluginSourceRequiredCode,
			"source_kind, source_ref and source_pin are required"}
	}
	pinOK := (src.kind == "git_commit" && gitCommitPinPattern.MatchString(src.pin)) ||
		(src.kind == "archive" && archivePinPattern.MatchString(src.pin))
	if !pinOK {
		return pluginSource{}, &pluginRefusal{http.StatusUnprocessableEntity, pluginSourceInvalidCode,
			"source_kind must be git_commit, pinned to a full 40-hex commit SHA, or archive, pinned to sha256:<64 hex>; a branch or a tag never pins a source"}
	}
	if !validPluginSourceRef(src.ref) {
		return pluginSource{}, &pluginRefusal{http.StatusUnprocessableEntity, pluginSourceInvalidCode,
			"source_ref must be an https URL with a host and no credentials, query or fragment"}
	}
	return src, nil
}

// validPluginSourceRef accepts an absolute https URL with a host and no userinfo, query or fragment,
// so a credential cannot be stored in the URL.
func validPluginSourceRef(raw string) bool {
	if len(raw) > maxPluginSourceRefBytes {
		return false
	}
	u, err := url.Parse(raw)
	if err != nil {
		return false
	}
	return u.Scheme == "https" && u.Host != "" && u.User == nil &&
		u.RawQuery == "" && !u.ForceQuery && u.Fragment == ""
}

// pluginManifestDigest is `sha256:<hex>` over the canonical form of a manifest document: object keys
// sorted at every depth, no insignificant whitespace, numbers kept as written, and no HTML escaping.
// Two writers that spell the same document differently get the same digest, and the loader must
// canonicalise a fetched manifest the same way before it compares. It reports false for a manifest
// that is not a JSON object.
func pluginManifestDigest(manifest json.RawMessage) (string, bool) {
	dec := json.NewDecoder(bytes.NewReader(manifest))
	dec.UseNumber()
	var doc any
	if err := dec.Decode(&doc); err != nil {
		return "", false
	}
	if _, ok := doc.(map[string]any); !ok {
		return "", false
	}
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(doc); err != nil {
		return "", false
	}
	return fmt.Sprintf("sha256:%x", sha256.Sum256(bytes.TrimSuffix(buf.Bytes(), []byte("\n")))), true
}

// derefString returns the value behind p, or "" when p is nil.
func derefString(p *string) string {
	if p == nil {
		return ""
	}
	return *p
}

func (h *PluginsHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID := verifiedOrganizationID(r)
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, version, description, manifest_json,
			       risk_level, enabled, pinned, rollout_state, created_at, updated_at,
			       source_kind, source_ref, source_pin, manifest_digest, installed_at
			FROM plugin_packages
			WHERE (org_id = $1 OR org_id = 'global') AND deleted_at IS NULL
			ORDER BY name, version
		`, orgID)
		if err != nil {
			slog.Error("list plugins failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		plugins := []pluginRow{}
		for rows.Next() {
			var p pluginRow
			if err := rows.Scan(&p.ID, &p.OrgID, &p.Name, &p.Version, &p.Description,
				&p.ManifestJSON, &p.RiskLevel, &p.Enabled, &p.Pinned, &p.RolloutState,
				&p.CreatedAt, &p.UpdatedAt,
				&p.SourceKind, &p.SourceRef, &p.SourcePin, &p.ManifestDigest, &p.InstalledAt); err != nil {
				slog.Error("scan plugin failed", "error", err)
				jsonErr(w, "database unavailable", http.StatusInternalServerError)
				return
			}
			plugins = append(plugins, p)
		}
		writeJSON(w, map[string]any{"plugins": plugins})
	case http.MethodPost:
		var p pluginRow
		if err := json.NewDecoder(r.Body).Decode(&p); err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		if p.Name == "" || p.Version == "" {
			jsonErr(w, "name and version are required", http.StatusBadRequest)
			return
		}
		if len(p.ManifestJSON) == 0 {
			p.ManifestJSON = json.RawMessage("{}")
		}
		if p.RiskLevel == "" {
			p.RiskLevel = "high"
		}
		if p.RolloutState == "" {
			p.RolloutState = "canary"
		}
		p.OrgID = verifiedOrganizationID(r)
		p.ID = "plugin_" + uuid.New().String()
		now := time.Now().UTC()
		if h.pins {
			h.createPinned(w, r, p, now)
			return
		}
		// New plugins default DISABLED (safe rollout): a plugin is inert until an
		// admin explicitly enables (and a host loads) it.
		_, err := h.pool.Exec(r.Context(), `
			INSERT INTO plugin_packages (id, org_id, name, version, description,
			    manifest_json, risk_level, enabled, pinned, rollout_state,
			    created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
		`, p.ID, p.OrgID, p.Name, p.Version, p.Description, p.ManifestJSON,
			p.RiskLevel, false, false, p.RolloutState, now, now)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindPlugin,
			reconcile.ActionRegistered, p.ID, p.OrgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", reconcile.KindPlugin, "id", p.ID, "error", eerr)
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, map[string]any{"id": p.ID})
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *PluginsHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	var p pluginRow
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, name, version, description, manifest_json,
		       risk_level, enabled, pinned, rollout_state, created_at, updated_at,
		       source_kind, source_ref, source_pin, manifest_digest, installed_at
		FROM plugin_packages WHERE id = $1 AND org_id = $2 AND deleted_at IS NULL
	`, id, verifiedOrganizationID(r)).Scan(&p.ID, &p.OrgID, &p.Name, &p.Version,
		&p.Description, &p.ManifestJSON, &p.RiskLevel, &p.Enabled, &p.Pinned,
		&p.RolloutState, &p.CreatedAt, &p.UpdatedAt,
		&p.SourceKind, &p.SourceRef, &p.SourcePin, &p.ManifestDigest, &p.InstalledAt)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, p)
}

func (h *PluginsHandler) update(w http.ResponseWriter, r *http.Request, id string) {
	var update pluginPatch
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	if h.pins && (update.SourceKind != nil || update.SourceRef != nil || update.SourcePin != nil) {
		writeTypedError(w, http.StatusConflict, pluginSourceImmutableCode,
			"a plugin's source pin is set when it is registered; register a new version to pin a different source")
		return
	}
	if update.Enabled == nil && update.Pinned == nil && update.Description == "" && update.RolloutState == "" {
		jsonErr(w, "at least one plugin field is required", http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	orgID := verifiedOrganizationID(r)
	var enabled any
	if update.Enabled != nil {
		enabled = *update.Enabled
	}
	var pinned any
	if update.Pinned != nil {
		pinned = *update.Pinned
	}
	if h.pins {
		h.updatePinned(w, r, id, update, now)
		return
	}
	result, err := h.pool.Exec(r.Context(), `
		UPDATE plugin_packages
		SET enabled=COALESCE($1, enabled), pinned=COALESCE($2, pinned),
			description=COALESCE(NULLIF($3, ''), description),
			rollout_state=COALESCE(NULLIF($4, ''), rollout_state), updated_at=$5
		WHERE id=$6 AND org_id=$7 AND deleted_at IS NULL
	`, enabled, pinned, update.Description, update.RolloutState, now, id, orgID)
	if !writeSingleScopedMutation(w, "plugin package", result, err) {
		return
	}
	if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindPlugin,
		reconcile.ActionUpdated, id, orgID); eerr != nil {
		slog.Warn("reconcile emit failed", "kind", reconcile.KindPlugin, "id", id, "error", eerr)
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

func (h *PluginsHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	orgID := verifiedOrganizationID(r)
	now := time.Now().UTC()
	result, err := h.pool.Exec(r.Context(),
		`UPDATE plugin_packages SET deleted_at=$1, updated_at=$1 WHERE id=$2 AND org_id=$3 AND deleted_at IS NULL`,
		now, id, orgID)
	if !writeSingleScopedMutation(w, "plugin package", result, err) {
		return
	}
	if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindPlugin,
		reconcile.ActionRemoved, id, orgID); eerr != nil {
		slog.Warn("reconcile emit failed", "kind", reconcile.KindPlugin, "id", id, "error", eerr)
	}
	w.WriteHeader(http.StatusNoContent)
}

// createPinned is the POST path under PLUGIN_LOADER_ENABLED. The source pin is checked before anything
// is written and the manifest digest is computed here, never taken from the caller. The insert itself
// refuses a killed digest, so the check and the write cannot race a kill-list entry. The plugin starts
// disabled and unpinned, as it always has; enabling it needs a pin and an install.
func (h *PluginsHandler) createPinned(w http.ResponseWriter, r *http.Request, p pluginRow, now time.Time) {
	src, refusal := checkPluginSource(derefString(p.SourceKind), derefString(p.SourceRef), derefString(p.SourcePin))
	if refusal != nil {
		refusal.write(w)
		return
	}
	digest, ok := pluginManifestDigest(p.ManifestJSON)
	if !ok {
		writeTypedError(w, http.StatusUnprocessableEntity, pluginManifestInvalidCode,
			"manifest_json must be a JSON object")
		return
	}
	result, err := h.pool.Exec(r.Context(), `
		INSERT INTO plugin_packages (id, org_id, name, version, description,
		    manifest_json, risk_level, enabled, pinned, rollout_state,
		    source_kind, source_ref, source_pin, manifest_digest,
		    created_at, updated_at)
		SELECT $1::text, $2::text, $3::text, $4::text, $5::text,
		    $6::jsonb, $7::text, FALSE, FALSE, $8::text,
		    $9::text, $10::text, $11::text, $12::text,
		    $13::timestamptz, $13::timestamptz
		WHERE NOT EXISTS (
		    SELECT 1 FROM plugin_kill_list
		    WHERE digest = $12::text AND org_id IN ('global', $2::text)
		)
	`, p.ID, p.OrgID, p.Name, p.Version, p.Description, p.ManifestJSON,
		p.RiskLevel, p.RolloutState, src.kind, src.ref, src.pin, digest, now)
	if err != nil {
		slog.Error("register plugin failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	if result.RowsAffected() == 0 {
		writeTypedError(w, http.StatusUnprocessableEntity, pluginKilledCode,
			"this plugin's manifest digest is on the kill list, so it cannot be registered")
		return
	}
	if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindPlugin,
		reconcile.ActionRegistered, p.ID, p.OrgID); eerr != nil {
		slog.Warn("reconcile emit failed", "kind", reconcile.KindPlugin, "id", p.ID, "error", eerr)
	}
	w.WriteHeader(http.StatusCreated)
	writeJSON(w, map[string]any{"id": p.ID})
}

// updatePinned is the PATCH path under PLUGIN_LOADER_ENABLED. Pinning needs a registered source, and
// enabling needs a pinned, installed plugin whose digest is not killed. Those rules sit in the UPDATE's
// WHERE clause, so a plugin that changes between a read and this write is refused rather than enabled.
// Disabling and unpinning always go through, so a killed plugin can still be turned off, and unpinning
// also disables, so an enabled plugin is never left unpinned.
func (h *PluginsHandler) updatePinned(w http.ResponseWriter, r *http.Request, id string, update pluginPatch, now time.Time) {
	orgID := verifiedOrganizationID(r)
	var enabled, pinned any
	if update.Enabled != nil {
		enabled = *update.Enabled
	}
	if update.Pinned != nil {
		pinned = *update.Pinned
	}
	enableReq, pinReq := update.enablesPlugin(), update.pinsPlugin()
	result, err := h.pool.Exec(r.Context(), `
		UPDATE plugin_packages
		SET enabled = CASE WHEN $2::boolean IS FALSE THEN FALSE ELSE COALESCE($1::boolean, enabled) END,
		    pinned = COALESCE($2::boolean, pinned),
		    description = COALESCE(NULLIF($3, ''), description),
		    rollout_state = COALESCE(NULLIF($4, ''), rollout_state),
		    updated_at = $5
		WHERE id = $6 AND org_id = $7 AND deleted_at IS NULL
		  AND ((NOT $8::boolean) OR (COALESCE($2::boolean, pinned) AND source_kind IS NOT NULL
		       AND source_ref IS NOT NULL AND source_pin IS NOT NULL AND installed_at IS NOT NULL))
		  AND ((NOT $9::boolean) OR (source_kind IS NOT NULL AND source_ref IS NOT NULL AND source_pin IS NOT NULL))
		  AND ((NOT $10::boolean) OR NOT EXISTS (
		      SELECT 1 FROM plugin_kill_list k
		      WHERE k.digest = plugin_packages.manifest_digest
		        AND k.org_id IN ('global', plugin_packages.org_id)))
	`, enabled, pinned, update.Description, update.RolloutState, now, id, orgID,
		enableReq, pinReq, enableReq || pinReq)
	if err != nil {
		slog.Error("update plugin failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	if result.RowsAffected() != 1 {
		h.refuseUpdate(w, r, id, orgID, update)
		return
	}
	if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindPlugin,
		reconcile.ActionUpdated, id, orgID); eerr != nil {
		slog.Warn("reconcile emit failed", "kind", reconcile.KindPlugin, "id", id, "error", eerr)
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now})
}

// refuseUpdate says why a guarded UPDATE changed nothing. It reads the row once, so each refusal is
// typed: a missing plugin is 404, a killed digest is 422 plugin_killed when the patch pins or enables,
// a pin with no registered source is 422 plugin_source_required, and an enable without a pin and an
// install is 409 plugin_not_installed. A row that changed between the UPDATE and this read is 409.
func (h *PluginsHandler) refuseUpdate(w http.ResponseWriter, r *http.Request, id, orgID string, update pluginPatch) {
	var pinned, hasSource, installed, killed bool
	err := h.pool.QueryRow(r.Context(), `
		SELECT p.pinned,
		       p.source_kind IS NOT NULL AND p.source_ref IS NOT NULL AND p.source_pin IS NOT NULL,
		       p.installed_at IS NOT NULL,
		       EXISTS (SELECT 1 FROM plugin_kill_list k
		               WHERE k.digest = p.manifest_digest AND k.org_id IN ('global', p.org_id))
		FROM plugin_packages p
		WHERE p.id = $1 AND p.org_id = $2 AND p.deleted_at IS NULL
	`, id, orgID).Scan(&pinned, &hasSource, &installed, &killed)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	if err != nil {
		slog.Error("plugin refusal lookup failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	pinAfter := pinned
	if update.Pinned != nil {
		pinAfter = *update.Pinned
	}
	switch {
	case (update.enablesPlugin() || update.pinsPlugin()) && killed:
		writeTypedError(w, http.StatusUnprocessableEntity, pluginKilledCode,
			"this plugin's manifest digest is on the kill list, so it cannot be pinned or enabled")
	case update.pinsPlugin() && !hasSource:
		writeTypedError(w, http.StatusUnprocessableEntity, pluginSourceRequiredCode,
			"a plugin needs a registered source (source_kind, source_ref and source_pin) before it can be pinned")
	case update.enablesPlugin() && !(pinAfter && hasSource && installed):
		writeTypedError(w, http.StatusConflict, pluginNotInstalledCode,
			"a plugin must be pinned and installed before it can be enabled")
	default:
		writeTypedError(w, http.StatusConflict, pluginChangedCode,
			"the plugin changed while it was being updated; try again")
	}
}

// ---------------------------------------------------------------------------
// Plugin loader  (W4-PL-2, behind PLUGIN_LOADER_ENABLED)
// ---------------------------------------------------------------------------

// maxPluginInstallBodyBytes bounds the install body: a list of at most a few hundred 71-byte digests.
const maxPluginInstallBodyBytes = 64 << 10

// pluginInstallBody is the body of POST /api/v1/plugins/{id}/install. The digests must be exactly the
// ones GET /api/v1/plugins/{id}/components listed for the installable components.
type pluginInstallBody struct {
	ConsentedDigests []string `json:"consented_digests"`
}

// pluginLoadSQL reads the registered plugin the loader works on. Parameters: $1 id, $2 org.
const pluginLoadSQL = `
	SELECT id, org_id, name, version, manifest_json, COALESCE(manifest_digest, ''), installed_at IS NOT NULL
	FROM plugin_packages
	WHERE id = $1 AND org_id = $2 AND deleted_at IS NULL`

// loadPlugin reads the plugin for the loader. With lock it takes the row lock, so two installs of one
// plugin cannot interleave.
func loadPlugin(r *http.Request, db registryDatabase, id, orgID string, lock bool) (pluginloader.Plugin, error) {
	query := pluginLoadSQL
	if lock {
		query += " FOR UPDATE"
	}
	var plugin pluginloader.Plugin
	err := db.QueryRow(r.Context(), query, id, orgID).Scan(&plugin.ID, &plugin.OrgID, &plugin.Name,
		&plugin.Version, &plugin.Manifest, &plugin.ManifestDigest, &plugin.Installed)
	return plugin, err
}

// pluginLoaderCaller builds the loader's view of the verified principal. Nothing in the request body
// or headers decides who the caller is or whether it is an admin.
func pluginLoaderCaller(r *http.Request) (pluginloader.Caller, bool) {
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		return pluginloader.Caller{}, false
	}
	return pluginloader.Caller{
		ID:                   principal.ActorID,
		IsPerson:             principal.PrincipalType == "user",
		IsAdmin:              verifiedIsAdmin(r),
		ZeroDataRetention:    principal.ZeroDataRetention,
		RetentionPolicyKnown: principal.RetentionPolicyPresent,
	}, true
}

// pluginLoaderStatus maps a loader refusal to its HTTP status.
var pluginLoaderStatus = map[string]int{
	pluginloader.CodeNotAdmin:          http.StatusForbidden,
	pluginloader.CodeZDRRefused:        http.StatusForbidden,
	pluginloader.CodeKilled:            http.StatusUnprocessableEntity,
	pluginloader.CodeNotPinned:         http.StatusUnprocessableEntity,
	pluginloader.CodeManifestInvalid:   http.StatusUnprocessableEntity,
	pluginloader.CodeConsentMismatch:   http.StatusConflict,
	pluginloader.CodeAlreadyInstalled:  http.StatusConflict,
	pluginloader.CodeNotInstalled:      http.StatusConflict,
	pluginloader.CodeComponentConflict: http.StatusConflict,
}

// writePluginLoaderError answers a loader refusal in the typed error envelope, with the refusal's
// details (digests, names, never content). Anything that is not a typed refusal is a 500.
func writePluginLoaderError(w http.ResponseWriter, err error) {
	var refusal *pluginloader.Error
	if !errors.As(err, &refusal) {
		slog.Error("plugin loader failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	status, ok := pluginLoaderStatus[refusal.Code]
	if !ok {
		status = http.StatusUnprocessableEntity
	}
	details := refusal.Details
	if details == nil {
		details = map[string]any{}
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]any{"error": map[string]any{
		"code": refusal.Code, "message": refusal.Message, "details": details,
	}})
}

// pluginDigestChanged reports whether the stored manifest no longer hashes to the digest registered with
// it. A plugin with no digest is left to the loader, which refuses it as not pinned.
func pluginDigestChanged(plugin pluginloader.Plugin) bool {
	if plugin.ManifestDigest == "" {
		return false
	}
	digest, ok := pluginManifestDigest(plugin.Manifest)
	return !ok || digest != plugin.ManifestDigest
}

// pluginComponentRow is a stored plugin_components row as the components route lists it.
type pluginComponentRow struct {
	Kind     string `json:"kind"`
	Name     string `json:"name"`
	ChildRef string `json:"child_ref,omitempty"`
	Status   string `json:"status"`
	Report   string `json:"report,omitempty"`
}

// components is GET /api/v1/plugins/{id}/components: the report an admin reads before installing. It
// lists every component the manifest declares, the digest of each, and whether the loader would create
// it or refuses it and why; consent_digests is the exact list POST install must send back. It writes
// nothing and returns no skill body, template or URL.
func (h *PluginsHandler) components(w http.ResponseWriter, r *http.Request) {
	caller, ok := pluginLoaderCaller(r)
	if !ok {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	if !caller.IsPerson || !caller.IsAdmin {
		writePluginLoaderError(w, &pluginloader.Error{Code: pluginloader.CodeNotAdmin,
			Message: "only an org admin acting as a person may read a plugin's install report"})
		return
	}
	plugin, err := loadPlugin(r, h.pool, r.PathValue("id"), verifiedOrganizationID(r), false)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	if err != nil {
		slog.Error("load plugin failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	manifest, perr := pluginloader.ParseManifest(plugin.Manifest)
	if perr != nil {
		writePluginLoaderError(w, perr)
		return
	}
	if manifest.Name != plugin.Name || manifest.Version != plugin.Version || pluginDigestChanged(plugin) {
		writeTypedError(w, http.StatusConflict, pluginChangedCode,
			"the stored manifest does not match the registered plugin; register the plugin again")
		return
	}
	report := h.loader.Plan(r.Context(), manifest)
	consent := []string{}
	for _, component := range report {
		if component.Installable() {
			consent = append(consent, component.Digest)
		}
	}
	sort.Strings(consent)
	stored, err := h.storedPluginComponents(r, plugin.ID)
	if err != nil {
		slog.Error("list plugin components failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{
		"plugin_id": plugin.ID, "name": plugin.Name, "version": plugin.Version,
		"manifest_digest": plugin.ManifestDigest, "installed": plugin.Installed,
		"components": report, "consent_digests": consent, "stored": stored,
	})
}

// storedPluginComponents lists what the loader recorded for a plugin.
func (h *PluginsHandler) storedPluginComponents(r *http.Request, pluginID string) ([]pluginComponentRow, error) {
	rows, err := h.pool.Query(r.Context(), `
		SELECT kind, name, COALESCE(child_ref, ''), status, report
		FROM plugin_components WHERE plugin_id = $1 ORDER BY kind, name
	`, pluginID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	stored := []pluginComponentRow{}
	for rows.Next() {
		var row pluginComponentRow
		if err := rows.Scan(&row.Kind, &row.Name, &row.ChildRef, &row.Status, &row.Report); err != nil {
			return nil, err
		}
		stored = append(stored, row)
	}
	return stored, rows.Err()
}

// runPluginLoader runs one loader operation in a transaction on the plugin's locked row and commits it
// only when the operation succeeds. done is false when the response has already been written.
func (h *PluginsHandler) runPluginLoader(w http.ResponseWriter, r *http.Request, requireDigest bool,
	run func(tx registryDatabase, plugin pluginloader.Plugin) (pluginloader.Result, error)) (plugin pluginloader.Plugin, result pluginloader.Result, done bool) {
	beginner, ok := h.pool.(skillTxBeginner)
	if !ok {
		jsonErr(w, "plugin loader needs a transactional database", http.StatusServiceUnavailable)
		return plugin, result, false
	}
	ctx := r.Context()
	tx, err := beginner.Begin(ctx)
	if err != nil {
		slog.Error("begin plugin transaction failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return plugin, result, false
	}
	defer func() { _ = tx.Rollback(ctx) }()
	plugin, err = loadPlugin(r, tx, r.PathValue("id"), verifiedOrganizationID(r), true)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, "not found", http.StatusNotFound)
		return plugin, result, false
	}
	if err != nil {
		slog.Error("load plugin failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return plugin, result, false
	}
	if requireDigest && pluginDigestChanged(plugin) {
		writeTypedError(w, http.StatusConflict, pluginChangedCode,
			"the stored manifest does not match the registered plugin; register the plugin again")
		return plugin, result, false
	}
	result, err = run(tx, plugin)
	if err != nil {
		writePluginLoaderError(w, err)
		return plugin, result, false
	}
	if err := tx.Commit(ctx); err != nil {
		slog.Error("commit plugin transaction failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return plugin, result, false
	}
	return plugin, result, true
}

// install is POST /api/v1/plugins/{id}/install. The body carries the digests the admin consented to
// (from GET components); the install proceeds only if they are exactly the installable components, and
// then writes every child, the plugin_components rows and the audit rows in one transaction.
func (h *PluginsHandler) install(w http.ResponseWriter, r *http.Request) {
	caller, ok := pluginLoaderCaller(r)
	if !ok {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, maxPluginInstallBodyBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	var body pluginInstallBody
	if err := decoder.Decode(&body); err != nil || ensureJSONEOF(decoder) != nil {
		jsonErr(w, "invalid request body", http.StatusBadRequest)
		return
	}
	plugin, result, done := h.runPluginLoader(w, r, true, func(tx registryDatabase, plugin pluginloader.Plugin) (pluginloader.Result, error) {
		return h.loader.Install(r.Context(), tx, plugin, caller, body.ConsentedDigests)
	})
	if !done {
		return
	}
	h.emitPluginChildren(r, plugin, result, reconcile.ActionRegistered)
	writeJSON(w, map[string]any{
		"plugin_id": plugin.ID, "installed": true, "components": result.Components, "children": result.Children,
	})
}

// uninstall is POST /api/v1/plugins/{id}/uninstall: it removes every child the install created, marks
// the components removed and takes the plugin out of service (disabled, not installed).
func (h *PluginsHandler) uninstall(w http.ResponseWriter, r *http.Request) {
	caller, ok := pluginLoaderCaller(r)
	if !ok {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	plugin, result, done := h.runPluginLoader(w, r, false, func(tx registryDatabase, plugin pluginloader.Plugin) (pluginloader.Result, error) {
		return h.loader.Uninstall(r.Context(), tx, plugin, caller, pluginloader.ReasonUninstall)
	})
	if !done {
		return
	}
	h.emitPluginChildren(r, plugin, result, reconcile.ActionRemoved)
	writeJSON(w, map[string]any{
		"plugin_id": plugin.ID, "installed": false, "components": result.Components, "children": result.Children,
	})
}

// emitPluginChildren emits the reconcile events of a committed install or uninstall: one for the plugin
// and one per child, so the gateway's caches refresh. Best-effort, like every other registry mutation.
func (h *PluginsHandler) emitPluginChildren(r *http.Request, plugin pluginloader.Plugin, result pluginloader.Result, childAction string) {
	emit := func(kind, action, id string) {
		if eerr := reconcile.Emit(r.Context(), h.pub, kind, action, id, plugin.OrgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", kind, "id", id, "error", eerr)
		}
	}
	emit(reconcile.KindPlugin, reconcile.ActionUpdated, plugin.ID)
	for _, child := range result.Children {
		if child.ID == "" {
			continue
		}
		if child.Kind == pluginloader.KindMCPServer {
			emit(reconcile.KindMCPServer, childAction, child.ID)
			continue
		}
		emit(reconcile.KindSkill, childAction, child.ID)
	}
}
