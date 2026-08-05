package config

import (
	"crypto/ed25519"
	"fmt"
	"net/url"
	"os"
	"regexp"
	"strconv"
	"strings"
	"time"

	"github.com/joho/godotenv"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/attestation"
)

type Config struct {
	HTTPPort       int
	DatabaseURL    string
	NATSURL        string
	NATSUser       string
	NATSPassword   string
	ServiceName    string
	DelegationKeys map[string]string
	// SharedNATSURL/SharedNATSUser/SharedNATSPassword configure a SECOND,
	// narrowly-scoped connection to the cross-plane control-shared-nats
	// broker (identity "conversation-core-gdpr"), used only by the GDPR
	// privacy consumers (the org-erasure and draft-only interactive-retention
	// consumers).
	// Deliberately DISTINCT env var names from NATSURL's own fallback chain
	// above, which already treats the literal name NATS_SHARED_URL as an
	// alternate Application-Plane-LOCAL broker URL — reusing that name here
	// would silently collide with that existing fallback. Empty
	// SharedNATSURL disables the privacy consumers without affecting the
	// plane-local NATS client or any other consumer.
	SharedNATSURL      string
	SharedNATSUser     string
	SharedNATSPassword string
	// IntegrationInternalKey is restricted to integration-corev2's internal
	// webhook-event read. Tenant APIs use only a tenant-bound Auth Core service
	// JWT; caller-supplied org/user headers are never an authority boundary.
	IntegrationBaseURL           string
	IntegrationInternalKey       string
	AuthCoreURL                  string
	IntegrationServiceID         string
	IntegrationServiceCredential string
	AttestationPrivateKey        ed25519.PrivateKey
	AttestationKeyID             string
	// OutboundReconcileInterval is how often the stuck-send sweep
	// (consumers.OutboundIntentReconciler) runs. OutboundReconcileStaleAfter is
	// how long an outbound intent may sit in `sending` before the sweep flips
	// it to `unknown` for operator reconciliation. Both always have a positive
	// default, independent of whether outbound send is configured, so the
	// sweep is never accidentally disabled by omission.
	OutboundReconcileInterval   time.Duration
	OutboundReconcileStaleAfter time.Duration
	// FeedbackMirrorOrgID is the Verevon-owned monitored organization that every
	// pilot-feedback submission (conversation.Service.SubmitFeedback) is
	// mirrored into, in addition to the submitter's own org. This exists
	// because conversation-core-go has no cross-org/platform-admin read
	// bypass: in the open pilot each external company gets its own isolated
	// org, so without a mirror, feedback submitted from inside an external
	// org's product session would be invisible to the team -- nobody on the
	// team is a member of that org's Inbox. Empty disables mirroring entirely
	// (not an error -- SubmitFeedback logs a warning and the submitter's
	// own-org copy still always succeeds). Update this single value if the
	// team's own operating org ever changes; never hardcode the literal org
	// id anywhere else in the codebase.
	FeedbackMirrorOrgID string
}

// DraftReplySendEnabled reports whether the outbound-send (draft.reply) leg is
// configured. Gating on this prevents a false send claim without configuration.
func (c *Config) DraftReplySendEnabled() bool {
	return c != nil &&
		strings.TrimSpace(c.IntegrationBaseURL) != "" &&
		strings.TrimSpace(c.IntegrationInternalKey) != "" &&
		strings.TrimSpace(c.AuthCoreURL) != "" &&
		strings.TrimSpace(c.IntegrationServiceID) != "" &&
		strings.TrimSpace(c.IntegrationServiceCredential) != "" &&
		len(c.AttestationPrivateKey) == ed25519.PrivateKeySize &&
		strings.TrimSpace(c.AttestationKeyID) != ""
}

