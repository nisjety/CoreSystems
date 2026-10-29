package audit

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"time"

	"go.uber.org/zap"
)

// AuditLogger provides structured, PII-safe audit logging
type AuditLogger struct {
	logger *zap.Logger
}

// AuditEvent represents an audit log event
type AuditEvent struct {
	Timestamp time.Time         `json:"timestamp"`
	Action    string            `json:"action"`
	Actor     string            `json:"actor"` // User/Service identifier (no PII)
	Resource  string            `json:"resource"`
	Success   bool              `json:"success"`
	Error     string            `json:"error,omitempty"`
	Details   map[string]string `json:"details,omitempty"`
	IPAddress string            `json:"ip_address"` // Masked for privacy
	UserAgent string            `json:"user_agent"`
	Duration  time.Duration     `json:"duration_ms"`
	RequestID string            `json:"request_id,omitempty"`
}

// NewAuditLogger creates a new audit logger
func NewAuditLogger(level string) (*AuditLogger, error) {
	var config zap.Config

	if level == "development" {
		config = zap.NewDevelopmentConfig()
	} else {
		config = zap.NewProductionConfig()
		config.Encoding = "json"
	}

	// Set level
	switch level {
	case "debug":
		config.Level = zap.NewAtomicLevelAt(zap.DebugLevel)
	case "info":
		config.Level = zap.NewAtomicLevelAt(zap.InfoLevel)
	case "warn":
		config.Level = zap.NewAtomicLevelAt(zap.WarnLevel)
	case "error":
		config.Level = zap.NewAtomicLevelAt(zap.ErrorLevel)
	default:
		config.Level = zap.NewAtomicLevelAt(zap.InfoLevel)
	}

	logger, err := config.Build()
	if err != nil {
		return nil, fmt.Errorf("failed to build logger: %w", err)
	}

	return &AuditLogger{logger: logger}, nil
}

// LogEvent logs an audit event
func (a *AuditLogger) LogEvent(event AuditEvent) {
	// Mask PII in IP address (keep first 2 octets)
	event.IPAddress = maskIP(event.IPAddress)

	// Convert to JSON for structured logging
	eventJSON, _ := json.Marshal(event)

	if event.Success {
		a.logger.Info("audit",
			zap.String("event", string(eventJSON)),
			zap.String("action", event.Action),
			zap.String("actor", event.Actor),
			zap.String("resource", event.Resource),
			zap.Duration("duration", event.Duration),
		)
	} else {
		a.logger.Error("audit",
			zap.String("event", string(eventJSON)),
			zap.String("action", event.Action),
			zap.String("actor", event.Actor),
			zap.String("resource", event.Resource),
			zap.String("error", event.Error),
			zap.Duration("duration", event.Duration),
		)
	}
}

// LogHTTPRequest logs an HTTP request with audit trail
func (a *AuditLogger) LogHTTPRequest(r *http.Request, action string, success bool, err error, duration time.Duration) {
	event := AuditEvent{
		Timestamp: time.Now(),
		Action:    action,
		Actor:     getActorFromRequest(r),
		Resource:  r.URL.Path,
		Success:   success,
		IPAddress: getIPAddress(r),
		UserAgent: r.UserAgent(),
		Duration:  duration,
		RequestID: getRequestID(r),
	}

	if err != nil {
		event.Error = err.Error()
	}

	a.LogEvent(event)
}

// LogIntegrationCall logs an external API call
func (a *AuditLogger) LogIntegrationCall(service, operation string, success bool, err error, duration time.Duration) {
	event := AuditEvent{
		Timestamp: time.Now(),
		Action:    fmt.Sprintf("integration_%s_%s", service, operation),
		Actor:     "gateway",
		Resource:  service,
		Success:   success,
		Duration:  duration,
	}

	if err != nil {
		event.Error = err.Error()
	}

	a.LogEvent(event)
}

