// Package config loads anbud-core's settings from the environment.
//
// Only what the service cannot run without is required (its own API key and
// database). Source credentials such as DOFFIN_API_KEY are optional on
// purpose: the plane must be able to come up before an operator has pasted a
// key in, and the service then reports the source as "not configured" instead
// of crashing. Secrets are held as secret.Value so they cannot be logged.
package config

import (
	"fmt"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/anbud/internal/orgcore"
	"github.com/triodelab/anbud/internal/secret"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

// EnvMigrateDSN names the optional variable that holds the DSN of the schema
// owner. Migrations run through it when it is set (docs/ROLES.md). It is declared
// here once; the commands that name the variable in a message take it from here.
const EnvMigrateDSN = "ANBUD_MIGRATE_DSN"

const (
	defaultPort              = "3138"
	defaultEnvironment       = "dev"
	defaultServiceName       = "anbud-core"
	defaultAPIKeyHeader      = "X-API-Key"
	defaultDoffinBaseURL     = "https://api.doffin.no/public/v2"
	defaultDoffinSearchRPM   = 24 // 80% of the documented 30 requests/minute per key
	defaultDoffinDownloadRPM = 8  // 80% of the documented 10 requests/minute per key
	defaultDoffinTimeout     = 30 * time.Second
)

// Config is the service configuration.
type Config struct {
	Port         string
	Environment  string
	ServiceName  string
	APIKey       secret.Value
	APIKeyHeader string
	DatabaseURL  secret.Value // the DSN carries the database password
	// SkipMigrations is set by split-role deployments. The API never receives
	// the schema owner's credential; a separate --migrate-only job runs first.
	SkipMigrations bool
	// MigrateDatabaseURL is the optional ANBUD_MIGRATE_DSN, the schema owner's
	// DSN. When it is set the migrations run through it and DatabaseURL is the
	// runtime pool; when it is unset DatabaseURL does both.
	MigrateDatabaseURL secret.Value

	OTELExporterEndpoint string

	Doffin     Doffin
	OrgCore    OrgCore
	TenderDocs TenderDocs
}

// OrgCore holds the settings for organisation lookups through the Control Plane's org-core
// (internal/orgcore). Optional: without them the lookup reports "not configured".
type OrgCore struct {
	URL   string
	Token secret.Value
}

// Configured reports whether both the URL and the token are set.
func (o OrgCore) Configured() bool { return o.URL != "" && o.Token.IsSet() }

// Doffin holds the settings for the Doffin Public API source.
type Doffin struct {
	APIKey secret.Value
	// SecondaryAPIKey is the subscription's second key, kept for rotation. Only
	// LoadDoffinFrom (cmd/doffin-probe) reads it: the service never holds it.
	SecondaryAPIKey secret.Value
	BaseURL         string
	SearchRPM       int
	DownloadRPM     int
	HTTPTimeout     time.Duration
}

// Configured reports whether a Doffin subscription key is present.
func (d Doffin) Configured() bool { return d.APIKey.IsSet() }

// Load reads the configuration from the process environment.
func Load() (Config, error) { return LoadFrom(os.Getenv) }

// LoadFrom reads the configuration through get, so tests need not touch the
// process environment. Every problem is reported at once, and secret values
// are never echoed.
func LoadFrom(get func(string) string) (Config, error) {
	var problems []string

	apiKey := trimmed(get, "ANBUD_API_KEY")
	if apiKey == "" {
		problems = append(problems, "ANBUD_API_KEY is required")
	}
	dsn := trimmed(get, "ANBUD_DSN")
	if dsn == "" {
		problems = append(problems, "ANBUD_DSN is required")
	}
	skipMigrations := trimmed(get, "ANBUD_SKIP_MIGRATIONS")
	if skipMigrations != "" && skipMigrations != "true" && skipMigrations != "false" {
		problems = append(problems, "ANBUD_SKIP_MIGRATIONS must be true or false")
	}

	port := valueOr(get, "PORT", defaultPort)
	if n, err := strconv.Atoi(port); err != nil || n < 1 || n > 65535 {
		// Rejected values are never echoed: a key pasted into the wrong variable
		// would otherwise be printed back by the error that rejects it.
		problems = append(problems, "PORT must be a TCP port between 1 and 65535")
	}

	doffin, doffinProblems := loadDoffin(get, false)
	problems = append(problems, doffinProblems...)
	orgCore, orgCoreProblem := loadOrgCore(get)
	if orgCoreProblem != "" {
		problems = append(problems, orgCoreProblem)
	}
	tenderDocs, tenderDocsProblems := loadTenderDocs(get)
	problems = append(problems, tenderDocsProblems...)

	if len(problems) > 0 {
		return Config{}, fmt.Errorf("config: %s", strings.Join(problems, "; "))
	}

	return Config{
		Port:                 port,
		Environment:          valueOr(get, "ENVIRONMENT", defaultEnvironment),
		ServiceName:          valueOr(get, "SERVICE_NAME", defaultServiceName),
		APIKey:               secret.Value(apiKey),
		APIKeyHeader:         valueOr(get, "ANBUD_API_KEY_HEADER", defaultAPIKeyHeader),
		DatabaseURL:          secret.Value(dsn),
		SkipMigrations:       skipMigrations == "true",
		MigrateDatabaseURL:   secret.Value(trimmed(get, EnvMigrateDSN)),
		OTELExporterEndpoint: trimmed(get, "OTEL_EXPORTER_OTLP_ENDPOINT"),
		Doffin:               doffin,
		OrgCore:              orgCore,
		TenderDocs:           tenderDocs,
	}, nil
}

// loadOrgCore reads ANBUD_ORG_CORE_URL and ANBUD_ORG_CORE_TOKEN. Both or neither: one without
// the other is a mistake, not a choice. When both are set they are checked as the client checks
// them, so a value org-core would refuse stops the service at start-up. Values are never echoed.
func loadOrgCore(get func(string) string) (OrgCore, string) {
	o := OrgCore{URL: trimmed(get, "ANBUD_ORG_CORE_URL"), Token: secret.Value(trimmed(get, "ANBUD_ORG_CORE_TOKEN"))}
	switch {
	case o.URL == "" && !o.Token.IsSet():
		return OrgCore{}, ""
	case o.URL == "" || !o.Token.IsSet():
		return OrgCore{}, "ANBUD_ORG_CORE_URL and ANBUD_ORG_CORE_TOKEN must be set together"
	}
	if _, err := orgcore.New(orgcore.Config{BaseURL: o.URL, Token: o.Token}); err != nil {
		return OrgCore{}, "ANBUD_ORG_CORE_URL or ANBUD_ORG_CORE_TOKEN is unusable: the URL must be an http(s) origin without credentials, and the token 32-512 characters and not a placeholder"
	}
	return o, ""
}

// LoadDoffinFrom reads only the Doffin settings. Command-line tools such as
// doffin-probe use it so they do not need the service's own key or database.
func LoadDoffinFrom(get func(string) string) (Doffin, error) {
	d, problems := loadDoffin(get, true)
	if len(problems) > 0 {
		return Doffin{}, fmt.Errorf("config: %s", strings.Join(problems, "; "))
	}
	return d, nil
}

// LoadDoffin reads the Doffin settings from the process environment.
func LoadDoffin() (Doffin, error) { return LoadDoffinFrom(os.Getenv) }

// loadDoffin reads the Doffin settings. The secondary key is read only when
// withSecondary is set, so the service never holds it, and a stray value in its
// environment can neither leak it nor stop the service starting.
func loadDoffin(get func(string) string, withSecondary bool) (Doffin, []string) {
	var problems []string

	// The documented per-key limits are enforced here, at load time, so a value
	// that would guarantee 429s fails at the earliest moment and names its variable.
	searchRPM, problem := boundedInt(get, "DOFFIN_SEARCH_RPM", defaultDoffinSearchRPM, doffin.MaxSearchRPM)
	if problem != "" {
		problems = append(problems, problem)
	}
	downloadRPM, problem := boundedInt(get, "DOFFIN_DOWNLOAD_RPM", defaultDoffinDownloadRPM, doffin.MaxDownloadRPM)
	if problem != "" {
		problems = append(problems, problem)
	}
	timeout, problem := positiveDuration(get, "DOFFIN_HTTP_TIMEOUT", defaultDoffinTimeout)
	if problem != "" {
		problems = append(problems, problem)
	}

	primary := trimmed(get, "DOFFIN_API_KEY")
	var secondary secret.Value
	if withSecondary {
		secondary = secret.Value(trimmed(get, "DOFFIN_API_KEY_SECONDARY"))
		// A secondary key on its own is almost certainly the two lines the wrong way
		// round; the probe would otherwise report "not configured" and hide that.
		if secondary != "" && primary == "" {
			problems = append(problems, "DOFFIN_API_KEY_SECONDARY is set but DOFFIN_API_KEY is not; the primary key belongs in DOFFIN_API_KEY")
		}
	}

	baseURL, problem := doffinBaseURL(get)
	if problem != "" {
		problems = append(problems, problem)
	}

	return Doffin{
		APIKey:          secret.Value(primary),
		SecondaryAPIKey: secondary,
		BaseURL:         baseURL,
		SearchRPM:       searchRPM,
		DownloadRPM:     downloadRPM,
		HTTPTimeout:     timeout,
	}, problems
}

// doffinBaseURL returns DOFFIN_BASE_URL, which defaults to the production API.
// Both subscription keys are sent to whatever host it names, so any other host
// must be confirmed by name in DOFFIN_BASE_URL_ALLOW_HOST, and loopback is
// allowed for local mocks. A rejected URL is never echoed: it could carry
// credentials.
func doffinBaseURL(get func(string) string) (string, string) {
	raw := valueOr(get, "DOFFIN_BASE_URL", defaultDoffinBaseURL)
	u, err := url.Parse(raw)
	if err != nil || u.Hostname() == "" {
		return "", "DOFFIN_BASE_URL must be an absolute URL"
	}
	host := u.Hostname()
	if strings.EqualFold(host, doffin.DefaultHost) || doffin.IsLoopbackHost(host) {
		return raw, ""
	}
	if allowed := trimmed(get, "DOFFIN_BASE_URL_ALLOW_HOST"); allowed != "" && strings.EqualFold(allowed, host) {
		return raw, ""
	}
	return "", "DOFFIN_BASE_URL must point at api.doffin.no; to use another host, name it exactly in DOFFIN_BASE_URL_ALLOW_HOST"
}

func trimmed(get func(string) string, key string) string { return strings.TrimSpace(get(key)) }

func valueOr(get func(string) string, key, fallback string) string {
	if v := trimmed(get, key); v != "" {
		return v
	}
	return fallback
}

// boundedInt reads a whole number from 1 to max. A rejected value is never
// echoed back; only the variable name is.
func boundedInt(get func(string) string, key string, fallback, max int) (int, string) {
	raw := trimmed(get, key)
	if raw == "" {
		return fallback, ""
	}
	n, err := strconv.Atoi(raw)
	if err != nil || n < 1 || n > max {
		return 0, fmt.Sprintf("%s must be a whole number from 1 to %d", key, max)
	}
	return n, ""
}

func positiveDuration(get func(string) string, key string, fallback time.Duration) (time.Duration, string) {
	raw := trimmed(get, key)
	if raw == "" {
		return fallback, ""
	}
	d, err := time.ParseDuration(raw)
	if err != nil || d <= 0 {
		return 0, fmt.Sprintf("%s must be a positive duration such as 30s", key)
	}
	return d, ""
}
