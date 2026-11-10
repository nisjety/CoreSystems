package plugins

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"sort"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// DB is the statement surface the loader needs. A pgx.Tx satisfies it, and so does the registry's
// database interface in the api package.
type DB interface {
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
	Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error)
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

// ErrNameConflict is what a ChildStore returns when the child's name is already taken in the org. The
// loader turns it into a CodeComponentConflict refusal that rolls the whole install back.
var ErrNameConflict = errors.New("a capability with this name already exists in the org")

// Uninstall reasons, recorded in the audit row.
const (
	// ReasonUninstall is an admin removing a plugin.
	ReasonUninstall = "uninstall"
	// ReasonKilled is a plugin removed because its manifest digest was killed.
	ReasonKilled = "killed"
)

// Caller is the verified principal behind an install or an uninstall. The api package fills it from the
// signed principal, never from the request body.
type Caller struct {
	// ID is the actor id, recorded in the audit rows.
	ID string
	// IsPerson is true for a user principal; a service or an API key never installs a plugin.
	IsPerson bool
	// IsAdmin is the org admin role check, taken from the signed scopes.
	IsAdmin bool
	// ZeroDataRetention is the signed ZDR posture; true refuses an install.
	ZeroDataRetention bool
	// RetentionPolicyKnown is true when the token carried an explicit retention posture. An absent
	// posture is treated like ZDR.
	RetentionPolicyKnown bool
}

// Plugin is the registered package row the loader works on.
type Plugin struct {
	// ID is the plugin_packages id.
	ID string
	// OrgID is the org that registered the plugin; every child is created in it.
	OrgID string
	// Name is the registered name; the manifest must match it.
	Name string
	// Version is the registered version; the manifest must match it.
	Version string
	// Manifest is manifest_json as stored.
	Manifest json.RawMessage
	// ManifestDigest is the manifest_digest the registration computed; empty for an unpinned plugin.
	ManifestDigest string
	// Installed is true when installed_at is set.
	Installed bool
}

// SkillWrite is a skill or a command (an authored template) the store creates in the org.
type SkillWrite struct {
	// OrgID is the plugin's org; the child has org scope.
	OrgID string
	// ActorID is the admin who consented.
	ActorID string
	// Kind is skill or template.
	Kind string
	// Name is the child's name.
	Name string
	// Description is the child's description.
	Description string
	// Content is the skill body or the template text.
	Content string
	// Source is plugin:<plugin id>@<component digest>; the store records it with the change.
	Source string
}

// MCPServerWrite is a remote HTTP MCP server the store registers in the org.
type MCPServerWrite struct {
	// OrgID is the plugin's org; the server has org scope.
	OrgID string
	// ActorID is the admin who consented.
	ActorID string
	// Name is the server's display name.
	Name string
	// URL is the server's https endpoint.
	URL string
	// ToolAllowlist is the exact tool allowlist; its tools stay unapproved until a person approves them.
	ToolAllowlist []string
	// Source is plugin:<plugin id>@<component digest>; the store records it on the server row.
	Source string
}

// ChildStore writes and removes the children through the registry stores, inside the caller's
// transaction. Check methods write nothing and return the reason a child would be refused, or "".
type ChildStore interface {
	// CheckSkill reports why a skill (kind skill) or command (kind template) cannot be created, or "".
	CheckSkill(ctx context.Context, kind, name, description, content string) string
	// CheckMCPServer reports why a remote MCP server cannot be registered, or "".
	CheckMCPServer(ctx context.Context, name, url string, toolAllowlist []string) string
	// CreateSkill inserts the skill or template and returns its id. It returns ErrNameConflict when the name is taken.
	CreateSkill(ctx context.Context, tx DB, write SkillWrite) (string, error)
	// CreateMCPServer registers the server and returns its id. It returns ErrNameConflict when the name is taken.
	CreateMCPServer(ctx context.Context, tx DB, write MCPServerWrite) (string, error)
	// RemoveSkill deletes the skill or template a plugin created. A child that is already gone is not an error.
	RemoveSkill(ctx context.Context, tx DB, orgID, id, actorID, reason string) error
	// RemoveMCPServer disables and soft-deletes the server a plugin created, with its tools. A child that is already gone is not an error.
	RemoveMCPServer(ctx context.Context, tx DB, orgID, id string) error
}

