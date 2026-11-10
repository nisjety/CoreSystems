// Package plugins expands a registered plugin package into the child capabilities it declares
// (F-t3-059, unit W4-PL-2): skills, remote HTTP MCP servers and slash commands.
//
// The manifest is a closed internal document (schema verevon.plugin.v1). Any key this package does
// not name is refused, so a plugin cannot smuggle a field past the loader. Hooks, bin entries,
// scripts and stdio MCP servers are recognised only so they can be refused with a report row: they are
// never executed, never stored as children and never installed.
//
// The package holds no HTTP and no authority of its own. The api package supplies the ChildStore that
// writes the children through the same stores the registry routes use, and the admin check, the ZDR
// refusal and the consent check run here, before any child is written.
package plugins

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/url"
	"strings"
	"unicode/utf8"
)

// SchemaV1 is the only manifest schema the loader reads.
const SchemaV1 = "verevon.plugin.v1"

// The limits of a manifest.
const (
	// MaxManifestBytes bounds the whole manifest document.
	MaxManifestBytes = 5 << 20
	// MaxSkillBodyBytes bounds one skill body or one command template.
	MaxSkillBodyBytes = 64 << 10
	// MaxToolAllowlist bounds the tool allowlist of one MCP server.
	MaxToolAllowlist = 64
	// MaxComponentsPerKind bounds how many skills, MCP servers or commands one manifest declares.
	MaxComponentsPerKind = 64
	maxNameBytes         = 200
	maxDescriptionBytes  = 2000
	maxURLBytes          = 2048
	maxToolNameBytes     = 128
)

// The component kinds. The first three are installable children and the only kinds plugin_components
// stores; the rest exist only as refused report rows.
const (
	// KindSkill is a skill child, stored as an agent_skills row of kind skill.
	KindSkill = "skill"
	// KindMCPServer is a remote HTTP MCP server child, stored as an mcp_servers row.
	KindMCPServer = "mcp_server"
	// KindCommand is a slash command child, stored as an agent_skills row of the authored kind template.
	KindCommand = "command"
	// KindHook is a declared hook; it is always refused.
	KindHook = "hook"
	// KindBin is a declared binary entry; it is always refused.
	KindBin = "bin"
	// KindScript is a declared script; it is always refused.
	KindScript = "script"
	// KindStdioServer is a declared stdio MCP server; it is always refused.
	KindStdioServer = "stdio_server"
)

// The component statuses, as plugin_components stores them. A component the plan reports before an
// install is pending when it would be installed.
const (
	// StatusPending is a component that an install would create.
	StatusPending = "pending"
	// StatusInstalled is a component whose child exists.
	StatusInstalled = "installed"
	// StatusRefused is a component the loader will not create, with the reason in its report.
	StatusRefused = "refused"
	// StatusRemoved is a component whose child an uninstall removed.
	StatusRemoved = "removed"
)

// The typed error codes of the loader. The api package maps each to an HTTP status.
const (
	// CodeNotAdmin is an install or uninstall by someone who is not an org admin acting as a person.
	CodeNotAdmin = "plugin_admin_required"
	// CodeZDRRefused is an install under zero data retention or with no verified retention posture.
	CodeZDRRefused = "plugin_install_under_zdr_refused"
	// CodeKilled is an install of a manifest whose digest is on the kill list.
	CodeKilled = "plugin_killed"
	// CodeNotPinned is an install of a plugin with no registered source pin or manifest digest.
	CodeNotPinned = "plugin_source_required"
	// CodeManifestInvalid is a manifest that is not a valid verevon.plugin.v1 document.
	CodeManifestInvalid = "plugin_manifest_invalid"
	// CodeConsentMismatch is a consent list that is not exactly the digests listed back to the admin.
	CodeConsentMismatch = "plugin_consent_mismatch"
	// CodeAlreadyInstalled is an install of a plugin that is already installed.
	CodeAlreadyInstalled = "plugin_already_installed"
	// CodeNotInstalled is an uninstall of a plugin that is not installed.
	CodeNotInstalled = "plugin_not_installed"
	// CodeComponentConflict is a child whose name is already taken in the org.
	CodeComponentConflict = "plugin_component_conflict"
)

// Error is a typed refusal. Details never carry manifest content, only names, kinds and digests.
type Error struct {
	// Code is one of the Code constants.
	Code string
	// Message is a sentence for the admin.
	Message string
	// Details is extra structure for the client, or nil.
	Details map[string]any
}

// Error implements error.
func (e *Error) Error() string { return e.Code + ": " + e.Message }

// IsCode reports whether err is a loader Error with the given code.
func IsCode(err error, code string) bool {
	var typed *Error
	return errors.As(err, &typed) && typed.Code == code
}

func invalid(format string, args ...any) *Error {
	return &Error{Code: CodeManifestInvalid, Message: fmt.Sprintf(format, args...)}
}

