package config

import "testing"

func TestLoadReadsDedicatedDataPlaneNATSToken(t *testing.T) {
	t.Setenv("DATAPLANE_NATS_TOKEN", "opaque-token")
	t.Setenv("EVENT_SIGNING_PRIVATE_KEY_PATH", "/run/event-keys/documents-events.pem")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if cfg.NatsToken != "opaque-token" {
		t.Fatalf("NatsToken was not loaded from the dedicated environment variable")
	}
}

func TestLoadRequiresScopedGDPRConsumerCredentialsInReleaseMode(t *testing.T) {
	t.Setenv("EVENT_SIGNING_PRIVATE_KEY_PATH", "/run/event-keys/documents-events.pem")
	t.Setenv("GDPR_DURABLE_CONSUMER_REQUIRED", "1")
	t.Setenv("NATS_SHARED_URL", "nats://control-shared-nats:4222")
	t.Setenv("NATS_SHARED_USER", "documents-api-gdpr")
	t.Setenv("NATS_SHARED_PASSWORD", "")
	t.Setenv("VELION_NATS_TOKEN", "legacy-token-must-not-be-used")
	if _, err := Load(); err == nil {
		t.Fatal("release config accepted missing scoped GDPR password")
	}

	t.Setenv("NATS_SHARED_PASSWORD", "0123456789abcdef0123456789abcdef")
	cfg, err := Load()
	if err != nil {
		t.Fatalf("scoped release config rejected: %v", err)
	}
	if cfg.SharedNatsUser != "documents-api-gdpr" || cfg.SharedNatsPassword == "" || !cfg.GDPRConsumerRequired {
		t.Fatalf("scoped GDPR config not loaded: %+v", cfg)
	}
}