// Loader expands plugins into children through its ChildStore.
type Loader struct {
	// Children writes the children.
	Children ChildStore
}

// Child is one child an install created or an uninstall removed.
type Child struct {
	// Kind is skill, mcp_server or command.
	Kind string `json:"kind"`
	// Name is the component name.
	Name string `json:"name"`
	// ID is the child's id in its own registry.
	ID string `json:"id"`
	// Digest is the component digest the admin consented to.
	Digest string `json:"digest,omitempty"`
}

// Result is what an install or an uninstall did.
type Result struct {
	// Components is the full report: every installable and every refused component.
	Components []Component `json:"components"`
	// Children are the children created (install) or removed (uninstall).
	Children []Child `json:"children"`
}

// planned is a component with the declaration it came from, so an install writes exactly what the
// admin was shown.
type planned struct {
	Component
	skill   *Skill
	server  *MCPServer
	command *Command
}

// Plan reports every component of the manifest: pending when an install would create it, refused with a
// reason otherwise. It writes nothing. The digests in the report are the ones an install requires as
// consent.
func (l *Loader) Plan(ctx context.Context, manifest Manifest) []Component {
	steps := l.plan(ctx, manifest)
	out := make([]Component, 0, len(steps))
	for _, step := range steps {
		out = append(out, step.Component)
	}
	return out
}

func (l *Loader) plan(ctx context.Context, manifest Manifest) []planned {
	steps := make([]planned, 0, len(manifest.Skills)+len(manifest.MCPServers)+len(manifest.Commands))
	settle := func(step planned, reason string) planned {
		step.Status = StatusPending
		if reason != "" {
			step.Status, step.Report = StatusRefused, reason
		}
		return step
	}
	for index := range manifest.Skills {
		skill := manifest.Skills[index]
		step := planned{Component: Component{Kind: KindSkill, Name: skill.Name, Digest: componentDigest(KindSkill, skill)}, skill: &skill}
		steps = append(steps, settle(step, l.Children.CheckSkill(ctx, "skill", skill.Name, skill.Description, skill.Body)))
	}
	for index := range manifest.MCPServers {
		server := manifest.MCPServers[index]
		step := planned{Component: Component{Kind: KindMCPServer, Name: server.Name, Digest: componentDigest(KindMCPServer, server)}, server: &server}
		steps = append(steps, settle(step, l.Children.CheckMCPServer(ctx, server.Name, server.URL, server.ToolAllowlist)))
	}
	for index := range manifest.Commands {
		command := manifest.Commands[index]
		step := planned{Component: Component{Kind: KindCommand, Name: command.Name, Digest: componentDigest(KindCommand, command)}, command: &command}
		steps = append(steps, settle(step, l.Children.CheckSkill(ctx, "template", command.Name, command.Description, command.Template)))
	}
	for _, group := range manifest.refusedGroups() {
		if !declared(group.raw) {
			continue
		}
		steps = append(steps, planned{Component: Component{
			Kind: group.kind, Name: group.key, Digest: componentDigest(group.kind, canonicalRaw(group.raw)),
			Status: StatusRefused, Report: group.report,
		}})
	}
	return steps
}

// authorize is the admin and ZDR gate shared by install and uninstall. A ZDR or unknown posture is
// refused only when requireNonZDR is set (install): removing a child persists nothing new.
func authorize(caller Caller, requireNonZDR bool) error {
	if requireNonZDR && (caller.ZeroDataRetention || !caller.RetentionPolicyKnown) {
		return &Error{Code: CodeZDRRefused, Message: "a plugin cannot be installed under zero data retention: it stores skills and servers"}
	}
	if !caller.IsPerson || !caller.IsAdmin {
		return &Error{Code: CodeNotAdmin, Message: "only an org admin acting as a person may install or uninstall a plugin"}
	}
	return nil
}

// Source is the source tag a child carries: the plugin and the digest of the component declaration.
func Source(pluginID, digest string) string { return "plugin:" + pluginID + "@" + digest }

