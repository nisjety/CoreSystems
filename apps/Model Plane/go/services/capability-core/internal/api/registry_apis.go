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
	"io"
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
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/reconcile"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillbundle"
	"github.com/triodelab/model-plane/services/capability-core/internal/skilllint"
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
	// lintMode is SKILL_LINT_MODE (EP-11 S4): off runs no lint and leaves every write and
	// body as it was.
	lintMode skilllint.Mode
	// versionsAPI is SKILL_VERSIONS_API (EP-11 S2): on, skill reads carry version and digest and
	// the history route is mounted; off, every body and route is as it was.
	versionsAPI bool
	// authoredKinds is AUTHORED_KINDS_ENABLED (EP-11 S6): on, the routes read and write
	// templates and presets beside skills (?kind=, kind and settings in bodies); off, every
	// statement and body is as it was and a template or preset write answers 422.
	authoredKinds bool
	// draftPublish is SKILL_DRAFT_PUBLISH (EP-11 S5): on, an edit of an accepted skill's text
	// writes its draft, the publish, discard and restore routes are mounted, and reads carry the
	// draft; off, every statement, route and body is as it was.
	draftPublish bool
	// bundleRetention is SKILL_BUNDLE_RETENTION (EP-11 S5): on, a publish also makes the draft's
	// bundle live and a skill delete removes the skill's bundles; off, neither touches a bundle.
	bundleRetention bool
}

// NewSkillsHandler constructs the handler.
func NewSkillsHandler(pool registryDatabase) *SkillsHandler {
	return &SkillsHandler{pool: pool, lintMode: skilllint.ModeFromLookup(os.Getenv), versionsAPI: SkillVersionsAPIFromLookup(os.Getenv),
		authoredKinds: AuthoredKindsFromLookup(os.Getenv), draftPublish: DraftPublishFromLookup(os.Getenv),
		bundleRetention: skillbundle.RetentionFromLookup(os.Getenv)}
}

const draftPublishEnv = "SKILL_DRAFT_PUBLISH"

// DraftPublishFromLookup reads SKILL_DRAFT_PUBLISH strictly: only the literal "true" puts edits of
// accepted skills behind a draft, so a typo or a stray "1" leaves every edit live as before.
func DraftPublishFromLookup(lookup func(string) string) bool {
	return lookup(draftPublishEnv) == "true"
}

const authoredKindsEnv = "AUTHORED_KINDS_ENABLED"

// AuthoredKindsFromLookup reads AUTHORED_KINDS_ENABLED strictly: only the literal "true" lets the
// routes carry templates and presets, so a typo or a stray "1" leaves them as they were.
func AuthoredKindsFromLookup(lookup func(string) string) bool {
	return lookup(authoredKindsEnv) == "true"
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
	// EP-11 S2: off, the path falls to the subtree above and answers 404 as before.
	if h.versionsAPI {
		mux.HandleFunc("/api/v1/skills/{id}/versions", h.versions)
	}
	// EP-11 S5: off, these paths fall to the subtree above and answer as before.
	if h.draftPublish {
		mux.HandleFunc("/api/v1/skills/{id}/publish", h.publish)
		mux.HandleFunc("/api/v1/skills/{id}/draft", h.discardDraft)
		mux.HandleFunc("/api/v1/skills/{id}/versions/{version}/restore", h.restore)
	}
}

type skillRow struct {
	ID                  string   `json:"id"`
	OrgID               string   `json:"org_id"`
	Name                string   `json:"name"`
	Description         string   `json:"description"`
	Content             string   `json:"content"`
	TriggerKeywords     []string `json:"trigger_keywords"`
	TriggerFilePatterns []string `json:"trigger_file_patterns"`
	ToolRestrictions    []string `json:"tool_restrictions"`
	Enabled             bool     `json:"enabled"`
	// Ownership/sharing (SKILL-1): "org" (default, visible to the whole org)
	// or "user" (private to OwnerUserID until explicitly shared). Mirrors
	// mcp_servers' scope + owner + explicit-share shape.
	Scope       string   `json:"scope"`
	OwnerUserID string   `json:"owner_user_id,omitempty"`
	SharedWith  []string `json:"shared_with,omitempty"`
	// Provenance and review (WS07 S7): who wrote the skill ("user" or
	// "background_review"), whether its owner has decided on it ("proposed",
	// "accepted" or "rejected"), and what a proposal was distilled from. Read
	// only: a create ignores whatever a client sends for them.
	Origin         string    `json:"origin"`
	ReviewState    string    `json:"review_state"`
	SourceRunID    string    `json:"source_run_id,omitempty"`
	ProposalReason string    `json:"proposal_reason,omitempty"`
	CreatedAt      time.Time `json:"created_at"`
	UpdatedAt      time.Time `json:"updated_at"`
	// PromotedFrom is read on create only: the id of the shared, accepted user
	// skill an admin is copying into an org skill. The copy is a new row; the
	// source is untouched, and the revision log records where it came from.
	PromotedFrom string `json:"promoted_from,omitempty"`
	// Version and Digest (EP-11 S2) are the skill's newest immutable version: 0 and "" for a
	// proposal never accepted. A trigger sets them; a write never does. Read only, and present
	// only while SKILL_VERSIONS_API is on, so with it off a body is as it was.
	Version *int32 `json:"version,omitempty"`
	Digest  string `json:"digest,omitempty"`

	// Kind and Settings (EP-11 S6) are the authored kind ("skill", "template" or "preset") and a
	// preset's closed settings object. Read and written only while AUTHORED_KINDS_ENABLED is on,
	// so with it off a body is as it was.
	Kind     string          `json:"kind,omitempty"`
	Settings json.RawMessage `json:"settings,omitempty"`

	// Draft, DraftUpdatedAt and DraftBy (EP-11 S5) are the skill's unpublished edit: the same
	// fields as a version body, when it was last saved and by whom. Read only (a PATCH writes the
	// draft, never this field), and read only while SKILL_DRAFT_PUBLISH is on, so with it off a
	// body is as it was. Absent when the skill has no draft.
	Draft          json.RawMessage `json:"draft,omitempty"`
	DraftUpdatedAt *time.Time      `json:"draft_updated_at,omitempty"`
	DraftBy        string          `json:"draft_by,omitempty"`
}

// skillScanTargets are the destinations of a skill read, in the order of the SELECTs in list and
// get, with version and digest last while the versions API is on.
func (h *SkillsHandler) skillScanTargets(s *skillRow) []any {
	targets := []any{&s.ID, &s.OrgID, &s.Name, &s.Description, &s.Content,
		&s.TriggerKeywords, &s.TriggerFilePatterns, &s.ToolRestrictions,
		&s.Enabled, &s.Scope, &s.OwnerUserID, &s.SharedWith,
		&s.Origin, &s.ReviewState, &s.SourceRunID, &s.ProposalReason, &s.CreatedAt, &s.UpdatedAt}
	if h.versionsAPI {
		s.Version = new(int32)
		targets = append(targets, s.Version, &s.Digest)
	}
	if h.authoredKinds {
		targets = append(targets, &s.Kind, &s.Settings)
	}
	if h.draftPublish {
		targets = append(targets, &s.Draft, &s.DraftUpdatedAt, &s.DraftBy)
	}
	return targets
}

// skillVersionColumns are the columns a skill read adds while the versions API is on, then
// kind and settings while authored kinds are on, then the draft while drafts are on (the order
// of skillScanTargets).
func (h *SkillsHandler) skillVersionColumns() string {
	columns := ""
	if h.versionsAPI {
		columns += ", version, digest"
	}
	if h.authoredKinds {
		columns += ", kind, settings"
	}
	if h.draftPublish {
		columns += ", draft, draft_updated_at, draft_by"
	}
	return columns
}

// requestedSkillKind is the ?kind= of a read: "skill" when absent. ok is false for a kind
// outside the contract's list.
func requestedSkillKind(r *http.Request) (kind string, ok bool) {
	kind = strings.TrimSpace(r.URL.Query().Get("kind"))
	if kind == "" {
		return skilllint.KindSkill, true
	}
	return kind, skilllint.ValidKind(kind)
}

// emptySettings reports whether raw carries no settings: absent, JSON null or {}.
func emptySettings(raw json.RawMessage) bool {
	trimmed := strings.TrimSpace(string(raw))
	return trimmed == "" || trimmed == "null" || trimmed == "{}"
}

// decodeSettings decodes a settings body for the lint; nil when there is none. A body that is
// not JSON cannot reach here: the request decoder already refused it.
func decodeSettings(raw json.RawMessage) any {
	if emptySettings(raw) {
		return nil
	}
	var settings any
	_ = json.Unmarshal(raw, &settings)
	return settings
}

// kindRuleRefusal runs the template and preset rules on a write of kind. They run whatever
// SKILL_LINT_MODE says, because no stored row predates them; a skill has none.
func kindRuleRefusal(input skilllint.Input) []skilllint.Finding {
	if input.Kind == skilllint.KindSkill || input.Kind == "" {
		return nil
	}
	return skilllint.KindRuleFindings(skilllint.Lint(input))
}

func validSkillScope(value string) bool {
	return value == "org" || value == "user"
}

// skillActorSQL names the actor of the skill change the transaction is about to
// make. Skill revisions are written by a trigger on agent_skills (session-core
// migration 0064), which reads these transaction-local settings; the names are
// pinned to the migration by TestSkillActorSettingsMatchTheMigration. run_id and
// thread_id are session-core's (a learned proposal's source); the registry has none.
const skillActorSQL = `SELECT set_config('mp.actor_kind', $1, true), set_config('mp.actor_id', $2, true),
	set_config('mp.run_id', '', true), set_config('mp.thread_id', '', true),
	set_config('mp.reason', $3, true), set_config('mp.skill_action', $4, true)`

// skillTxBeginner is the pool's transaction entry point. The registry's own
// interface stays Exec/Query/QueryRow so fakes need not implement it; a skill
// mutation needs it, and a database without it cannot mutate a skill at all.
type skillTxBeginner interface {
	Begin(context.Context) (pgx.Tx, error)
}

var errSkillTransactionUnavailable = errors.New("skill mutations need a transactional database")

