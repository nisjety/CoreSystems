package config

import (
	"errors"
	"net/url"
	"os"
	"regexp"
	"strconv"
	"strings"
	"time"
)

type Config struct {
	DatabaseURL                 string
	NatsURL                     string
	NatsToken                   string
	HTTPPort                    int
	JWTAudience                 string
	JWTIssuer                   string
	JWKSURL                     string
	JWTPublicKeyFile            string
	SignedCostEventsEnabled     bool
	EmbeddingEventPublicKeyPath string
	RetrievalEventPublicKeyPath string
	// GraphEventPublicKeyPath verifies graph-index's usage events (CR-T10). Its key pair is
	// provisioned beside the other event keys, by hand, never under .secrets/event-keys in
	// production (the cross-plane preflight rejects that path).
	GraphEventPublicKeyPath string

	// The usage forwarder (CR-T10): posts every version 2 usage row to cost-core. Off unless
	// USAGE_FORWARDER_ENABLED is exactly "1"; it must stay off until cost-core accepts the
	// fields it posts and prices the models the Data Plane meters (see the forwarder's
	// documentation and the contextual retrieval runbook).
	UsageForwarderEnabled bool
	CostCoreURL           string
	// CostCoreTokenURL is Auth Core's mint for the cost-core audience.
	CostCoreTokenURL string
	// CostServiceID and CostServiceAPIKey are the principal's identity and credential in the
	// registry (`data-orchestrator` on cost-core, scope cost:write, retention persistent). The
	// credential is deployment-owned (DATA_ORCHESTRATOR_COST_SERVICE_API_KEY) and is sent to
	// Auth Core and nowhere else.
	CostServiceID          string
	CostServiceAPIKey      string
	UsageForwarderBatch    int
	UsageForwarderInterval time.Duration
	// SharedNats* configure this service's OWN dedicated identity on the
	// cross-plane shared broker (control-shared-nats), used ONLY by the
	// GDPR org-erasure durable consumer (internal/gdpr). This is a
	// deliberately separate connection from NatsURL/NatsToken above, which
	// is the plane-local Data Plane v2 broker the cost-ledger consumer and
	// job executor already use — never reuse that connection or any other
	// service's shared identity for this consumer.
	SharedNatsURL      string
	SharedNatsUser     string
	SharedNatsPassword string
	// GDPROrgPurgeConsumerRequired gates the "data-orchestrator-org-erasure"
	// durable (internal/gdpr): when true, the service fails to boot if the
	// pre-provisioned consumer can't be bound.
	GDPROrgPurgeConsumerRequired bool
}

func Load() *Config {
	return &Config{
		DatabaseURL:                  envOr("DATABASE_URL", "postgres://dataplane:dataplane@localhost:5442/dataplane?sslmode=disable"),
		NatsURL:                      envOr("NATS_URL", "nats://localhost:4232"),
		NatsToken:                    strings.TrimSpace(os.Getenv("DATAPLANE_NATS_TOKEN")),
		HTTPPort:                     envIntOr("HTTP_PORT", 8012),
		JWTAudience:                  envOr("DATA_PLANE_AUTH_AUDIENCE", "data-plane"),
		JWTIssuer:                    envOr("AUTH_CORE_ISSUER", "http://localhost:3011/api/convex-auth"),
		JWKSURL:                      envOr("AUTH_CORE_JWKS_URL", "http://auth-core:3011/api/convex-auth/jwks"),
		JWTPublicKeyFile:             envOr("JWT_PUBLIC_KEY_FILE", ""),
		SignedCostEventsEnabled:      os.Getenv("SIGNED_COST_EVENTS_ENABLED") == "1",
		EmbeddingEventPublicKeyPath:  strings.TrimSpace(os.Getenv("EMBEDDING_EVENT_PUBLIC_KEY_PATH")),
		RetrievalEventPublicKeyPath:  strings.TrimSpace(os.Getenv("RETRIEVAL_EVENT_PUBLIC_KEY_PATH")),
		GraphEventPublicKeyPath:      strings.TrimSpace(os.Getenv("GRAPH_EVENT_PUBLIC_KEY_PATH")),
		UsageForwarderEnabled:        os.Getenv("USAGE_FORWARDER_ENABLED") == "1",
		CostCoreURL:                  strings.TrimSpace(envOr("COST_CORE_URL", "http://cost-core:8089")),
		CostCoreTokenURL:             strings.TrimSpace(envOr("COST_CORE_TOKEN_URL", "http://auth-core:3011/api/cost-core/internal-token")),
		CostServiceID:                strings.TrimSpace(envOr("DATA_ORCHESTRATOR_COST_SERVICE_ID", "data-orchestrator")),
		CostServiceAPIKey:            strings.TrimSpace(os.Getenv("DATA_ORCHESTRATOR_COST_SERVICE_API_KEY")),
		UsageForwarderBatch:          envIntOr("USAGE_FORWARDER_BATCH", 50),
		UsageForwarderInterval:       time.Duration(envIntOr("USAGE_FORWARDER_INTERVAL_SECS", 15)) * time.Second,
		SharedNatsURL:                strings.TrimSpace(os.Getenv("NATS_SHARED_URL")),
		SharedNatsUser:               strings.TrimSpace(os.Getenv("NATS_SHARED_USER")),
		SharedNatsPassword:           strings.TrimSpace(os.Getenv("NATS_SHARED_PASSWORD")),
		GDPROrgPurgeConsumerRequired: os.Getenv("GDPR_ORG_PURGE_CONSUMER_REQUIRED") == "1",
	}
}