// Install expands the plugin's manifest into children in one transaction tx. The caller must be an org
// admin acting as a person, with a non-ZDR posture. consentedDigests must be exactly the digests of the
// installable components the admin was shown (Plan), so an admin consents to the declared set and
// nothing else. Refused components are reported and never created. Children get org scope and are
// created by the same rules as a direct write: the skill lint applies, an MCP server must pass the
// registration checks (https, public DNS, auth none) and its tools stay unapproved until a person
// approves them as today. The caller commits tx and then emits the reconcile events.
func (l *Loader) Install(ctx context.Context, tx DB, plugin Plugin, caller Caller, consentedDigests []string) (Result, error) {
	if err := authorize(caller, true); err != nil {
		return Result{}, err
	}
	if plugin.ManifestDigest == "" {
		return Result{}, &Error{Code: CodeNotPinned, Message: "the plugin has no registered source pin and manifest digest"}
	}
	if plugin.Installed {
		return Result{}, &Error{Code: CodeAlreadyInstalled, Message: "the plugin is already installed; uninstall it first"}
	}
	manifest, err := ParseManifest(plugin.Manifest)
	if err != nil {
		return Result{}, err
	}
	if manifest.Name != plugin.Name || manifest.Version != plugin.Version {
		return Result{}, invalid("the manifest name and version do not match the registered plugin")
	}
	var killed bool
	if err := tx.QueryRow(ctx, `
		SELECT EXISTS (SELECT 1 FROM plugin_kill_list
		               WHERE digest = $1::text AND org_id IN ('global', $2::text))
	`, plugin.ManifestDigest, plugin.OrgID).Scan(&killed); err != nil {
		return Result{}, fmt.Errorf("read plugin kill list: %w", err)
	}
	if killed {
		return Result{}, &Error{Code: CodeKilled, Message: "this plugin's manifest digest is on the kill list, so it cannot be installed"}
	}

	steps := l.plan(ctx, manifest)
	expected := make([]string, 0, len(steps))
	for _, step := range steps {
		if step.Installable() {
			expected = append(expected, step.Digest)
		}
	}
	if len(expected) == 0 {
		return Result{}, invalid("the manifest declares no component the loader can install")
	}
	if !sameDigestSet(expected, consentedDigests) {
		sort.Strings(expected)
		return Result{}, &Error{
			Code:    CodeConsentMismatch,
			Message: "consented_digests must be exactly the component digests listed for this plugin",
			Details: map[string]any{"expected_digests": expected},
		}
	}

	// The guard comes first: only one install of a plugin wins, and a failure below rolls this back.
	claimed, err := tx.Exec(ctx, `
		UPDATE plugin_packages SET installed_at = now(), updated_at = now()
		WHERE id = $1::text AND org_id = $2::text AND deleted_at IS NULL AND installed_at IS NULL
	`, plugin.ID, plugin.OrgID)
	if err != nil {
		return Result{}, fmt.Errorf("mark plugin installed: %w", err)
	}
	if claimed.RowsAffected() != 1 {
		return Result{}, &Error{Code: CodeAlreadyInstalled, Message: "the plugin is already installed or no longer exists"}
	}

	result := Result{Components: make([]Component, 0, len(steps)), Children: []Child{}}
	for _, step := range steps {
		component := step.Component
		switch {
		case !step.Installable():
			if isChildKind(step.Kind) {
				if err := l.recordComponent(ctx, tx, plugin.ID, component, ""); err != nil {
					return Result{}, err
				}
			}
		default:
			id, err := l.create(ctx, tx, plugin, caller, step)
			if err != nil {
				return Result{}, err
			}
			component.Status = StatusInstalled
			component.Report = Source(plugin.ID, step.Digest)
			if err := l.recordComponent(ctx, tx, plugin.ID, component, id); err != nil {
				return Result{}, err
			}
			result.Children = append(result.Children, Child{Kind: step.Kind, Name: step.Name, ID: id, Digest: step.Digest})
			if err := audit(ctx, tx, childAuditKind(step.Kind), id, "created", caller.ID, plugin.OrgID, map[string]any{
				"plugin_id": plugin.ID, "source": Source(plugin.ID, step.Digest), "component": step.Kind, "name": step.Name,
			}); err != nil {
				return Result{}, err
			}
		}
		result.Components = append(result.Components, component)
	}
	if err := audit(ctx, tx, "plugin", plugin.ID, "plugin_installed", caller.ID, plugin.OrgID, map[string]any{
		"manifest_digest": plugin.ManifestDigest, "components": componentSummaries(result.Components),
	}); err != nil {
		return Result{}, err
	}
	return result, nil
}

