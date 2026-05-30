// Package session provides an in-memory, goroutine-safe session registry for
// bridge-core. Each session represents a single user connection from a
// specific ingress channel (CLI, VS Code, web, or API).
package session

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"sync"
	"time"
)

// Status represents the lifecycle state of a session.
type Status string

const (
	StatusActive Status = "active"
	StatusIdle   Status = "idle"
	StatusClosed Status = "closed"
)

// Session holds the state for a single ingress session.
type Session struct {
	ID             string    `json:"id"`
	OrgID          string    `json:"org_id"`
	UserID         string    `json:"user_id"`
	Channel        string    `json:"channel"`
	Status         Status    `json:"status"`
	CreatedAt      time.Time `json:"created_at"`
	LastActivityAt time.Time `json:"last_activity_at"`
}

// Errors returned by registry operations.
var (
	ErrSessionNotFound = errors.New("session not found")
	ErrSessionClosed   = errors.New("session already closed")
)

// validChannels enumerates the accepted ingress channels.
var validChannels = map[string]bool{
	"cli":    true,
	"vscode": true,
	"web":    true,
	"api":    true,
}

// Registry is a goroutine-safe, in-memory store of sessions.
type Registry struct {
	mu       sync.RWMutex
	sessions map[string]*Session
}

// NewRegistry constructs an empty session registry.
func NewRegistry() *Registry {
	return &Registry{
		sessions: make(map[string]*Session),
	}
}

// Register creates a new active session for the given org, user, and channel.
// It returns the created session or an error if the channel is invalid.
func (r *Registry) Register(orgID, userID, channel string) (*Session, error) {
	if orgID == "" {
		return nil, fmt.Errorf("org_id is required")
	}
	if userID == "" {
		return nil, fmt.Errorf("user_id is required")
	}
	if !validChannels[channel] {
		return nil, fmt.Errorf("unsupported channel %q", channel)
	}

	id, err := generateID()
	if err != nil {
		return nil, fmt.Errorf("failed to generate session id: %w", err)
	}

	now := time.Now().UTC()
	s := &Session{
		ID:             id,
		OrgID:          orgID,
		UserID:         userID,
		Channel:        channel,
		Status:         StatusActive,
		CreatedAt:      now,
		LastActivityAt: now,
	}

	r.mu.Lock()
	r.sessions[id] = s
	r.mu.Unlock()

	return s, nil
}

// Get retrieves a session by ID.
func (r *Registry) Get(id string) (*Session, error) {
	r.mu.RLock()
	s, ok := r.sessions[id]
	r.mu.RUnlock()
	if !ok {
		return nil, ErrSessionNotFound
	}
	return s, nil
}

// List returns all sessions belonging to the given org. Closed sessions are
// included so callers can observe full lifecycle history.
func (r *Registry) List(orgID string) []*Session {
	r.mu.RLock()
	defer r.mu.RUnlock()

	var result []*Session
	for _, s := range r.sessions {
		if s.OrgID == orgID {
			result = append(result, s)
		}
	}
	return result
}

// UpdateActivity bumps the session's LastActivityAt timestamp and resets its
// status to active. Returns an error if the session is closed or not found.
func (r *Registry) UpdateActivity(id string) error {
	r.mu.Lock()
	defer r.mu.Unlock()

	s, ok := r.sessions[id]
	if !ok {
		return ErrSessionNotFound
	}
	if s.Status == StatusClosed {
		return ErrSessionClosed
	}
	s.LastActivityAt = time.Now().UTC()
	s.Status = StatusActive
	return nil
}

// Close marks a session as closed. Returns an error if the session does not
// exist or is already closed.
func (r *Registry) Close(id string) error {
	r.mu.Lock()
	defer r.mu.Unlock()

	s, ok := r.sessions[id]
	if !ok {
		return ErrSessionNotFound
	}
	if s.Status == StatusClosed {
		return ErrSessionClosed
	}
	s.Status = StatusClosed
	s.LastActivityAt = time.Now().UTC()
	return nil
}

// generateID returns a 16-byte hex-encoded random identifier.
func generateID() (string, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "", fmt.Errorf("failed to read random bytes: %w", err)
	}
	return hex.EncodeToString(b), nil
}