// Skill is a skill a plugin declares. Body is the skill text, at most MaxSkillBodyBytes.
type Skill struct {
	// Name is the skill's name in the org.
	Name string `json:"name"`
	// Description is the skill's one-line description.
	Description string `json:"description"`
	// Body is the skill text.
	Body string `json:"body"`
}

// MCPServer is a remote HTTP MCP server a plugin declares. It is registered with no authentication.
type MCPServer struct {
	// Name is the server's display name, unique in the org.
	Name string `json:"name"`
	// URL is the server's https endpoint.
	URL string `json:"url"`
	// ToolAllowlist is the exact set of tool names the server may expose, at most MaxToolAllowlist.
	ToolAllowlist []string `json:"tool_allowlist"`
}

// Command is a slash command a plugin declares; it is stored as an authored template.
type Command struct {
	// Name is the command's name in the org.
	Name string `json:"name"`
	// Description is the command's one-line description.
	Description string `json:"description"`
	// Template is the command text; {{input}} is the only placeholder it may hold.
	Template string `json:"template"`
}

// Manifest is a parsed verevon.plugin.v1 document. The four raw fields hold what the manifest declared
// for the components the loader refuses; they are kept only to be digested and reported.
type Manifest struct {
	// Schema is always SchemaV1.
	Schema string `json:"schema"`
	// Name is the plugin's name; it must match the registered plugin.
	Name string `json:"name"`
	// Version is the plugin's version; it must match the registered plugin.
	Version string `json:"version"`
	// Skills are the declared skills.
	Skills []Skill `json:"skills"`
	// MCPServers are the declared remote MCP servers.
	MCPServers []MCPServer `json:"mcp_servers"`
	// Commands are the declared slash commands.
	Commands []Command `json:"commands"`
	// Hooks is whatever the manifest declared under hooks; it is refused.
	Hooks json.RawMessage `json:"hooks"`
	// Bin is whatever the manifest declared under bin; it is refused.
	Bin json.RawMessage `json:"bin"`
	// Scripts is whatever the manifest declared under scripts; it is refused.
	Scripts json.RawMessage `json:"scripts"`
	// StdioServers is whatever the manifest declared under stdio_servers; it is refused.
	StdioServers json.RawMessage `json:"stdio_servers"`
}

