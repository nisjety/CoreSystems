// Package api — the org-curated plugin catalogue and the plugin kill list (F-t3-059, unit W4-PL-3).
//
// capability-core owns both. An org admin lists a plugin package in the org catalogue once the package is
// pinned to a source. The catalogue shows only the org's approved packages that are still pinned and not
// killed, with their risk level and a summary of their components. The kill list refuses a manifest digest
// for one org, or for every org (scope global). A kill also uninstalls every installed copy of that digest
// through the loader and clears the catalogue approval of the rows it covers, all in one transaction.
//
// Who may write: an org admin acting as a person, with the write scope and an explicit non-ZDR posture, for
// the catalogue and for an org-scope kill. A global kill needs a platform service principal: the service
// type, the global org and capability:global:write, the scope reserved for process-wide registries. Reads of
// the catalogue are open to any member of the org, ZDR included. The kill list is read by an org admin. Off
// (PLUGIN_LOADER_ENABLED is not the literal true), nothing here is mounted and the paths answer 404.
package api

import (
	"encoding/json"
	"errors"
	"log/slog"
	"net/http"
	"regexp"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/pkg/publisher"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	pluginloader "github.com/triodelab/model-plane/services/capability-core/internal/plugins"
	"github.com/triodelab/model-plane/services/capability-core/internal/reconcile"
)

const (
	// pluginCatalogPath is the catalogue route; a package's approve and unapprove routes hang off it.
	pluginCatalogPath = "/api/v1/plugin-catalog"
	// pluginKillListPath is the kill-list route.
	pluginKillListPath = "/api/v1/plugin-kill-list"

	// pluginGlobalOrg is the platform's org id: the scope of a global kill and the org of a platform principal.
	pluginGlobalOrg = "global"
	// pluginKillScopeOrg is an org kill, which covers the caller's own org. It is the default scope.
	pluginKillScopeOrg = "org"
	// pluginKillScopeGlobal is a global kill, which covers every org.
	pluginKillScopeGlobal = "global"

	// maxPluginKillBodyBytes bounds the kill-list body: a digest, a scope and a short reason.
	maxPluginKillBodyBytes = 8 << 10
	// maxPluginKillReasonBytes bounds the reason an entry records.
	maxPluginKillReasonBytes = 500
)

// The typed refusals of the catalogue and the kill list. Each is written in the {error:{code,message,details}}
// envelope. The pins' own refusals (plugin_killed, plugin_source_required, plugin_changed) are reused as they are.
const (
	// pluginWriteZDRCode is a catalogue or kill-list write under zero data retention or an unstated posture.
	pluginWriteZDRCode = "plugin_write_under_zdr_refused"
	// pluginWriteScopeCode is a write by a caller without the capability write scope.
	pluginWriteScopeCode = "plugin_write_scope_required"
	// pluginKillGlobalCode is a global kill by a caller other than a platform service principal.
	pluginKillGlobalCode = "plugin_kill_global_refused"
	// pluginKillScopeCode is a kill-list scope that is neither org nor global.
	pluginKillScopeCode = "plugin_kill_scope_invalid"
	// pluginKillReasonCode is a kill-list reason that is not valid text of the allowed length.
	pluginKillReasonCode = "plugin_kill_reason_invalid"
	// pluginDigestInvalidCode is a manifest digest that is not sha256 followed by 64 lowercase hex digits.
	pluginDigestInvalidCode = "plugin_digest_invalid"
)

// killDigestPattern is a manifest digest as capability-core writes it: sha256 and 64 lowercase hex digits.
var killDigestPattern = regexp.MustCompile(`^sha256:[0-9a-f]{64}$`)

// errKillTransactionUnavailable is what a kill answers when the database cannot run a transaction.
var errKillTransactionUnavailable = errors.New("the plugin kill list needs a transactional database")

// PluginCatalogHandler serves /api/v1/plugin-catalog (the org catalogue and its approvals) and
// /api/v1/plugin-kill-list (the kill list). It is mounted only while PLUGIN_LOADER_ENABLED is the literal
// true, and only with the child stores a kill needs to uninstall a plugin's children.
type PluginCatalogHandler struct {
	pool registryDatabase
	pub  publisher.EventPublisher
	// on turns the routes on, behind PLUGIN_LOADER_ENABLED (see WithLoaderEnabled).
	on bool
	// loader removes the children of a killed plugin (see WithChildStores). Nothing is mounted without it.
	loader *pluginloader.Loader
}

