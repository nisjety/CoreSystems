package api

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"log/slog"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/pkg/publisher"
	"github.com/triodelab/model-plane/services/capability-core/internal/reconcile"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillbundle"
	"github.com/triodelab/model-plane/services/capability-core/internal/skilllint"
)

// ---------------------------------------------------------------------------
// SkillImportHandler  POST /api/v1/skills/import (EP-11 S8)
// ---------------------------------------------------------------------------

const skillImportEnv = "SKILL_IMPORT_ENABLED"

// SkillImportFromLookup reads SKILL_IMPORT_ENABLED strictly: only the literal "true" mounts the
// import, and only together with SKILL_BUNDLES_ENABLED and SKILL_DRAFT_PUBLISH, because an import
// is a draft (with a draft bundle) that nothing uses until it is published.
func SkillImportFromLookup(lookup func(string) string) bool {
	return lookup(skillImportEnv) == "true" && SkillBundlesAPIFromLookup(lookup) && DraftPublishFromLookup(lookup)
}

// skillAuthoringZDRFeature is the zdr-refusals.json feature an import refusal names.
const skillAuthoringZDRFeature = "skill_authoring"

// skillAuthoringZDRMessage is the catalogue sentence of zdr_feature_not_supported with that
// feature's label, as mp-contracts' Refusal::message writes it.
const skillAuthoringZDRMessage = "This is not supported with zero data retention yet: saving or editing skills. The request was refused before any model call."

// maxSkillImportBodyBytes bounds the JSON body before it is read: the largest bundle upload
// (maxSkillBundleBodyBytes) in base64, plus room for the other field.
const maxSkillImportBodyBytes = maxSkillBundleBodyBytes/3*4 + 64<<10

// zipMagic starts every zip archive; anything else is read as a lone SKILL.md.
var zipMagic = []byte("PK\x03\x04")

// importDraftSQL opens the imported skill's draft from its own live fields (session-core's version
// body, migrations 0108/0109) with the draft bundle's digest ($3, empty for a lone SKILL.md). The
// draft is what the author reviews and publishes; publishing it makes the bundle live.
const importDraftSQL = `
	UPDATE agent_skills
	SET draft = agent_skill_version_body(agent_skills)
	        || CASE WHEN $3 <> '' THEN jsonb_build_object('bundle_sha256', $3::text) ELSE '{}'::jsonb END,
	    draft_updated_at = $4, draft_by = $5
	WHERE id = $1 AND org_id = $2
`

// importSkillSQL creates the imported skill switched off, with empty trigger lists and no shares:
// it steers no turn until its author has read it, published it and switched it on.
const importSkillSQL = `
	INSERT INTO agent_skills (id, org_id, name, description, content,
	    trigger_keywords, trigger_file_patterns, tool_restrictions,
	    enabled, scope, owner_user_id, shared_with, created_at, updated_at)
	VALUES ($1, $2, $3, $4, $5, '[]'::jsonb, '[]'::jsonb, '[]'::jsonb, false, $6, $7, '[]'::jsonb, $8, $8)
`

// errSkillImportNameTaken ends an import whose name another skill of the same scope and owner holds.
var errSkillImportNameTaken = errors.New("skill_name_taken")

// SkillImportHandler brings a skill in from a file: a lone SKILL.md (Agent Skills format) or a zip
// with SKILL.md at its root and the files it uses. The file is checked by the bundle validator
// (caps, zip-slip, symlinks, the executable bit only under scripts/), and the skill is created
// switched off with its text as a draft and its files as a draft bundle. Nothing an import writes
// reaches a turn until the author publishes the draft and switches the skill on. Import never
// fetches anything.
type SkillImportHandler struct {
	// skills carries the registry's transaction (mutateSkill names the actor for the revision and
	// version triggers), its lint mode and its retention switch.
	skills  *SkillsHandler
	enabled bool
}

// NewSkillImportHandler builds the handler; enabled comes from SkillImportFromLookup. With it off
// Register mounts nothing, so the path is the skill subtree's as before.
func NewSkillImportHandler(pool registryDatabase, enabled bool) *SkillImportHandler {
	return &SkillImportHandler{skills: NewSkillsHandler(pool), enabled: enabled}
}

