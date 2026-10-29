// Package commands implements the slash command registry for the capability-core
// service (gap MP-11 — slash commands / shell parity). The registry is seeded
// with built-in system commands and exposes an HTTP API for listing, looking up,
// and executing commands. The catalog is in-memory and read-only.
package commands

// Category classifies a slash command's origin.
type Category string

// Supported command categories.
const (
	CategorySystem Category = "system"
	CategoryUser   Category = "user"
	CategoryPlugin Category = "plugin"
)

// Command describes a registered slash command in the registry.
type Command struct {
	ID          string   `json:"id"`
	OrgID       string   `json:"orgId"`
	Name        string   `json:"name"`
	Description string   `json:"description"`
	Category    Category `json:"category"`
	Handler     string   `json:"handler"`
	Enabled     bool     `json:"enabled"`
	Scope       string   `json:"scope"`
	CreatedAt   string   `json:"createdAt"`
	UpdatedAt   string   `json:"updatedAt"`
}

// CommandExecRequest is the payload for executing a slash command.
type CommandExecRequest struct {
	CommandName string            `json:"commandName"`
	Args        map[string]string `json:"args"`
	OrgID       string            `json:"orgId"`
	UserID      string            `json:"userId"`
	SessionID   string            `json:"sessionId"`
}

// CommandExecResult is the response after executing a slash command.
type CommandExecResult struct {
	Output  string `json:"output"`
	Success bool   `json:"success"`
	Error   string `json:"error,omitempty"`
}

// Catalog is the read-only list of registered slash commands.
type Catalog struct {
	Commands []Command `json:"commands"`
}

// Load returns the seeded command catalog.
func Load() Catalog { return catalog }