// errSkillLintRefused ends a mutation whose text failed the skill lint under enforce, so the
// transaction rolls back before its statement runs.
var errSkillLintRefused = errors.New(skilllint.ErrorCode)

// lintSkill runs the shared skill lint (EP-11 S4) under mode and counts what it finds. It
// returns the findings and whether enforce refuses the write; off runs nothing.
func lintSkill(ctx context.Context, mode skilllint.Mode, input skilllint.Input) ([]skilllint.Finding, bool) {
	if mode == skilllint.ModeOff {
		return nil, false
	}
	findings := skilllint.Lint(input)
	skilllint.Count(ctx, mode, findings)
	return findings, mode == skilllint.ModeEnforce && skilllint.HasError(findings)
}

// writeSkillLintRefusal answers a refused write with the typed envelope. The findings name
// rules and fields, never the text; they sit under a named key because details is an object.
func writeSkillLintRefusal(w http.ResponseWriter, findings []skilllint.Finding) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusUnprocessableEntity)
	_ = json.NewEncoder(w).Encode(map[string]any{"error": map[string]any{
		"code":    skilllint.ErrorCode,
		"message": "The skill does not pass the skill lint.",
		"details": map[string]any{"findings": findings},
	}})
}

// withLintWarnings adds the findings of a write that went ahead to its body. With no
// findings, which is always the case while the lint is off, the body is unchanged.
func withLintWarnings(body map[string]any, findings []skilllint.Finding) map[string]any {
	if len(findings) > 0 {
		body["warnings"] = findings
	}
	return body
}

// lintStoredSkill lints the row a PATCH is about to change, with the patch's description
// and content laid over it the way the UPDATE lays them. A row that is not there passes:
// the UPDATE then changes nothing and the caller gets 404.
func lintStoredSkill(ctx context.Context, tx registryDatabase, mode skilllint.Mode, id, orgID, description, content string) ([]skilllint.Finding, bool, error) {
	var input skilllint.Input
	err := tx.QueryRow(ctx, `
		SELECT name, description, content, trigger_keywords, tool_restrictions
		FROM agent_skills WHERE id=$1 AND org_id=$2
	`, id, orgID).Scan(&input.Name, &input.Description, &input.Content, &input.TriggerKeywords, &input.ToolRestrictions)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	if description != "" {
		input.Description = description
	}
	if content != "" {
		input.Content = content
	}
	findings, refused := lintSkill(ctx, mode, input)
	return findings, refused, nil
}

// errSkillSettingsNotAllowed ends a PATCH that sends settings for a row that is not a preset.
var errSkillSettingsNotAllowed = errors.New("only a preset carries settings")

// kindRulesForPatch runs the template and preset rules (EP-11 S6) on the row a PATCH is about to
// change, with the patch's content and settings laid over it the way the UPDATE lays them. A row
// that is not there passes: the UPDATE then changes nothing and the caller gets 404.
func kindRulesForPatch(ctx context.Context, tx registryDatabase, id, orgID, content string, settings json.RawMessage) ([]skilllint.Finding, error) {
	var kind, storedContent string
	var storedSettings json.RawMessage
	err := tx.QueryRow(ctx, `
		SELECT kind, content, settings FROM agent_skills WHERE id=$1 AND org_id=$2
	`, id, orgID).Scan(&kind, &storedContent, &storedSettings)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	if settings != nil && kind != skilllint.KindPreset {
		return nil, errSkillSettingsNotAllowed
	}
	if content == "" {
		content = storedContent
	}
	if settings == nil {
		settings = storedSettings
	}
	return kindRuleRefusal(skilllint.Input{Content: content, Kind: kind, Settings: decodeSettings(settings)}), nil
}

// skillActor is the verified caller as the revision log names it: "user" or
// "service" and the principal id, never anything the request body claims.
func skillActor(r *http.Request) (kind, id string) {
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok {
		return "", ""
	}
	if principal.PrincipalType == "user" || principal.PrincipalType == "service" {
		kind = principal.PrincipalType
	}
	return kind, principal.ActorID
}

// mutateSkill runs fn in one transaction that has first named the caller as the
// actor, so the revision the trigger writes for every row fn changes carries who
// did it. action is "" unless the change is a decision the trigger cannot derive
// ("promoted").
func (h *SkillsHandler) mutateSkill(r *http.Request, action, reason string, fn func(tx registryDatabase) error) error {
	beginner, ok := h.pool.(skillTxBeginner)
	if !ok {
		return errSkillTransactionUnavailable
	}
	ctx := r.Context()
	tx, err := beginner.Begin(ctx)
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback(ctx) }()
	kind, id := skillActor(r)
	if _, err := tx.Exec(ctx, skillActorSQL, kind, id, reason, action); err != nil {
		return err
	}
	if err := fn(tx); err != nil {
		return err
	}
	return tx.Commit(ctx)
}

// normalizeSkillShares validates and cleans a caller-supplied grantee list:
// trims whitespace, drops blanks and the owner themselves, de-dups, and
// rejects anything that isn't a well-formed identifier. This is the
// authoritative sharing validator for skills — unlike mcp_servers (whose
// real enforcement lives in model-gateway's OwnershipStore), capability-core
// is the sole write path for agent_skills, so it owns this outright.
func normalizeSkillShares(ownerUserID string, grantees []string) ([]string, error) {
	if len(grantees) > 64 {
		return nil, errors.New("too many skill grantees")
	}
	seen := make(map[string]struct{}, len(grantees))
	clean := make([]string, 0, len(grantees))
	for _, grantee := range grantees {
		grantee = strings.TrimSpace(grantee)
		if grantee == "" || grantee == ownerUserID {
			continue
		}
		if !mcpIdentifier.MatchString(grantee) {
			return nil, errors.New("invalid skill sharing metadata")
		}
		if _, exists := seen[grantee]; exists {
			continue
		}
		seen[grantee] = struct{}{}
		clean = append(clean, grantee)
	}
	return clean, nil
}

// errSkillNotCallersToChange answers a change to a user-scoped skill by a caller who neither
// owns it nor is an org admin. It is a 403 because the sharing check in update already answers
// 403 to the same caller about the same skill.
var errSkillNotCallersToChange = errors.New("only the skill owner or an org admin may change or delete this skill")

// ensureSkillCallerMayChange runs inside the mutation's transaction, before its statement. It
// locks the skill's row and reads its scope and owner, so the statement that follows cannot
// race a change to either. An org-scoped skill stays open to every member, as it always was.
// A skill that is not there passes: the statement then changes nothing and the caller gets 404.
func ensureSkillCallerMayChange(ctx context.Context, tx registryDatabase, id, orgID, callerID string, isAdmin bool) error {
	rows, err := tx.Query(ctx, `
		SELECT scope, owner_user_id FROM agent_skills
		WHERE id=$1 AND org_id=$2 FOR UPDATE
	`, id, orgID)
	if err != nil {
		return err
	}
	defer rows.Close()
	if !rows.Next() {
		return rows.Err()
	}
	var scope, ownerUserID string
	if err := rows.Scan(&scope, &ownerUserID); err != nil {
		return err
	}
	// A Space skill (EP-11 S9) is changed by a Space decision, never by an org role or by owning
	// it; none can be verified yet, so it is refused whatever the caller holds.
	if scope == spaceSkillScope {
		return errSpaceSkillDecisionRequired
	}
	isOwner := ownerUserID != "" && ownerUserID == callerID
	if scope == "org" || isAdmin || isOwner {
		return nil
	}
	return errSkillNotCallersToChange
}

