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
	// DefaultMaxRunDomains bounds the hosts of one derived run grant, and how
	// many hosts a run may inherit from its parent without naming them.
	DefaultMaxRunDomains = grant.DefaultMaxRunDomains
	// hardMaxSessionDomains is the broker's absolute bound (grant.NormalizeAllowedDomains).
	hardMaxSessionDomains = 32
	// MaxHumanAbsoluteTTL clamps BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS.
	MaxHumanAbsoluteTTL = 24 * time.Hour
	// DefaultMaxAttachDomains bounds the hosts of one attach grant.
	DefaultMaxAttachDomains = grant.DefaultMaxAttachDomains
	// INNH-10 key grants: own lifetime (default 600 s, clamped 30-900 s, never
	// renewed) and a live-grant cap per API key.
	DefaultKeyGrantTTL           = 10 * time.Minute
	MinKeyGrantTTL               = 30 * time.Second
	MaxKeyGrantTTL               = 15 * time.Minute
	DefaultMaxActiveGrantsPerKey = grant.DefaultMaxActivePerKey
)

// Environment variables read by ConfigFromEnv.
const (
	EnvGrantTTLSeconds        = "BROWSER_BROKER_GRANT_TTL_SECONDS"
	EnvMaxSessionDomains      = "BROWSER_BROKER_MAX_SESSION_DOMAINS"
	EnvMaxActiveGrantsPerUser = "BROWSER_BROKER_MAX_ACTIVE_GRANTS_PER_OWNER"
	EnvMaxRunDomains          = "BROWSER_BROKER_MAX_RUN_DOMAINS"
	// WS8.C: human_browse grants, renewal and attach grants.
	EnvHumanBrowseEnabled      = "BROWSER_BROKER_HUMAN_BROWSE_ENABLED"
	EnvHumanAbsoluteTTLSeconds = "BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS"
	EnvDenySuffixes            = "BROWSER_BROKER_DENY_SUFFIXES"
	EnvMaxAttachDomains        = "BROWSER_BROKER_MAX_ATTACH_DOMAINS"
	// INNH-10: API-key grants. Off unless the switch is exactly 1, true or on.
	EnvKeyGrantsEnabled      = "BROWSER_BROKER_KEY_GRANTS_ENABLED"
	EnvKeyGrantTTLSeconds    = "BROWSER_BROKER_KEY_GRANT_TTL_SECONDS"
	EnvMaxActiveGrantsPerKey = "BROWSER_BROKER_MAX_ACTIVE_GRANTS_PER_KEY"
)

// Config holds the limits applied when issuing grants.
type Config struct {
	// TTL is the lifetime of a new grant.
	TTL time.Duration
	// MaxActivePerOwner caps live ordinary grants per owner. It must be positive.
	MaxActivePerOwner int
	// MaxSessionDomains is the hosts-per-grant bound the default Policy applies.
	MaxSessionDomains int
	// MaxRunDomains bounds a derived run grant's hosts (explicit or inherited).
	MaxRunDomains int
	// HumanBrowseEnabled issues human_browse (open-web) grants. Off by default
	// (BROWSER_BROKER_HUMAN_BROWSE_ENABLED), so a broker that is not told
	// otherwise never mints one.
	HumanBrowseEnabled bool
	// HumanAbsoluteTTL is the hard cap of a human_browse session, counted from
	// its creation; renewals extend it by TTL at a time up to this.
	HumanAbsoluteTTL time.Duration
	// DenySuffixes is the human_browse deny-list; nil uses
	// grant.DefaultDeniedSuffixes.
	DenySuffixes []string
	// MaxAttachDomains bounds the hosts of one attach grant.
	MaxAttachDomains int
	// Policy decides which hosts may be granted. Nil means a StaticPolicy with
	// MaxSessionDomains.
	Policy grant.PolicyResolver
	// KeyGrantsEnabled lets an api_key principal acquire a key grant
	// (INNH-10, BROWSER_BROKER_KEY_GRANTS_ENABLED). Off by default: every
	// api_key call is PermissionDenied and an existing key grant validates as
	// revoked, so turning it off is also the kill switch.
	KeyGrantsEnabled bool
	// KeyGrantTTL is a key grant's lifetime; a request may only shorten it.
	KeyGrantTTL time.Duration
	// MaxActivePerKey caps live grants per API key. Key grants count against
	// the owner's MaxActivePerOwner too.
	MaxActivePerKey int
}