// WithPublisher wires reconcile-event emission, as for a created skill. Optional, nil-safe.
func (h *SkillImportHandler) WithPublisher(pub publisher.EventPublisher) *SkillImportHandler {
	h.skills.WithPublisher(pub)
	return h
}

// Register mounts POST /api/v1/skills/import while the switch is on. Its exact pattern takes
// precedence over the /api/v1/skills/ subtree.
func (h *SkillImportHandler) Register(mux *http.ServeMux) {
	if !h.enabled {
		return
	}
	mux.HandleFunc("/api/v1/skills/import", h.importSkill)
}

// skillImportBody carries the file as base64 inside JSON, the only body the model-gateway and BFF
// proxies forward. org_id, which model-gateway adds to skill creates, is ignored: the org is the
// verified caller's.
type skillImportBody struct {
	ContentBase64 string `json:"content_base64"`
	Scope         string `json:"scope"`
}

// importedFile is a checked import: the skill's text and, for a zip with more than SKILL.md, the
// bundle of its files.
type importedFile struct {
	document  skillbundle.SkillDocument
	bundle    skillbundle.Bundle
	hasBundle bool
}

func (h *SkillImportHandler) importSkill(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeTypedError(w, http.StatusMethodNotAllowed, "method_not_allowed", "Only POST imports a skill.")
		return
	}
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		writeTypedError(w, http.StatusUnauthorized, "unauthenticated", "authentication required")
		return
	}
	// authz refuses a ZDR principal before this handler; the check stays here so the refusal is
	// the typed one and cannot depend on the middleware's order. Nothing has been read yet.
	if principal.ZeroDataRetention || !principal.RetentionPolicyPresent {
		writeSkillAuthoringZDRRefusal(w)
		return
	}
	// An imported skill is owned by the person who brought it in; a workload or an API key has
	// no one to own it.
	if principal.PrincipalType != "user" {
		writeTypedError(w, http.StatusForbidden, "skill_import_requires_person", "Only a signed-in person can import a skill.")
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, maxSkillImportBodyBytes)
	var body skillImportBody
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			writeTypedError(w, http.StatusRequestEntityTooLarge, "bundle_too_large", "The file is larger than a skill bundle may be.")
			return
		}
		writeTypedError(w, http.StatusBadRequest, "invalid_body", "The body must be {content_base64, scope?}.")
		return
	}
	raw, err := base64.StdEncoding.DecodeString(body.ContentBase64)
	if err != nil || len(raw) == 0 {
		writeTypedError(w, http.StatusBadRequest, "invalid_body", "content_base64 must hold the file, base64-encoded.")
		return
	}
	scope := strings.TrimSpace(body.Scope)
	if scope == "" {
		scope = "user"
	}
	if !validSkillScope(scope) {
		writeTypedError(w, http.StatusUnprocessableEntity, "invalid_skill_scope", "scope must be user or org.")
		return
	}
	admin := verifiedIsAdmin(r)
	if scope == "org" && !admin {
		writeTypedError(w, http.StatusForbidden, "forbidden", "Only an admin can import an org-wide skill.")
		return
	}
	imported, ok := h.checkFile(w, raw)
	if !ok {
		return
	}
	if imported.hasBundle {
		// Files beside SKILL.md may be scripts the agent can run, so they are org configuration:
		// admin-only, as the bundle route is.
		if !admin {
			writeTypedError(w, http.StatusForbidden, "skill_import_bundle_requires_admin", "Only an admin can import a skill with files beside SKILL.md.")
			return
		}
		// Without retention a publish never makes a draft bundle live, so the files could not be used.
		if !h.skills.bundleRetention {
			writeTypedError(w, http.StatusConflict, "skill_import_bundle_unavailable", "Importing a skill with files beside SKILL.md is not turned on.")
			return
		}
	}
	document := imported.document
	// The whole lint runs on an import whatever SKILL_LINT_MODE says, as the kind rules do: no
	// stored row predates an import, and a 1 MiB SKILL.md must not become a skill unchecked.
	findings := skilllint.Lint(skilllint.Input{
		Name: document.Name, Description: document.Description, Content: document.Body, Kind: skilllint.KindSkill,
	})
	if h.skills.lintMode != skilllint.ModeOff {
		skilllint.Count(r.Context(), h.skills.lintMode, findings)
	}
	if skilllint.HasError(findings) {
		writeSkillLintRefusal(w, findings)
		return
	}

	orgID := principal.OrganizationID
	ownerUserID := ""
	if scope == "user" {
		ownerUserID = principal.ActorID
	}
	skillID := "skill_" + uuid.New().String()
	manifest := ""
	if imported.hasBundle {
		manifest = imported.bundle.ManifestSHA256
	}
	now := time.Now().UTC()
	err = h.skills.mutateSkill(r, "", "imported", func(tx registryDatabase) error {
		_, err := tx.Exec(r.Context(), importSkillSQL, skillID, orgID, document.Name, document.Description,
			document.Body, scope, ownerUserID, now)
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23505" {
			return errSkillImportNameTaken
		}
		if err != nil {
			return err
		}
		if imported.hasBundle {
			pgxTx, ok := tx.(pgx.Tx)
			if !ok {
				return errSkillTransactionUnavailable
			}
			if err := skillbundle.PutImportedDraft(r.Context(), pgxTx, orgID, skillID, principal.ActorID, imported.bundle); err != nil {
				return err
			}
		}
		_, err = tx.Exec(r.Context(), importDraftSQL, skillID, orgID, manifest, now, principal.ActorID)
		return err
	})
	if errors.Is(err, errSkillImportNameTaken) {
		writeTypedError(w, http.StatusConflict, "skill_name_taken", "A skill with this name already exists.")
		return
	}
	if err != nil {
		slog.Error("skill import failed", "error", err)
		writeTypedError(w, http.StatusInternalServerError, "database_unavailable", "The skill could not be imported.")
		return
	}
	if eerr := reconcile.Emit(r.Context(), h.skills.pub, reconcile.KindSkill,
		reconcile.ActionRegistered, skillID, orgID); eerr != nil {
		slog.Warn("reconcile emit failed", "kind", reconcile.KindSkill, "id", skillID, "error", eerr)
	}
	answer := map[string]any{"id": skillID, "name": document.Name, "scope": scope, "enabled": false, "draft": true}
	if imported.hasBundle {
		answer["bundle"] = map[string]any{
			"manifest_sha256": manifest, "file_count": len(imported.bundle.Files), "total_bytes": imported.bundle.TotalBytes,
		}
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusCreated)
	_ = json.NewEncoder(w).Encode(withLintWarnings(answer, findings))
}