func (h *SkillsHandler) listOrCreate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		orgID := verifiedOrganizationID(r)
		callerID := verifiedActorID(r)
		isAdmin := verifiedIsAdmin(r)
		// EP-11 S6: ?kind= picks the authored kind, skills when absent. Off, a skills read is
		// as it was and any other kind answers an empty list that says why, so the SPA learns
		// availability from the route rather than from a build flag.
		kind, validKind := requestedSkillKind(r)
		if !h.authoredKinds && kind != skilllint.KindSkill {
			writeJSON(w, map[string]any{"skills": []skillRow{}, "kinds_enabled": false})
			return
		}
		if !validKind {
			writeTypedError(w, http.StatusUnprocessableEntity, "invalid_kind", "kind must be skill, template or preset")
			return
		}
		kindFilter := ""
		args := []any{orgID, callerID, isAdmin}
		if h.authoredKinds {
			kindFilter = `
			  AND kind = $4`
			args = append(args, kind)
		}
		// Visibility (SKILL-1): org-scoped skills, the caller's own, ones
		// explicitly shared with the caller, and — for governance — any
		// shared (never private) skill an admin didn't create themselves. A
		// proposal is its owner's alone until it is accepted: a share written
		// onto it shows it to no one else.
		rows, err := h.pool.Query(r.Context(), `
			SELECT id, org_id, name, description, content,
			       trigger_keywords, trigger_file_patterns, tool_restrictions,
			       enabled, scope, owner_user_id, shared_with,
			       origin, review_state, source_run_id, proposal_reason, created_at, updated_at`+h.skillVersionColumns()+`
			FROM agent_skills
			WHERE org_id = $1
			  AND (scope = 'org' OR owner_user_id = $2
			       OR (review_state = 'accepted' AND (shared_with ? $2
			           OR ($3 AND jsonb_array_length(shared_with) > 0))))`+kindFilter+`
			ORDER BY name
		`, args...)
		if err != nil {
			slog.Error("list skills failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		defer rows.Close()
		var skills []skillRow
		for rows.Next() {
			var s skillRow
			if err := rows.Scan(h.skillScanTargets(&s)...); err != nil {
				jsonErr(w, err.Error(), http.StatusInternalServerError)
				return
			}
			skills = append(skills, s)
		}
		body := map[string]any{"skills": skills}
		if h.authoredKinds {
			body["kinds_enabled"] = true
		}
		// EP-11 S5: the Skills page offers draft controls only when the listing says drafts are on.
		if h.draftPublish {
			body["drafts_enabled"] = true
		}
		writeJSON(w, body)
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
		s.Scope = strings.ToLower(strings.TrimSpace(s.Scope))
		if s.Scope == "" {
			s.Scope = "org"
		}
		// EP-11 S9 (D7): off, "space" stays an invalid scope; on, it needs a Space decision.
		if s.Scope == spaceSkillScope && skillSpaceScopeEnabled() {
			writeSpaceSkillDecisionRequired(w)
			return
		}
		if !validSkillScope(s.Scope) {
			jsonErr(w, "invalid skill scope", http.StatusUnprocessableEntity)
			return
		}
		if s.Scope == "org" && !verifiedIsAdmin(r) {
			jsonErr(w, "only an admin can create an org-wide skill", http.StatusForbidden)
			return
		}
		// Ownership is bound to the authenticated caller server-side, never
		// to whatever owner_user_id the client happened to send.
		if s.Scope == "user" {
			s.OwnerUserID = verifiedActorID(r)
		} else {
			s.OwnerUserID = ""
		}
		s.SharedWith = []string{}
		// EP-11 S6: a template or preset is written only while authored kinds are on; off, a
		// body naming one is refused rather than silently stored as a skill.
		s.Kind = strings.TrimSpace(s.Kind)
		if s.Kind == "" {
			s.Kind = skilllint.KindSkill
		}
		if !h.authoredKinds && (s.Kind != skilllint.KindSkill || !emptySettings(s.Settings)) {
			writeTypedError(w, http.StatusUnprocessableEntity, "kind_not_enabled", "templates and presets are not enabled")
			return
		}
		if !skilllint.ValidKind(s.Kind) {
			writeTypedError(w, http.StatusUnprocessableEntity, "invalid_kind", "kind must be skill, template or preset")
			return
		}
		if s.Kind != skilllint.KindPreset && !emptySettings(s.Settings) {
			writeTypedError(w, http.StatusUnprocessableEntity, "settings_not_allowed", "only a preset carries settings")
			return
		}
		lintInput := skilllint.Input{
			Name: s.Name, Description: s.Description, Content: s.Content,
			TriggerKeywords: s.TriggerKeywords, ToolRestrictions: s.ToolRestrictions,
			Kind: s.Kind, Settings: decodeSettings(s.Settings),
		}
		if findings := kindRuleRefusal(lintInput); len(findings) > 0 {
			writeSkillLintRefusal(w, findings)
			return
		}
		lintFindings, lintRefused := lintSkill(r.Context(), h.lintMode, lintInput)
		if lintRefused {
			writeSkillLintRefusal(w, lintFindings)
			return
		}
		settings := []byte("{}")
		if !emptySettings(s.Settings) {
			settings = s.Settings
		}
		kw, _ := json.Marshal(s.TriggerKeywords)
		fp, _ := json.Marshal(s.TriggerFilePatterns)
		tr, _ := json.Marshal(s.ToolRestrictions)
		sw, _ := json.Marshal(s.SharedWith)
		// A promotion copies a skill that was shared with the admin into an org
		// skill. It is never a way to reach a private one: the source must be a
		// user skill its owner accepted and shared, by the same rule the list uses.
		action, reason := "", ""
		promotedFrom := strings.TrimSpace(s.PromotedFrom)
		if promotedFrom != "" {
			if !mcpIdentifier.MatchString(promotedFrom) || s.Scope != "org" {
				jsonErr(w, "promoted_from must name a skill and create an org skill", http.StatusUnprocessableEntity)
				return
			}
			action, reason = "promoted", "promoted_from:"+promotedFrom
		}
		var sourceMissing bool
		err := h.mutateSkill(r, action, reason, func(tx registryDatabase) error {
			if promotedFrom != "" {
				var visible bool
				if err := tx.QueryRow(r.Context(), `
					SELECT EXISTS (SELECT 1 FROM agent_skills
					WHERE id = $1 AND org_id = $2 AND scope = 'user' AND review_state = 'accepted'
					  AND (shared_with ? $3 OR ($4 AND jsonb_array_length(shared_with) > 0)))
				`, promotedFrom, s.OrgID, verifiedActorID(r), verifiedIsAdmin(r)).Scan(&visible); err != nil {
					return err
				}
				if !visible {
					sourceMissing = true
					return nil
				}
			}
			if h.authoredKinds {
				// The unique key (session-core 0109) carries the kind, so a template may
				// share a skill's name.
				_, err := tx.Exec(r.Context(), `
					INSERT INTO agent_skills (id, org_id, name, description, content,
					    trigger_keywords, trigger_file_patterns, tool_restrictions,
					    enabled, scope, owner_user_id, shared_with, created_at, updated_at,
					    kind, settings)
					VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
				`, s.ID, s.OrgID, s.Name, s.Description, s.Content, kw, fp, tr, s.Enabled,
					s.Scope, s.OwnerUserID, sw, now, now, s.Kind, settings)
				return err
			}
			_, err := tx.Exec(r.Context(), `
				INSERT INTO agent_skills (id, org_id, name, description, content,
				    trigger_keywords, trigger_file_patterns, tool_restrictions,
				    enabled, scope, owner_user_id, shared_with, created_at, updated_at)
				VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
			`, s.ID, s.OrgID, s.Name, s.Description, s.Content, kw, fp, tr, s.Enabled,
				s.Scope, s.OwnerUserID, sw, now, now)
			return err
		})
		if sourceMissing {
			jsonErr(w, "promotion source not found", http.StatusNotFound)
			return
		}
		if err != nil {
			jsonErr(w, err.Error(), http.StatusInternalServerError)
			return
		}
		if eerr := reconcile.Emit(r.Context(), h.pub, reconcile.KindSkill,
			reconcile.ActionRegistered, s.ID, s.OrgID); eerr != nil {
			slog.Warn("reconcile emit failed", "kind", reconcile.KindSkill, "id", s.ID, "error", eerr)
		}
		w.WriteHeader(http.StatusCreated)
		writeJSON(w, withLintWarnings(map[string]any{"id": s.ID}, lintFindings))
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (h *SkillsHandler) get(w http.ResponseWriter, r *http.Request, id string) {
	callerID := verifiedActorID(r)
	isAdmin := verifiedIsAdmin(r)
	var s skillRow
	// EP-11 S6: a read by id answers only for the ?kind= it names (skills when absent), so a
	// template or preset never reads as a skill. Off, the statement is as it was.
	kindFilter := ""
	args := []any{id, verifiedOrganizationID(r), callerID, isAdmin}
	if h.authoredKinds {
		kind, ok := requestedSkillKind(r)
		if !ok {
			writeTypedError(w, http.StatusUnprocessableEntity, "invalid_kind", "kind must be skill, template or preset")
			return
		}
		kindFilter = `
		  AND kind = $5`
		args = append(args, kind)
	}
	err := h.pool.QueryRow(r.Context(), `
		SELECT id, org_id, name, description, content,
		       trigger_keywords, trigger_file_patterns, tool_restrictions,
		       enabled, scope, owner_user_id, shared_with,
		       origin, review_state, source_run_id, proposal_reason, created_at, updated_at`+h.skillVersionColumns()+`
		FROM agent_skills
		WHERE id = $1 AND org_id = $2
		  AND (scope = 'org' OR owner_user_id = $3
		       OR (review_state = 'accepted' AND (shared_with ? $3
		           OR ($4 AND jsonb_array_length(shared_with) > 0))))`+kindFilter+`
	`, args...).Scan(h.skillScanTargets(&s)...)
	if err != nil {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	writeJSON(w, s)
}

func (h *SkillsHandler) update(w http.ResponseWriter, r *http.Request, id string) {
	var update struct {
		Enabled     *bool     `json:"enabled"`
		Description string    `json:"description"`
		Content     string    `json:"content"`
		SharedWith  *[]string `json:"shared_with"`
		// Settings (EP-11 S6) replaces a preset's settings; read only while authored kinds
		// are on, so with them off a PATCH is as it was.
		Settings json.RawMessage `json:"settings"`
	}
	if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
		jsonErr(w, err.Error(), http.StatusBadRequest)
		return
	}
	if !h.authoredKinds || strings.TrimSpace(string(update.Settings)) == "null" {
		update.Settings = nil
	}
	if update.Enabled == nil && update.Description == "" && update.Content == "" && update.SharedWith == nil && update.Settings == nil {
		jsonErr(w, "at least one skill field is required", http.StatusBadRequest)
		return
	}
	now := time.Now().UTC()
	orgID := verifiedOrganizationID(r)
	callerID := verifiedActorID(r)
	isAdmin := verifiedIsAdmin(r)
	var sharedJSON any
	if update.SharedWith != nil {
		var scope, ownerUserID string
		if err := h.pool.QueryRow(r.Context(), `
			SELECT scope, owner_user_id FROM agent_skills WHERE id=$1 AND org_id=$2
		`, id, orgID).Scan(&scope, &ownerUserID); err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				jsonErr(w, "skill not found", http.StatusNotFound)
			} else {
				slog.Error("read skill sharing config failed", "error", err)
				jsonErr(w, "database unavailable", http.StatusInternalServerError)
			}
			return
		}
		if scope != "user" {
			jsonErr(w, "only a user-owned skill can be shared", http.StatusUnprocessableEntity)
			return
		}
		// A user-owned resource may only be shared by its durable owner.
		if ownerUserID == "" || callerID != ownerUserID {
			jsonErr(w, "only the skill owner may change sharing", http.StatusForbidden)
			return
		}
		clean, err := normalizeSkillShares(ownerUserID, *update.SharedWith)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusUnprocessableEntity)
			return
		}
		payload, err := json.Marshal(clean)
		if err != nil {
			jsonErr(w, "invalid skill sharing configuration", http.StatusUnprocessableEntity)
			return
		}
		sharedJSON = payload
	}
	var enabled any
	if update.Enabled != nil {
		enabled = *update.Enabled
	}
	// A learned skill becomes active by its owner's decision, never by a switch:
	// enabling is refused for anything not accepted, in the statement itself so
	// no write can race the check. The change runs in a transaction that names
	// the caller, so its revision carries the actor.
	var result pgconn.CommandTag
	proposalUndecided := false
	// An edit of the text, or switching a skill on, is linted on the row as it will be, so a
	// person who edits or re-enables learned text is held to the same rules as a new skill.
	// A sharing change or a switch-off writes no text and is not linted.
	lintPatch := h.lintMode != skilllint.ModeOff &&
		(update.Description != "" || update.Content != "" || (update.Enabled != nil && *update.Enabled))
	var lintFindings []skilllint.Finding
	drafted := false
	err := h.mutateSkill(r, "", "", func(tx registryDatabase) error {
		// A user-owned skill changes only for its owner or an admin. The check takes the row
		// lock before the statement below, so the check and the write see the same skill.
		if err := ensureSkillCallerMayChange(r.Context(), tx, id, orgID, callerID, isAdmin); err != nil {
			return err
		}
		if lintPatch {
			findings, refused, err := lintStoredSkill(r.Context(), tx, h.lintMode, id, orgID, update.Description, update.Content)
			if err != nil {
				return err
			}
			lintFindings = findings
			if refused {
				return errSkillLintRefused
			}
		}
		if h.authoredKinds && (update.Content != "" || update.Settings != nil) {
			findings, err := kindRulesForPatch(r.Context(), tx, id, orgID, update.Content, update.Settings)
			if err != nil {
				return err
			}
			if len(findings) > 0 {
				lintFindings = findings
				return errSkillLintRefused
			}
		}
		// EP-11 S5: an edit of an accepted skill's text goes to its draft while drafts are on, so
		// turns keep the published version until someone publishes it. enabled and sharing are not
		// versioned and still apply at once; a proposal is edited live, as before (accepting it is
		// its publish).
		if h.draftPublish && (update.Description != "" || update.Content != "" || update.Settings != nil) {
			var accepted bool
			if err := tx.QueryRow(r.Context(), acceptedSkillRowSQL, id, orgID).Scan(&accepted); err != nil && !errors.Is(err, pgx.ErrNoRows) {
				return err
			}
			if accepted {
				var settings any
				if update.Settings != nil {
					settings = []byte(update.Settings)
				}
				var err error
				result, err = tx.Exec(r.Context(), draftPatchSQL, enabled, update.Description, update.Content,
					sharedJSON, now, id, orgID, settings, callerID)
				drafted = err == nil && result.RowsAffected() == 1
				return err
			}
		}
		var err error
		if h.authoredKinds {
			// Settings change only on a preset: kindRulesForPatch refused any other kind.
			var settings any
			if update.Settings != nil {
				settings = []byte(update.Settings)
			}
			result, err = tx.Exec(r.Context(), `
				UPDATE agent_skills
				SET enabled=COALESCE($1, enabled), description=COALESCE(NULLIF($2, ''), description),
					content=COALESCE(NULLIF($3, ''), content), shared_with=COALESCE($4, shared_with),
					updated_at=$5, settings=COALESCE($8::jsonb, settings)
				WHERE id=$6 AND org_id=$7 AND ($1::boolean IS NOT TRUE OR review_state = 'accepted')
			`, enabled, update.Description, update.Content, sharedJSON, now, id, orgID, settings)
		} else {
			result, err = tx.Exec(r.Context(), `
			UPDATE agent_skills
			SET enabled=COALESCE($1, enabled), description=COALESCE(NULLIF($2, ''), description),
				content=COALESCE(NULLIF($3, ''), content), shared_with=COALESCE($4, shared_with),
				updated_at=$5
			WHERE id=$6 AND org_id=$7 AND ($1::boolean IS NOT TRUE OR review_state = 'accepted')
		`, enabled, update.Description, update.Content, sharedJSON, now, id, orgID)
		}
		if err == nil && result.RowsAffected() == 0 && update.Enabled != nil && *update.Enabled {
			var reviewState string
			if tx.QueryRow(r.Context(), `
				SELECT review_state FROM agent_skills WHERE id=$1 AND org_id=$2
			`, id, orgID).Scan(&reviewState) == nil && reviewState != "accepted" {
				proposalUndecided = true
			}
		}
		return err
	})
	if errors.Is(err, errSkillNotCallersToChange) {
		jsonErr(w, err.Error(), http.StatusForbidden)
		return
	}
	if errors.Is(err, errSpaceSkillDecisionRequired) {
		writeSpaceSkillDecisionRequired(w)
		return
	}
	if errors.Is(err, errSkillLintRefused) {
		writeSkillLintRefusal(w, lintFindings)
		return
	}
	if errors.Is(err, errSkillSettingsNotAllowed) {
		writeTypedError(w, http.StatusUnprocessableEntity, "settings_not_allowed", err.Error())
		return
	}
	if proposalUndecided {
		jsonErr(w, "proposal_requires_decision", http.StatusConflict)
		return
	}
	if !writeSingleScopedMutation(w, "agent skill", result, err) {
		return
	}
	body := map[string]any{"id": id, "updated_at": now}
	if drafted {
		body["draft"] = true
	}
	writeJSON(w, withLintWarnings(body, lintFindings))
}

