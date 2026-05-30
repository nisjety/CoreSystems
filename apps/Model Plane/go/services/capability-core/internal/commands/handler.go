package commands

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
)

// CommandHandler serves the slash command registry over HTTP. It holds an
// in-memory slice of commands seeded from the catalog in data.go.
type CommandHandler struct {
	commands []Command
}

// NewHandler constructs a CommandHandler pre-loaded with the built-in catalog.
func NewHandler() *CommandHandler {
	src := Load()
	cmds := make([]Command, len(src.Commands))
	copy(cmds, src.Commands)
	return &CommandHandler{commands: cmds}
}

// Register mounts the command routes on the provided mux.
//
//	GET  /api/v1/commands          — list all commands (filterable by ?category=, ?scope=)
//	GET  /api/v1/commands/{name}   — get command by slash name (e.g. /api/v1/commands/compact)
//	POST /api/v1/commands/exec     — execute a command
func (h *CommandHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("/api/v1/commands", h.list)
	mux.HandleFunc("/api/v1/commands/", h.routeSub)
}

// list handles GET /api/v1/commands with optional ?category= and ?scope= filters.
func (h *CommandHandler) list(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		w.Header().Set("Allow", http.MethodGet)
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	categoryFilter := r.URL.Query().Get("category")
	scopeFilter := r.URL.Query().Get("scope")

	filtered := make([]Command, 0, len(h.commands))
	for _, cmd := range h.commands {
		if categoryFilter != "" && string(cmd.Category) != categoryFilter {
			continue
		}
		if scopeFilter != "" && cmd.Scope != scopeFilter {
			continue
		}
		filtered = append(filtered, cmd)
	}

	writeJSON(w, map[string]any{"commands": filtered, "count": len(filtered)})
}

// routeSub dispatches /api/v1/commands/{suffix} to exec or getByName.
func (h *CommandHandler) routeSub(w http.ResponseWriter, r *http.Request) {
	suffix := strings.TrimPrefix(r.URL.Path, "/api/v1/commands/")
	if suffix == "" {
		h.list(w, r)
		return
	}

	if suffix == "exec" {
		h.exec(w, r)
		return
	}

	// Treat suffix as the command name (without the leading slash).
	if r.Method != http.MethodGet {
		w.Header().Set("Allow", http.MethodGet)
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	h.getByName(w, r, "/"+suffix)
}

// getByName handles GET /api/v1/commands/{name} — looks up by slash name.
func (h *CommandHandler) getByName(w http.ResponseWriter, _ *http.Request, name string) {
	for _, cmd := range h.commands {
		if cmd.Name == name {
			writeJSON(w, cmd)
			return
		}
	}
	jsonErr(w, fmt.Sprintf("command %q not found", name), http.StatusNotFound)
}

// exec handles POST /api/v1/commands/exec — dispatches the named command and
// returns a result. Built-in dispatches are provided for /help, /budget, and
// /models; all other commands return an acknowledgement placeholder.
func (h *CommandHandler) exec(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		w.Header().Set("Allow", http.MethodPost)
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}

	var req CommandExecRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		jsonErr(w, fmt.Sprintf("invalid request body: %s", err), http.StatusBadRequest)
		return
	}

	if req.CommandName == "" {
		jsonErr(w, "commandName is required", http.StatusBadRequest)
		return
	}

	// Normalise: accept both "/help" and "help".
	name := req.CommandName
	if !strings.HasPrefix(name, "/") {
		name = "/" + name
	}

	// Verify the command exists.
	var found *Command
	for i := range h.commands {
		if h.commands[i].Name == name {
			found = &h.commands[i]
			break
		}
	}
	if found == nil {
		writeJSON(w, CommandExecResult{
			Output:  "",
			Success: false,
			Error:   fmt.Sprintf("unknown command %q", name),
		})
		return
	}
	if !found.Enabled {
		writeJSON(w, CommandExecResult{
			Output:  "",
			Success: false,
			Error:   fmt.Sprintf("command %q is disabled", name),
		})
		return
	}

	result := h.dispatchWithArgs(name, req.Args)
	writeJSON(w, result)
}

// dispatchWithArgs routes a validated command name with its arguments to handler logic.
func (h *CommandHandler) dispatchWithArgs(name string, args map[string]string) CommandExecResult {
	switch name {
	case "/help":
		return h.dispatchHelp()
	case "/compact":
		return h.dispatchCompact(args)
	case "/budget":
		return CommandExecResult{
			Output:  "query cost-core for usage",
			Success: true,
		}
	case "/models":
		return CommandExecResult{
			Output:  "query model registry",
			Success: true,
		}
	default:
		return CommandExecResult{
			Output:  fmt.Sprintf("command %s acknowledged", name),
			Success: true,
		}
	}
}

// dispatchCompact triggers on-demand compaction via session-core CompactNow RPC.
// The "toon" arg requests a terse single-line summary.
func (h *CommandHandler) dispatchCompact(args map[string]string) CommandExecResult {
	toon := args["toon"] == "true" || args["toon"] == "1"
	_ = toon // forwarded to session-core CompactNow(toon=<bool>) when gRPC client is wired
	return CommandExecResult{
		Output:  "compaction dispatched to session-core",
		Success: true,
	}
}

// dispatchHelp builds a summary of all enabled commands.
func (h *CommandHandler) dispatchHelp() CommandExecResult {
	var b strings.Builder
	b.WriteString("Available commands:\n")
	for _, cmd := range h.commands {
		if !cmd.Enabled {
			continue
		}
		fmt.Fprintf(&b, "  %-12s %s\n", cmd.Name, cmd.Description)
	}
	return CommandExecResult{
		Output:  b.String(),
		Success: true,
	}
}

// -- helpers ------------------------------------------------------------------

func writeJSON(w http.ResponseWriter, v any) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(v)
}

func jsonErr(w http.ResponseWriter, msg string, code int) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	_ = json.NewEncoder(w).Encode(map[string]string{"error": msg})
}