// create writes one installable component through the ChildStore.
func (l *Loader) create(ctx context.Context, tx DB, plugin Plugin, caller Caller, step planned) (string, error) {
	source := Source(plugin.ID, step.Digest)
	var id string
	var err error
	switch step.Kind {
	case KindSkill:
		id, err = l.Children.CreateSkill(ctx, tx, SkillWrite{
			OrgID: plugin.OrgID, ActorID: caller.ID, Kind: "skill", Name: step.skill.Name,
			Description: step.skill.Description, Content: step.skill.Body, Source: source,
		})
	case KindCommand:
		id, err = l.Children.CreateSkill(ctx, tx, SkillWrite{
			OrgID: plugin.OrgID, ActorID: caller.ID, Kind: "template", Name: step.command.Name,
			Description: step.command.Description, Content: step.command.Template, Source: source,
		})
	case KindMCPServer:
		id, err = l.Children.CreateMCPServer(ctx, tx, MCPServerWrite{
			OrgID: plugin.OrgID, ActorID: caller.ID, Name: step.server.Name, URL: step.server.URL,
			ToolAllowlist: step.server.ToolAllowlist, Source: source,
		})
	default:
		return "", fmt.Errorf("plugin component kind %q is not installable", step.Kind)
	}
	if errors.Is(err, ErrNameConflict) {
		return "", &Error{
			Code:    CodeComponentConflict,
			Message: "a component's name is already taken in the org; nothing was installed",
			Details: map[string]any{"kind": step.Kind, "name": step.Name},
		}
	}
	if err != nil {
		return "", fmt.Errorf("create plugin %s %q: %w", step.Kind, step.Name, err)
	}
	return id, nil
}

// Uninstall removes the children an install created and marks the plugin not installed and disabled, in
// one transaction tx. The caller must be an org admin acting as a person. The plugin_components rows stay
// as history with status removed.
func (l *Loader) Uninstall(ctx context.Context, tx DB, plugin Plugin, caller Caller, reason string) (Result, error) {
	if err := authorize(caller, false); err != nil {
		return Result{}, err
	}
	return l.uninstall(ctx, tx, plugin, caller.ID, reason)
}

// UninstallKilled is the removal a kill runs for each installed copy of a killed digest, in the kill's
// transaction tx, with reason ReasonKilled. It takes the actor's id and no authority: the kill route has
// checked the killer first (an org admin acting as a person for an org kill, a platform service principal
// holding the global write scope for a global one). Uninstall's person check cannot pass for a platform
// principal, which is why a global kill does not go through Uninstall.
func (l *Loader) UninstallKilled(ctx context.Context, tx DB, plugin Plugin, actorID string) (Result, error) {
	return l.uninstall(ctx, tx, plugin, actorID, ReasonKilled)
}

// uninstall is the removal both entry points share. The caller has already been authorized.
func (l *Loader) uninstall(ctx context.Context, tx DB, plugin Plugin, actorID, reason string) (Result, error) {
	if !plugin.Installed {
		return Result{}, &Error{Code: CodeNotInstalled, Message: "the plugin is not installed"}
	}
	rows, err := tx.Query(ctx, `
		SELECT kind, name, COALESCE(child_ref, '')
		FROM plugin_components
		WHERE plugin_id = $1::text AND status = 'installed'
		ORDER BY kind, name
	`, plugin.ID)
	if err != nil {
		return Result{}, fmt.Errorf("list plugin components: %w", err)
	}
	var installed []Child
	for rows.Next() {
		var child Child
		if err := rows.Scan(&child.Kind, &child.Name, &child.ID); err != nil {
			rows.Close()
			return Result{}, fmt.Errorf("scan plugin component: %w", err)
		}
		installed = append(installed, child)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return Result{}, fmt.Errorf("list plugin components: %w", err)
	}

	result := Result{Components: []Component{}, Children: []Child{}}
	for _, child := range installed {
		if child.ID != "" {
			var removeErr error
			if child.Kind == KindMCPServer {
				removeErr = l.Children.RemoveMCPServer(ctx, tx, plugin.OrgID, child.ID)
			} else {
				removeErr = l.Children.RemoveSkill(ctx, tx, plugin.OrgID, child.ID, actorID, "plugin_removed:"+plugin.ID)
			}
			if removeErr != nil {
				return Result{}, fmt.Errorf("remove plugin %s %q: %w", child.Kind, child.Name, removeErr)
			}
		}
		if _, err := tx.Exec(ctx, `
			UPDATE plugin_components SET status = 'removed'
			WHERE plugin_id = $1::text AND kind = $2::text AND name = $3::text
		`, plugin.ID, child.Kind, child.Name); err != nil {
			return Result{}, fmt.Errorf("mark plugin component removed: %w", err)
		}
		if err := audit(ctx, tx, childAuditKind(child.Kind), child.ID, "deleted", actorID, plugin.OrgID, map[string]any{
			"plugin_id": plugin.ID, "component": child.Kind, "name": child.Name, "reason": reason,
		}); err != nil {
			return Result{}, err
		}
		result.Children = append(result.Children, child)
		result.Components = append(result.Components, Component{Kind: child.Kind, Name: child.Name, Status: StatusRemoved})
	}
	// Enabling needs an install, so clearing installed_at also takes the plugin out of service.
	if _, err := tx.Exec(ctx, `
		UPDATE plugin_packages SET enabled = FALSE, installed_at = NULL, updated_at = now()
		WHERE id = $1::text AND org_id = $2::text AND deleted_at IS NULL
	`, plugin.ID, plugin.OrgID); err != nil {
		return Result{}, fmt.Errorf("mark plugin uninstalled: %w", err)
	}
	if err := audit(ctx, tx, "plugin", plugin.ID, "plugin_uninstalled", actorID, plugin.OrgID, map[string]any{
		"manifest_digest": plugin.ManifestDigest, "reason": reason, "removed": len(result.Children),
	}); err != nil {
		return Result{}, err
	}
	return result, nil
}

