package platform

import (
	"context"
	"encoding/json"
	"net/http"
	"time"
)

// HealthStatus represents the health status of the application
type HealthStatus struct {
	Status    string            `json:"status"`
	Timestamp time.Time         `json:"timestamp"`
	Version   string            `json:"version,omitempty"`
	Services  map[string]string `json:"services,omitempty"`
}

// HealthChecker interface for health check implementations
type HealthChecker interface {
	Check(ctx context.Context) error
	Name() string
}

// HealthService manages health checks
type HealthService struct {
	checkers []HealthChecker
	version  string
}

// NewHealthService creates a new health service
func NewHealthService(version string) *HealthService {
	return &HealthService{
		checkers: make([]HealthChecker, 0),
		version:  version,
	}
}

// AddChecker adds a health checker
func (h *HealthService) AddChecker(checker HealthChecker) {
	h.checkers = append(h.checkers, checker)
}

// Check performs all health checks
func (h *HealthService) Check(ctx context.Context) HealthStatus {
	status := HealthStatus{
		Status:    "healthy",
		Timestamp: time.Now(),
		Version:   h.version,
		Services:  make(map[string]string),
	}

	for _, checker := range h.checkers {
		err := checker.Check(ctx)
		if err != nil {
			status.Status = "unhealthy"
			status.Services[checker.Name()] = "unhealthy: " + err.Error()
		} else {
			status.Services[checker.Name()] = "healthy"
		}
	}

	return status
}

// Handler returns an HTTP handler for health checks
func (h *HealthService) Handler(timeout time.Duration) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		ctx, cancel := context.WithTimeout(r.Context(), timeout)
		defer cancel()

		status := h.Check(ctx)

		w.Header().Set("Content-Type", "application/json")

		// Set HTTP status code based on health status
		if status.Status == "unhealthy" {
			w.WriteHeader(http.StatusServiceUnavailable)
		} else {
			w.WriteHeader(http.StatusOK)
		}

		json.NewEncoder(w).Encode(status)
	}
}

// BasicHealthChecker is a simple health checker
type BasicHealthChecker struct {
	name string
}

// NewBasicHealthChecker creates a basic health checker
func NewBasicHealthChecker(name string) *BasicHealthChecker {
	return &BasicHealthChecker{name: name}
}

// Check implements HealthChecker
func (b *BasicHealthChecker) Check(ctx context.Context) error {
	// Basic implementation - always healthy
	return nil
}

// Name implements HealthChecker
func (b *BasicHealthChecker) Name() string {
	return b.name
}