// DefaultConfig returns the defaults.
func DefaultConfig() Config {
	return Config{
		TTL:               DefaultGrantTTL,
		MaxActivePerOwner: DefaultMaxActiveGrantsPerOwner,
		MaxSessionDomains: DefaultMaxSessionDomains,
		MaxRunDomains:     DefaultMaxRunDomains,
		HumanAbsoluteTTL:  grant.DefaultHumanAbsoluteTTL,
		MaxAttachDomains:  DefaultMaxAttachDomains,
		KeyGrantTTL:       DefaultKeyGrantTTL,
		MaxActivePerKey:   DefaultMaxActiveGrantsPerKey,
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
	if v, ok, err := positiveInt(getenv, EnvMaxRunDomains); err != nil {
		return Config{}, err
	} else if ok {
		cfg.MaxRunDomains = min(v, hardMaxSessionDomains)
	}
	enabled, err := boolEnv(getenv, EnvHumanBrowseEnabled)
	if err != nil {
		return Config{}, err
	}
	cfg.HumanBrowseEnabled = enabled
	if v, ok, err := positiveInt(getenv, EnvHumanAbsoluteTTLSeconds); err != nil {
		return Config{}, err
	} else if ok {
		cfg.HumanAbsoluteTTL = clampDuration(time.Duration(v)*time.Second, MinGrantTTL, MaxHumanAbsoluteTTL)
	}
	if v, ok, err := positiveInt(getenv, EnvMaxAttachDomains); err != nil {
		return Config{}, err
	} else if ok {
		cfg.MaxAttachDomains = min(v, hardMaxSessionDomains)
	}
	cfg.KeyGrantsEnabled = switchOn(getenv(EnvKeyGrantsEnabled))
	if v, ok, err := positiveInt(getenv, EnvKeyGrantTTLSeconds); err != nil {
		return Config{}, err
	} else if ok {
		cfg.KeyGrantTTL = clampDuration(time.Duration(v)*time.Second, MinKeyGrantTTL, MaxKeyGrantTTL)
	}
	if v, ok, err := positiveInt(getenv, EnvMaxActiveGrantsPerKey); err != nil {
		return Config{}, err
	} else if ok {
		cfg.MaxActivePerKey = v
	}
	if raw := strings.TrimSpace(getenv(EnvDenySuffixes)); raw != "" {
		denied, err := grant.NormalizeDeniedDomains(strings.Split(raw, ","))
		if err != nil {
			return Config{}, fmt.Errorf("%s must be a comma-separated list of DNS names, each optionally with a leading dot", EnvDenySuffixes)
		}
		cfg.DenySuffixes = denied
	}
	return cfg, nil
}

// boolEnv reads a switch: unset or empty is off, "1" or "true" is on, "0" or
// "false" is off, anything else is an error (a typo must stop the broker, not
// silently turn open-web browsing on or off).
func boolEnv(getenv func(string) string, name string) (bool, error) {
	switch strings.ToLower(strings.TrimSpace(getenv(name))) {
	case "", "0", "false":
		return false, nil
	case "1", "true":
		return true, nil
	default:
		return false, fmt.Errorf("%s must be 1, 0, true or false", name)
	}
}

// switchOn reads a default-off switch: exactly 1, true or on (any case) turns
// it on; anything else, a typo included, leaves it off.
func switchOn(raw string) bool {
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case "1", "true", "on":
		return true
	default:
		return false
	}
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