func (h *SkillsHandler) delete(w http.ResponseWriter, r *http.Request, id string) {
	orgID := verifiedOrganizationID(r)
	var result pgconn.CommandTag
	err := h.mutateSkill(r, "", "", func(tx registryDatabase) error {
		if err := ensureSkillCallerMayChange(r.Context(), tx, id, orgID, verifiedActorID(r), verifiedIsAdmin(r)); err != nil {
			return err
		}
		var err error
		result, err = tx.Exec(r.Context(), `DELETE FROM agent_skills WHERE id=$1 AND org_id=$2`, id, orgID)
		// EP-11 S5: with retention on the skill's bundles go with it (live, draft and kept ones),
		// so a deleted skill leaves no files behind (register D2-382).
		if err == nil && h.bundleRetention && result.RowsAffected() == 1 {
			err = skillbundle.DeleteSkillBundles(r.Context(), tx, orgID, id)
		}
		return err
	})
	if errors.Is(err, errSkillNotCallersToChange) {
		jsonErr(w, err.Error(), http.StatusForbidden)
		return
	}
	if errors.Is(err, errSpaceSkillDecisionRequired) {
		writeSpaceSkillDecisionRequired(w)
		return
	}
	if !writeSingleScopedMutation(w, "agent skill", result, err) {
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// ---------------------------------------------------------------------------
// Skill drafts  publish, discard, restore (EP-11 S5)
// ---------------------------------------------------------------------------

// acceptedSkillRowSQL says whether the skill is accepted: only an accepted skill has a draft.
const acceptedSkillRowSQL = `SELECT review_state = 'accepted' FROM agent_skills WHERE id=$1 AND org_id=$2`

// draftPatchSQL is a PATCH of an accepted skill's text while drafts are on. The text goes into the
// draft, opened from the live fields (session-core's version body, migrations 0108/0109) when there
// is none; enabled and sharing apply to the live row. No versioned field changes, so no version is
// cut. Pinned to session-core's draft test by TestDraftStatementsMatchTheSessionCoreTest.
const draftPatchSQL = `
	UPDATE agent_skills
	SET enabled=COALESCE($1, enabled), shared_with=COALESCE($4, shared_with), updated_at=$5,
	    draft = COALESCE(draft, agent_skill_version_body(agent_skills))
	        || CASE WHEN $2 <> '' THEN jsonb_build_object('description', $2::text) ELSE '{}'::jsonb END
	        || CASE WHEN $3 <> '' THEN jsonb_build_object('content', $3::text) ELSE '{}'::jsonb END
	        || CASE WHEN $8::jsonb IS NOT NULL THEN jsonb_build_object('settings', $8::jsonb) ELSE '{}'::jsonb END,
	    draft_updated_at = $5, draft_by = $9
	WHERE id=$6 AND org_id=$7 AND review_state = 'accepted'
`

// publishSkillSQL copies the draft into the live fields and clears it in one statement; session-core's
// trigger then cuts the version (when the digest moved) and the revision log records `edited`. The
// kind never changes. The bundle digest is copied only with retention on: without it the draft's
// bundle could not be kept, and the live bundle is the only one there is.
func publishSkillSQL(withBundle bool) string {
	bundle := ""
	if withBundle {
		bundle = `
	    bundle_sha256 = COALESCE(draft->>'bundle_sha256', bundle_sha256),`
	}
	return `
	UPDATE agent_skills
	SET name = COALESCE(draft->>'name', name),
	    description = COALESCE(draft->>'description', description),
	    content = COALESCE(draft->>'content', content),
	    trigger_keywords = COALESCE(draft->'trigger_keywords', trigger_keywords),
	    trigger_file_patterns = COALESCE(draft->'trigger_file_patterns', trigger_file_patterns),
	    tool_restrictions = COALESCE(draft->'tool_restrictions', tool_restrictions),
	    settings = COALESCE(draft->'settings', settings),` + bundle + `
	    draft = NULL, draft_updated_at = NULL, draft_by = '', updated_at = $3
	WHERE id = $1 AND org_id = $2 AND review_state = 'accepted' AND draft IS NOT NULL
	RETURNING version, bundle_sha256
`
}

// discardDraftSQL drops the draft; the live row is untouched.
const discardDraftSQL = `
	UPDATE agent_skills SET draft = NULL, draft_updated_at = NULL, draft_by = ''
	WHERE id = $1 AND org_id = $2 AND draft IS NOT NULL
`

// restoreDraftSQL writes an old version's body ($3) into the draft. Its bundle digest is kept only
// when $4 says the bundle is still held; otherwise the draft keeps the live one, so publishing a
// restore never points the skill at files that are gone.
const restoreDraftSQL = `
	UPDATE agent_skills
	SET draft = CASE WHEN $4 THEN $3::jsonb ELSE $3::jsonb || jsonb_build_object('bundle_sha256', bundle_sha256) END,
	    draft_updated_at = $5, draft_by = $6
	WHERE id = $1 AND org_id = $2 AND review_state = 'accepted'
`

// skillDraftBody is a draft or version body as the lint reads it.
type skillDraftBody struct {
	Name             string          `json:"name"`
	Description      string          `json:"description"`
	Content          string          `json:"content"`
	TriggerKeywords  []string        `json:"trigger_keywords"`
	ToolRestrictions []string        `json:"tool_restrictions"`
	Kind             string          `json:"kind"`
	Settings         json.RawMessage `json:"settings"`
	BundleSHA256     string          `json:"bundle_sha256"`
}

// Draft route refusals; each ends a mutation before its statement and is answered with the typed
// envelope.
var (
	errSkillNotFound        = errors.New("skill_not_found")
	errSkillNotPublished    = errors.New("skill_not_published")
	errSkillDraftNotFound   = errors.New("skill_draft_not_found")
	errSkillVersionNotFound = errors.New("skill_version_not_found")
	errSkillDraftNameTaken  = errors.New("skill_name_taken")
	errSkillDraftBundleGone = errors.New("skill_draft_bundle_missing")
	errSkillDraftUnreadable = errors.New("skill_draft_unreadable")
)

// writeSkillDraftError answers a draft route's failure with the typed envelope; details is always
// an object. Each refusal's code is its error text. It reports whether err was answered.
func writeSkillDraftError(w http.ResponseWriter, err error, findings []skilllint.Finding) bool {
	refuse := func(status int, refusal error, message string) bool {
		writeTypedError(w, status, refusal.Error(), message)
		return true
	}
	switch {
	case err == nil:
		return false
	case errors.Is(err, errSkillNotCallersToChange):
		writeTypedError(w, http.StatusForbidden, "forbidden", err.Error())
		return true
	case errors.Is(err, errSkillLintRefused):
		writeSkillLintRefusal(w, findings)
		return true
	case errors.Is(err, errSkillNotFound):
		return refuse(http.StatusNotFound, errSkillNotFound, "The skill was not found.")
	case errors.Is(err, errSkillNotPublished):
		return refuse(http.StatusConflict, errSkillNotPublished, "Only an accepted skill has a draft; decide on the proposal first.")
	case errors.Is(err, errSkillDraftNotFound):
		return refuse(http.StatusNotFound, errSkillDraftNotFound, "The skill has no draft.")
	case errors.Is(err, errSkillVersionNotFound):
		return refuse(http.StatusNotFound, errSkillVersionNotFound, "The skill has no such version.")
	case errors.Is(err, errSkillDraftNameTaken):
		return refuse(http.StatusConflict, errSkillDraftNameTaken, "Another skill already has the draft's name.")
	case errors.Is(err, errSkillDraftBundleGone):
		return refuse(http.StatusConflict, errSkillDraftBundleGone, "The draft names a bundle the skill no longer holds; upload it again or restore another version.")
	case errors.Is(err, errSkillDraftUnreadable):
		return refuse(http.StatusInternalServerError, errSkillDraftUnreadable, "The skill's draft could not be read.")
	}
	slog.Error("skill draft change failed", "error", err)
	writeTypedError(w, http.StatusInternalServerError, "database_unavailable", "The skill could not be changed.")
	return true
}

// lockAcceptedSkill runs the owner-or-admin check (which takes the row lock) and refuses a skill
// that is absent or not accepted.
func lockAcceptedSkill(r *http.Request, tx registryDatabase, id, orgID string) error {
	if err := ensureSkillCallerMayChange(r.Context(), tx, id, orgID, verifiedActorID(r), verifiedIsAdmin(r)); err != nil {
		return err
	}
	var accepted bool
	err := tx.QueryRow(r.Context(), acceptedSkillRowSQL, id, orgID).Scan(&accepted)
	if errors.Is(err, pgx.ErrNoRows) {
		return errSkillNotFound
	}
	if err != nil {
		return err
	}
	if !accepted {
		return errSkillNotPublished
	}
	return nil
}

// publish is POST /api/v1/skills/{id}/publish: the draft becomes the live text, one statement,
// and session-core's trigger cuts the new version. Owner-or-admin, as a PATCH. The draft is linted
// as the text it is about to make live. With retention on, the draft's bundle becomes live too.
func (h *SkillsHandler) publish(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeTypedError(w, http.StatusMethodNotAllowed, "method_not_allowed", "Only POST publishes a skill draft.")
		return
	}
	id := r.PathValue("id")
	if !mcpIdentifier.MatchString(id) {
		writeTypedError(w, http.StatusBadRequest, "invalid_skill_id", "The skill id is not valid.")
		return
	}
	orgID := verifiedOrganizationID(r)
	now := time.Now().UTC()
	var version int32
	var findings []skilllint.Finding
	err := h.mutateSkill(r, "", "draft_published", func(tx registryDatabase) error {
		if err := lockAcceptedSkill(r, tx, id, orgID); err != nil {
			return err
		}
		var raw []byte
		err := tx.QueryRow(r.Context(), `SELECT draft FROM agent_skills WHERE id=$1 AND org_id=$2`, id, orgID).Scan(&raw)
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
			return err
		}
		if len(raw) == 0 {
			return errSkillDraftNotFound
		}
		var draft skillDraftBody
		if err := json.Unmarshal(raw, &draft); err != nil {
			return errSkillDraftUnreadable
		}
		input := skilllint.Input{
			Name: draft.Name, Description: draft.Description, Content: draft.Content,
			TriggerKeywords: draft.TriggerKeywords, ToolRestrictions: draft.ToolRestrictions,
			Kind: draft.Kind, Settings: decodeSettings(draft.Settings),
		}
		if kindFindings := kindRuleRefusal(input); len(kindFindings) > 0 {
			findings = kindFindings
			return errSkillLintRefused
		}
		lintFindings, refused := lintSkill(r.Context(), h.lintMode, input)
		findings = lintFindings
		if refused {
			return errSkillLintRefused
		}
		var bundle string
		err = tx.QueryRow(r.Context(), publishSkillSQL(h.bundleRetention), id, orgID, now).Scan(&version, &bundle)
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23505" {
			return errSkillDraftNameTaken
		}
		if errors.Is(err, pgx.ErrNoRows) {
			return errSkillDraftNotFound
		}
		if err != nil {
			return err
		}
		if !h.bundleRetention {
			return nil
		}
		_, actorID := skillActor(r)
		if err := skillbundle.PublishDraftBundle(r.Context(), tx, orgID, id, actorID, bundle); err != nil {
			if errors.Is(err, skillbundle.ErrDraftBundleMissing) {
				return errSkillDraftBundleGone
			}
			return err
		}
		return nil
	})
	if writeSkillDraftError(w, err, findings) {
		return
	}
	writeJSON(w, withLintWarnings(map[string]any{"id": id, "version": version, "published": true, "updated_at": now}, findings))
}