// NewPluginCatalogHandler constructs the handler.
func NewPluginCatalogHandler(pool registryDatabase) *PluginCatalogHandler {
	return &PluginCatalogHandler{pool: pool}
}

// WithPublisher wires reconcile-event emission. Optional, nil-safe.
func (h *PluginCatalogHandler) WithPublisher(pub publisher.EventPublisher) *PluginCatalogHandler {
	h.pub = pub
	return h
}

// WithLoaderEnabled turns the catalogue and kill-list routes on or off. It takes the value PluginLoaderFromLookup
// reads from PLUGIN_LOADER_ENABLED, so only the literal true mounts them.
func (h *PluginCatalogHandler) WithLoaderEnabled(on bool) *PluginCatalogHandler {
	h.on = on
	return h
}

// WithChildStores wires the loader a kill uses to remove a killed plugin's children. It takes the same child
// stores as the plugin routes, so a kill removes what an install created through the same stores.
func (h *PluginCatalogHandler) WithChildStores(skills *SkillsHandler, mcp *MCPHandler) *PluginCatalogHandler {
	h.loader = &pluginloader.Loader{Children: newPluginChildStore(skills, mcp)}
	return h
}

// Register mounts the routes. Off, or without the child stores, it mounts nothing and the paths answer 404.
func (h *PluginCatalogHandler) Register(mux *http.ServeMux) {
	if !h.on || h.loader == nil {
		return
	}
	mux.HandleFunc("GET "+pluginCatalogPath, h.listCatalogue)
	mux.HandleFunc("POST "+pluginCatalogPath+"/{id}/approve", h.approve)
	mux.HandleFunc("POST "+pluginCatalogPath+"/{id}/unapprove", h.unapprove)
	mux.HandleFunc("GET "+pluginKillListPath, h.listKills)
	mux.HandleFunc("POST "+pluginKillListPath, h.createKill)
	mux.HandleFunc("DELETE "+pluginKillListPath, h.deleteKill)
}

// pluginCatalogEntry is one package the catalogue lists: its pins and state, its risk level and a summary of its
// components. It never carries a manifest, a skill body, a template or a child's URL.
type pluginCatalogEntry struct {
	ID             string                   `json:"id"`
	Name           string                   `json:"name"`
	Version        string                   `json:"version"`
	Description    string                   `json:"description"`
	RiskLevel      string                   `json:"risk_level"`
	Enabled        bool                     `json:"enabled"`
	RolloutState   string                   `json:"rollout_state"`
	SourceKind     string                   `json:"source_kind"`
	SourceRef      string                   `json:"source_ref"`
	SourcePin      string                   `json:"source_pin"`
	ManifestDigest string                   `json:"manifest_digest"`
	Installed      bool                     `json:"installed"`
	Components     []pluginCatalogComponent `json:"components"`
}

// pluginCatalogComponent is one component of a listed package, as the loader recorded it.
type pluginCatalogComponent struct {
	Kind   string `json:"kind"`
	Name   string `json:"name"`
	Status string `json:"status"`
}

// pluginCatalogSQL reads the org's catalogue. Parameter $1 is the org. A package is listed when it belongs to the
// org, is approved, is pinned to a source with its digest, and its digest is not on the kill list for the org or
// for every org.
const pluginCatalogSQL = `
	SELECT p.id, p.name, p.version, p.description, p.risk_level, p.enabled, p.rollout_state,
	       p.source_kind, p.source_ref, p.source_pin, p.manifest_digest, p.installed_at IS NOT NULL
	FROM plugin_packages p
	WHERE p.org_id = $1 AND p.deleted_at IS NULL AND p.catalog_approved AND p.pinned
	  AND p.source_kind IS NOT NULL AND p.source_ref IS NOT NULL AND p.source_pin IS NOT NULL
	  AND p.manifest_digest IS NOT NULL
	  AND NOT EXISTS (
	      SELECT 1 FROM plugin_kill_list k
	      WHERE k.digest = p.manifest_digest AND k.org_id IN ('global', p.org_id)
	  )
	ORDER BY p.name, p.version, p.id`

