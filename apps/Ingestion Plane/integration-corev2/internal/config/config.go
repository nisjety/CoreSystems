package config

import (
	"encoding/base64"
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

type Config struct {
	Port                       string
	ServiceName                string
	Environment                string
	DatabaseURL                string
	InternalAPIKey             string
	InternalAPIKeyHeader       string
	AuthCoreURL                string
	AuthCoreInternalAPIKey     string
	UserCoreURL                string
	OrgCoreURL                 string
	SessionCoreURL             string
	BillingCoreURL             string
	AuditCoreURL               string
	FinspoCoreURL              string
	FinspoCoreAPIKey           string
	FinspoCoreAPIKeyHeader     string
	DataPlaneDocumentsURL      string
	DataPlaneInternalAPIKey    string
	DataPlaneInternalAPIHeader string
	DataPlaneGraphIndexURL     string
	WebhookHotPathURL          string
	IntegrationCoreURL         string
	PublicBaseURL              string
	EncryptionKey              []byte
	AllowInMemoryStore         bool
	NATSEnabled                bool
	NATSURL                    string
	NATSUsername               string
	NATSPassword               string
	NATSSubjectPrefix          string
	RateLimitEnabled           bool
	RateLimitMax               int
	RateLimitWindow            time.Duration
	TokenLeaseConsumers        []string
	MicrosoftTenantID          string
	MicrosoftClientID          string
	MicrosoftClientSecret      string
	MicrosoftAuthorizationURL  string
	MicrosoftTokenURL          string
	MicrosoftGraphBaseURL      string
	SlackClientID              string
	SlackClientSecret          string
	SlackSigningSecret         string
	SlackAuthorizationURL      string
	SlackTokenURL              string
	SlackAPIBaseURL            string
	GoogleClientID             string
	GoogleClientSecret         string
	GoogleAuthorizationURL     string
	GoogleTokenURL             string
	GoogleAPIBaseURL           string
	NotionClientID             string
	NotionClientSecret         string
	NotionAuthorizationURL     string
	NotionTokenURL             string
	NotionAPIBaseURL           string
	GitHubClientID             string
	GitHubClientSecret         string
	GitHubWebhookSecret        string
	GitHubAuthorizationURL     string
	GitHubTokenURL             string
	GitHubAPIBaseURL           string
	ShopifyClientID            string
	ShopifyClientSecret        string
	ShopifyWebhookSecret       string
	ShopifyAPIBaseURL          string
	StripeClientID             string
	StripeClientSecret         string
	StripeAuthorizationURL     string
	StripeTokenURL             string
	StripeAPIBaseURL           string
	StripeWebhookSecret        string
	LinkedInClientID           string
	LinkedInClientSecret       string
	LinkedInAuthorizationURL   string
	LinkedInTokenURL           string
	LinkedInAPIBaseURL         string
	XClientID                  string
	XClientSecret              string
	XAuthorizationURL          string
	XTokenURL                  string
	XAPIBaseURL                string
	InstagramClientID          string
	InstagramClientSecret      string
	InstagramAuthorizationURL  string
	InstagramTokenURL          string
	InstagramAPIBaseURL        string
	FacebookClientID           string
	FacebookClientSecret       string
	FacebookAuthorizationURL   string
	FacebookTokenURL           string
	FacebookAPIBaseURL         string
	SnapchatClientID           string
	SnapchatClientSecret       string
	SnapchatAuthorizationURL   string
	SnapchatTokenURL           string
	SnapchatAPIBaseURL         string
	OktaDomain                 string
	OktaClientID               string
	OktaClientSecret           string
	OktaAPIToken               string
	OktaAPIBaseURL             string
	SCIMBearerToken            string
	SCIMBearerTokens           map[string]string
	SessionTTL                 time.Duration
	TokenRefreshSkew           time.Duration
}

func Load() (Config, error) {
	encryptionKey, err := loadEncryptionKey("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY")
	if err != nil {
		return Config{}, err
	}

	publicBaseURL := strings.TrimRight(envOr("INTEGRATION_PUBLIC_BASE_URL", "http://localhost:3026"), "/")
	tenant := envOr("AZURE_TENANT_ID", "common")

	return Config{
		Port:                       envOr("PORT", "3026"),
		ServiceName:                envOr("SERVICE_NAME", "integration-corev2"),
		Environment:                envOr("ENVIRONMENT", "dev"),
		DatabaseURL:                strings.TrimSpace(os.Getenv("DATABASE_URL")),
		InternalAPIKey:             strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")),
		InternalAPIKeyHeader:       envOr("INTERNAL_API_KEY_HEADER", "X-Internal-API-Key"),
		AuthCoreURL:                envOr("AUTH_CORE_URL", "http://auth-core:3011"),
		AuthCoreInternalAPIKey:     strings.TrimSpace(envOr("AUTH_CORE_INTERNAL_API_KEY", strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")))),
		UserCoreURL:                envOr("USER_CORE_URL", "http://user-core:3012"),
		OrgCoreURL:                 envOr("ORG_CORE_URL", "http://org-core:8080"),
		SessionCoreURL:             envOr("SESSION_CORE_URL", "http://session-core:9091"),
		BillingCoreURL:             envOr("BILLING_CORE_URL", "http://billing-core:3014"),
		AuditCoreURL:               envOr("AUDIT_CORE_URL", "http://audit-core:3015"),
		FinspoCoreURL:              envOr("FINSPO_CORE_URL", envOr("FINSPO_API_URL", "http://finspo-api:3130")),
		FinspoCoreAPIKey:           strings.TrimSpace(os.Getenv("FINSPO_API_KEY")),
		FinspoCoreAPIKeyHeader:     envOr("FINSPO_API_KEY_HEADER", "X-API-Key"),
		DataPlaneDocumentsURL:      envOr("DATA_PLANE_DOCUMENTS_URL", envOr("DATA_PLANE_DOCUMENTS_BASE_URL", "http://dpv2-documents-api:8010")),
		DataPlaneInternalAPIKey:    strings.TrimSpace(envOr("DATA_PLANE_INTERNAL_API_KEY", strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")))),
		DataPlaneInternalAPIHeader: envOr("DATA_PLANE_INTERNAL_API_KEY_HEADER", "X-Internal-Api-Key"),
		DataPlaneGraphIndexURL:     envOr("DATA_PLANE_GRAPH_INDEX_URL", envOr("GRAPH_INDEX_URL", "http://dpv2-graph-index:9203")),
		WebhookHotPathURL:          strings.TrimRight(strings.TrimSpace(envOr("INTEGRATION_WEBHOOK_HOTPATH_URL", "")), "/"),
		IntegrationCoreURL:         strings.TrimRight(envOr("INTEGRATION_CORE_URL", publicBaseURL), "/"),
		PublicBaseURL:              publicBaseURL,
		EncryptionKey:              encryptionKey,
		AllowInMemoryStore:         envBool("INTEGRATION_ALLOW_IN_MEMORY_STORE", false),
		NATSEnabled:                envBool("NATS_ENABLED", false),
		NATSURL:                    envOr("NATS_URL", "nats://localhost:4222"),
		NATSUsername:               strings.TrimSpace(os.Getenv("NATS_USERNAME")),
		NATSPassword:               strings.TrimSpace(os.Getenv("NATS_PASSWORD")),
		NATSSubjectPrefix:          envOr("NATS_SUBJECT_PREFIX", ""),
		RateLimitEnabled:           envBool("INTEGRATION_RATE_LIMIT_ENABLED", true),
		RateLimitMax:               envInt("INTEGRATION_RATE_LIMIT_MAX", 120),
		RateLimitWindow:            envDuration("INTEGRATION_RATE_LIMIT_WINDOW", time.Minute),
		TokenLeaseConsumers:        envCSV("INTEGRATION_TOKEN_LEASE_CONSUMERS", "finspo-core,conversation-core,data-plane-v2,model-plane,application-plane,velion-v2-bff,velion-v3-gateway,social-publisher"),
		MicrosoftTenantID:          tenant,
		MicrosoftClientID:          envOr("AZURE_CLIENT_ID", os.Getenv("MICROSOFT_CLIENT_ID")),
		MicrosoftClientSecret:      envOr("AZURE_CLIENT_SECRET", os.Getenv("MICROSOFT_CLIENT_SECRET")),
		MicrosoftAuthorizationURL:  microsoftAuthorizeURL(tenant),
		MicrosoftTokenURL:          microsoftTokenURL(tenant),
		MicrosoftGraphBaseURL:      envOr("MICROSOFT_GRAPH_BASE_URL", "https://graph.microsoft.com"),
		SlackClientID:              strings.TrimSpace(os.Getenv("SLACK_CLIENT_ID")),
		SlackClientSecret:          strings.TrimSpace(os.Getenv("SLACK_CLIENT_SECRET")),
		SlackSigningSecret:         strings.TrimSpace(os.Getenv("SLACK_SIGNING_SECRET")),
		SlackAuthorizationURL:      envOr("SLACK_AUTHORIZATION_URL", "https://slack.com/oauth/v2/authorize"),
		SlackTokenURL:              envOr("SLACK_TOKEN_URL", "https://slack.com/api/oauth.v2.access"),
		SlackAPIBaseURL:            envOr("SLACK_API_BASE_URL", "https://slack.com/api"),
		GoogleClientID:             strings.TrimSpace(os.Getenv("GOOGLE_CLIENT_ID")),
		GoogleClientSecret:         strings.TrimSpace(os.Getenv("GOOGLE_CLIENT_SECRET")),
		GoogleAuthorizationURL:     envOr("GOOGLE_AUTHORIZATION_URL", "https://accounts.google.com/o/oauth2/v2/auth"),
		GoogleTokenURL:             envOr("GOOGLE_TOKEN_URL", "https://oauth2.googleapis.com/token"),
		GoogleAPIBaseURL:           envOr("GOOGLE_API_BASE_URL", "https://www.googleapis.com"),
		NotionClientID:             strings.TrimSpace(os.Getenv("NOTION_CLIENT_ID")),
		NotionClientSecret:         strings.TrimSpace(os.Getenv("NOTION_CLIENT_SECRET")),
		NotionAuthorizationURL:     envOr("NOTION_AUTHORIZATION_URL", "https://api.notion.com/v1/oauth/authorize"),
		NotionTokenURL:             envOr("NOTION_TOKEN_URL", "https://api.notion.com/v1/oauth/token"),
		NotionAPIBaseURL:           envOr("NOTION_API_BASE_URL", "https://api.notion.com"),
		GitHubClientID:             strings.TrimSpace(os.Getenv("GITHUB_CLIENT_ID")),
		GitHubClientSecret:         strings.TrimSpace(os.Getenv("GITHUB_CLIENT_SECRET")),
		GitHubWebhookSecret:        strings.TrimSpace(os.Getenv("GITHUB_WEBHOOK_SECRET")),
		GitHubAuthorizationURL:     envOr("GITHUB_AUTHORIZATION_URL", "https://github.com/login/oauth/authorize"),
		GitHubTokenURL:             envOr("GITHUB_TOKEN_URL", "https://github.com/login/oauth/access_token"),
		GitHubAPIBaseURL:           envOr("GITHUB_API_BASE_URL", "https://api.github.com"),
		ShopifyClientID:            strings.TrimSpace(os.Getenv("SHOPIFY_CLIENT_ID")),
		ShopifyClientSecret:        strings.TrimSpace(os.Getenv("SHOPIFY_CLIENT_SECRET")),
		ShopifyWebhookSecret:       strings.TrimSpace(os.Getenv("SHOPIFY_WEBHOOK_SECRET")),
		ShopifyAPIBaseURL:          envOr("SHOPIFY_API_BASE_URL", "https://{shop}"),
		StripeClientID:             strings.TrimSpace(os.Getenv("STRIPE_CLIENT_ID")),
		StripeClientSecret:         strings.TrimSpace(os.Getenv("STRIPE_CLIENT_SECRET")),
		StripeAuthorizationURL:     envOr("STRIPE_AUTHORIZATION_URL", "https://connect.stripe.com/oauth/authorize"),
		StripeTokenURL:             envOr("STRIPE_TOKEN_URL", "https://connect.stripe.com/oauth/token"),
		StripeAPIBaseURL:           envOr("STRIPE_API_BASE_URL", "https://api.stripe.com"),
		StripeWebhookSecret:        strings.TrimSpace(os.Getenv("STRIPE_WEBHOOK_SECRET")),
		LinkedInClientID:           strings.TrimSpace(os.Getenv("LINKEDIN_CLIENT_ID")),
		LinkedInClientSecret:       strings.TrimSpace(os.Getenv("LINKEDIN_CLIENT_SECRET")),
		LinkedInAuthorizationURL:   envOr("LINKEDIN_AUTHORIZATION_URL", "https://www.linkedin.com/oauth/v2/authorization"),
		LinkedInTokenURL:           envOr("LINKEDIN_TOKEN_URL", "https://www.linkedin.com/oauth/v2/accessToken"),
		LinkedInAPIBaseURL:         envOr("LINKEDIN_API_BASE_URL", "https://api.linkedin.com"),
		XClientID:                  strings.TrimSpace(os.Getenv("X_CLIENT_ID")),
		XClientSecret:              strings.TrimSpace(os.Getenv("X_CLIENT_SECRET")),
		XAuthorizationURL:          envOr("X_AUTHORIZATION_URL", "https://x.com/i/oauth2/authorize"),
		XTokenURL:                  envOr("X_TOKEN_URL", "https://api.x.com/2/oauth2/token"),
		XAPIBaseURL:                envOr("X_API_BASE_URL", "https://api.x.com"),
		InstagramClientID:          strings.TrimSpace(os.Getenv("INSTAGRAM_CLIENT_ID")),
		InstagramClientSecret:      strings.TrimSpace(os.Getenv("INSTAGRAM_CLIENT_SECRET")),
		InstagramAuthorizationURL:  envOr("INSTAGRAM_AUTHORIZATION_URL", "https://www.facebook.com/v23.0/dialog/oauth"),
		InstagramTokenURL:          envOr("INSTAGRAM_TOKEN_URL", "https://graph.facebook.com/v23.0/oauth/access_token"),
		InstagramAPIBaseURL:        envOr("INSTAGRAM_GRAPH_API_BASE_URL", "https://graph.facebook.com/v23.0"),
		FacebookClientID:           strings.TrimSpace(envOr("FACEBOOK_CLIENT_ID", os.Getenv("INSTAGRAM_CLIENT_ID"))),
		FacebookClientSecret:       strings.TrimSpace(envOr("FACEBOOK_CLIENT_SECRET", os.Getenv("INSTAGRAM_CLIENT_SECRET"))),
		FacebookAuthorizationURL:   envOr("FACEBOOK_AUTHORIZATION_URL", "https://www.facebook.com/v23.0/dialog/oauth"),
		FacebookTokenURL:           envOr("FACEBOOK_TOKEN_URL", "https://graph.facebook.com/v23.0/oauth/access_token"),
		FacebookAPIBaseURL:         envOr("FACEBOOK_GRAPH_API_BASE_URL", "https://graph.facebook.com/v23.0"),
		SnapchatClientID:           strings.TrimSpace(os.Getenv("SNAPCHAT_CLIENT_ID")),
		SnapchatClientSecret:       strings.TrimSpace(os.Getenv("SNAPCHAT_CLIENT_SECRET")),
		SnapchatAuthorizationURL:   envOr("SNAPCHAT_AUTHORIZATION_URL", "https://accounts.snapchat.com/login/oauth2/authorize"),
		SnapchatTokenURL:           envOr("SNAPCHAT_TOKEN_URL", "https://accounts.snapchat.com/login/oauth2/access_token"),
		SnapchatAPIBaseURL:         envOr("SNAPCHAT_API_BASE_URL", "https://adsapi.snapchat.com/v1"),
		OktaDomain:                 strings.TrimRight(strings.TrimSpace(os.Getenv("OKTA_DOMAIN")), "/"),
		OktaClientID:               strings.TrimSpace(os.Getenv("OKTA_CLIENT_ID")),
		OktaClientSecret:           strings.TrimSpace(os.Getenv("OKTA_CLIENT_SECRET")),
		OktaAPIToken:               strings.TrimSpace(os.Getenv("OKTA_API_TOKEN")),
		OktaAPIBaseURL:             strings.TrimRight(envOr("OKTA_API_BASE_URL", strings.TrimSpace(os.Getenv("OKTA_DOMAIN"))), "/"),
		SCIMBearerToken:            strings.TrimSpace(os.Getenv("SCIM_BEARER_TOKEN")),
		SCIMBearerTokens:           parseSCIMBearerTokens(os.Getenv("SCIM_ORG_BEARER_TOKENS")),
		SessionTTL:                 envDuration("INTEGRATION_CONNECT_SESSION_TTL", 10*time.Minute),
		TokenRefreshSkew:           envDuration("INTEGRATION_TOKEN_REFRESH_SKEW", 2*time.Minute),
	}, nil
}

func (c Config) ValidateRuntime() error {
	if c.InternalAPIKey == "" {
		return fmt.Errorf("INTERNAL_API_KEY is required")
	}
	if c.AuthCoreURL == "" {
		return fmt.Errorf("AUTH_CORE_URL is required")
	}
	if c.ControlPlaneInternalAPIKey() == "" {
		return fmt.Errorf("AUTH_CORE_INTERNAL_API_KEY or INTERNAL_API_KEY is required")
	}
	if c.OrgCoreURL == "" {
		return fmt.Errorf("ORG_CORE_URL is required")
	}
	if c.PublicBaseURL == "" {
		return fmt.Errorf("INTEGRATION_PUBLIC_BASE_URL is required")
	}
	if c.DatabaseURL == "" && !c.AllowInMemoryStore {
		return fmt.Errorf("DATABASE_URL is required unless INTEGRATION_ALLOW_IN_MEMORY_STORE=true")
	}
	if len(c.EncryptionKey) != 32 {
		return fmt.Errorf("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY must decode to 32 bytes")
	}
	return nil
}

func (c Config) ValidateFinspoWorkerRuntime() error {
	if c.InternalAPIKey == "" {
		return fmt.Errorf("INTERNAL_API_KEY is required")
	}
	if c.IntegrationCoreURL == "" {
		return fmt.Errorf("INTEGRATION_CORE_URL is required")
	}
	if c.FinspoCoreURL == "" {
		return fmt.Errorf("FINSPO_CORE_URL is required")
	}
	if c.FinspoCoreAPIKey == "" {
		return fmt.Errorf("FINSPO_API_KEY is required")
	}
	return nil
}

func (c Config) ControlPlaneInternalAPIKey() string {
	if strings.TrimSpace(c.AuthCoreInternalAPIKey) != "" {
		return strings.TrimSpace(c.AuthCoreInternalAPIKey)
	}
	return strings.TrimSpace(c.InternalAPIKey)
}

func (c Config) ValidateProvider(providerKey string) error {
	switch providerKey {
	case "microsoft":
		if c.MicrosoftClientID == "" {
			return fmt.Errorf("AZURE_CLIENT_ID is required for Microsoft OAuth")
		}
		if c.MicrosoftClientSecret == "" {
			return fmt.Errorf("AZURE_CLIENT_SECRET is required for Microsoft OAuth")
		}
	case "slack":
		if c.SlackClientID == "" {
			return fmt.Errorf("SLACK_CLIENT_ID is required for Slack OAuth")
		}
		if c.SlackClientSecret == "" {
			return fmt.Errorf("SLACK_CLIENT_SECRET is required for Slack OAuth")
		}
	case "google":
		if c.GoogleClientID == "" {
			return fmt.Errorf("GOOGLE_CLIENT_ID is required for Google OAuth")
		}
		if c.GoogleClientSecret == "" {
			return fmt.Errorf("GOOGLE_CLIENT_SECRET is required for Google OAuth")
		}
	case "notion":
		if c.NotionClientID == "" {
			return fmt.Errorf("NOTION_CLIENT_ID is required for Notion OAuth")
		}
		if c.NotionClientSecret == "" {
			return fmt.Errorf("NOTION_CLIENT_SECRET is required for Notion OAuth")
		}
	case "github":
		if c.GitHubClientID == "" {
			return fmt.Errorf("GITHUB_CLIENT_ID is required for GitHub OAuth")
		}
		if c.GitHubClientSecret == "" {
			return fmt.Errorf("GITHUB_CLIENT_SECRET is required for GitHub OAuth")
		}
	case "shopify":
		if c.ShopifyClientID == "" {
			return fmt.Errorf("SHOPIFY_CLIENT_ID is required for Shopify OAuth")
		}
		if c.ShopifyClientSecret == "" {
			return fmt.Errorf("SHOPIFY_CLIENT_SECRET is required for Shopify OAuth")
		}
	case "stripe":
		if c.StripeClientID == "" {
			return fmt.Errorf("STRIPE_CLIENT_ID is required for Stripe OAuth")
		}
		if c.StripeClientSecret == "" {
			return fmt.Errorf("STRIPE_CLIENT_SECRET is required for Stripe OAuth")
		}
	case "linkedin":
		if c.LinkedInClientID == "" {
			return fmt.Errorf("LINKEDIN_CLIENT_ID is required for LinkedIn OAuth")
		}
		if c.LinkedInClientSecret == "" {
			return fmt.Errorf("LINKEDIN_CLIENT_SECRET is required for LinkedIn OAuth")
		}
	case "x":
		if c.XClientID == "" {
			return fmt.Errorf("X_CLIENT_ID is required for X OAuth")
		}
		if c.XClientSecret == "" {
			return fmt.Errorf("X_CLIENT_SECRET is required for X OAuth")
		}
	case "instagram":
		if c.InstagramClientID == "" {
			return fmt.Errorf("INSTAGRAM_CLIENT_ID is required for Instagram OAuth")
		}
		if c.InstagramClientSecret == "" {
			return fmt.Errorf("INSTAGRAM_CLIENT_SECRET is required for Instagram OAuth")
		}
	case "facebook":
		if c.FacebookClientID == "" {
			return fmt.Errorf("FACEBOOK_CLIENT_ID or INSTAGRAM_CLIENT_ID is required for Facebook OAuth")
		}
		if c.FacebookClientSecret == "" {
			return fmt.Errorf("FACEBOOK_CLIENT_SECRET or INSTAGRAM_CLIENT_SECRET is required for Facebook OAuth")
		}
	case "snapchat":
		if c.SnapchatClientID == "" {
			return fmt.Errorf("SNAPCHAT_CLIENT_ID is required for Snapchat OAuth")
		}
		if c.SnapchatClientSecret == "" {
			return fmt.Errorf("SNAPCHAT_CLIENT_SECRET is required for Snapchat OAuth")
		}
	case "okta":
		if c.OktaAPIBaseURL == "" && c.OktaDomain == "" {
			return fmt.Errorf("OKTA_DOMAIN or OKTA_API_BASE_URL is required for Okta")
		}
		if c.OktaAPIToken == "" && (c.OktaClientID == "" || c.OktaClientSecret == "") {
			return fmt.Errorf("OKTA_API_TOKEN or OKTA_CLIENT_ID/OKTA_CLIENT_SECRET is required for Okta")
		}
	case "scim":
		if c.SCIMBearerToken == "" && len(c.SCIMBearerTokens) == 0 {
			return fmt.Errorf("SCIM_BEARER_TOKEN or SCIM_ORG_BEARER_TOKENS is required for SCIM")
		}
	}
	return nil
}

func (c Config) ProviderReadiness() map[string][]string {
	readiness := map[string][]string{}
	checks := map[string]map[string]string{
		"microsoft": {
			"AZURE_CLIENT_ID":     c.MicrosoftClientID,
			"AZURE_CLIENT_SECRET": c.MicrosoftClientSecret,
		},
		"google": {
			"GOOGLE_CLIENT_ID":     c.GoogleClientID,
			"GOOGLE_CLIENT_SECRET": c.GoogleClientSecret,
		},
		"slack": {
			"SLACK_CLIENT_ID":     c.SlackClientID,
			"SLACK_CLIENT_SECRET": c.SlackClientSecret,
		},
		"github": {
			"GITHUB_CLIENT_ID":     c.GitHubClientID,
			"GITHUB_CLIENT_SECRET": c.GitHubClientSecret,
		},
		"notion": {
			"NOTION_CLIENT_ID":     c.NotionClientID,
			"NOTION_CLIENT_SECRET": c.NotionClientSecret,
		},
		"shopify": {
			"SHOPIFY_CLIENT_ID":     c.ShopifyClientID,
			"SHOPIFY_CLIENT_SECRET": c.ShopifyClientSecret,
		},
		"stripe": {
			"STRIPE_CLIENT_ID":     c.StripeClientID,
			"STRIPE_CLIENT_SECRET": c.StripeClientSecret,
		},
		"linkedin": {
			"LINKEDIN_CLIENT_ID":     c.LinkedInClientID,
			"LINKEDIN_CLIENT_SECRET": c.LinkedInClientSecret,
		},
		"x": {
			"X_CLIENT_ID":     c.XClientID,
			"X_CLIENT_SECRET": c.XClientSecret,
		},
		"instagram": {
			"INSTAGRAM_CLIENT_ID":     c.InstagramClientID,
			"INSTAGRAM_CLIENT_SECRET": c.InstagramClientSecret,
		},
		"facebook": {
			"FACEBOOK_CLIENT_ID or INSTAGRAM_CLIENT_ID":         c.FacebookClientID,
			"FACEBOOK_CLIENT_SECRET or INSTAGRAM_CLIENT_SECRET": c.FacebookClientSecret,
		},
		"snapchat": {
			"SNAPCHAT_CLIENT_ID":     c.SnapchatClientID,
			"SNAPCHAT_CLIENT_SECRET": c.SnapchatClientSecret,
		},
	}
	for provider, providerChecks := range checks {
		for key, value := range providerChecks {
			if strings.TrimSpace(value) == "" {
				readiness[provider] = append(readiness[provider], key)
			}
		}
	}
	if strings.TrimSpace(c.OktaAPIBaseURL) == "" && strings.TrimSpace(c.OktaDomain) == "" {
		readiness["okta"] = append(readiness["okta"], "OKTA_DOMAIN")
	}
	if strings.TrimSpace(c.OktaAPIToken) == "" && (strings.TrimSpace(c.OktaClientID) == "" || strings.TrimSpace(c.OktaClientSecret) == "") {
		readiness["okta"] = append(readiness["okta"], "OKTA_API_TOKEN or OKTA_CLIENT_ID/OKTA_CLIENT_SECRET")
	}
	if strings.TrimSpace(c.SCIMBearerToken) == "" && len(c.SCIMBearerTokens) == 0 {
		readiness["scim"] = append(readiness["scim"], "SCIM_BEARER_TOKEN or SCIM_ORG_BEARER_TOKENS")
	}
	return readiness
}

func parseSCIMBearerTokens(raw string) map[string]string {
	out := map[string]string{}
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return out
	}
	for _, pair := range strings.Split(raw, ",") {
		orgID, token, ok := strings.Cut(strings.TrimSpace(pair), ":")
		if !ok {
			continue
		}
		orgID = strings.TrimSpace(orgID)
		token = strings.TrimSpace(token)
		if orgID != "" && token != "" {
			out[orgID] = token
		}
	}
	return out
}

func microsoftAuthorizeURL(tenant string) string {
	return "https://login.microsoftonline.com/" + tenant + "/oauth2/v2.0/authorize"
}

func microsoftTokenURL(tenant string) string {
	return "https://login.microsoftonline.com/" + tenant + "/oauth2/v2.0/token"
}

func loadEncryptionKey(key string) ([]byte, error) {
	raw := strings.TrimSpace(os.Getenv(key))
	if raw == "" {
		return nil, fmt.Errorf("%s is required", key)
	}
	decoded, err := base64.StdEncoding.DecodeString(raw)
	if err == nil && len(decoded) == 32 {
		return decoded, nil
	}
	if len(raw) == 32 {
		return []byte(raw), nil
	}
	return nil, fmt.Errorf("%s must be 32 raw bytes or base64-encoded 32 bytes", key)
}

func envOr(key, fallback string) string {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	return value
}

func envBool(key string, fallback bool) bool {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	parsed, err := strconv.ParseBool(value)
	if err != nil {
		return fallback
	}
	return parsed
}

func envInt(key string, fallback int) int {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	parsed, err := strconv.Atoi(value)
	if err != nil || parsed <= 0 {
		return fallback
	}
	return parsed
}

func envCSV(key, fallback string) []string {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		value = fallback
	}
	out := []string{}
	seen := map[string]struct{}{}
	for _, item := range strings.Split(value, ",") {
		item = strings.TrimSpace(item)
		if item == "" {
			continue
		}
		if _, ok := seen[item]; ok {
			continue
		}
		seen[item] = struct{}{}
		out = append(out, item)
	}
	return out
}

func envDuration(key string, fallback time.Duration) time.Duration {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	parsed, err := time.ParseDuration(value)
	if err != nil || parsed <= 0 {
		return fallback
	}
	return parsed
}