// discardDraft is DELETE /api/v1/skills/{id}/draft: the draft goes and the live text stays.
// Owner-or-admin, as a PATCH. With retention on, the draft's own bundle upload goes too.
func (h *SkillsHandler) discardDraft(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodDelete {
		writeTypedError(w, http.StatusMethodNotAllowed, "method_not_allowed", "Only DELETE discards a skill draft.")
		return
	}
	id := r.PathValue("id")
	if !mcpIdentifier.MatchString(id) {
		writeTypedError(w, http.StatusBadRequest, "invalid_skill_id", "The skill id is not valid.")
		return
	}
	orgID := verifiedOrganizationID(r)
	err := h.mutateSkill(r, "", "draft_discarded", func(tx registryDatabase) error {
		if err := ensureSkillCallerMayChange(r.Context(), tx, id, orgID, verifiedActorID(r), verifiedIsAdmin(r)); err != nil {
			return err
		}
		result, err := tx.Exec(r.Context(), discardDraftSQL, id, orgID)
		if err != nil {
			return err
		}
		if result.RowsAffected() == 0 {
			return errSkillDraftNotFound
		}
		if h.bundleRetention {
			return skillbundle.DiscardDraftBundle(r.Context(), tx, orgID, id)
		}
		return nil
	})
	if writeSkillDraftError(w, err, nil) {
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// restore is POST /api/v1/skills/{id}/versions/{version}/restore: that version's body becomes the
// skill's draft. It is never live by itself: publishing it is a second, explicit step that cuts a
// NEW version (EP-11 D2: nothing re-points to an old version). Owner-or-admin, as a PATCH. The
// old bundle comes back only with retention on and while the skill still holds it.
func (h *SkillsHandler) restore(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeTypedError(w, http.StatusMethodNotAllowed, "method_not_allowed", "Only POST restores a skill version.")
		return
	}
	id := r.PathValue("id")
	if !mcpIdentifier.MatchString(id) {
		writeTypedError(w, http.StatusBadRequest, "invalid_skill_id", "The skill id is not valid.")
		return
	}
	version, err := strconv.Atoi(r.PathValue("version"))
	if err != nil || version < 1 {
		writeTypedError(w, http.StatusBadRequest, "invalid_version", "The version must be a whole number, 1 or more.")
		return
	}
	orgID := verifiedOrganizationID(r)
	now := time.Now().UTC()
	bundleRestored := false
	err = h.mutateSkill(r, "", "draft_restored_from:v"+strconv.Itoa(version), func(tx registryDatabase) error {
		if err := lockAcceptedSkill(r, tx, id, orgID); err != nil {
			return err
		}
		var body []byte
		var bundle string
		err := tx.QueryRow(r.Context(), `
			SELECT body, bundle_sha256 FROM agent_skill_versions
			WHERE org_id = $1 AND skill_id = $2 AND version = $3
		`, orgID, id, version).Scan(&body, &bundle)
		if errors.Is(err, pgx.ErrNoRows) {
			return errSkillVersionNotFound
		}
		if err != nil {
			return err
		}
		if h.bundleRetention {
			held := bundle == ""
			if !held {
				if held, err = skillbundle.HeldManifest(r.Context(), tx, orgID, id, bundle); err != nil {
					return err
				}
			}
			bundleRestored = held
		}
		_, err = tx.Exec(r.Context(), restoreDraftSQL, id, orgID, body, bundleRestored, now, verifiedActorID(r))
		return err
	})
	if writeSkillDraftError(w, err, nil) {
		return
	}
	writeJSON(w, map[string]any{"id": id, "draft": true, "restored_from": version, "bundle_restored": bundleRestored, "updated_at": now})
}

// ---------------------------------------------------------------------------
// Skill versions  GET /api/v1/skills/{id}/versions (EP-11 S2)
// ---------------------------------------------------------------------------

const skillVersionsAPIEnv = "SKILL_VERSIONS_API"

// SkillVersionsAPIFromLookup reads SKILL_VERSIONS_API strictly: only the literal "true" turns the
// version reads on (the history route, version and digest on skill reads, and the bundle digest
// stamp), so a typo or a stray "1" leaves everything as it was. session-core reads the same name
// for its SkillVersionService.
func SkillVersionsAPIFromLookup(lookup func(string) string) bool {
	return lookup(skillVersionsAPIEnv) == "true"
}

// A history page when the caller names none, and the most one page returns.
const (
	defaultSkillVersionsPage = 20
	maxSkillVersionsPage     = 100
)

// skillVisibleSQL is get's visibility rule as a yes/no: org skills, the caller's own, accepted
// skills shared with the caller, and for an admin any shared accepted skill. Pinned to get's
// statement by TestSkillVersionsRouteUsesTheGetVisibility.
const skillVisibleSQL = `
	SELECT EXISTS (SELECT 1 FROM agent_skills
		WHERE id = $1 AND org_id = $2
		  AND (scope = 'org' OR owner_user_id = $3
		       OR (review_state = 'accepted' AND (shared_with ? $3
		           OR ($4 AND jsonb_array_length(shared_with) > 0)))))
`

// skillVersionBody is the body a version row holds (session-core migration 0108,
// agent_skill_version_body). kind and settings are not shown until EP-11 S6 gives them meaning.
type skillVersionBody struct {
	Name                string   `json:"name"`
	Description         string   `json:"description"`
	Content             string   `json:"content"`
	TriggerKeywords     []string `json:"trigger_keywords"`
	TriggerFilePatterns []string `json:"trigger_file_patterns"`
	ToolRestrictions    []string `json:"tool_restrictions"`
}

type skillVersionView struct {
	Version             int32     `json:"version"`
	Digest              string    `json:"digest"`
	Name                string    `json:"name"`
	Description         string    `json:"description"`
	Content             string    `json:"content"`
	TriggerKeywords     []string  `json:"trigger_keywords"`
	TriggerFilePatterns []string  `json:"trigger_file_patterns"`
	ToolRestrictions    []string  `json:"tool_restrictions"`
	BundleSHA256        string    `json:"bundle_sha256,omitempty"`
	ActorKind           string    `json:"actor_kind"`
	ActorID             string    `json:"actor_id,omitempty"`
	CreatedAt           time.Time `json:"created_at"`
}

// writeSkillVersionsError answers with the typed envelope; details is always an object.
func writeSkillVersionsError(w http.ResponseWriter, status int, code, message string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]any{"error": map[string]any{
		"code": code, "message": message, "details": map[string]any{},
	}})
}