func Load() (*Config, error) {
	_ = godotenv.Load()

	attestationPrivateKeyEncoded := strings.TrimSpace(getEnv("CONVERSATION_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY", ""))
	cfg := &Config{
		HTTPPort:           getEnvInt("PORT", 3160),
		DatabaseURL:        strings.TrimSpace(getEnv("DATABASE_URL", "")),
		NATSURL:            strings.TrimSpace(getEnv("VEREVON_NATS_URL", getEnv("NATS_SHARED_URL", getEnv("NATS_URL", "nats://nats:4222")))),
		NATSUser:           strings.TrimSpace(getEnv("NATS_USER", "")),
		NATSPassword:       strings.TrimSpace(getEnv("NATS_PASSWORD", "")),
		ServiceName:        strings.TrimSpace(getEnv("SERVICE_NAME", "conversation-core-go")),
		SharedNATSURL:      strings.TrimSpace(getEnv("CONVERSATION_GDPR_SHARED_NATS_URL", "")),
		SharedNATSUser:     strings.TrimSpace(getEnv("CONVERSATION_GDPR_SHARED_NATS_USER", "")),
		SharedNATSPassword: strings.TrimSpace(getEnv("CONVERSATION_GDPR_SHARED_NATS_PASSWORD", "")),
		DelegationKeys: map[string]string{
			"verevon-gateway":      strings.TrimSpace(getEnv("CONVERSATION_GATEWAY_SERVICE_TOKEN", "")),
			"conversation-ingest": strings.TrimSpace(getEnv("CONVERSATION_CORE_INGEST_SERVICE_TOKEN", "")),
		},
		// Non-fatal only when the entire integration block is unset. Any partial
		// service-principal configuration fails startup below.
		IntegrationBaseURL:           strings.TrimSpace(getEnv("INTEGRATION_BASE_URL", getEnv("INTEGRATION_COREV2_URL", ""))),
		IntegrationInternalKey:       strings.TrimSpace(getEnv("INTEGRATION_INTERNAL_API_KEY", "")),
		AuthCoreURL:                  strings.TrimSpace(getEnv("AUTH_CORE_URL", "")),
		IntegrationServiceID:         strings.TrimSpace(getEnv("CONVERSATION_INTEGRATION_SERVICE_ID", "")),
		IntegrationServiceCredential: strings.TrimSpace(getEnv("CONVERSATION_INTEGRATION_SERVICE_API_KEY", "")),
		AttestationKeyID:             strings.TrimSpace(getEnv("CONVERSATION_PROVIDER_WRITE_ATTESTATION_KEY_ID", "")),
		OutboundReconcileInterval:    time.Duration(getEnvPositiveInt("CONVERSATION_OUTBOUND_RECONCILE_INTERVAL_SECONDS", 300)) * time.Second,
		OutboundReconcileStaleAfter:  time.Duration(getEnvPositiveInt("CONVERSATION_OUTBOUND_RECONCILE_STALE_AFTER_SECONDS", 900)) * time.Second,
		FeedbackMirrorOrgID:          strings.TrimSpace(getEnv("FEEDBACK_MIRROR_ORG_ID", "")),
	}

	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}
	if err := validateDelegationKeys(cfg.DelegationKeys); err != nil {
		return nil, err
	}
	integrationValues := map[string]string{
		"INTEGRATION_BASE_URL":                     cfg.IntegrationBaseURL,
		"INTEGRATION_INTERNAL_API_KEY":             cfg.IntegrationInternalKey,
		"AUTH_CORE_URL":                            cfg.AuthCoreURL,
		"CONVERSATION_INTEGRATION_SERVICE_ID":      cfg.IntegrationServiceID,
		"CONVERSATION_INTEGRATION_SERVICE_API_KEY": cfg.IntegrationServiceCredential,
	}
	configured := 0
	for _, value := range integrationValues {
		if value != "" {
			configured++
		}
	}
	if configured != 0 && configured != len(integrationValues) {
		return nil, fmt.Errorf("integration outbound requires INTEGRATION_BASE_URL, INTEGRATION_INTERNAL_API_KEY, AUTH_CORE_URL, CONVERSATION_INTEGRATION_SERVICE_ID, and CONVERSATION_INTEGRATION_SERVICE_API_KEY together")
	}
	if configured == len(integrationValues) {
		if !validServiceURL(cfg.IntegrationBaseURL) {
			return nil, fmt.Errorf("INTEGRATION_BASE_URL must be an absolute http(s) service URL without credentials, query, or fragment")
		}
		if !validServiceURL(cfg.AuthCoreURL) {
			return nil, fmt.Errorf("AUTH_CORE_URL must be an absolute http(s) service URL without credentials, query, or fragment")
		}
		if !serviceIDPattern.MatchString(cfg.IntegrationServiceID) {
			return nil, fmt.Errorf("CONVERSATION_INTEGRATION_SERVICE_ID is invalid")
		}
		if !validDelegationSecret(cfg.IntegrationInternalKey) {
			return nil, fmt.Errorf("INTEGRATION_INTERNAL_API_KEY must be a non-placeholder secret of at least 32 bytes")
		}
		if !validDelegationSecret(cfg.IntegrationServiceCredential) {
			return nil, fmt.Errorf("CONVERSATION_INTEGRATION_SERVICE_API_KEY must be a non-placeholder secret of at least 32 bytes")
		}
		if cfg.IntegrationInternalKey == cfg.IntegrationServiceCredential {
			return nil, fmt.Errorf("CONVERSATION_INTEGRATION_SERVICE_API_KEY must be distinct from INTEGRATION_INTERNAL_API_KEY")
		}
		for serviceID, delegationKey := range cfg.DelegationKeys {
			if cfg.IntegrationInternalKey == delegationKey {
				return nil, fmt.Errorf("INTEGRATION_INTERNAL_API_KEY must be distinct from the %s delegation token", serviceID)
			}
			if cfg.IntegrationServiceCredential == delegationKey {
				return nil, fmt.Errorf("CONVERSATION_INTEGRATION_SERVICE_API_KEY must be distinct from the %s delegation token", serviceID)
			}
		}
		if !validKeyID(cfg.AttestationKeyID) {
			return nil, fmt.Errorf("CONVERSATION_PROVIDER_WRITE_ATTESTATION_KEY_ID must be a non-placeholder identifier")
		}
		for name, secret := range map[string]string{
			"INTEGRATION_INTERNAL_API_KEY":             cfg.IntegrationInternalKey,
			"CONVERSATION_INTEGRATION_SERVICE_API_KEY": cfg.IntegrationServiceCredential,
			"CONVERSATION_GATEWAY_SERVICE_TOKEN":       cfg.DelegationKeys["verevon-gateway"],
			"CONVERSATION_CORE_INGEST_SERVICE_TOKEN":   cfg.DelegationKeys["conversation-ingest"],
		} {
			if attestationPrivateKeyEncoded == secret {
				return nil, fmt.Errorf("CONVERSATION_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY must be distinct from %s", name)
			}
		}
		privateKey, err := attestation.DecodePrivateKey(attestationPrivateKeyEncoded)
		if err != nil {
			return nil, err
		}
		if _, err := attestation.NewSigner(attestation.Config{
			PrivateKey: privateKey, KeyID: cfg.AttestationKeyID,
			Issuer: attestation.IssuerConversationCore, Audience: attestation.AudienceIntegrationCore,
			Presenter: attestation.PresenterConversationCore,
		}); err != nil {
			return nil, err
		}
		cfg.AttestationPrivateKey = privateKey
	}

	return cfg, nil
}

var serviceIDPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{1,127}$`)

func validKeyID(value string) bool {
	value = strings.TrimSpace(value)
	if !serviceIDPattern.MatchString(value) {
		return false
	}
	normalized := strings.ToLower(strings.NewReplacer("_", "-", " ", "-").Replace(value))
	for _, prefix := range []string{"change-me", "replace-with", "replace-me", "placeholder", "your-"} {
		if strings.HasPrefix(normalized, prefix) {
			return false
		}
	}
	return true
}

func validServiceURL(value string) bool {
	parsed, err := url.Parse(strings.TrimSpace(value))
	return err == nil &&
		(parsed.Scheme == "http" || parsed.Scheme == "https") &&
		parsed.Host != "" &&
		parsed.User == nil &&
		parsed.RawQuery == "" &&
		parsed.Fragment == ""
}

func validateDelegationKeys(keys map[string]string) error {
	for _, serviceID := range []string{"verevon-gateway", "conversation-ingest"} {
		if !validDelegationSecret(keys[serviceID]) {
			return fmt.Errorf("delegation token for %s must be a non-placeholder secret of at least 32 bytes", serviceID)
		}
	}
	if keys["verevon-gateway"] == keys["conversation-ingest"] {
		return fmt.Errorf("conversation delegation tokens must be distinct per service")
	}
	return nil
}

func validDelegationSecret(secret string) bool {
	secret = strings.TrimSpace(secret)
	lower := strings.ToLower(secret)
	normalized := strings.NewReplacer("_", "-", " ", "-").Replace(lower)
	if len(secret) < 32 {
		return false
	}
	for _, prefix := range []string{"change-me", "replace-with", "replace-me", "placeholder", "your-"} {
		if strings.HasPrefix(normalized, prefix) {
			return false
		}
	}
	return true
}

func getEnv(key, fallback string) string {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	return value
}

func getEnvInt(key string, fallback int) int {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	parsed, err := strconv.Atoi(value)
	if err != nil || parsed < 1 || parsed > 65535 {
		return fallback
	}
	return parsed
}

// getEnvPositiveInt is like getEnvInt but without the 65535 port-range
// clamp, for settings measured in seconds (e.g. reconciliation interval/
// timeout) rather than a port number.
func getEnvPositiveInt(key string, fallback int) int {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	parsed, err := strconv.Atoi(value)
	if err != nil || parsed < 1 {
		return fallback
	}
	return parsed
}
