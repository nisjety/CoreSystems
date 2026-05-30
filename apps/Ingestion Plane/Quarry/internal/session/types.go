package session

import (
	"encoding/base64"
	"time"
)

// CreateOptions holds options for creating a new browser session.
type CreateOptions struct {
	Viewport *ViewportConfig `json:"viewport,omitempty"`
	Mobile   bool            `json:"mobile,omitempty"`
	Profile  string          `json:"profile,omitempty"`
	OrgID    string          `json:"org_id,omitempty"`
	UserID   string          `json:"user_id,omitempty"`
}

// ViewportConfig overrides browser viewport dimensions.
type ViewportConfig struct {
	Width             int     `json:"width"`
	Height            int     `json:"height"`
	DeviceScaleFactor float64 `json:"device_scale_factor,omitempty"`
}

// SessionInfo is a JSON-safe summary of a session (no internal pointers).
type SessionInfo struct {
	ID        string    `json:"id"`
	URL       string    `json:"url"`
	Profile   string    `json:"profile,omitempty"`
	OrgID     string    `json:"org_id,omitempty"`
	UserID    string    `json:"user_id,omitempty"`
	CreatedAt time.Time `json:"created_at"`
	ExpiresAt time.Time `json:"expires_at"`
	LastUsed  time.Time `json:"last_used"`
	StepCount int       `json:"step_count"`
}

func encodeBase64(data []byte) string {
	return base64.StdEncoding.EncodeToString(data)
}
