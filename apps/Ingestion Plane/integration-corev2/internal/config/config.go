package config

import (
	"crypto/sha256"
	"encoding/base64"
	"fmt"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/attestation"
)

type Config struct {
	Port                             string
	ServiceName                      string
	Environment                      string
	DatabaseURL                      string
	InternalAPIKey                   string
	InternalAPIKeyHeader             string
	AllowLegacyTenantKey             bool
	AuthCoreURL                      string
	AuthCoreInternalAPIKey           string
	AuthCoreJWKSURL                  string
	PlaneTokenIssuer                 string
	IngestionAuthAudience            string
	ProviderWriteAttestationKeysJSON string
	UserCoreURL                      string
	OrgCoreURL                       string
	OrgCoreServiceToken              string
	SessionCoreURL                   string
	BillingCoreURL                   string
	BillingCoreServiceToken          string
	AuditCoreURL                     string
	AuditCoreServiceToken            string
	FinspoCoreURL                    string
	FinspoCoreAPIKey                 string
	FinspoCoreAPIKeyHeader           string
	ConversationIngestURL            string
	ConversationIngestServiceToken   string
	EmailSyncInterval                time.Duration
	EmailSyncBackfillWindow          time.Duration
	EmailSyncGraphFullBackfill       bool
	EmailSyncMaxPerCycle             int
	DataPlaneDocumentsURL            string
	IntegrationServiceID             string
	IntegrationServiceAPIKey         string
	DataPlaneGraphIndexURL           string
	WebhookHotPathURL                string
	IntegrationCoreURL               string
	PublicBaseURL                    string
	MetaJSSDKAppID                   string
	MetaJSSDKAPIVersion              string
	MetaJSSDKLocale                  string
	MetaBusinessLoginConfigID        string
	MetaBusinessLoginConfigIDs       map[string]string
	MetaWebhookVerifyToken           string
	MetaWebhookSecret                string
	MetaThreadsAPIBaseURL            string
	EncryptionKey                    []byte
	AllowInMemoryStore               bool
	NATSEnabled                      bool
	NATSURL                          string
	NATSUsername                     string
	NATSPassword                     string
	NATSToken                        string
	NATSSubjectPrefix                string
	RateLimitEnabled                 bool
	RateLimitMax                     int
	RateLimitWindow                  time.Duration
	TokenLeaseConsumers              []string
	MicrosoftTenantID                string
	MicrosoftClientID                string
	MicrosoftClientSecret            string
	MicrosoftClientAuthMode          string
	MicrosoftTokenOrigin             string
	MicrosoftAuthorizationURL        string
	MicrosoftTokenURL                string
	MicrosoftGraphBaseURL            string
	SlackClientID                    string
	SlackClientSecret                string
	SlackSigningSecret               string
	SlackAuthorizationURL            string
	SlackTokenURL                    string
	SlackAPIBaseURL                  string
	GoogleClientID                   string
	GoogleClientSecret               string
	GoogleAuthorizationURL           string
	GoogleTokenURL                   string
	GoogleAPIBaseURL                 string
	NotionClientID                   string
	NotionClientSecret               string
	NotionAuthorizationURL           string
	NotionTokenURL                   string
	NotionAPIBaseURL                 string
	GitHubClientID                   string
	GitHubClientSecret               string
	GitHubWebhookSecret              string
	GitHubAuthorizationURL           string
	GitHubTokenURL                   string
	GitHubAPIBaseURL                 string
	ShopifyClientID                  string
	ShopifyClientSecret              string
	ShopifyWebhookSecret             string
	ShopifyAPIBaseURL                string
	StripeClientID                   string
	StripeClientSecret               string
	StripeAuthorizationURL           string
	StripeTokenURL                   string
	StripeAPIBaseURL                 string
	StripeWebhookSecret              string
	// AllowUnverifiedWebhooks is a dev-only escape hatch: provider webhooks
	// are rejected fail-closed when their signature scheme is unconfigured
	// or unimplemented, unless this is explicitly true (never in production).
	AllowUnverifiedWebhooks   bool
	LinkedInClientID          string
	LinkedInClientSecret      string
	LinkedInAuthorizationURL  string
	LinkedInTokenURL          string
	LinkedInAPIBaseURL        string
	LinkedInMarketingVersion  string
	XClientID                 string
	XClientSecret             string
	XAuthorizationURL         string
	XTokenURL                 string
	XAPIBaseURL               string
	InstagramClientID         string
	InstagramClientSecret     string
	InstagramAuthorizationURL string
	InstagramTokenURL         string
	InstagramAPIBaseURL       string
	FacebookClientID          string
	FacebookClientSecret      string
	FacebookAuthorizationURL  string
	FacebookTokenURL          string
	FacebookAPIBaseURL        string
	SnapchatClientID          string
	SnapchatClientSecret      string
	SnapchatRedirectBaseURL   string
	SnapchatAuthorizationURL  string
	SnapchatTokenURL          string
	SnapchatAPIBaseURL        string
	// SnapchatBusinessAPIBaseURL is the Public Profile API host
	// (businessapi.snapchat.com) used for organic Story/Spotlight/Saved Story
	// content management. It is a DIFFERENT host from SnapchatAPIBaseURL
	// (adsapi.snapchat.com, the Ads/Marketing API), though both authenticate
	// with the same Snapchat Marketing API OAuth (scope snapchat-marketing-api).
	SnapchatBusinessAPIBaseURL string
	// TikTok Login Kit v2. TikTok calls the client id "client_key" — it is NOT
	// a client_id and must be sent as client_key in both the authorize query
	// and the token body.
	TikTokClientKey         string
	TikTokClientSecret      string
	TikTokAuthorizationURL  string
	TikTokTokenURL          string
	TikTokAPIBaseURL        string
	DiscordClientID         string
	DiscordClientSecret     string
	DiscordAuthorizationURL string
	DiscordTokenURL         string
	DiscordAPIBaseURL       string
	// DiscordBotToken is the app-level bot token the inbound message worker
	// reads guild channels with (the OAuth connection only proves the
	// install). Optional: without it the Discord inbox source stays off.
	DiscordBotToken string
	// GoogleAdsDeveloperToken is the Google Ads API developer token (sent as a
	// `developer-token` header on Ads API calls — separate from OAuth). Ads
	// capability is offered without it, but API calls require it.
	GoogleAdsDeveloperToken string
	OktaDomain              string
	OktaClientID            string
	OktaClientSecret        string
	OktaAPIToken            string
	OktaAPIBaseURL          string
	SCIMBearerToken         string
	SCIMBearerTokens        map[string]string
	SessionTTL              time.Duration
	TokenRefreshSkew        time.Duration
}

