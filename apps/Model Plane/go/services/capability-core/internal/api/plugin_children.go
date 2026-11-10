package api

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"time"

	"github.com/google/uuid"

	"github.com/triodelab/model-plane/services/capability-core/internal/plugins"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillbundle"
	"github.com/triodelab/model-plane/services/capability-core/internal/skilllint"
)

// pluginChildStore is the plugin loader's ChildStore (W4-PL-2). It writes the children a plugin expands
// into with the same rules and statements as the registry's own routes, so a plugin can never create a
// child a person could not: skills and commands go through the SkillsHandler's lint settings and insert
// statements, and MCP servers go through the MCPHandler's registration checks.
type pluginChildStore struct {
	skills *SkillsHandler
	mcp    *MCPHandler
}

func newPluginChildStore(skills *SkillsHandler, mcp *MCPHandler) *pluginChildStore {
	return &pluginChildStore{skills: skills, mcp: mcp}
}

// CheckSkill reports why a skill or command would be refused, with the finding codes and never the
// text. A command is the authored kind template, so it needs AUTHORED_KINDS_ENABLED; the template rules
// run whatever SKILL_LINT_MODE says, and the skill lint applies in enforce mode as on a direct write.
func (s *pluginChildStore) CheckSkill(_ context.Context, kind, name, description, content string) string {
	if s.skills == nil {
		return "skills are not available in this deployment"
	}
	if kind == skilllint.KindTemplate && !s.skills.authoredKinds {
		return "commands are stored as templates, and authored kinds are not enabled (AUTHORED_KINDS_ENABLED)"
	}
	input := skilllint.Input{
		Name: name, Description: description, Content: content,
		TriggerKeywords: []string{}, ToolRestrictions: []string{}, Kind: kind,
	}
	var codes []string
	for _, finding := range kindRuleRefusal(input) {
		codes = append(codes, finding.Code)
	}
	if s.skills.lintMode == skilllint.ModeEnforce {
		if all := skilllint.Lint(input); skilllint.HasError(all) {
			codes = codes[:0]
			for _, finding := range all {
				if finding.Severity == skilllint.SeverityError {
					codes = append(codes, finding.Code)
				}
			}
		}
	}
	if len(codes) > 0 {
		return "refused by the skill lint: " + strings.Join(codes, ", ")
	}
	return ""
}

// pluginMCPRegistration is the registration a plugin's MCP server is normalised from: remote HTTP, no
// authentication, org scope, canary rollout at high risk. Its tools stay unapproved until a person
// approves them, as for any server.
func pluginMCPRegistration(name, endpoint, source string, allowlist []string) mcpServerRegistration {
	return mcpServerRegistration{
		Name:        name,
		Description: source,
		EndpointURL: endpoint,
		Transport:   "http",
		AuthKind:    "none",
		ConfigJSON:  mcpServerConfig{ToolAllowlist: append([]string(nil), allowlist...)},
		Scope:       "org",
		Enabled:     true, RolloutState: "canary", RiskLevel: "high",
	}
}

// CheckMCPServer runs the MCP registration checks (https endpoint, public DNS, exact allowlist, auth
// none) without writing, and returns the reason the server would be refused.
func (s *pluginChildStore) CheckMCPServer(ctx context.Context, name, endpoint string, allowlist []string) string {
	if s.mcp == nil {
		return "MCP servers are not available in this deployment"
	}
	if _, _, err := s.mcp.normalizeMCPRegistration(ctx, pluginMCPRegistration(name, endpoint, "", allowlist), ""); err != nil {
		return "refused by the MCP registration checks: " + err.Error()
	}
	return ""
}