// checkFile validates the uploaded bytes and answers a refusal itself. A zip goes through the
// bundle validator (and the deck template checks while they are on, as an upload does); a zip
// holding only SKILL.md imports as that file alone. Anything else must be a SKILL.md.
func (h *SkillImportHandler) checkFile(w http.ResponseWriter, raw []byte) (importedFile, bool) {
	var imported importedFile
	var err error
	if bytes.HasPrefix(raw, zipMagic) {
		var bundle skillbundle.Bundle
		bundle, err = skillbundle.Parse(raw)
		if err == nil && skillbundle.DeckTemplatesFromLookup(os.Getenv) {
			err = skillbundle.ValidateDeckTemplate(bundle)
		}
		if err == nil {
			imported = importedFile{document: skillbundle.BundleSkillDocument(bundle), bundle: bundle, hasBundle: len(bundle.Files) > 1}
		}
	} else {
		imported.document, err = skillbundle.ParseSkillFile(raw)
	}
	if err != nil {
		var refusal *skillbundle.RefusalError
		if errors.As(err, &refusal) {
			writeTypedError(w, http.StatusUnprocessableEntity, refusal.Code, "The file is not a skill this registry can import.")
			return importedFile{}, false
		}
		writeTypedError(w, http.StatusUnprocessableEntity, "invalid_skill_bundle", "The file is not a skill this registry can import.")
		return importedFile{}, false
	}
	return imported, true
}

// writeSkillAuthoringZDRRefusal answers the catalogue's zdr_feature_not_supported for
// skill_authoring, in the body mp-contracts writes, with the feature also under details as the
// envelope rule asks.
func writeSkillAuthoringZDRRefusal(w http.ResponseWriter) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusPreconditionFailed)
	_ = json.NewEncoder(w).Encode(map[string]any{"error": map[string]any{
		"code":      "zdr_feature_not_supported",
		"message":   skillAuthoringZDRMessage,
		"retryable": false,
		"action":    "turn_off_feature",
		"feature":   skillAuthoringZDRFeature,
		"details":   map[string]any{"feature": skillAuthoringZDRFeature},
	}})
}