// LogTokenRefresh logs OAuth2 token refresh events
func (a *AuditLogger) LogTokenRefresh(service string, success bool, err error) {
	event := AuditEvent{
		Timestamp: time.Now(),
		Action:    "token_refresh",
		Actor:     "gateway",
		Resource:  service,
		Success:   success,
	}

	if err != nil {
		event.Error = err.Error()
	}

	a.LogEvent(event)
}

// LogCircuitBreakerStateChange logs circuit breaker state changes
func (a *AuditLogger) LogCircuitBreakerStateChange(name, from, to string) {
	event := AuditEvent{
		Timestamp: time.Now(),
		Action:    "circuit_breaker_state_change",
		Actor:     "gateway",
		Resource:  name,
		Success:   true,
		Details: map[string]string{
			"from": from,
			"to":   to,
		},
	}

	a.LogEvent(event)
}

// LogRateLimitExceeded logs rate limit violations
func (a *AuditLogger) LogRateLimitExceeded(r *http.Request, limit string) {
	event := AuditEvent{
		Timestamp: time.Now(),
		Action:    "rate_limit_exceeded",
		Actor:     getActorFromRequest(r),
		Resource:  r.URL.Path,
		Success:   false,
		IPAddress: getIPAddress(r),
		UserAgent: r.UserAgent(),
		Details: map[string]string{
			"limit": limit,
		},
	}

	a.LogEvent(event)
}

// LogAuthFailure logs authentication failures
func (a *AuditLogger) LogAuthFailure(r *http.Request, reason string) {
	event := AuditEvent{
		Timestamp: time.Now(),
		Action:    "auth_failure",
		Actor:     "unknown",
		Resource:  r.URL.Path,
		Success:   false,
		IPAddress: getIPAddress(r),
		UserAgent: r.UserAgent(),
		Error:     reason,
	}

	a.LogEvent(event)
}

// Sync flushes any buffered log entries
func (a *AuditLogger) Sync() {
	_ = a.logger.Sync()
}

// GetLogger returns the underlying zap.Logger
// This is useful for components that need direct access to zap logging
func (a *AuditLogger) GetLogger() *zap.Logger {
	return a.logger
}

// Helper functions

// maskIP masks IP address for privacy (keeps first 2 octets for IPv4)
func maskIP(ip string) string {
	if ip == "" {
		return "unknown"
	}

	// Simple masking - in production use more sophisticated approach
	if len(ip) > 7 {
		return ip[:7] + ".xxx.xxx"
	}
	return "masked"
}

// getIPAddress extracts the real IP address from request
func getIPAddress(r *http.Request) string {
	// Check X-Forwarded-For header first (Traefik sets this)
	if xff := r.Header.Get("X-Forwarded-For"); xff != "" {
		return xff
	}

	// Check X-Real-IP header
	if xri := r.Header.Get("X-Real-IP"); xri != "" {
		return xri
	}

	// Fall back to RemoteAddr
	return r.RemoteAddr
}

// getActorFromRequest extracts actor identifier from request
func getActorFromRequest(r *http.Request) string {
	// Check for API key or user identifier in context
	if ctx := r.Context(); ctx != nil {
		if actor, ok := ctx.Value("actor").(string); ok {
			return actor
		}
	}

	// Default to IP-based identifier (masked)
	return maskIP(getIPAddress(r))
}

// getRequestID extracts request ID from context or headers
func getRequestID(r *http.Request) string {
	// Check context first
	if ctx := r.Context(); ctx != nil {
		if reqID, ok := ctx.Value("request_id").(string); ok {
			return reqID
		}
	}

	// Check X-Request-ID header (Traefik sets this)
	if reqID := r.Header.Get("X-Request-ID"); reqID != "" {
		return reqID
	}

	return ""
}

// WithContext adds audit logger to context
func WithContext(ctx context.Context, logger *AuditLogger) context.Context {
	return context.WithValue(ctx, "audit_logger", logger)
}

// FromContext retrieves audit logger from context
func FromContext(ctx context.Context) *AuditLogger {
	if logger, ok := ctx.Value("audit_logger").(*AuditLogger); ok {
		return logger
	}
	return nil
}
