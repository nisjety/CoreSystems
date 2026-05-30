package server

import (
	"context"
	"encoding/json"
	"log/slog"
	"net/http"
	"strings"
	"time"

	"github.com/triodelab/model-plane/services/task-core/internal/cron"
	"github.com/triodelab/model-plane/services/task-core/internal/store"
	"github.com/triodelab/model-plane/services/task-core/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
)

// Server is the task-core HTTP server.
type Server struct {
	store  *store.Store
	logger *slog.Logger
	mux    *http.ServeMux
}

// NewServer constructs a Server wired to the provided task store.
func NewServer(s *store.Store, logger *slog.Logger) *Server {
	srv := &Server{
		store:  s,
		logger: logger,
		mux:    http.NewServeMux(),
	}
	srv.routes()
	return srv
}

// Handler returns the root HTTP handler.
func (s *Server) Handler() http.Handler {
	return s.mux
}

// routes registers all HTTP endpoints.
func (s *Server) routes() {
	s.mux.HandleFunc("/healthz", s.handleHealthz)
	s.mux.HandleFunc("/readyz", s.handleReadyz)
	s.mux.HandleFunc("/api/v1/tasks", s.handleTasks)
	s.mux.HandleFunc("/api/v1/tasks/", s.handleTaskByID)
}

// ---------- Health endpoints ----------

func (s *Server) handleHealthz(w http.ResponseWriter, _ *http.Request) {
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte("ok"))
}

func (s *Server) handleReadyz(w http.ResponseWriter, _ *http.Request) {
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte("ok"))
}

// ---------- /api/v1/tasks ----------

func (s *Server) handleTasks(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodPost:
		s.createTask(w, r)
	case http.MethodGet:
		s.listTasks(w, r)
	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

// createTaskRequest is the JSON body for POST /api/v1/tasks.
type createTaskRequest struct {
	OrgID    string `json:"org_id"`
	Name     string `json:"name"`
	CronExpr string `json:"cron_expr,omitempty"`
	Payload  string `json:"payload,omitempty"`
}

// taskResponse is the JSON representation of a task returned to clients.
type taskResponse struct {
	ID        string     `json:"id"`
	OrgID     string     `json:"org_id"`
	Name      string     `json:"name"`
	CronExpr  string     `json:"cron_expr,omitempty"`
	Payload   string     `json:"payload,omitempty"`
	Status    string     `json:"status"`
	NextRunAt *time.Time `json:"next_run_at,omitempty"`
	LastRunAt *time.Time `json:"last_run_at,omitempty"`
	CreatedAt time.Time  `json:"created_at"`
}

func toTaskResponse(t *store.Task) taskResponse {
	tr := taskResponse{
		ID:        t.ID,
		OrgID:     t.OrgID,
		Name:      t.Name,
		CronExpr:  t.CronExpr,
		Payload:   t.Payload,
		Status:    string(t.Status),
		CreatedAt: t.CreatedAt,
	}
	if !t.NextRunAt.IsZero() {
		tr.NextRunAt = &t.NextRunAt
	}
	if !t.LastRunAt.IsZero() {
		tr.LastRunAt = &t.LastRunAt
	}
	return tr
}

func (s *Server) createTask(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "POST"),
		attribute.String("path", "/api/v1/tasks"),
	))

	var req createTaskRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		telemetry.TasksCreatedTotal.Add(r.Context(), 1, metric.WithAttributes(attribute.String("outcome", "invalid_json")))
		writeError(w, http.StatusBadRequest, ErrInvalidJSON)
		return
	}

	// Compute initial NextRunAt for cron tasks.
	var nextRunAt time.Time
	if req.CronExpr != "" {
		next, err := cron.NextRun(req.CronExpr, time.Now().UTC())
		if err != nil {
			telemetry.TasksCreatedTotal.Add(r.Context(), 1, metric.WithAttributes(attribute.String("outcome", "invalid_cron")))
			writeError(w, http.StatusBadRequest, err)
			return
		}
		nextRunAt = next
	}

	t, err := s.store.Create(req.OrgID, req.Name, req.CronExpr, req.Payload, nextRunAt)
	if err != nil {
		telemetry.TasksCreatedTotal.Add(r.Context(), 1, metric.WithAttributes(attribute.String("outcome", "store_error")))
		writeError(w, mapHTTPStatus(err), err)
		return
	}

	telemetry.TasksCreatedTotal.Add(r.Context(), 1, metric.WithAttributes(attribute.String("outcome", "created")))
	s.logger.Info("task created", "task_id", t.ID, "org_id", t.OrgID, "name", t.Name)

	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusCreated)
	_ = json.NewEncoder(w).Encode(toTaskResponse(t))
}