func Load() (Config, error) {
	encryptionKey, err := loadEncryptionKey("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY")
	if err != nil {
		return Config{}, err
	}

	publicBaseURL := strings.TrimRight(envOr("INTEGRATION_PUBLIC_BASE_URL", "http://localhost:3026"), "/")
	tenant := envOr("AZURE_TENANT_ID", "common")
	rawFacebookClientID := strings.TrimSpace(os.Getenv("FACEBOOK_CLIENT_ID"))
	rawFacebookClientSecret := strings.TrimSpace(os.Getenv("FACEBOOK_CLIENT_SECRET"))
	rawInstagramClientID := strings.TrimSpace(os.Getenv("INSTAGRAM_CLIENT_ID"))
	rawInstagramClientSecret := strings.TrimSpace(os.Getenv("INSTAGRAM_CLIENT_SECRET"))
	facebookClientID := firstNonEmpty(rawFacebookClientID, rawInstagramClientID)
	facebookClientSecret := firstNonEmpty(rawFacebookClientSecret, rawInstagramClientSecret)
	instagramClientID := firstNonEmpty(rawInstagramClientID, facebookClientID)
	instagramClientSecret := firstNonEmpty(rawInstagramClientSecret, facebookClientSecret)

	return Config{
		Port:                             envOr("PORT", "3026"),
		ServiceName:                      envOr("SERVICE_NAME", "integration-corev2"),
		Environment:                      envOr("ENVIRONMENT", "dev"),
		DatabaseURL:                      strings.TrimSpace(os.Getenv("DATABASE_URL")),
		InternalAPIKey:                   strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")),
		InternalAPIKeyHeader:             envOr("INTERNAL_API_KEY_HEADER", "X-Internal-API-Key"),
		AllowLegacyTenantKey:             os.Getenv("ALLOW_LEGACY_TENANT_KEY") == "1" && os.Getenv("ALLOW_INSECURE_DEV_DEFAULTS") == "1" && os.Getenv("ISOLATED_E2E") == "1",
		AuthCoreURL:                      envOr("AUTH_CORE_URL", "http://auth-core:3011"),
		AuthCoreInternalAPIKey:           strings.TrimSpace(envOr("AUTH_CORE_INTERNAL_API_KEY", strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")))),
		AuthCoreJWKSURL:                  strings.TrimSpace(os.Getenv("AUTH_CORE_JWKS_URL")),
		PlaneTokenIssuer:                 strings.TrimSpace(os.Getenv("PLANE_TOKEN_ISSUER")),
		IngestionAuthAudience:            envOr("INGESTION_AUTH_AUDIENCE", "ingestion"),
		ProviderWriteAttestationKeysJSON: strings.TrimSpace(os.Getenv("INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON")),
		UserCoreURL:                      envOr("USER_CORE_URL", "http://user-core:3012"),
		OrgCoreURL:                       envOr("ORG_CORE_URL", "http://org-core:8080"),
		OrgCoreServiceToken:              strings.TrimSpace(os.Getenv("ORG_CORE_SERVICE_TOKEN")),
		SessionCoreURL:                   envOr("SESSION_CORE_URL", "http://session-core:9091"),
		BillingCoreURL:                   envOr("BILLING_CORE_URL", "http://billing-core:3014"),
		BillingCoreServiceToken:          strings.TrimSpace(os.Getenv("BILLING_CORE_SERVICE_TOKEN")),
		AuditCoreURL:                     envOr("AUDIT_CORE_URL", "http://audit-core:8187"),
		AuditCoreServiceToken:            strings.TrimSpace(os.Getenv("AUDIT_CORE_SERVICE_TOKEN")),
		FinspoCoreURL:                    envOr("FINSPO_CORE_URL", envOr("FINSPO_API_URL", "http://finspo-api:3130")),
		FinspoCoreAPIKey:                 strings.TrimSpace(os.Getenv("FINSPO_API_KEY")),
		FinspoCoreAPIKeyHeader:           envOr("FINSPO_API_KEY_HEADER", "X-API-Key"),
		ConversationIngestURL:            strings.TrimRight(envOr("CONVERSATION_INGEST_URL", "http://conversation-ingest-rs:3161"), "/"),
		ConversationIngestServiceToken:   strings.TrimSpace(os.Getenv("CONVERSATION_EMAIL_INGEST_SERVICE_TOKEN")),
		EmailSyncInterval:                envDuration("EMAIL_SYNC_INTERVAL", 60*time.Second),
		EmailSyncBackfillWindow:          envDuration("EMAIL_SYNC_BACKFILL_WINDOW", 24*time.Hour),
		EmailSyncGraphFullBackfill:       envBool("EMAIL_SYNC_GRAPH_FULL_BACKFILL", true),
		EmailSyncMaxPerCycle:             envInt("EMAIL_SYNC_MAX_PER_CYCLE", 100),
		DataPlaneDocumentsURL:            envOr("DATA_PLANE_DOCUMENTS_URL", envOr("DATA_PLANE_DOCUMENTS_BASE_URL", "http://dpv2-documents-api:8010")),
		IntegrationServiceID:             envOr("INTEGRATION_SERVICE_ID", "integration-corev2"),
		IntegrationServiceAPIKey:         strings.TrimSpace(os.Getenv("INTEGRATION_SERVICE_API_KEY")),
		DataPlaneGraphIndexURL:           envOr("DATA_PLANE_GRAPH_INDEX_URL", envOr("GRAPH_INDEX_URL", "http://dpv2-graph-index:9203")),
		WebhookHotPathURL:                strings.TrimRight(strings.TrimSpace(envOr("INTEGRATION_WEBHOOK_HOTPATH_URL", "")), "/"),
		IntegrationCoreURL:               strings.TrimRight(envOr("INTEGRATION_CORE_URL", publicBaseURL), "/"),
		PublicBaseURL:                    publicBaseURL,
		MetaJSSDKAppID:                   strings.TrimSpace(envOr("META_JS_SDK_APP_ID", facebookClientID)),
		MetaJSSDKAPIVersion:              normalizeGraphAPIVersion(envOr("META_JS_SDK_API_VERSION", "v25.0")),
		MetaJSSDKLocale:                  envOr("META_JS_SDK_LOCALE", "en_US"),
		MetaBusinessLoginConfigID:        strings.TrimSpace(os.Getenv("META_BUSINESS_LOGIN_CONFIG_ID")),
		MetaBusinessLoginConfigIDs:       metaBusinessLoginConfigIDs(),
		MetaWebhookVerifyToken:           strings.TrimSpace(os.Getenv("META_WEBHOOK_VERIFY_TOKEN")),
		MetaWebhookSecret:                strings.TrimSpace(envOr("META_WEBHOOK_SECRET", facebookClientSecret)),
		MetaThreadsAPIBaseURL:            strings.TrimRight(envOr("THREADS_API_BASE_URL", "https://graph.threads.net/v1.0"), "/"),
		EncryptionKey:                    encryptionKey,
		AllowInMemoryStore:               envBool("INTEGRATION_ALLOW_IN_MEMORY_STORE", false),
		NATSEnabled:                      envBool("NATS_ENABLED", false),
		NATSURL:                          envOr("NATS_URL", "nats://localhost:4222"),
		NATSUsername:                     strings.TrimSpace(os.Getenv("NATS_USERNAME")),
		NATSPassword:                     strings.TrimSpace(os.Getenv("NATS_PASSWORD")),
		// NATS_TOKEN authenticates against the shared cross-plane velion-nats
		// broker, which enforces `authorization { token: $VELION_NATS_TOKEN }`
		// (see nats-shared.conf) rather than username/password.
		NATSToken:                 strings.TrimSpace(envOr("NATS_TOKEN", os.Getenv("VELION_NATS_TOKEN"))),
		NATSSubjectPrefix:         envOr("NATS_SUBJECT_PREFIX", ""),
		RateLimitEnabled:          envBool("INTEGRATION_RATE_LIMIT_ENABLED", true),
		RateLimitMax:              envInt("INTEGRATION_RATE_LIMIT_MAX", 120),
		RateLimitWindow:           envDuration("INTEGRATION_RATE_LIMIT_WINDOW", time.Minute),
		TokenLeaseConsumers:       envCSV("INTEGRATION_TOKEN_LEASE_CONSUMERS", "finspo-core,conversation-core,data-plane-v2,model-plane,application-plane,velion-v2-bff,velion-v3-gateway,social-publisher"),
		MicrosoftTenantID:         tenant,
		MicrosoftClientID:         envOr("AZURE_CLIENT_ID", os.Getenv("MICROSOFT_CLIENT_ID")),
		MicrosoftClientSecret:     envOr("AZURE_CLIENT_SECRET", os.Getenv("MICROSOFT_CLIENT_SECRET")),
		MicrosoftClientAuthMode:   normalizeMicrosoftClientAuthMode(envOr("MICROSOFT_CLIENT_AUTH_MODE", "public")),
		MicrosoftTokenOrigin:      envOr("MICROSOFT_TOKEN_ORIGIN", urlOrigin(publicBaseURL)),
		MicrosoftAuthorizationURL: microsoftAuthorizeURL(tenant),
		MicrosoftTokenURL:         microsoftTokenURL(tenant),
		MicrosoftGraphBaseURL:     envOr("MICROSOFT_GRAPH_BASE_URL", "https://graph.microsoft.com"),
		SlackClientID:             strings.TrimSpace(os.Getenv("SLACK_CLIENT_ID")),
		SlackClientSecret:         strings.TrimSpace(os.Getenv("SLACK_CLIENT_SECRET")),
		SlackSigningSecret:        strings.TrimSpace(os.Getenv("SLACK_SIGNING_SECRET")),
		SlackAuthorizationURL:     envOr("SLACK_AUTHORIZATION_URL", "https://slack.com/oauth/v2/authorize"),
		SlackTokenURL:             envOr("SLACK_TOKEN_URL", "https://slack.com/api/oauth.v2.access"),
		SlackAPIBaseURL:           envOr("SLACK_API_BASE_URL", "https://slack.com/api"),
		GoogleClientID:            strings.TrimSpace(os.Getenv("GOOGLE_CLIENT_ID")),
		GoogleClientSecret:        strings.TrimSpace(os.Getenv("GOOGLE_CLIENT_SECRET")),
		GoogleAuthorizationURL:    envOr("GOOGLE_AUTHORIZATION_URL", "https://accounts.google.com/o/oauth2/v2/auth"),
		GoogleTokenURL:            envOr("GOOGLE_TOKEN_URL", "https://oauth2.googleapis.com/token"),
		GoogleAPIBaseURL:          envOr("GOOGLE_API_BASE_URL", "https://www.googleapis.com"),
		NotionClientID:            strings.TrimSpace(os.Getenv("NOTION_CLIENT_ID")),
		NotionClientSecret:        strings.TrimSpace(os.Getenv("NOTION_CLIENT_SECRET")),
		NotionAuthorizationURL:    envOr("NOTION_AUTHORIZATION_URL", "https://api.notion.com/v1/oauth/authorize"),
		NotionTokenURL:            envOr("NOTION_TOKEN_URL", "https://api.notion.com/v1/oauth/token"),
		NotionAPIBaseURL:          envOr("NOTION_API_BASE_URL", "https://api.notion.com"),
		GitHubClientID:            strings.TrimSpace(os.Getenv("GITHUB_CLIENT_ID")),
		GitHubClientSecret:        strings.TrimSpace(os.Getenv("GITHUB_CLIENT_SECRET")),
		GitHubWebhookSecret:       strings.TrimSpace(os.Getenv("GITHUB_WEBHOOK_SECRET")),
		GitHubAuthorizationURL:    envOr("GITHUB_AUTHORIZATION_URL", "https://github.com/login/oauth/authorize"),
		GitHubTokenURL:            envOr("GITHUB_TOKEN_URL", "https://github.com/login/oauth/access_token"),
		GitHubAPIBaseURL:          normalizeGitHubAPIBaseURL(envOr("GITHUB_API_BASE_URL", "https://api.github.com")),
		ShopifyClientID:           strings.TrimSpace(os.Getenv("SHOPIFY_CLIENT_ID")),
		ShopifyClientSecret:       strings.TrimSpace(os.Getenv("SHOPIFY_CLIENT_SECRET")),
		ShopifyWebhookSecret:      strings.TrimSpace(os.Getenv("SHOPIFY_WEBHOOK_SECRET")),
		ShopifyAPIBaseURL:         envOr("SHOPIFY_API_BASE_URL", "https://{shop}"),
		StripeClientID:            strings.TrimSpace(os.Getenv("STRIPE_CLIENT_ID")),
		StripeClientSecret:        strings.TrimSpace(os.Getenv("STRIPE_CLIENT_SECRET")),
		StripeAuthorizationURL:    envOr("STRIPE_AUTHORIZATION_URL", "https://connect.stripe.com/oauth/authorize"),
		StripeTokenURL:            envOr("STRIPE_TOKEN_URL", "https://connect.stripe.com/oauth/token"),
		StripeAPIBaseURL:          envOr("STRIPE_API_BASE_URL", "https://api.stripe.com"),
		StripeWebhookSecret:       strings.TrimSpace(os.Getenv("STRIPE_WEBHOOK_SECRET")),
		AllowUnverifiedWebhooks:   envBool("ALLOW_UNVERIFIED_WEBHOOKS", false),
		LinkedInClientID:          strings.TrimSpace(os.Getenv("LINKEDIN_CLIENT_ID")),
		LinkedInClientSecret:      strings.TrimSpace(os.Getenv("LINKEDIN_CLIENT_SECRET")),
		LinkedInAuthorizationURL:  envOr("LINKEDIN_AUTHORIZATION_URL", "https://www.linkedin.com/oauth/v2/authorization"),
		LinkedInTokenURL:          envOr("LINKEDIN_TOKEN_URL", "https://www.linkedin.com/oauth/v2/accessToken"),
		LinkedInAPIBaseURL:        envOr("LINKEDIN_API_BASE_URL", "https://api.linkedin.com"),
		LinkedInMarketingVersion:  envOr("LINKEDIN_MARKETING_VERSION", "202606"),
		XClientID:                 strings.TrimSpace(os.Getenv("X_CLIENT_ID")),
		XClientSecret:             strings.TrimSpace(os.Getenv("X_CLIENT_SECRET")),
		XAuthorizationURL:         envOr("X_AUTHORIZATION_URL", "https://x.com/i/oauth2/authorize"),
		XTokenURL:                 envOr("X_TOKEN_URL", "https://api.x.com/2/oauth2/token"),
		XAPIBaseURL:               envOr("X_API_BASE_URL", "https://api.x.com"),
		InstagramClientID:         instagramClientID,
		InstagramClientSecret:     instagramClientSecret,
		// Graph API v25.0 is current (Graph + Marketing API version in lockstep);
		// instagram_* scopes here are the "Instagram API with Facebook Login"
		// flavor — instagram_basic is NOT deprecated for that product.
		InstagramAuthorizationURL:  envOr("INSTAGRAM_AUTHORIZATION_URL", "https://www.facebook.com/v25.0/dialog/oauth"),
		InstagramTokenURL:          envOr("INSTAGRAM_TOKEN_URL", "https://graph.facebook.com/v25.0/oauth/access_token"),
		InstagramAPIBaseURL:        envOr("INSTAGRAM_GRAPH_API_BASE_URL", "https://graph.facebook.com/v25.0"),
		FacebookClientID:           facebookClientID,
		FacebookClientSecret:       facebookClientSecret,
		FacebookAuthorizationURL:   envOr("FACEBOOK_AUTHORIZATION_URL", "https://www.facebook.com/v25.0/dialog/oauth"),
		FacebookTokenURL:           envOr("FACEBOOK_TOKEN_URL", "https://graph.facebook.com/v25.0/oauth/access_token"),
		FacebookAPIBaseURL:         envOr("FACEBOOK_GRAPH_API_BASE_URL", "https://graph.facebook.com/v25.0"),
		SnapchatClientID:           strings.TrimSpace(os.Getenv("SNAPCHAT_CLIENT_ID")),
		SnapchatClientSecret:       strings.TrimSpace(os.Getenv("SNAPCHAT_CLIENT_SECRET")),
		SnapchatRedirectBaseURL:    strings.TrimRight(envOr("SNAPCHAT_REDIRECT_BASE_URL", publicBaseURL), "/"),
		SnapchatAuthorizationURL:   envOr("SNAPCHAT_AUTHORIZATION_URL", "https://accounts.snapchat.com/login/oauth2/authorize"),
		SnapchatTokenURL:           envOr("SNAPCHAT_TOKEN_URL", "https://accounts.snapchat.com/login/oauth2/access_token"),
		SnapchatAPIBaseURL:         envOr("SNAPCHAT_API_BASE_URL", "https://adsapi.snapchat.com/v1"),
		SnapchatBusinessAPIBaseURL: envOr("SNAPCHAT_BUSINESS_API_BASE_URL", "https://businessapi.snapchat.com/v1"),
		// TikTok Login Kit v2 (client_key naming; v1 open-api.tiktok.com endpoints
		// died Feb 2024). Web-app redirect URIs must be public https — localhost
		// only works for Desktop-type TikTok apps.
		TikTokClientKey:         strings.TrimSpace(firstNonEmpty(os.Getenv("TIKTOK_CLIENT_KEY"), os.Getenv("TIKTOK_CLIENT_ID"))),
		TikTokClientSecret:      strings.TrimSpace(os.Getenv("TIKTOK_CLIENT_SECRET")),
		TikTokAuthorizationURL:  envOr("TIKTOK_AUTHORIZATION_URL", "https://www.tiktok.com/v2/auth/authorize/"),
		TikTokTokenURL:          envOr("TIKTOK_TOKEN_URL", "https://open.tiktokapis.com/v2/oauth/token/"),
		TikTokAPIBaseURL:        envOr("TIKTOK_API_BASE_URL", "https://open.tiktokapis.com/v2"),
		DiscordClientID:         strings.TrimSpace(os.Getenv("DISCORD_CLIENT_ID")),
		DiscordClientSecret:     strings.TrimSpace(os.Getenv("DISCORD_CLIENT_SECRET")),
		DiscordAuthorizationURL: envOr("DISCORD_AUTHORIZATION_URL", "https://discord.com/oauth2/authorize"),
		DiscordTokenURL:         envOr("DISCORD_TOKEN_URL", "https://discord.com/api/oauth2/token"),
		DiscordAPIBaseURL:       envOr("DISCORD_API_BASE_URL", "https://discord.com/api/v10"),
		DiscordBotToken:         strings.TrimSpace(os.Getenv("DISCORD_BOT_TOKEN")),
		GoogleAdsDeveloperToken: strings.TrimSpace(os.Getenv("GOOGLE_ADS_DEVELOPER_TOKEN")),
		OktaDomain:              strings.TrimRight(strings.TrimSpace(os.Getenv("OKTA_DOMAIN")), "/"),
		OktaClientID:            strings.TrimSpace(os.Getenv("OKTA_CLIENT_ID")),
		OktaClientSecret:        strings.TrimSpace(os.Getenv("OKTA_CLIENT_SECRET")),
		OktaAPIToken:            strings.TrimSpace(os.Getenv("OKTA_API_TOKEN")),
		OktaAPIBaseURL:          strings.TrimRight(envOr("OKTA_API_BASE_URL", strings.TrimSpace(os.Getenv("OKTA_DOMAIN"))), "/"),
		SCIMBearerToken:         strings.TrimSpace(os.Getenv("SCIM_BEARER_TOKEN")),
		SCIMBearerTokens:        parseSCIMBearerTokens(os.Getenv("SCIM_ORG_BEARER_TOKENS")),
		SessionTTL:              envDuration("INTEGRATION_CONNECT_SESSION_TTL", 10*time.Minute),
		TokenRefreshSkew:        envDuration("INTEGRATION_TOKEN_REFRESH_SKEW", 2*time.Minute),
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
	if !validDedicatedServiceToken(c.OrgCoreServiceToken) {
		return fmt.Errorf("ORG_CORE_SERVICE_TOKEN must be a non-placeholder secret of at least 32 bytes")
	}
	if !validDedicatedServiceToken(c.BillingCoreServiceToken) {
		return fmt.Errorf("BILLING_CORE_SERVICE_TOKEN must be a non-placeholder secret of at least 32 bytes")
	}
	if !validDedicatedServiceToken(c.AuditCoreServiceToken) {
		return fmt.Errorf("AUDIT_CORE_SERVICE_TOKEN must be a non-placeholder secret of at least 32 bytes")
	}
	seenCredentials := make(map[[sha256.Size]byte]string, 5)
	for name, value := range map[string]string{
		"INTERNAL_API_KEY":           c.InternalAPIKey,
		"AUTH_CORE_INTERNAL_API_KEY": c.AuthCoreInternalAPIKey,
	} {
		if strings.TrimSpace(value) != "" {
			seenCredentials[sha256.Sum256([]byte(strings.TrimSpace(value)))] = name
		}
	}
	for name, value := range map[string]string{
		"ORG_CORE_SERVICE_TOKEN":     c.OrgCoreServiceToken,
		"BILLING_CORE_SERVICE_TOKEN": c.BillingCoreServiceToken,
		"AUDIT_CORE_SERVICE_TOKEN":   c.AuditCoreServiceToken,
	} {
		digest := sha256.Sum256([]byte(strings.TrimSpace(value)))
		if reusedFrom, exists := seenCredentials[digest]; exists {
			return fmt.Errorf("%s must not reuse %s", name, reusedFrom)
		}
		seenCredentials[digest] = name
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
	if _, err := attestation.ParseTrustedKeysJSON(c.ProviderWriteAttestationKeysJSON); err != nil {
		return fmt.Errorf("INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON must contain trusted conversation-core Ed25519 public keys: %w", err)
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
	if c.DataPlaneDocumentsURL != "" {
		if c.AuthCoreURL == "" {
			return fmt.Errorf("AUTH_CORE_URL is required for Data Plane token minting")
		}
		if c.IntegrationServiceID == "" {
			return fmt.Errorf("INTEGRATION_SERVICE_ID is required for Data Plane token minting")
		}
		if c.IntegrationServiceAPIKey == "" {
			return fmt.Errorf("INTEGRATION_SERVICE_API_KEY is required for Data Plane token minting")
		}
	}
	return nil
}

// ValidateEmailWorkerRuntime checks the env the email inbound sync worker
// needs: direct DB access (connections + cursors), the token vault key, the
// conversation-ingest bridge target, and the internal key it authenticates
// to that bridge with.
func (c Config) ValidateEmailWorkerRuntime() error {
	if c.DatabaseURL == "" {
		return fmt.Errorf("DATABASE_URL is required")
	}
	if len(c.EncryptionKey) == 0 {
		return fmt.Errorf("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY is required")
	}
	if c.ConversationIngestURL == "" {
		return fmt.Errorf("CONVERSATION_INGEST_URL is required")
	}
	if !validDedicatedServiceToken(c.ConversationIngestServiceToken) {
		return fmt.Errorf("CONVERSATION_EMAIL_INGEST_SERVICE_TOKEN must be a non-placeholder secret of at least 32 bytes")
	}
	return nil
}

func validDedicatedServiceToken(token string) bool {
	token = strings.TrimSpace(token)
	lower := strings.ToLower(token)
	return len(token) >= 32 &&
		!strings.HasPrefix(lower, "test") &&
		!strings.HasPrefix(lower, "placeholder") &&
		!strings.HasPrefix(lower, "change-me") &&
		!strings.HasPrefix(lower, "replace-with")
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
		mode := normalizeMicrosoftClientAuthMode(c.MicrosoftClientAuthMode)
		if mode != "public" && mode != "confidential" {
			return fmt.Errorf("MICROSOFT_CLIENT_AUTH_MODE must be public or confidential")
		}
		if mode == "confidential" && c.MicrosoftClientSecret == "" {
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
			return fmt.Errorf("INSTAGRAM_CLIENT_ID or FACEBOOK_CLIENT_ID is required for Instagram OAuth")
		}
		if c.InstagramClientSecret == "" {
			return fmt.Errorf("INSTAGRAM_CLIENT_SECRET or FACEBOOK_CLIENT_SECRET is required for Instagram OAuth")
		}
	case "meta", "facebook", "whatsapp", "meta-ads":
		label := "Meta"
		switch providerKey {
		case "facebook":
			label = "Facebook"
		case "whatsapp":
			label = "WhatsApp"
		case "meta-ads":
			label = "Meta Ads"
		}
		if c.FacebookClientID == "" {
			return fmt.Errorf("FACEBOOK_CLIENT_ID or INSTAGRAM_CLIENT_ID is required for %s OAuth", label)
		}
		if c.FacebookClientSecret == "" {
			return fmt.Errorf("FACEBOOK_CLIENT_SECRET or INSTAGRAM_CLIENT_SECRET is required for %s OAuth", label)
		}
	case "snapchat":
		if c.SnapchatClientID == "" {
			return fmt.Errorf("SNAPCHAT_CLIENT_ID is required for Snapchat OAuth")
		}
		if c.SnapchatClientSecret == "" {
			return fmt.Errorf("SNAPCHAT_CLIENT_SECRET is required for Snapchat OAuth")
		}
		if c.snapchatHTTPSRedirectBaseURL() == "" {
			return fmt.Errorf("SNAPCHAT_REDIRECT_BASE_URL or HTTPS INTEGRATION_PUBLIC_BASE_URL is required for Snapchat OAuth")
		}
	case "tiktok":
		if c.TikTokClientKey == "" {
			return fmt.Errorf("TIKTOK_CLIENT_KEY or TIKTOK_CLIENT_ID is required for TikTok OAuth")
		}
		if c.TikTokClientSecret == "" {
			return fmt.Errorf("TIKTOK_CLIENT_SECRET is required for TikTok OAuth")
		}
	case "discord":
		if c.DiscordClientID == "" {
			return fmt.Errorf("DISCORD_CLIENT_ID is required for Discord OAuth")
		}
		if c.DiscordClientSecret == "" {
			return fmt.Errorf("DISCORD_CLIENT_SECRET is required for Discord OAuth")
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
	microsoftChecks := map[string]string{
		"AZURE_CLIENT_ID": c.MicrosoftClientID,
	}
	if normalizeMicrosoftClientAuthMode(c.MicrosoftClientAuthMode) == "confidential" {
		microsoftChecks["AZURE_CLIENT_SECRET"] = c.MicrosoftClientSecret
	}
	checks := map[string]map[string]string{
		"microsoft": microsoftChecks,
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
			"INSTAGRAM_CLIENT_ID or FACEBOOK_CLIENT_ID":         c.InstagramClientID,
			"INSTAGRAM_CLIENT_SECRET or FACEBOOK_CLIENT_SECRET": c.InstagramClientSecret,
		},
		"facebook": {
			"FACEBOOK_CLIENT_ID or INSTAGRAM_CLIENT_ID":         c.FacebookClientID,
			"FACEBOOK_CLIENT_SECRET or INSTAGRAM_CLIENT_SECRET": c.FacebookClientSecret,
		},
		"whatsapp": {
			"FACEBOOK_CLIENT_ID or INSTAGRAM_CLIENT_ID":         c.FacebookClientID,
			"FACEBOOK_CLIENT_SECRET or INSTAGRAM_CLIENT_SECRET": c.FacebookClientSecret,
		},
		"meta-ads": {
			"FACEBOOK_CLIENT_ID or INSTAGRAM_CLIENT_ID":         c.FacebookClientID,
			"FACEBOOK_CLIENT_SECRET or INSTAGRAM_CLIENT_SECRET": c.FacebookClientSecret,
		},
		"meta": {
			"FACEBOOK_CLIENT_ID or INSTAGRAM_CLIENT_ID":         c.FacebookClientID,
			"FACEBOOK_CLIENT_SECRET or INSTAGRAM_CLIENT_SECRET": c.FacebookClientSecret,
		},
		"snapchat": {
			"SNAPCHAT_CLIENT_ID":     c.SnapchatClientID,
			"SNAPCHAT_CLIENT_SECRET": c.SnapchatClientSecret,
			"SNAPCHAT_REDIRECT_BASE_URL or HTTPS INTEGRATION_PUBLIC_BASE_URL": c.snapchatHTTPSRedirectBaseURL(),
		},
		"tiktok": {
			"TIKTOK_CLIENT_KEY or TIKTOK_CLIENT_ID": c.TikTokClientKey,
			"TIKTOK_CLIENT_SECRET":                  c.TikTokClientSecret,
		},
		"discord": {
			"DISCORD_CLIENT_ID":     c.DiscordClientID,
			"DISCORD_CLIENT_SECRET": c.DiscordClientSecret,
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

func (c Config) snapchatHTTPSRedirectBaseURL() string {
	baseURL := strings.TrimRight(strings.TrimSpace(firstNonEmpty(c.SnapchatRedirectBaseURL, c.PublicBaseURL)), "/")
	parsed, err := url.Parse(baseURL)
	if err != nil || !strings.EqualFold(parsed.Scheme, "https") || parsed.Host == "" {
		return ""
	}
	return baseURL
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

func normalizeGitHubAPIBaseURL(raw string) string {
	value := strings.TrimRight(strings.TrimSpace(raw), "/")
	if value == "" {
		return "https://api.github.com"
	}
	parsed, err := url.Parse(value)
	if err != nil || parsed.Scheme == "" || parsed.Host == "" {
		return value
	}
	host := strings.ToLower(parsed.Host)
	switch host {
	case "github.com", "www.github.com":
		return "https://api.github.com"
	case "api.github.com":
		parsed.Path = strings.TrimRight(parsed.Path, "/")
		if parsed.Path == "" {
			return parsed.Scheme + "://" + parsed.Host
		}
		return parsed.String()
	}
	if host == "localhost" || strings.HasPrefix(host, "localhost:") ||
		strings.HasPrefix(host, "127.") || strings.HasPrefix(host, "[::1]") {
		return value
	}
	if strings.Trim(parsed.Path, "/") == "" {
		parsed.Path = "/api/v3"
	}
	return strings.TrimRight(parsed.String(), "/")
}

func normalizeMicrosoftClientAuthMode(raw string) string {
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case "", "public", "pkce", "none":
		return "public"
	case "confidential", "secret", "web":
		return "confidential"
	default:
		return strings.ToLower(strings.TrimSpace(raw))
	}
}

func urlOrigin(raw string) string {
	parsed, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || parsed.Scheme == "" || parsed.Host == "" {
		return ""
	}
	return parsed.Scheme + "://" + parsed.Host
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

func normalizeGraphAPIVersion(value string) string {
	trimmed := strings.TrimSpace(value)
	if trimmed == "" {
		return "v25.0"
	}
	if strings.HasPrefix(strings.ToLower(trimmed), "v") {
		return trimmed
	}
	return "v" + trimmed
}

// metaBusinessLoginConfigIDs collects every named Facebook Login for
// Business configuration this deployment has been given. Meta bakes
// permissions into the configuration itself rather than a per-request scope
// param, so a Business-type app that needs more than one permission set
// (e.g. general Pages/Instagram access vs. a narrower Conversions API
// partner integration) needs one configuration per set, not one shared
// across every bundle. "default" is used whenever a connect session doesn't
// name a specific one (providerContext["business_login_config"]).
func metaBusinessLoginConfigIDs() map[string]string {
	ids := map[string]string{}
	if id := strings.TrimSpace(os.Getenv("META_BUSINESS_LOGIN_CONFIG_ID")); id != "" {
		ids["default"] = id
	}
	if id := strings.TrimSpace(os.Getenv("META_BUSINESS_LOGIN_CONVERSIONS_CONFIG_ID")); id != "" {
		ids["conversions"] = id
	}
	return ids
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		trimmed := strings.TrimSpace(value)
		if trimmed != "" {
			return trimmed
		}
	}
	return ""
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