// recordComponent upserts the plugin_components row of a child kind. A refused component has no child.
func (l *Loader) recordComponent(ctx context.Context, tx DB, pluginID string, component Component, childID string) error {
	var childRef any
	if childID != "" {
		childRef = childID
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO plugin_components (plugin_id, kind, name, child_ref, status, report)
		VALUES ($1::text, $2::text, $3::text, $4::text, $5::text, $6::text)
		ON CONFLICT (plugin_id, kind, name) DO UPDATE
		    SET child_ref = EXCLUDED.child_ref, status = EXCLUDED.status, report = EXCLUDED.report
	`, pluginID, component.Kind, component.Name, childRef, component.Status, component.Report); err != nil {
		return fmt.Errorf("record plugin component: %w", err)
	}
	return nil
}

// isChildKind reports whether plugin_components can store the kind.
func isChildKind(kind string) bool {
	return kind == KindSkill || kind == KindMCPServer || kind == KindCommand
}

// childAuditKind is the registry_audit_log entity_kind of a child.
func childAuditKind(kind string) string {
	if kind == KindMCPServer {
		return "mcp_server"
	}
	return "skill"
}

// sameDigestSet reports whether the two lists hold the same set of digests.
func sameDigestSet(expected, consented []string) bool {
	want := make(map[string]struct{}, len(expected))
	for _, digest := range expected {
		want[digest] = struct{}{}
	}
	got := make(map[string]struct{}, len(consented))
	for _, digest := range consented {
		got[digest] = struct{}{}
	}
	if len(want) != len(got) {
		return false
	}
	for digest := range want {
		if _, ok := got[digest]; !ok {
			return false
		}
	}
	return true
}

// componentSummaries is the content-free form of a report for an audit row: kinds, names, digests and
// statuses, never a body, a template or a URL.
func componentSummaries(components []Component) []map[string]string {
	out := make([]map[string]string, 0, len(components))
	for _, component := range components {
		out = append(out, map[string]string{
			"kind": component.Kind, "name": component.Name, "digest": component.Digest, "status": component.Status,
		})
	}
	return out
}

// audit appends a registry_audit_log row in the install's transaction.
func audit(ctx context.Context, tx DB, entityKind, entityID, action, actor, orgID string, diff map[string]any) error {
	encoded, err := json.Marshal(diff)
	if err != nil {
		return fmt.Errorf("encode plugin audit diff: %w", err)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
		VALUES ($1::text, $2::text, $3::text, $4::text, $5::text, $6::text, $7::jsonb)
	`, "ral_"+uuid.NewString(), entityKind, entityID, action, actor, orgID, string(encoded)); err != nil {
		return fmt.Errorf("write plugin audit row: %w", err)
	}
	return nil
}