// pluginCatalogComponentsSQL summarises the components of the listed packages. Parameter $1 is their ids. A removed
// component is history and is not summarised.
const pluginCatalogComponentsSQL = `
	SELECT plugin_id, kind, name, status
	FROM plugin_components
	WHERE plugin_id = ANY($1::text[]) AND status <> 'removed'
	ORDER BY plugin_id, kind, name`

// listCatalogue is GET /api/v1/plugin-catalog. Any member of the org reads it, a ZDR member included.
func (h *PluginCatalogHandler) listCatalogue(w http.ResponseWriter, r *http.Request) {
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	entries, err := h.catalogue(r, principal.OrganizationID)
	if err != nil {
		slog.Error("list plugin catalogue failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"plugins": entries})
}

// catalogue reads the org's listed packages with the component summary of each.
func (h *PluginCatalogHandler) catalogue(r *http.Request, orgID string) ([]pluginCatalogEntry, error) {
	rows, err := h.pool.Query(r.Context(), pluginCatalogSQL, orgID)
	if err != nil {
		return nil, err
	}
	entries := []pluginCatalogEntry{}
	position := map[string]int{}
	for rows.Next() {
		var entry pluginCatalogEntry
		if err := rows.Scan(&entry.ID, &entry.Name, &entry.Version, &entry.Description, &entry.RiskLevel,
			&entry.Enabled, &entry.RolloutState, &entry.SourceKind, &entry.SourceRef, &entry.SourcePin,
			&entry.ManifestDigest, &entry.Installed); err != nil {
			rows.Close()
			return nil, err
		}
		entry.Components = []pluginCatalogComponent{}
		position[entry.ID] = len(entries)
		entries = append(entries, entry)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}
	if len(entries) == 0 {
		return entries, nil
	}
	ids := make([]string, 0, len(entries))
	for _, entry := range entries {
		ids = append(ids, entry.ID)
	}
	components, err := h.pool.Query(r.Context(), pluginCatalogComponentsSQL, ids)
	if err != nil {
		return nil, err
	}
	defer components.Close()
	for components.Next() {
		var pluginID string
		var component pluginCatalogComponent
		if err := components.Scan(&pluginID, &component.Kind, &component.Name, &component.Status); err != nil {
			return nil, err
		}
		if index, ok := position[pluginID]; ok {
			entries[index].Components = append(entries[index].Components, component)
		}
	}
	return entries, components.Err()
}

// approve is POST /api/v1/plugin-catalog/{id}/approve: an org admin lists one of the org's packages in the
// catalogue. The package must be pinned to a source, carry its digest and not be killed. The UPDATE's WHERE
// clause checks all three, so a kill that lands after a read is refused rather than approved.
func (h *PluginCatalogHandler) approve(w http.ResponseWriter, r *http.Request) {
	principal, ok := pluginWriter(w, r)
	if !ok || !pluginOrgAdmin(w, r, principal) {
		return
	}
	id := r.PathValue("id")
	now := time.Now().UTC()
	result, err := h.pool.Exec(r.Context(), `
		UPDATE plugin_packages SET catalog_approved = TRUE, updated_at = $1
		WHERE id = $2 AND org_id = $3 AND deleted_at IS NULL
		  AND pinned AND source_kind IS NOT NULL AND source_ref IS NOT NULL AND source_pin IS NOT NULL
		  AND manifest_digest IS NOT NULL
		  AND NOT EXISTS (
		      SELECT 1 FROM plugin_kill_list k
		      WHERE k.digest = plugin_packages.manifest_digest AND k.org_id IN ('global', plugin_packages.org_id)
		  )
	`, now, id, principal.OrganizationID)
	if err != nil {
		slog.Error("approve plugin for catalogue failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	if result.RowsAffected() != 1 {
		h.refuseCatalogueChange(w, r, id, principal.OrganizationID)
		return
	}
	h.emitPluginUpdated(r, id, principal.OrganizationID)
	writeJSON(w, map[string]any{"id": id, "catalog_approved": true, "updated_at": now})
}

// unapprove is POST /api/v1/plugin-catalog/{id}/unapprove: an org admin takes one of the org's packages out of the
// catalogue. Removing a listing is always allowed, so nothing else is checked.
func (h *PluginCatalogHandler) unapprove(w http.ResponseWriter, r *http.Request) {
	principal, ok := pluginWriter(w, r)
	if !ok || !pluginOrgAdmin(w, r, principal) {
		return
	}
	id := r.PathValue("id")
	now := time.Now().UTC()
	result, err := h.pool.Exec(r.Context(), `
		UPDATE plugin_packages SET catalog_approved = FALSE, updated_at = $1
		WHERE id = $2 AND org_id = $3 AND deleted_at IS NULL
	`, now, id, principal.OrganizationID)
	if !writeSingleScopedMutation(w, "plugin package", result, err) {
		return
	}
	h.emitPluginUpdated(r, id, principal.OrganizationID)
	writeJSON(w, map[string]any{"id": id, "catalog_approved": false, "updated_at": now})
}

// refuseCatalogueChange says why an approval changed nothing: 404 when the package is not the org's, 422
// plugin_killed when its digest is killed, 422 plugin_source_required when it is not pinned to a source, and
// 409 plugin_changed when it changed between the UPDATE and this read.
func (h *PluginCatalogHandler) refuseCatalogueChange(w http.ResponseWriter, r *http.Request, id, orgID string) {
	var pinned, sourced, killed bool
	err := h.pool.QueryRow(r.Context(), `
		SELECT p.pinned,
		       p.source_kind IS NOT NULL AND p.source_ref IS NOT NULL AND p.source_pin IS NOT NULL
		           AND p.manifest_digest IS NOT NULL,
		       EXISTS (SELECT 1 FROM plugin_kill_list k
		               WHERE k.digest = p.manifest_digest AND k.org_id IN ('global', p.org_id))
		FROM plugin_packages p
		WHERE p.id = $1 AND p.org_id = $2 AND p.deleted_at IS NULL
	`, id, orgID).Scan(&pinned, &sourced, &killed)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	if err != nil {
		slog.Error("plugin catalogue refusal lookup failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	switch {
	case killed:
		writeTypedError(w, http.StatusUnprocessableEntity, pluginKilledCode,
			"this plugin's manifest digest is on the kill list, so it cannot be approved for the catalogue")
	case !pinned || !sourced:
		writeTypedError(w, http.StatusUnprocessableEntity, pluginSourceRequiredCode,
			"a plugin needs a registered source pin before it can be approved for the catalogue")
	default:
		writeTypedError(w, http.StatusConflict, pluginChangedCode,
			"the plugin changed while it was being approved; try again")
	}
}

// pluginKillBody is the body of POST /api/v1/plugin-kill-list. Scope is org (the default) or global. The org is
// never in the body: an org kill covers the caller's own org.
type pluginKillBody struct {
	Digest string `json:"digest"`
	Scope  string `json:"scope"`
	Reason string `json:"reason"`
}

// pluginKillTarget is a validated kill-list entry: the digest in canonical form, its scope, and the org it covers
// ("global" for a global kill).
type pluginKillTarget struct {
	digest string
	scope  string
	orgID  string
}

// killTarget validates a digest and a scope, and checks that the caller may write that scope. It writes the refusal
// itself and reports false when the write must stop.
func killTarget(w http.ResponseWriter, r *http.Request, principal authctx.Principal, digest, scope string) (pluginKillTarget, bool) {
	digest = strings.ToLower(strings.TrimSpace(digest))
	if !killDigestPattern.MatchString(digest) {
		writeTypedError(w, http.StatusUnprocessableEntity, pluginDigestInvalidCode,
			"digest must be sha256 followed by 64 lowercase hex digits")
		return pluginKillTarget{}, false
	}
	switch strings.TrimSpace(scope) {
	case "", pluginKillScopeOrg:
		if !pluginOrgAdmin(w, r, principal) {
			return pluginKillTarget{}, false
		}
		return pluginKillTarget{digest: digest, scope: pluginKillScopeOrg, orgID: principal.OrganizationID}, true
	case pluginKillScopeGlobal:
		if !pluginPlatform(principal) {
			writeTypedError(w, http.StatusForbidden, pluginKillGlobalCode,
				"a global kill needs a platform service principal holding the global write scope")
			return pluginKillTarget{}, false
		}
		return pluginKillTarget{digest: digest, scope: pluginKillScopeGlobal, orgID: pluginGlobalOrg}, true
	default:
		writeTypedError(w, http.StatusUnprocessableEntity, pluginKillScopeCode, "scope must be org or global")
		return pluginKillTarget{}, false
	}
}

// pluginWriter answers the refusals every catalogue and kill-list write needs, before anything is read or written: a
// verified principal, an explicit non-ZDR posture (a ZDR or unstated posture writes nothing) and the write scope.
func pluginWriter(w http.ResponseWriter, r *http.Request) (authctx.Principal, bool) {
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return principal, false
	}
	if !principal.RetentionPolicyPresent || principal.ZeroDataRetention {
		writeTypedError(w, http.StatusForbidden, pluginWriteZDRCode,
			"the plugin catalogue and kill list are durable state; a zero-retention or unstated retention posture cannot change them")
		return principal, false
	}
	if !principal.HasScope(authz.WriteScope) {
		writeTypedError(w, http.StatusForbidden, pluginWriteScopeCode, "capability write scope required")
		return principal, false
	}
	return principal, true
}

// pluginOrgAdmin is the authority for a catalogue change and for an org-scope kill: an org admin acting as a person,
// in an org. The global org's rows belong to the platform, so no caller acts for it here.
func pluginOrgAdmin(w http.ResponseWriter, r *http.Request, principal authctx.Principal) bool {
	if principal.PrincipalType != "user" || !verifiedIsAdmin(r) || principal.OrganizationID == pluginGlobalOrg {
		writeTypedError(w, http.StatusForbidden, pluginloader.CodeNotAdmin,
			"only an org admin acting as a person may change the plugin catalogue or kill a plugin for the org")
		return false
	}
	return true
}

// pluginPlatform is the authority for a global kill: a platform service principal (the service type, in the global
// org) holding capability:global:write, the scope reserved for process-wide registries. A tenant never holds it.
func pluginPlatform(principal authctx.Principal) bool {
	return principal.PrincipalType == "service" && principal.OrganizationID == pluginGlobalOrg &&
		principal.HasScope(authz.GlobalWriteScope)
}

// pluginsOfDigestSQL locks the plugin rows of a digest in the kill's scope, in id order, so two kills of one digest
// cannot deadlock. Parameters: $1 the digest, $2 whether the scope is global, $3 the org. A row is locked whether or
// not it is installed or deleted, because a deleted row can still hold live children.
const pluginsOfDigestSQL = `
	SELECT id, org_id, name, version, manifest_json, COALESCE(manifest_digest, ''), installed_at IS NOT NULL
	FROM plugin_packages
	WHERE manifest_digest = $1::text AND ($2::boolean OR org_id = $3::text)
	ORDER BY id
	FOR UPDATE`

// killOutcome is what one kill did: whether its entry is new, the plugin rows it locked, and the plugins it uninstalled
// with the children each removal took.
type killOutcome struct {
	created bool
	locked  []pluginloader.Plugin
	removed []pluginKillRemoval
}

// pluginKillRemoval is one installed plugin a kill uninstalled.
type pluginKillRemoval struct {
	plugin pluginloader.Plugin
	result pluginloader.Result
}

// pluginKillListSQL reads the kill entries that cover an org: its own and the platform's global ones. Parameter $1 is
// the org. Only the digest, the scope the row was written under, the reason POST stored and the time are read.
const pluginKillListSQL = `
	SELECT digest, org_id, reason, created_at
	FROM plugin_kill_list
	WHERE org_id IN ('global', $1::text)
	ORDER BY created_at DESC, digest, org_id`

// pluginKillEntry is one kill-list entry an org admin reads: the digest, the source that put it on the list (org, or
// global for a platform-wide kill), the reason POST stored and when the kill was written. It carries no actor.
type pluginKillEntry struct {
	Digest   string    `json:"digest"`
	Source   string    `json:"source"`
	Reason   string    `json:"reason"`
	KilledAt time.Time `json:"killed_at"`
}

// listKills is GET /api/v1/plugin-kill-list. An org admin acting as a person reads the kill entries that cover the
// org, its own and the platform's global ones, each with its source, so a global kill is shown as one the org cannot
// lift here. It reads no content and writes nothing, so a zero-retention admin may read it, as the catalogue is read.
func (h *PluginCatalogHandler) listKills(w http.ResponseWriter, r *http.Request) {
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	if !pluginOrgAdmin(w, r, principal) {
		return
	}
	rows, err := h.pool.Query(r.Context(), pluginKillListSQL, principal.OrganizationID)
	if err != nil {
		slog.Error("list plugin kill list failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	defer rows.Close()
	kills := []pluginKillEntry{}
	for rows.Next() {
		var entry pluginKillEntry
		var entryOrg string
		if err := rows.Scan(&entry.Digest, &entryOrg, &entry.Reason, &entry.KilledAt); err != nil {
			slog.Error("scan plugin kill list failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		entry.Source = pluginKillScopeOrg
		if entryOrg == pluginGlobalOrg {
			entry.Source = pluginKillScopeGlobal
		}
		entry.KilledAt = entry.KilledAt.UTC()
		kills = append(kills, entry)
	}
	if err := rows.Err(); err != nil {
		slog.Error("list plugin kill list failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"kills": kills})
}

// createKill is POST /api/v1/plugin-kill-list. It writes the entry, clears the catalogue approval of the digest in
// scope and uninstalls every installed copy of it, all in one transaction. A repeat of a kill keeps the entry and
// still removes any copy installed since: 201 when the entry is new, 200 when it already existed.
func (h *PluginCatalogHandler) createKill(w http.ResponseWriter, r *http.Request) {
	principal, ok := pluginWriter(w, r)
	if !ok {
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, maxPluginKillBodyBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	var body pluginKillBody
	if err := decoder.Decode(&body); err != nil || ensureJSONEOF(decoder) != nil {
		jsonErr(w, "invalid request body", http.StatusBadRequest)
		return
	}
	reason := strings.TrimSpace(body.Reason)
	if len(reason) > maxPluginKillReasonBytes || !utf8.ValidString(reason) {
		writeTypedError(w, http.StatusUnprocessableEntity, pluginKillReasonCode,
			"reason must be valid text of at most 500 bytes")
		return
	}
	target, ok := killTarget(w, r, principal, body.Digest, body.Scope)
	if !ok {
		return
	}
	outcome, err := h.applyKill(r, principal, target, reason)
	if errors.Is(err, errKillTransactionUnavailable) {
		jsonErr(w, err.Error(), http.StatusServiceUnavailable)
		return
	}
	if err != nil {
		slog.Error("apply plugin kill failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	h.emitKill(r, outcome)
	uninstalled := []string{}
	children := 0
	for _, removal := range outcome.removed {
		uninstalled = append(uninstalled, removal.plugin.ID)
		children += len(removal.result.Children)
	}
	status := http.StatusOK
	if outcome.created {
		status = http.StatusCreated
	}
	writeJSONStatus(w, status, map[string]any{
		"digest": target.digest, "scope": target.scope, "org_id": target.orgID, "created": outcome.created,
		"uninstalled_plugins": uninstalled, "children_removed": children,
	})
}

// deleteKillSQL removes one kill-list entry and appends the audit row of the lift in the same statement, so the row is
// written only when an entry was removed. The row names the digest and the scope, never the reason. Parameters: $1 the
// digest, $2 the org the entry covers, $3 the audit id, $4 the acting principal, $5 the scope.
const deleteKillSQL = `
	WITH removed AS (
	    DELETE FROM plugin_kill_list
	    WHERE digest = $1::text AND org_id = $2::text
	    RETURNING digest, org_id
	)
	INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
	SELECT $3::text, 'plugin_kill', removed.digest, 'lifted', $4::text, removed.org_id,
	       jsonb_build_object('scope', $5::text)
	FROM removed`

// deleteKill is DELETE /api/v1/plugin-kill-list?digest=&scope=. It removes the entry for the scope and appends the audit
// row of the lift in the same statement, so a lift is audited exactly when it lands. What the kill removed stays
// removed: a plugin must be installed, and approved for the catalogue, again.
func (h *PluginCatalogHandler) deleteKill(w http.ResponseWriter, r *http.Request) {
	principal, ok := pluginWriter(w, r)
	if !ok {
		return
	}
	query := r.URL.Query()
	target, ok := killTarget(w, r, principal, query.Get("digest"), query.Get("scope"))
	if !ok {
		return
	}
	result, err := h.pool.Exec(r.Context(), deleteKillSQL,
		target.digest, target.orgID, "ral_"+uuid.NewString(), principal.ActorID, target.scope)
	if !writeSingleScopedMutation(w, "plugin kill entry", result, err) {
		return
	}
	writeJSON(w, map[string]any{"digest": target.digest, "scope": target.scope, "org_id": target.orgID, "removed": true})
}

// applyKill runs one kill in a transaction. It locks the digest's plugin rows, writes the entry, clears the catalogue
// approval of those rows and uninstalls each installed copy through the loader. Either all of it lands or none does.
func (h *PluginCatalogHandler) applyKill(r *http.Request, principal authctx.Principal, target pluginKillTarget, reason string) (killOutcome, error) {
	beginner, ok := h.pool.(skillTxBeginner)
	if !ok {
		return killOutcome{}, errKillTransactionUnavailable
	}
	ctx := r.Context()
	tx, err := beginner.Begin(ctx)
	if err != nil {
		return killOutcome{}, err
	}
	defer func() { _ = tx.Rollback(ctx) }()

	global := target.scope == pluginKillScopeGlobal
	rows, err := tx.Query(ctx, pluginsOfDigestSQL, target.digest, global, target.orgID)
	if err != nil {
		return killOutcome{}, err
	}
	var locked []pluginloader.Plugin
	for rows.Next() {
		var plugin pluginloader.Plugin
		if err := rows.Scan(&plugin.ID, &plugin.OrgID, &plugin.Name, &plugin.Version, &plugin.Manifest,
			&plugin.ManifestDigest, &plugin.Installed); err != nil {
			rows.Close()
			return killOutcome{}, err
		}
		locked = append(locked, plugin)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return killOutcome{}, err
	}

	now := time.Now().UTC()
	tag, err := tx.Exec(ctx, `
		INSERT INTO plugin_kill_list (digest, org_id, reason, created_by, created_at)
		VALUES ($1::text, $2::text, $3::text, $4::text, $5::timestamptz)
		ON CONFLICT (org_id, digest) DO NOTHING
	`, target.digest, target.orgID, reason, principal.ActorID, now)
	if err != nil {
		return killOutcome{}, err
	}
	if _, err := tx.Exec(ctx, `
		UPDATE plugin_packages SET catalog_approved = FALSE, updated_at = $1
		WHERE manifest_digest = $2::text AND ($3::boolean OR org_id = $4::text) AND catalog_approved
	`, now, target.digest, global, target.orgID); err != nil {
		return killOutcome{}, err
	}
	out := killOutcome{created: tag.RowsAffected() == 1, locked: locked}
	for _, plugin := range locked {
		if !plugin.Installed {
			continue
		}
		result, err := h.loader.UninstallKilled(ctx, tx, plugin, principal.ActorID)
		if err != nil {
			return killOutcome{}, err
		}
		out.removed = append(out.removed, pluginKillRemoval{plugin: plugin, result: result})
	}
	if err := tx.Commit(ctx); err != nil {
		return killOutcome{}, err
	}
	return out, nil
}

// emitKill emits the reconcile events of a committed kill: one per plugin row it locked and one per child it removed,
// so the gateway's caches refresh. Best-effort, like every other registry mutation.
func (h *PluginCatalogHandler) emitKill(r *http.Request, outcome killOutcome) {
	for _, plugin := range outcome.locked {
		h.emitPluginUpdated(r, plugin.ID, plugin.OrgID)
	}
	for _, removal := range outcome.removed {
		for _, child := range removal.result.Children {
			if child.ID == "" {
				continue
			}
			kind := reconcile.KindSkill
			if child.Kind == pluginloader.KindMCPServer {
				kind = reconcile.KindMCPServer
			}
			if eerr := reconcile.Emit(r.Context(), h.pub, kind, reconcile.ActionRemoved, child.ID, removal.plugin.OrgID); eerr != nil {
				slog.Warn("reconcile emit failed", "kind", kind, "id", child.ID, "error", eerr)
			}
		}
	}
}

// emitPluginUpdated emits the reconcile event of a plugin row that changed. Best-effort.
func (h *PluginCatalogHandler) emitPluginUpdated(r *http.Request, id, orgID string) {
	if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindPlugin, reconcile.ActionUpdated, id, orgID); eerr != nil {
		slog.Warn("reconcile emit failed", "kind", reconcile.KindPlugin, "id", id, "error", eerr)
	}
}

// writeJSONStatus writes a JSON answer with an explicit status code.
func writeJSONStatus(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}