// CreateSkill inserts an org-scoped skill or template. The revision trigger records the admin as the
// actor and the plugin source (plugin:<id>@<digest>) as the reason, so the revision log says where the
// text came from.
func (s *pluginChildStore) CreateSkill(ctx context.Context, tx plugins.DB, write plugins.SkillWrite) (string, error) {
	if s.skills == nil {
		return "", errors.New("skills are not available in this deployment")
	}
	if write.Kind == skilllint.KindTemplate && !s.skills.authoredKinds {
		return "", errors.New("authored kinds are not enabled")
	}
	if _, err := tx.Exec(ctx, skillActorSQL, "user", write.ActorID, write.Source, ""); err != nil {
		return "", err
	}
	id := "skill_" + uuid.New().String()
	now := time.Now().UTC()
	empty, _ := json.Marshal([]string{})
	var err error
	if s.skills.authoredKinds {
		_, err = tx.Exec(ctx, `
			INSERT INTO agent_skills (id, org_id, name, description, content,
			    trigger_keywords, trigger_file_patterns, tool_restrictions,
			    enabled, scope, owner_user_id, shared_with, created_at, updated_at,
			    kind, settings)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
		`, id, write.OrgID, write.Name, write.Description, write.Content, empty, empty, empty, true,
			"org", "", empty, now, now, write.Kind, []byte("{}"))
	} else {
		_, err = tx.Exec(ctx, `
			INSERT INTO agent_skills (id, org_id, name, description, content,
			    trigger_keywords, trigger_file_patterns, tool_restrictions,
			    enabled, scope, owner_user_id, shared_with, created_at, updated_at)
			VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
		`, id, write.OrgID, write.Name, write.Description, write.Content, empty, empty, empty, true,
			"org", "", empty, now, now)
	}
	if isUniqueViolation(err) {
		return "", plugins.ErrNameConflict
	}
	if err != nil {
		return "", err
	}
	return id, nil
}

// CreateMCPServer registers an org-scoped remote MCP server. The registration is normalised by the same
// checks as a direct registration, and the insert is a plain INSERT: it never overwrites a server of the
// same name, which is a name conflict that rolls the install back. The server row carries the plugin
// source in its description.
func (s *pluginChildStore) CreateMCPServer(ctx context.Context, tx plugins.DB, write plugins.MCPServerWrite) (string, error) {
	if s.mcp == nil {
		return "", errors.New("MCP servers are not available in this deployment")
	}
	row, config, err := s.mcp.normalizeMCPRegistration(ctx,
		pluginMCPRegistration(write.Name, write.URL, write.Source, write.ToolAllowlist), write.OrgID)
	if err != nil {
		return "", err
	}
	encoded, err := json.Marshal(config)
	if err != nil {
		return "", err
	}
	now := time.Now().UTC()
	_, err = tx.Exec(ctx, `
		INSERT INTO mcp_servers (id, org_id, name, description, endpoint_url, transport, auth_kind,
		    config_json, scope, enabled, rollout_state, risk_level, created_by, created_at, updated_at)
		VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
	`, row.ID, row.OrgID, row.Name, row.Description, row.EndpointURL, row.Transport, row.AuthKind,
		encoded, row.Scope, row.Enabled, row.RolloutState, row.RiskLevel, write.ActorID, now, now)
	if isUniqueViolation(err) {
		return "", plugins.ErrNameConflict
	}
	if err != nil {
		return "", err
	}
	return row.ID, nil
}

// RemoveSkill deletes a skill or template a plugin created, as the skill delete route does: the
// revision trigger records the admin and the plugin as the reason, and with bundle retention on the
// skill's bundles go with it. A skill already deleted by hand changes nothing.
func (s *pluginChildStore) RemoveSkill(ctx context.Context, tx plugins.DB, orgID, id, actorID, reason string) error {
	if s.skills == nil {
		return errors.New("skills are not available in this deployment")
	}
	if _, err := tx.Exec(ctx, skillActorSQL, "user", actorID, reason, ""); err != nil {
		return err
	}
	result, err := tx.Exec(ctx, `DELETE FROM agent_skills WHERE id=$1 AND org_id=$2`, id, orgID)
	if err == nil && s.skills.bundleRetention && result.RowsAffected() == 1 {
		err = skillbundle.DeleteSkillBundles(ctx, tx, orgID, id)
	}
	return err
}

// RemoveMCPServer soft-deletes a server a plugin created and switches off its tools in the same
// statement, as the MCP delete route does. A server already deleted by hand changes nothing.
func (s *pluginChildStore) RemoveMCPServer(ctx context.Context, tx plugins.DB, orgID, id string) error {
	_, err := tx.Exec(ctx, withMCPToolCascade(
		`UPDATE mcp_servers SET deleted_at=$1, updated_at=$1 WHERE id=$2 AND org_id=$3 AND deleted_at IS NULL`,
		"$3", "$2",
	), time.Now().UTC(), id, orgID)
	return err
}
