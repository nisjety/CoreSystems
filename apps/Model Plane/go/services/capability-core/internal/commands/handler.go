package commands

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc"
)

// modelLister is the one inference-core method `/models` delegates to (the
// canonical model registry). compactor is the one session-core method
// `/compact` delegates to. Interface segregation keeps the handler unit-testable
// with tiny fakes and avoids depending on the full generated client surface.
type modelLister interface {
	ListModels(ctx context.Context, in *mpv1.ListModelsRequest, opts ...grpc.CallOption) (*mpv1.ListModelsResponse, error)
}

type compactor interface {
	CompactNow(ctx context.Context, in *mpv1.CompactNowRequest, opts ...grpc.CallOption) (*mpv1.CompactNowResponse, error)
}

// CommandHandler serves the slash command registry over HTTP. It holds an
// in-memory slice of commands seeded from the catalog in data.go.
type CommandHandler struct {
	commands  []Command
	models    modelLister // optional; nil → /models reports unavailable
	compactor compactor   // optional; nil → /compact reports unavailable
}

// NewHandler constructs a CommandHandler pre-loaded with the built-in catalog.
func NewHandler() *CommandHandler {
	src := Load()
	cmds := make([]Command, len(src.Commands))
	copy(cmds, src.Commands)
	return &CommandHandler{commands: cmds}
}

// WithModels wires the inference-core client `/models` delegates to. Nil-safe
// and chainable: NewHandler().WithModels(ic).WithCompactor(sc).Register(mux).
func (h *CommandHandler) WithModels(m modelLister) *CommandHandler {
	h.models = m
	return h
}

// WithCompactor wires the session-core client `/compact` delegates to.
func (h *CommandHandler) WithCompactor(c compactor) *CommandHandler {
	h.compactor = c
	return h
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

	result := h.dispatch(r.Context(), name, req)
	writeJSON(w, result)
}

// dispatch routes a validated command to handler logic. Commands with a
// canonical owner DELEGATE to it (no duplicate logic): /models → inference-core
// ListModels, /compact → session-core CompactNow. /help is served locally from
// the catalog.
func (h *CommandHandler) dispatch(ctx context.Context, name string, req CommandExecRequest) CommandExecResult {
	switch name {
	case "/help":
		return h.dispatchHelp()
	case "/compact":
		return h.dispatchCompact(ctx, req)
	case "/models":
		return h.dispatchModels(ctx, req)
	case "/budget":
		// cost-core owns usage/billing; point to it rather than fabricating
		// numbers (no cost-core client is wired in this layer).
		return CommandExecResult{
			Output:  "budget/usage is served by cost-core (GET /v1/usage)",
			Success: true,
		}
	default:
		// Registered (verified by the caller) but no server-side action here —
		// it is dispatched client-side. Report that honestly rather than
		// claiming a fabricated result.
		return CommandExecResult{
			Output:  fmt.Sprintf("%s has no server-side action (client-handled)", name),
			Success: true,
		}
	}
}

// dispatchModels delegates to inference-core's ListModels — the canonical model
// registry — rather than keeping a duplicate catalog here. Optional args
// `modality` and `provider` filter the result.
func (h *CommandHandler) dispatchModels(ctx context.Context, req CommandExecRequest) CommandExecResult {
	if h.models == nil {
		return CommandExecResult{Output: "model registry unavailable (inference-core not wired)", Success: false}
	}
	resp, err := h.models.ListModels(ctx, &mpv1.ListModelsRequest{
		Modality: req.Args["modality"],
		Provider: req.Args["provider"],
	})
	if err != nil {
		return CommandExecResult{Success: false, Error: fmt.Sprintf("list models: %s", err)}
	}
	var b strings.Builder
	fmt.Fprintf(&b, "%d model(s):\n", len(resp.GetModels()))
	for _, m := range resp.GetModels() {
		streaming := ""
		if m.GetStreaming() {
			streaming = " (streaming)"
		}
		fmt.Fprintf(&b, "  %-28s %s/%s%s\n", m.GetId(), m.GetProvider(), m.GetModality(), streaming)
	}
	return CommandExecResult{Output: b.String(), Success: true}
}

// dispatchCompact delegates to session-core's CompactNow. The "toon" arg
// requests a terse single-line summary.
func (h *CommandHandler) dispatchCompact(ctx context.Context, req CommandExecRequest) CommandExecResult {
	if h.compactor == nil {
		return CommandExecResult{Output: "compaction unavailable (session-core not wired)", Success: false}
	}
	toon := req.Args["toon"] == "true" || req.Args["toon"] == "1"
	resp, err := h.compactor.CompactNow(ctx, &mpv1.CompactNowRequest{Toon: toon})
	if err != nil {
		return CommandExecResult{Success: false, Error: fmt.Sprintf("compact: %s", err)}
	}
	return CommandExecResult{Output: resp.GetSummary(), Success: true}
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