func (s *Server) listTasks(w http.ResponseWriter, r *http.Request) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/api/v1/tasks"),
	))

	orgID := r.URL.Query().Get("org_id")
	if orgID == "" {
		writeError(w, http.StatusBadRequest, ErrMissingOrgID)
		return
	}

	tasks := s.store.List(orgID)
	out := make([]taskResponse, 0, len(tasks))
	for _, t := range tasks {
		out = append(out, toTaskResponse(t))
	}

	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(out)
}

// ---------- /api/v1/tasks/:id ----------

func (s *Server) handleTaskByID(w http.ResponseWriter, r *http.Request) {
	// Extract the task ID from the path: /api/v1/tasks/{id}[/trigger]
	path := strings.TrimPrefix(r.URL.Path, "/api/v1/tasks/")
	parts := strings.SplitN(path, "/", 2)
	taskID := parts[0]
	if taskID == "" {
		writeError(w, http.StatusBadRequest, ErrMissingID)
		return
	}

	// Check for /trigger sub-path.
	if len(parts) == 2 && parts[1] == "trigger" {
		if r.Method != http.MethodPost {
			http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		s.triggerTask(w, r, taskID)
		return
	}

	if r.Method != http.MethodGet {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	s.getTask(w, r, taskID)
}

func (s *Server) getTask(w http.ResponseWriter, r *http.Request, id string) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "GET"),
		attribute.String("path", "/api/v1/tasks/:id"),
	))

	t, err := s.store.Get(id)
	if err != nil {
		writeError(w, mapHTTPStatus(err), err)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(toTaskResponse(t))
}

func (s *Server) triggerTask(w http.ResponseWriter, r *http.Request, id string) {
	telemetry.RequestsTotal.Add(r.Context(), 1, metric.WithAttributes(
		attribute.String("method", "POST"),
		attribute.String("path", "/api/v1/tasks/:id/trigger"),
	))

	t, err := s.store.Get(id)
	if err != nil {
		telemetry.TasksTriggeredTotal.Add(r.Context(), 1, metric.WithAttributes(attribute.String("outcome", "not_found")))
		writeError(w, mapHTTPStatus(err), err)
		return
	}

	if err := s.store.UpdateStatus(id, store.StatusRunning); err != nil {
		telemetry.TasksTriggeredTotal.Add(r.Context(), 1, metric.WithAttributes(attribute.String("outcome", "update_error")))
		writeError(w, mapHTTPStatus(err), err)
		return
	}

	// Mark completed — a production implementation would enqueue real work.
	if err := s.store.UpdateStatus(id, store.StatusCompleted); err != nil {
		telemetry.TasksTriggeredTotal.Add(r.Context(), 1, metric.WithAttributes(attribute.String("outcome", "completion_error")))
		writeError(w, mapHTTPStatus(err), err)
		return
	}

	// Reschedule if recurring.
	if t.CronExpr != "" {
		next, cronErr := cron.NextRun(t.CronExpr, time.Now().UTC())
		if cronErr == nil {
			_ = s.store.SetNextRun(id, next)
			_ = s.store.UpdateStatus(id, store.StatusPending)
		}
	}

	telemetry.TasksTriggeredTotal.Add(r.Context(), 1, metric.WithAttributes(attribute.String("outcome", "triggered")))
	s.logger.Info("task manually triggered", "task_id", id)

	// Re-fetch to return updated state.
	updated, err := s.store.Get(id)
	if err != nil {
		writeError(w, mapHTTPStatus(err), err)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(toTaskResponse(updated))
}

// ListenAndServe starts the HTTP server. It blocks until the context is
// cancelled and then shuts down gracefully.
func (s *Server) ListenAndServe(ctx context.Context, addr string) error {
	srv := &http.Server{Addr: addr, Handler: s.mux}
	go func() {
		<-ctx.Done()
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = srv.Shutdown(shutdownCtx)
	}()
	s.logger.Info("HTTP server listening", "addr", addr)
	if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		return err
	}
	return nil
}
