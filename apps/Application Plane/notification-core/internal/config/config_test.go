package config

import "testing"

func baseEnvironment(t *testing.T) {
	t.Helper()
	t.Setenv("DATABASE_URL", "postgres://test")
	t.Setenv("INTERNAL_API_KEY", "test-key")
	t.Setenv("NOTIFICATION_GATEWAY_SERVICE_TOKEN", "gateway-test-secret-at-least-32-bytes")
	t.Setenv("NOVU_SECRET_KEY", "")
}

func TestLoadRequiresGatewayDelegationToken(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_GATEWAY_SERVICE_TOKEN", "")
	if _, err := Load(); err == nil {
		t.Fatal("Load() error = nil, want missing gateway delegation token error")
	}
}

func TestLoadRejectsShortGatewayDelegationToken(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_GATEWAY_SERVICE_TOKEN", "too-short")
	if _, err := Load(); err == nil {
		t.Fatal("Load() error = nil, want short gateway delegation token error")
	}
}

func TestLoadRejectsPublishedPlaceholderDelegationTokens(t *testing.T) {
	for _, placeholder := range []string{
		"replace-with-dedicated-random-32-byte-minimum-key",
		"change-me-notification-gateway-secret-32-bytes",
	} {
		t.Run(placeholder[:9], func(t *testing.T) {
			baseEnvironment(t)
			t.Setenv("NOTIFICATION_GATEWAY_SERVICE_TOKEN", placeholder)
			if _, err := Load(); err == nil {
				t.Fatal("Load() error = nil, want placeholder token rejection")
			}
		})
	}
}

func TestLoadRequiresNovuSecretInNovuMode(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_MODE", "novu")
	if _, err := Load(); err == nil {
		t.Fatal("Load() error = nil, want missing NOVU_SECRET_KEY error")
	}
}

func TestLoadAllowsExplicitDisabledMode(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_MODE", "disabled")
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if cfg.DeliveryMode != "disabled" {
		t.Fatalf("DeliveryMode = %q", cfg.DeliveryMode)
	}
}

func TestLoadDefaultsToDisabledMode(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_MODE", "")
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if cfg.DeliveryMode != "disabled" {
		t.Fatalf("DeliveryMode = %q, want disabled", cfg.DeliveryMode)
	}
}

func TestLoadAcceptsConfiguredNovuMode(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_MODE", "novu")
	t.Setenv("NOVU_SECRET_KEY", "test-provider-key")
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if cfg.DeliveryMode != "novu" {
		t.Fatalf("DeliveryMode = %q, want novu", cfg.DeliveryMode)
	}
}

func TestLoadRejectsUnknownDeliveryMode(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_MODE", "stub")
	if _, err := Load(); err == nil {
		t.Fatal("Load() error = nil, want invalid delivery mode error")
	}
}
