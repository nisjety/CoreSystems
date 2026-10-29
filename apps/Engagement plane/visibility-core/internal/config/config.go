package config

import (
	"fmt"
	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/orgcore"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

var organizationPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)

type Config struct {
	Port                       int
	InternalAPIKey             string
	IntegrationCoreURL         string
	GoogleConsentReturnURL     string
	GoogleOAuthCallbackURL     string
	GoogleConnectionID         string
	GoogleGSCProperty          string
	IntegrationBearerTokenFile string
	OrganizationID             string
	WebsitesFile               string
	InitialSite                string
	QuarryURL                  string
	QuarryBearerTokenFile      string
	OrgCoreURL                 string
	OrgCoreServiceID           string
	OrgCoreServiceToken        string
}

func Load() (Config, error) {
	port := 3165
	if raw := strings.TrimSpace(os.Getenv("PORT")); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil || parsed < 1 || parsed > 65535 {
			return Config{}, fmt.Errorf("PORT must be between 1 and 65535")
		}
		port = parsed
	}
	key := strings.TrimSpace(os.Getenv("INTERNAL_API_KEY"))
	if key == "" {
		return Config{}, fmt.Errorf("INTERNAL_API_KEY is required")
	}
	cfg := Config{
		Port:                       port,
		InternalAPIKey:             key,
		IntegrationCoreURL:         strings.TrimSpace(os.Getenv("INTEGRATION_CORE_URL")),
		GoogleConnectionID:         strings.TrimSpace(os.Getenv("GOOGLE_CONNECTION_ID")),
		GoogleGSCProperty:          strings.TrimSpace(os.Getenv("GOOGLE_GSC_PROPERTY")),
		IntegrationBearerTokenFile: strings.TrimSpace(os.Getenv("INTEGRATION_BEARER_TOKEN_FILE")),
		OrganizationID:             strings.TrimSpace(os.Getenv("MARKETING_ORGANIZATION_ID")),
		WebsitesFile:               strings.TrimSpace(os.Getenv("MARKETING_WEBSITES_FILE")),
		InitialSite:                strings.TrimSpace(os.Getenv("MARKETING_INITIAL_SITE")),
		QuarryURL:                  strings.TrimSpace(os.Getenv("MARKETING_QUARRY_URL")),
		QuarryBearerTokenFile:      strings.TrimSpace(os.Getenv("MARKETING_QUARRY_BEARER_TOKEN_FILE")),
		OrgCoreURL:                 strings.TrimSpace(os.Getenv("ORG_CORE_URL")),
		OrgCoreServiceID:           strings.TrimSpace(os.Getenv("ORG_CORE_SERVICE_ID")),
		OrgCoreServiceToken:        strings.TrimSpace(os.Getenv("ORG_CORE_SERVICE_TOKEN")),
	}
	var err error
	cfg.GoogleConsentReturnURL, cfg.GoogleOAuthCallbackURL, err = pulse.ResolveGoogleConsentURLs(os.Getenv("MARKETING_GOOGLE_CONSENT_RETURN_URL"), os.Getenv("MARKETING_GOOGLE_OAUTH_CALLBACK_URL"))
	if err != nil {
		return Config{}, fmt.Errorf("Google consent configuration: %w", err)
	}
	if cfg.IntegrationCoreURL != "" || cfg.GoogleConnectionID != "" || cfg.GoogleGSCProperty != "" || cfg.IntegrationBearerTokenFile != "" {
		if err := pulse.ValidateConfig(pulse.Config{
			IntegrationCoreURL:         cfg.IntegrationCoreURL,
			GoogleConnectionID:         cfg.GoogleConnectionID,
			GoogleGSCProperty:          cfg.GoogleGSCProperty,
			IntegrationBearerTokenFile: cfg.IntegrationBearerTokenFile,
		}); err != nil {
			return Config{}, fmt.Errorf("pulse source configuration: %w", err)
		}
	}
	if cfg.OrganizationID != "" || cfg.WebsitesFile != "" || cfg.InitialSite != "" {
		if !organizationPattern.MatchString(cfg.OrganizationID) || !filepath.IsAbs(cfg.WebsitesFile) || filepath.Clean(cfg.WebsitesFile) != cfg.WebsitesFile {
			return Config{}, fmt.Errorf("website registry requires a fixed organization and absolute storage file")
		}
		if cfg.InitialSite != "" {
			site, err := pulse.CanonicalSite(cfg.InitialSite)
			if err != nil || site != cfg.InitialSite {
				return Config{}, fmt.Errorf("MARKETING_INITIAL_SITE must be a canonical HTTPS origin")
			}
		}
	}
	if cfg.OrgCoreURL != "" || cfg.OrgCoreServiceToken != "" {
		if _, err := orgcore.NewClient(orgcore.Config{URL: cfg.OrgCoreURL, ServiceID: cfg.OrgCoreServiceID, Token: cfg.OrgCoreServiceToken, OrganizationID: cfg.OrganizationID}, nil); err != nil {
			return Config{}, fmt.Errorf("org-core configuration: %w", err)
		}
	}
	if cfg.QuarryURL != "" || cfg.QuarryBearerTokenFile != "" {
		if cfg.WebsitesFile == "" || cfg.QuarryBearerTokenFile == cfg.IntegrationBearerTokenFile {
			return Config{}, fmt.Errorf("Quarry requires the website registry and a separate Quarry bearer file")
		}
		if _, err := discovery.NewClient(discovery.ClientConfig{URL: cfg.QuarryURL, TokenFile: cfg.QuarryBearerTokenFile, OrganizationID: cfg.OrganizationID}, nil); err != nil {
			return Config{}, fmt.Errorf("Quarry discovery configuration: %w", err)
		}
	}
	return cfg, nil
}