// ParseManifest reads and validates a manifest. Unknown keys, at any depth of a typed component, are
// refused, as are a wrong schema, a bad name, a duplicate component name, an oversized body, an http
// endpoint and an allowlist over the limit. The error is always an *Error with CodeManifestInvalid.
func ParseManifest(raw []byte) (Manifest, error) {
	if len(raw) == 0 || len(raw) > MaxManifestBytes {
		return Manifest{}, invalid("the manifest is empty or larger than %d bytes", MaxManifestBytes)
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var manifest Manifest
	if err := decoder.Decode(&manifest); err != nil {
		return Manifest{}, invalid("the manifest is not a closed verevon.plugin.v1 document: %s", decodeReason(err))
	}
	if _, err := decoder.Token(); !errors.Is(err, io.EOF) {
		return Manifest{}, invalid("the manifest holds more than one JSON value")
	}
	if manifest.Schema != SchemaV1 {
		return Manifest{}, invalid("schema must be %s", SchemaV1)
	}
	if !validName(manifest.Name) || !validName(manifest.Version) {
		return Manifest{}, invalid("name and version are required")
	}
	if err := manifest.validateComponents(); err != nil {
		return Manifest{}, err
	}
	return manifest, nil
}

// decodeReason keeps a decoder error short and free of manifest content.
func decodeReason(err error) string {
	var syntax *json.SyntaxError
	var typed *json.UnmarshalTypeError
	switch {
	case errors.As(err, &syntax):
		return "invalid JSON"
	case errors.As(err, &typed):
		return "field " + typed.Field + " has the wrong type"
	case strings.HasPrefix(err.Error(), "json: unknown field "):
		return strings.TrimPrefix(err.Error(), "json: ")
	default:
		return "invalid JSON"
	}
}

// validName accepts a trimmed, non-empty name of at most maxNameBytes with no control characters.
func validName(value string) bool {
	if value == "" || value != strings.TrimSpace(value) || len(value) > maxNameBytes || !utf8.ValidString(value) {
		return false
	}
	for _, character := range value {
		if character < 32 || character == 127 {
			return false
		}
	}
	return true
}

func validText(value string, limit int) bool {
	return len(value) <= limit && utf8.ValidString(value)
}

func (m Manifest) validateComponents() error {
	if len(m.Skills) > MaxComponentsPerKind || len(m.MCPServers) > MaxComponentsPerKind ||
		len(m.Commands) > MaxComponentsPerKind {
		return invalid("a manifest declares at most %d components of one kind", MaxComponentsPerKind)
	}
	seen := map[string]struct{}{}
	claim := func(kind, name string) error {
		if !validName(name) {
			return invalid("every %s needs a name", kind)
		}
		key := kind + "\x00" + name
		if _, dup := seen[key]; dup {
			return invalid("two %s components share a name", kind)
		}
		seen[key] = struct{}{}
		return nil
	}
	for _, skill := range m.Skills {
		if err := claim(KindSkill, skill.Name); err != nil {
			return err
		}
		if strings.TrimSpace(skill.Body) == "" || len(skill.Body) > MaxSkillBodyBytes ||
			!validText(skill.Body, MaxSkillBodyBytes) || !validText(skill.Description, maxDescriptionBytes) {
			return invalid("a skill body is required and may be at most %d bytes", MaxSkillBodyBytes)
		}
	}
	for _, server := range m.MCPServers {
		if err := claim(KindMCPServer, server.Name); err != nil {
			return err
		}
		if !validHTTPSURL(server.URL) {
			return invalid("an MCP server url must be https with a host and no credentials, query or fragment")
		}
		if len(server.ToolAllowlist) == 0 || len(server.ToolAllowlist) > MaxToolAllowlist {
			return invalid("an MCP server tool_allowlist needs between 1 and %d entries", MaxToolAllowlist)
		}
		for _, tool := range server.ToolAllowlist {
			if tool == "" || len(tool) > maxToolNameBytes {
				return invalid("an MCP server tool_allowlist entry is empty or too long")
			}
		}
	}
	for _, command := range m.Commands {
		if err := claim(KindCommand, command.Name); err != nil {
			return err
		}
		if strings.TrimSpace(command.Template) == "" || len(command.Template) > MaxSkillBodyBytes ||
			!validText(command.Template, MaxSkillBodyBytes) || !validText(command.Description, maxDescriptionBytes) {
			return invalid("a command template is required and may be at most %d bytes", MaxSkillBodyBytes)
		}
	}
	return nil
}

// validHTTPSURL accepts an absolute https URL with a host and no userinfo, query or fragment.
func validHTTPSURL(raw string) bool {
	if raw == "" || len(raw) > maxURLBytes {
		return false
	}
	parsed, err := url.Parse(raw)
	if err != nil {
		return false
	}
	return parsed.Scheme == "https" && parsed.Host != "" && parsed.User == nil &&
		parsed.RawQuery == "" && !parsed.ForceQuery && parsed.Fragment == ""
}

// Component is one row of the report the admin consents to: what the plugin declares, the digest of
// that declaration, and whether the loader would create it.
type Component struct {
	// Kind is a component kind.
	Kind string `json:"kind"`
	// Name is the component's name; for a refused hook, bin, script or stdio group it is the manifest key.
	Name string `json:"name"`
	// Digest is sha256:<hex> over the component's canonical declaration.
	Digest string `json:"digest"`
	// Status is StatusPending for a component an install would create, or StatusRefused.
	Status string `json:"status"`
	// Report says why a component is refused; empty when it is pending.
	Report string `json:"report,omitempty"`
}

// Installable reports whether the component is one an install would create.
func (c Component) Installable() bool { return c.Status == StatusPending }

// componentDigest is `sha256:<hex>` over the kind and the declaration. Go marshals a struct in field
// order and a map with sorted keys, so the same declaration always gives the same digest.
func componentDigest(kind string, declaration any) string {
	encoded, err := json.Marshal(struct {
		Kind        string `json:"kind"`
		Declaration any    `json:"declaration"`
	}{kind, declaration})
	if err != nil {
		// Every declaration here is a plain struct or a decoded JSON value, so this cannot happen;
		// a digest of nothing is a value no consent list can match.
		return "sha256:" + strings.Repeat("0", 64)
	}
	return fmt.Sprintf("sha256:%x", sha256.Sum256(encoded))
}

// refusedGroups are the manifest keys the loader never executes, with the kind and the sentence each
// one is reported under.
func (m Manifest) refusedGroups() []struct {
	kind   string
	key    string
	raw    json.RawMessage
	report string
} {
	return []struct {
		kind   string
		key    string
		raw    json.RawMessage
		report string
	}{
		{KindHook, "hooks", m.Hooks, "plugin hooks are never run; declare org hook rules through the org hooks API"},
		{KindBin, "bin", m.Bin, "plugin binaries are never installed or run"},
		{KindScript, "scripts", m.Scripts, "plugin scripts are never run"},
		{KindStdioServer, "stdio_servers", m.StdioServers, "stdio MCP servers are never started; only remote https MCP servers are installed"},
	}
}

// declared reports whether a raw manifest value is present: not absent, not JSON null.
func declared(raw json.RawMessage) bool {
	trimmed := bytes.TrimSpace(raw)
	return len(trimmed) > 0 && !bytes.Equal(trimmed, []byte("null"))
}

// canonicalRaw decodes a raw value and re-encodes it with sorted keys so its digest does not depend on
// how the manifest spelt it. A value that cannot be decoded is digested as written.
func canonicalRaw(raw json.RawMessage) any {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	var value any
	if err := decoder.Decode(&value); err != nil {
		return string(raw)
	}
	return value
}