// ValidateGDPRConsumer enforces the shared-broker credential contract for the
// GDPR org-erasure durable consumer (internal/gdpr): all three
// NATS_SHARED_* variables must be configured together (never partially), the
// password must meet a minimum strength bar, and if the consumer is marked
// required, the credentials must actually be present so the service fails
// closed instead of silently running without erasure coverage.
func (c *Config) ValidateGDPRConsumer() error {
	sharedConfigured := c.SharedNatsURL != "" || c.SharedNatsUser != "" || c.SharedNatsPassword != ""
	if sharedConfigured && (c.SharedNatsURL == "" || c.SharedNatsUser == "" || c.SharedNatsPassword == "") {
		return errors.New("NATS_SHARED_URL, NATS_SHARED_USER, and NATS_SHARED_PASSWORD must be configured together")
	}
	if c.SharedNatsPassword != "" && len(c.SharedNatsPassword) < 32 {
		return errors.New("NATS_SHARED_PASSWORD must contain at least 32 characters")
	}
	if c.GDPROrgPurgeConsumerRequired && !sharedConfigured {
		return errors.New("scoped shared NATS credentials are required for the durable GDPR org-purge consumer")
	}
	return nil
}

func (c *Config) ValidateSignedCostEvents() error {
	if !c.SignedCostEventsEnabled {
		return nil
	}
	if c.EmbeddingEventPublicKeyPath == "" || c.RetrievalEventPublicKeyPath == "" || c.GraphEventPublicKeyPath == "" {
		return errors.New("signed cost events require embedding, retrieval and graph public keys")
	}
	return nil
}

// credentialPlaceholder is the marker the release preflight
// (apps/Control Plane/scripts/validate-release-credentials.sh) rejects in a service credential.
var credentialPlaceholder = regexp.MustCompile(`(?i)(change[-_]?me|placeholder|example|your[-_]|default[-_]?secret|insecure|dummy)`)

// ValidateUsageForwarder is the boot-time check of the forwarder. A disabled forwarder needs
// nothing. An enabled one must be able to reach cost-core and Auth Core and to authenticate as
// its principal, or the service stops, instead of running with a forwarder that fails every
// pass. Nothing about the credential is in an error.
func (c *Config) ValidateUsageForwarder() error {
	if !c.UsageForwarderEnabled {
		return nil
	}
	if err := validateServiceURL(c.CostCoreURL); err != nil {
		return errors.New("USAGE_FORWARDER_ENABLED=1 needs COST_CORE_URL to be an absolute http(s) address without credentials, query or fragment")
	}
	if err := validateServiceURL(c.CostCoreTokenURL); err != nil {
		return errors.New("USAGE_FORWARDER_ENABLED=1 needs COST_CORE_TOKEN_URL to be an absolute http(s) address without credentials, query or fragment")
	}
	if c.CostServiceID == "" {
		return errors.New("USAGE_FORWARDER_ENABLED=1 needs DATA_ORCHESTRATOR_COST_SERVICE_ID")
	}
	if len(c.CostServiceAPIKey) < 32 || credentialPlaceholder.MatchString(c.CostServiceAPIKey) {
		return errors.New("USAGE_FORWARDER_ENABLED=1 needs DATA_ORCHESTRATOR_COST_SERVICE_API_KEY: the principal's credential, at least 32 characters, no placeholder marker")
	}
	if c.UsageForwarderBatch < 1 || c.UsageForwarderBatch > 500 {
		return errors.New("USAGE_FORWARDER_BATCH must be between 1 and 500")
	}
	if c.UsageForwarderInterval < time.Second || c.UsageForwarderInterval > time.Hour {
		return errors.New("USAGE_FORWARDER_INTERVAL_SECS must be between 1 and 3600")
	}
	return nil
}

func validateServiceURL(raw string) error {
	parsed, err := url.Parse(raw)
	if err != nil || (parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.Host == "" ||
		parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" {
		return errors.New("invalid service address")
	}
	return nil
}

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func envIntOr(key string, fallback int) int {
	if v := os.Getenv(key); v != "" {
		if n, err := strconv.Atoi(v); err == nil {
			return n
		}
	}
	return fallback
}
