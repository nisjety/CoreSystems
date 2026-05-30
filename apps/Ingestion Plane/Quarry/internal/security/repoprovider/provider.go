package repoprovider

import (
	"context"
	"net"
	"time"
)

// ReputationLevel represents the security threat level
type ReputationLevel int

const (
	Clean ReputationLevel = iota
	Suspicious
	Blacklisted
	Unknown
)

func (r ReputationLevel) String() string {
	switch r {
	case Clean:
		return "Clean"
	case Suspicious:
		return "Suspicious"
	case Blacklisted:
		return "Blacklisted"
	default:
		return "Unknown"
	}
}

// ReputationResult represents the result from a reputation check
type ReputationResult struct {
	Provider     string            `json:"provider"`
	URL          string            `json:"url,omitempty"`
	IP           string            `json:"ip,omitempty"`
	Domain       string            `json:"domain,omitempty"`
	Level        ReputationLevel   `json:"level"`
	Score        float64           `json:"score"`                // 0-1, where 1 is most dangerous
	Confidence   float64           `json:"confidence"`           // 0-1, how confident the provider is
	Categories   []string          `json:"categories,omitempty"` // e.g., "phishing", "malware", "spam"
	Description  string            `json:"description,omitempty"`
	LastSeen     *time.Time        `json:"last_seen,omitempty"`
	FirstSeen    *time.Time        `json:"first_seen,omitempty"`
	Metadata     map[string]string `json:"metadata,omitempty"`
	CheckedAt    time.Time         `json:"checked_at"`
	ResponseTime time.Duration     `json:"response_time"`
	Error        string            `json:"error,omitempty"`
}

// URLProvider defines interface for URL reputation providers
type URLProvider interface {
	CheckURL(ctx context.Context, url string) (*ReputationResult, error)
	CheckDomain(ctx context.Context, domain string) (*ReputationResult, error)
	Name() string
	IsAvailable() bool
	GetRateLimit() RateLimit
}

// IPProvider defines interface for IP reputation providers
type IPProvider interface {
	CheckIP(ctx context.Context, ip net.IP) (*ReputationResult, error)
	Name() string
	IsAvailable() bool
	GetRateLimit() RateLimit
}

// RateLimit represents API rate limiting information
type RateLimit struct {
	RequestsPerMinute int
	RequestsPerHour   int
	RequestsPerDay    int
	BurstAllowed      int
	ResetTime         *time.Time
}

// ProviderConfig holds configuration for reputation providers
type ProviderConfig struct {
	Name       string            `json:"name"`
	Enabled    bool              `json:"enabled"`
	APIKey     string            `json:"api_key,omitempty"`
	BaseURL    string            `json:"base_url,omitempty"`
	Timeout    time.Duration     `json:"timeout"`
	RetryCount int               `json:"retry_count"`
	CacheTTL   time.Duration     `json:"cache_ttl"`
	Priority   int               `json:"priority"` // Lower number = higher priority
	Settings   map[string]string `json:"settings,omitempty"`
}