// versions is GET /api/v1/skills/{id}/versions: the skill's immutable versions, newest first,
// with their text, for the Skills page's history. Readable by whoever may read the skill (get's
// rule); a skill the caller cannot see is a 404, as get answers. Versions are written only by
// session-core's trigger (EP-11 D10), so this route never writes. Query: limit (default 20, at
// most 100) and before_version (paging; 0 starts at the newest).
func (h *SkillsHandler) versions(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeSkillVersionsError(w, http.StatusMethodNotAllowed, "method_not_allowed", "Only GET reads skill versions.")
		return
	}
	id := r.PathValue("id")
	if !mcpIdentifier.MatchString(id) {
		writeSkillVersionsError(w, http.StatusBadRequest, "invalid_skill_id", "The skill id is not valid.")
		return
	}
	limit, before := defaultSkillVersionsPage, 0
	if raw := r.URL.Query().Get("limit"); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil || parsed < 1 {
			writeSkillVersionsError(w, http.StatusBadRequest, "invalid_limit", "limit must be a positive whole number.")
			return
		}
		limit = min(parsed, maxSkillVersionsPage)
	}
	if raw := r.URL.Query().Get("before_version"); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil || parsed < 0 {
			writeSkillVersionsError(w, http.StatusBadRequest, "invalid_before_version", "before_version must be a whole number, 0 or more.")
			return
		}
		before = parsed
	}
	orgID := verifiedOrganizationID(r)
	var visible bool
	if err := h.pool.QueryRow(r.Context(), skillVisibleSQL, id, orgID, verifiedActorID(r), verifiedIsAdmin(r)).Scan(&visible); err != nil {
		slog.Error("read skill visibility failed", "error", err)
		writeSkillVersionsError(w, http.StatusInternalServerError, "database_unavailable", "Skill versions could not be read.")
		return
	}
	if !visible {
		writeSkillVersionsError(w, http.StatusNotFound, "skill_not_found", "The skill was not found.")
		return
	}
	// One more than the page, to say whether older versions remain.
	rows, err := h.pool.Query(r.Context(), `
		SELECT version, digest, body, bundle_sha256, actor_kind, actor_id, created_at
		FROM agent_skill_versions
		WHERE org_id = $1 AND skill_id = $2 AND ($3::int = 0 OR version < $3::int)
		ORDER BY version DESC
		LIMIT $4
	`, orgID, id, before, limit+1)
	if err != nil {
		slog.Error("list skill versions failed", "error", err)
		writeSkillVersionsError(w, http.StatusInternalServerError, "database_unavailable", "Skill versions could not be read.")
		return
	}
	defer rows.Close()
	versions := []skillVersionView{}
	for rows.Next() {
		var view skillVersionView
		var body skillVersionBody
		if err := rows.Scan(&view.Version, &view.Digest, &body, &view.BundleSHA256,
			&view.ActorKind, &view.ActorID, &view.CreatedAt); err != nil {
			slog.Error("scan skill version failed", "error", err)
			writeSkillVersionsError(w, http.StatusInternalServerError, "database_unavailable", "Skill versions could not be read.")
			return
		}
		view.Name, view.Description, view.Content = body.Name, body.Description, body.Content
		view.TriggerKeywords, view.TriggerFilePatterns, view.ToolRestrictions =
			nonNilStrings(body.TriggerKeywords), nonNilStrings(body.TriggerFilePatterns), nonNilStrings(body.ToolRestrictions)
		versions = append(versions, view)
	}
	if err := rows.Err(); err != nil {
		slog.Error("list skill versions failed", "error", err)
		writeSkillVersionsError(w, http.StatusInternalServerError, "database_unavailable", "Skill versions could not be read.")
		return
	}
	hasMore := len(versions) > limit
	if hasMore {
		versions = versions[:limit]
	}
	writeJSON(w, map[string]any{"versions": versions, "has_more": hasMore})
}

// nonNilStrings keeps a list a JSON array, never null.
func nonNilStrings(values []string) []string {
	if values == nil {
		return []string{}
	}
	return values
}

// ---------------------------------------------------------------------------
// SkillBundlesHandler  /api/v1/skills/{id}/bundle (WS19-S6)
// ---------------------------------------------------------------------------

const skillBundlesAPIEnv = "SKILL_BUNDLES_ENABLED"

// maxSkillBundleBodyBytes bounds an upload before it is read: the caps on the unpacked files
// and room for the zip headers of the most entries a bundle may hold.
const maxSkillBundleBodyBytes = skillbundle.MaxTotalBytes + 256<<10

// SkillBundlesAPIFromLookup reads SKILL_BUNDLES_ENABLED strictly: only the literal "true"
// turns the surface on, so a typo or a stray "1" leaves it dark.
func SkillBundlesAPIFromLookup(lookup func(string) string) bool {
	return lookup(skillBundlesAPIEnv) == "true"
}

// SkillBundlesHandler serves PUT, GET and DELETE on /api/v1/skills/{id}/bundle. Every route is
// an org admin's: uploading a bundle puts executable scripts in front of the agent, so the
// surface is org configuration, never a member's. While the switch is off every route is a 404
// before anything is read or written.
type SkillBundlesHandler struct {
	store   *skillbundle.Store
	enabled bool
	// db and lintMode check a bundle's SKILL.md name against its skill (EP-11 S4); while the
	// lint is off the skill is not read.
	db       skillbundle.DB
	lintMode skilllint.Mode
}

// NewSkillBundlesHandler constructs the handler; enabled comes from SkillBundlesAPIFromLookup.
func NewSkillBundlesHandler(db skillbundle.DB, enabled bool) *SkillBundlesHandler {
	// EP-11 S2: with the versions API on, an upload or a delete also stamps the skill's bundle
	// digest, which cuts a new skill version.
	// EP-11 S5: with retention on a replaced bundle is kept while a version names it, and with
	// drafts on too an upload to an accepted skill is its draft's bundle.
	store := skillbundle.NewStore(db).WithSkillVersions(SkillVersionsAPIFromLookup(os.Getenv)).
		WithRetention(skillbundle.RetentionFromLookup(os.Getenv)).WithDrafts(DraftPublishFromLookup(os.Getenv))
	return &SkillBundlesHandler{store: store, enabled: enabled, db: db, lintMode: skilllint.ModeFromLookup(os.Getenv)}
}

// Register mounts the bundle route. Its more specific pattern takes precedence over the
// /api/v1/skills/ subtree that SkillsHandler serves.
func (h *SkillBundlesHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/skills/{id}/bundle", h.serve)
}

type skillBundleFileView struct {
	Path       string `json:"path"`
	SHA256     string `json:"sha256"`
	SizeBytes  int64  `json:"size_bytes"`
	Executable bool   `json:"executable"`
	Binary     bool   `json:"binary"`
}

type skillBundleManifestView struct {
	SkillID        string                `json:"skill_id"`
	ManifestSHA256 string                `json:"manifest_sha256"`
	Files          []skillBundleFileView `json:"files"`
}

type skillBundleSummaryView struct {
	SkillID        string    `json:"skill_id"`
	ManifestSHA256 string    `json:"manifest_sha256"`
	FileCount      int       `json:"file_count"`
	TotalBytes     int64     `json:"total_bytes"`
	CreatedBy      string    `json:"created_by"`
	CreatedAt      time.Time `json:"created_at"`
	Replaced       bool      `json:"replaced"`
	// Draft is true when the upload became the skill's draft bundle (EP-11 S5), live once the
	// draft is published; absent otherwise, so with retention off a body is as it was.
	Draft bool `json:"draft,omitempty"`
	// Warnings are the skill lint findings of an upload that went ahead; absent while the
	// lint is off.
	Warnings []skilllint.Finding `json:"warnings,omitempty"`
}

