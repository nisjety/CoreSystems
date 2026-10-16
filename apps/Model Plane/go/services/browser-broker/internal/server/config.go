package server

import (
	"fmt"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
)

const (
	// DefaultGrantTTL is how long a grant stays valid unless configured.
	DefaultGrantTTL = 15 * time.Minute
	// MinGrantTTL and MaxGrantTTL clamp BROWSER_BROKER_GRANT_TTL_SECONDS.
	MinGrantTTL = 30 * time.Second
	MaxGrantTTL = time.Hour
	// DefaultMaxSessionDomains is the default hosts-per-grant bound.
	DefaultMaxSessionDomains = grant.DefaultMaxSessionDomains
	// DefaultMaxActiveGrantsPerOwner bounds how many live grants one user holds.
	DefaultMaxActiveGrantsPerOwner = 20
	// hardMaxSessionDomains is the broker's absolute bound (grant.NormalizeAllowedDomains).
	hardMaxSessionDomains = 32
)

// Environment variables read by ConfigFromEnv.
const (
	EnvGrantTTLSeconds        = "BROWSER_BROKER_GRANT_TTL_SECONDS"
	EnvMaxSessionDomains      = "BROWSER_BROKER_MAX_SESSION_DOMAINS"
	EnvMaxActiveGrantsPerUser = "BROWSER_BROKER_MAX_ACTIVE_GRANTS_PER_OWNER"
)

// Config holds the limits applied when issuing grants.
type Config struct {
	// TTL is the lifetime of a new grant.
	TTL time.Duration
	// MaxActivePerOwner caps live ordinary grants per owner. It must be positive.
	MaxActivePerOwner int
	// MaxSessionDomains is the hosts-per-grant bound the default Policy applies.
	MaxSessionDomains int
	// Policy decides which hosts may be granted. Nil means a StaticPolicy with
	// MaxSessionDomains.
	Policy grant.PolicyResolver
}

// DefaultConfig returns the defaults.
func DefaultConfig() Config {
	return Config{
		TTL:               DefaultGrantTTL,
		MaxActivePerOwner: DefaultMaxActiveGrantsPerOwner,
		MaxSessionDomains: DefaultMaxSessionDomains,
	}
}

// ConfigFromEnv reads the limits from the environment. An unset or empty
// variable uses its default; a value that is not a positive integer is an
// error (a typo must stop the broker, not silently loosen a limit). The TTL
// is clamped to [MinGrantTTL, MaxGrantTTL] and the domain bound to the
// broker's hard maximum.
func ConfigFromEnv(getenv func(string) string) (Config, error) {
	cfg := DefaultConfig()
	if v, ok, err := positiveInt(getenv, EnvGrantTTLSeconds); err != nil {
		return Config{}, err
	} else if ok {
		cfg.TTL = clampDuration(time.Duration(v)*time.Second, MinGrantTTL, MaxGrantTTL)
	}
	if v, ok, err := positiveInt(getenv, EnvMaxSessionDomains); err != nil {
		return Config{}, err
	} else if ok {
		cfg.MaxSessionDomains = min(v, hardMaxSessionDomains)
	}
	if v, ok, err := positiveInt(getenv, EnvMaxActiveGrantsPerUser); err != nil {
		return Config{}, err
	} else if ok {
		cfg.MaxActivePerOwner = v
	}
	return cfg, nil
}

func positiveInt(getenv func(string) string, name string) (int, bool, error) {
	raw := strings.TrimSpace(getenv(name))
	if raw == "" {
		return 0, false, nil
	}
	v, err := strconv.Atoi(raw)
	if err != nil || v < 1 {
		return 0, false, fmt.Errorf("%s must be a positive integer", name)
	}
	return v, true, nil
}

func clampDuration(d, lo, hi time.Duration) time.Duration {
	return min(max(d, lo), hi)
}