func (h *SkillBundlesHandler) serve(w http.ResponseWriter, r *http.Request) {
	if !h.enabled {
		http.NotFound(w, r)
		return
	}
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	if !verifiedIsAdmin(r) {
		jsonErr(w, "only an admin may manage skill bundles", http.StatusForbidden)
		return
	}
	skillID := r.PathValue("id")
	if !mcpIdentifier.MatchString(skillID) {
		jsonErr(w, "invalid skill id", http.StatusBadRequest)
		return
	}
	switch r.Method {
	case http.MethodGet:
		h.get(w, r, principal.OrganizationID, skillID)
	case http.MethodPut:
		if !skillBundleWriter(w, principal) {
			return
		}
		h.put(w, r, principal, skillID)
	case http.MethodDelete:
		if !skillBundleWriter(w, principal) {
			return
		}
		h.delete(w, r, principal, skillID)
	default:
		jsonErr(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

// skillBundleWriter answers the refusals a write needs: a person holding the capability write
// scope. A workload identity, even one with admin scopes, may not put a bundle in place.
func skillBundleWriter(w http.ResponseWriter, principal authctx.Principal) bool {
	if principal.PrincipalType != "user" {
		jsonErr(w, "only a person may change a skill bundle", http.StatusForbidden)
		return false
	}
	if !principal.HasScope(authz.WriteScope) {
		jsonErr(w, "capability write scope required", http.StatusForbidden)
		return false
	}
	return true
}

// get is GET /api/v1/skills/{id}/bundle: the manifest of the org's bundle for the skill. The
// bytes are read by digest through the RPC, so the manifest carries no file body.
func (h *SkillBundlesHandler) get(w http.ResponseWriter, r *http.Request, orgID, skillID string) {
	manifest, err := h.store.Manifest(r.Context(), orgID, skillID)
	if errors.Is(err, skillbundle.ErrNotFound) {
		jsonErr(w, "skill bundle not found", http.StatusNotFound)
		return
	}
	if err != nil {
		slog.Error("read skill bundle manifest failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	files := make([]skillBundleFileView, 0, len(manifest.Files))
	for _, file := range manifest.Files {
		files = append(files, skillBundleFileView{Path: file.Path, SHA256: file.SHA256, SizeBytes: file.Size, Executable: file.Executable, Binary: file.Binary})
	}
	writeJSON(w, skillBundleManifestView{SkillID: skillID, ManifestSHA256: manifest.ManifestSHA256, Files: files})
}

// put is PUT /api/v1/skills/{id}/bundle: the body is the bundle zip. A bundle that fails any
// check is refused with its closed code and nothing is stored. A new upload replaces the
// skill's bundle, and answers 201 for a first upload and 200 for a replacement.
func (h *SkillBundlesHandler) put(w http.ResponseWriter, r *http.Request, principal authctx.Principal, skillID string) {
	r.Body = http.MaxBytesReader(w, r.Body, maxSkillBundleBodyBytes)
	body, err := io.ReadAll(r.Body)
	if err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			jsonErr(w, "bundle_too_large", http.StatusRequestEntityTooLarge)
			return
		}
		jsonErr(w, "invalid request body", http.StatusBadRequest)
		return
	}
	bundle, err := skillbundle.Parse(body)
	if err == nil && skillbundle.DeckTemplatesFromLookup(os.Getenv) {
		// EP-5 S6: a deck template's theme, master and logo are checked before anything is stored.
		err = skillbundle.ValidateDeckTemplate(bundle)
	}
	if err != nil {
		var refusal *skillbundle.RefusalError
		if errors.As(err, &refusal) {
			jsonErr(w, refusal.Code, http.StatusUnprocessableEntity)
			return
		}
		jsonErr(w, "invalid skill bundle", http.StatusUnprocessableEntity)
		return
	}
	lintFindings, lintRefused, err := h.lintFrontMatter(r.Context(), principal.OrganizationID, skillID, bundle)
	if err != nil {
		slog.Error("read skill for bundle lint failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	if lintRefused {
		writeSkillLintRefusal(w, lintFindings)
		return
	}
	result, err := h.store.Put(r.Context(), principal.OrganizationID, skillID, principal.ActorID, bundle)
	if err != nil {
		slog.Error("write skill bundle failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	status := http.StatusCreated
	if result.Replaced {
		status = http.StatusOK
	}
	summary := result.Summary
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(skillBundleSummaryView{
		SkillID: summary.SkillID, ManifestSHA256: summary.ManifestSHA256, FileCount: summary.FileCount,
		TotalBytes: summary.TotalBytes, CreatedBy: summary.CreatedBy, CreatedAt: summary.CreatedAt, Replaced: result.Replaced,
		Draft: result.Draft, Warnings: lintFindings,
	})
}

// lintFrontMatter checks that the bundle's SKILL.md names the skill it is uploaded to. Only
// the front matter rule applies here: the skill's own text was linted when it was written,
// and an upload must not be refused for a row it does not change. A skill that is not there
// is not checked.
func (h *SkillBundlesHandler) lintFrontMatter(ctx context.Context, orgID, skillID string, bundle skillbundle.Bundle) ([]skilllint.Finding, bool, error) {
	if h.lintMode == skilllint.ModeOff {
		return nil, false, nil
	}
	var name string
	err := h.db.QueryRow(ctx, `SELECT name FROM agent_skills WHERE id=$1 AND org_id=$2`, skillID, orgID).Scan(&name)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	frontMatterName := skillbundle.FrontMatterName(bundle)
	var findings []skilllint.Finding
	for _, finding := range skilllint.Lint(skilllint.Input{Name: name, FrontmatterName: &frontMatterName}) {
		if finding.Code == skilllint.CodeFrontmatterNameMismatch {
			findings = append(findings, finding)
		}
	}
	skilllint.Count(ctx, h.lintMode, findings)
	return findings, h.lintMode == skilllint.ModeEnforce && skilllint.HasError(findings), nil
}

// delete is DELETE /api/v1/skills/{id}/bundle: removes the org's bundle; its files cascade and
// blobs no other bundle names are dropped.
func (h *SkillBundlesHandler) delete(w http.ResponseWriter, r *http.Request, principal authctx.Principal, skillID string) {
	err := h.store.Delete(r.Context(), principal.OrganizationID, skillID, principal.ActorID)
	if errors.Is(err, skillbundle.ErrNotFound) {
		jsonErr(w, "skill bundle not found", http.StatusNotFound)
		return
	}
	if err != nil {
		slog.Error("delete skill bundle failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
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
	// orgExtensions is the org's extension rule, read only when orgExtensionsEnforced (WS25-S9). With it off the
	// listing shows every server the registry holds.
	orgExtensions         policy.OrgExtensionSource
	orgExtensionsEnforced bool
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
	// AnnotationTrusted reports whether the registrant chose to believe this
	// server's read-only annotations (see mcpServerConfig.AnnotationTrust).
	AnnotationTrusted bool      `json:"annotation_trusted"`
	CreatedAt         time.Time `json:"created_at"`
	UpdatedAt         time.Time `json:"updated_at"`
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
	// AnnotationTrust says whether the server's own `readOnlyHint` is believed
	// when a class is derived for its tools (WS05-S2). A server's annotations are
	// claims by a third party, so the default, and the meaning of absent, is
	// untrusted: a read-only claim then derives as an external write. Nil is
	// "not stated" and keeps what is stored; only an explicit true trusts the
	// server, and for a server shared beyond one user that takes an admin.
	AnnotationTrust *bool `json:"annotation_trust,omitempty"`
}

// annotationsTrusted is the one reading of the tri-state: only an explicit true
// is trust.
func (c mcpServerConfig) annotationsTrusted() bool {
	return c.AnnotationTrust != nil && *c.AnnotationTrust
}

// mcpServerShared reports whether anyone besides the owner can use a server: a
// server of any scope but `user` is shared, and a user-scoped one is shared as
// soon as shared_with names someone. Trusting a server's annotations is an
// admin's call for a shared server and the owner's own risk otherwise.
func mcpServerShared(scope string, config mcpServerConfig) bool {
	return scope != "user" || len(config.SharedWith) > 0
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
// WithOrgExtensionPolicy attaches the org's extension rule to the server listing (WS25-S9). It applies only when
// enabled is true, which main wires from ORG_EXTENSION_ALLOWLIST, so the listing is what it was while that is off.
func (h *MCPHandler) WithOrgExtensionPolicy(source policy.OrgExtensionSource, enabled bool) *MCPHandler {
	h.orgExtensions = source
	h.orgExtensionsEnforced = enabled && source != nil
	return h
}

// orgExtensionReader reads an org's extension rule from the agent policy the policy API serves (WS25-S9), through the
// same loader, so the listing and the signed gate cannot disagree about what the org allows.
type orgExtensionReader struct {
	pool registryDatabase
}

// NewOrgExtensionReader returns the rule source the signed gate and the MCP listing share.
func NewOrgExtensionReader(pool registryDatabase) policy.OrgExtensionSource {
	return orgExtensionReader{pool: pool}
}

// OrgExtensionRule implements policy.OrgExtensionSource.
func (r orgExtensionReader) OrgExtensionRule(ctx context.Context, orgID string) (policy.OrgExtensionRule, error) {
	handler := &OrgAgentPolicyHandler{pool: r.pool}
	row, err := handler.load(ctx, orgID)
	if err != nil {
		return policy.OrgExtensionRule{}, err
	}
	return policy.OrgExtensionRuleFromSettings(row.Settings), nil
}

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
	// mcpIdentifier allows `__`, but a tool is addressed as mcp__<server>__<tool>
	// and anything that parses that name splits on the first `__`: a server id
	// holding one makes two different (server, tool) pairs share a name.
	if !mcpIdentifier.MatchString(input.ID) || strings.Contains(input.ID, "__") ||
		!validMCPDisplayName(input.Name) || len(input.Description) > 2_000 {
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
	server.AnnotationTrusted = config.annotationsTrusted()
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
		// Per-tool registry rows (WS05-S2): the list, and a person's approval of
		// a tool whose definition changed.
		if id, tool, ok := parseMCPToolApprovePath(rest); ok {
			switch r.Method {
			case http.MethodPost:
				h.approveTool(w, r, id, tool)
			default:
				http.NotFound(w, r)
			}
			return
		}
		if id := strings.TrimSuffix(rest, "/tools"); id != rest {
			switch r.Method {
			case http.MethodGet:
				h.listTools(w, r, id)
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
	// Tool observation: the gateway reports what it discovered. It can create or
	// refresh an observation and nothing else (see observeToolsInternal).
	mux.HandleFunc("/api/v1/internal/mcp/tools/observe", h.observeToolsInternal)
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
		// With the org's extension allow-list on (WS25-S9), a server the org has not approved is not listed.
		var rule policy.OrgExtensionRule
		if h.orgExtensionsEnforced {
			loaded, err := h.orgExtensions.OrgExtensionRule(r.Context(), orgID)
			if err != nil {
				slog.Error("load org extension rule failed", "error", err)
				jsonErr(w, "database unavailable", http.StatusInternalServerError)
				return
			}
			rule = loaded
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
		// Initialized, not `var servers []mcpServerRow`: a nil slice marshals as
		// `null`, and an org with no MCP servers yet therefore answered
		// `{"servers": null}`. model-gateway decodes this into
		// `#[serde(default)] Vec<CatalogMcpServer>`, and serde's `default` covers
		// a MISSING key, never an explicit null — so hydration failed with
		// "error decoding response body" and every /api/v1/mcp/servers call
		// returned 503 "capability registry is unavailable". The first server
		// could never be connected, because having none was the failure.
		servers := []mcpServerRow{}
		for rows.Next() {
			var s mcpServerRow
			if err := rows.Scan(&s.ID, &s.OrgID, &s.Name, &s.Description, &s.EndpointURL,
				&s.Transport, &s.AuthKind, &s.ConfigJSON, &s.Scope, &s.Enabled,
				&s.RolloutState, &s.RiskLevel, &s.CreatedAt, &s.UpdatedAt); err != nil {
				slog.Error("scan MCP server failed", "error", err)
				jsonErr(w, "database unavailable", http.StatusInternalServerError)
				return
			}
			if h.orgExtensionsEnforced && !rule.AllowsServer(s.ID) {
				continue
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
		// Trusting a server's read-only claims lowers the class its tools derive
		// to, so for a server others use it is an admin's call, taken from the
		// signed scopes. A server only its owner uses is the owner's own risk; a
		// user-scoped server that names someone in shared_with is not that.
		if config.annotationsTrusted() && mcpServerShared(s.Scope, config) && !verifiedIsAdmin(r) {
			jsonErr(w, "only an admin can trust the annotations of a server shared beyond one user", http.StatusForbidden)
			return
		}
		now := time.Now().UTC()
		cfgJSON, err := json.Marshal(config)
		if err != nil {
			jsonErr(w, "invalid MCP registration", http.StatusBadRequest)
			return
		}
		// A registration that does not mention annotation_trust (the gateway's
		// write-through never does) keeps the stored choice, but only while the scope
		// and the endpoint are the ones it was made for: pointing the record at
		// another server, or widening a user-scoped server to a shared scope (which
		// needs an admin to trust), drops the trust instead of carrying it over.
		// Sharing a user-scoped server with other users is that same widening: a
		// non-admin whose registration names someone in shared_with on a server that
		// had no one in it does not carry the owner's trust along ($15).
		nonAdminWidening := s.Scope == "user" && len(config.SharedWith) > 0 && !verifiedIsAdmin(r)
		//
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
			WITH registered AS (
			INSERT INTO mcp_servers (id, org_id, name, description, endpoint_url, transport, auth_kind,
			    config_json, scope, enabled, rollout_state, risk_level, created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
			ON CONFLICT (org_id, name) WHERE deleted_at IS NULL DO UPDATE SET
			    description=$4, endpoint_url=$5, transport=$6, auth_kind=$7,
			    config_json=CASE
			        WHEN $8::jsonb ? 'annotation_trust' THEN $8::jsonb
			        WHEN mcp_servers.endpoint_url = $5 AND mcp_servers.scope = $9 AND mcp_servers.config_json ? 'annotation_trust'
			             AND (NOT $15::boolean OR CASE WHEN jsonb_typeof(mcp_servers.config_json->'shared_with') = 'array'
			                  THEN jsonb_array_length(mcp_servers.config_json->'shared_with') ELSE 0 END > 0)
			            THEN $8::jsonb || jsonb_build_object('annotation_trust', mcp_servers.config_json->'annotation_trust')
			        ELSE $8::jsonb
			    END,
			    scope=$9, enabled=$10, rollout_state=$11,
			    risk_level=$12, updated_at=$14
			RETURNING id
			), cascade AS (
			    -- A registration switches a tool off, in the same statement, when it
			    -- leaves the server disabled or outside stable/canary rollout, or when
			    -- the tool is no longer on the allowlist it registers ($16). A tool
			    -- comes back through the next observation, which checks the same three.
			    UPDATE capabilities SET enabled=false, availability_state='unavailable',
			        availability_reason_code='capability_disabled',
			        availability_reason='The MCP server is disabled or was removed.',
			        execution_mode='unavailable', updated_at=now()
			    WHERE kind='mcp_tool' AND org_id=$2 AND deleted_at IS NULL
			      AND id IN (SELECT ct.capability_id FROM capability_tools ct
			                 WHERE ct.org_id=$2 AND ct.server_id=(SELECT id FROM registered)
			                   AND (NOT $10::boolean OR $11::text NOT IN ('stable', 'canary')
			                        OR NOT (ct.tool_name = ANY($16::text[]))))
			)
			SELECT id FROM registered
		`, s.ID, s.OrgID, s.Name, s.Description, s.EndpointURL, s.Transport, s.AuthKind,
			cfgJSON, s.Scope, s.Enabled, s.RolloutState, s.RiskLevel, now, now,
			nonAdminWidening, config.ToolAllowlist).Scan(&persistedID)
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
		wasShared := mcpServerShared(scope, config)
		config.SharedWith = append([]string(nil), (*update.SharedWith)...)
		// A trust choice made while the server was only its owner's does not follow
		// it to other users unless an admin is the one sharing it: a non-admin who
		// widens shared_with from empty to non-empty leaves it untrusted.
		if config.annotationsTrusted() && !wasShared && mcpServerShared(scope, config) && !verifiedIsAdmin(r) {
			config.AnnotationTrust = nil
		}
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
	query := `
		UPDATE mcp_servers
		SET enabled=COALESCE($1, enabled), rollout_state=COALESCE(NULLIF($2, ''), rollout_state),
		    config_json=COALESCE($3, config_json), updated_at=$4
		WHERE id=$5 AND org_id=$6 AND deleted_at IS NULL
	`
	// Taking the server out of service takes its tools out with it, in the same
	// statement, so no tool row outlives the switch that turned its server off.
	if (update.Enabled != nil && !*update.Enabled) || update.RolloutState == "quarantine" || update.RolloutState == "deprecated" {
		query = withMCPToolCascade(query, "$6", "$5")
	}
	result, err := h.pool.Exec(r.Context(), query, enabled, update.RolloutState, configJSON, now, id, orgID)
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
	result, err := h.pool.Exec(r.Context(), withMCPToolCascade(
		`UPDATE mcp_servers SET deleted_at=$1, updated_at=$1 WHERE id=$2 AND org_id=$3 AND deleted_at IS NULL`,
		"$3", "$2",
	), now, id, orgID)
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
	// documentPII turns on the document_pii catalogue key (E3.6-8): creating a
	// policy of that kind and reading the effective org default. Off by default.
	documentPII bool
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

// WithDocumentPIIPolicy switches the document_pii catalogue key on (E3.6-8,
// DOCUMENT_PII_POLICY). While it is off a document_pii policy cannot be created
// and the effective-default route answers 404.
func (h *SafetyHandler) WithDocumentPIIPolicy(enabled bool) *SafetyHandler {
	h.documentPII = enabled
	return h
}

// Register mounts routes.
func (h *SafetyHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/safety", h.listOrCreate)
	mux.HandleFunc("/api/v1/safety/", func(w http.ResponseWriter, r *http.Request) {
		id := r.URL.Path[len("/api/v1/safety/"):]
		if id == documentPIIEffectiveSuffix {
			if r.Method != http.MethodGet {
				http.NotFound(w, r)
				return
			}
			h.documentPIIEffective(w, r)
			return
		}
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
		// Initialized, not `var policies []safetyPolicyRow`: a nil slice
		// marshals as JSON `null`, and an org with no safety policies yet
		// (every org, until one is explicitly configured — confirmed zero
		// rows in `safety_policies` for this deployment) answered
		// `{"policies": null}`. model-gateway's PII-redaction lookup
		// (moderation.rs, F-14 chat-parity audit follow-up) deserializes this
		// response into a non-optional `Vec<SafetyPolicy>` and a bare `null`
		// fails that parse outright — "malformed", not "empty" — so the
		// lookup fell back to fail-closed redaction on every single call,
		// masking the real answer ("this org has no PII policy configured")
		// behind a decode error. Same bug class as the MCP-registry null
		// list this package already fixed once (see registry_apis.go's MCP
		// servers handler) — a nil Go slice must never reach an API response.
		policies := []safetyPolicyRow{}
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
		isDocumentPII := isDocumentPIIKind(p.Kind)
		if isDocumentPII {
			// The E3.6-8 catalogue key has a closed schema and one row per org.
			normalized, status, message := h.prepareDocumentPII(&p)
			if status != 0 {
				jsonErr(w, message, status)
				return
			}
			cfgJSON = normalized
		}
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
			if isDocumentPII && isUniqueViolation(err) {
				jsonErr(w, "a document_pii policy already exists for this organization", http.StatusConflict)
				return
			}
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
