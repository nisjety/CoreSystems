package config

import (
	"strings"
	"testing"
)

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

func TestLoadRejectsShortDeliveryCallbackSecret(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_CALLBACK_SECRET", "too-short")

	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "NOTIFICATION_DELIVERY_CALLBACK_SECRET") {
		t.Fatalf("Load() error = %v, want callback secret validation error", err)
	}
}

func TestLoadAllowsEmptyDeliveryCallbackSecret(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_CALLBACK_SECRET", "")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if cfg.DeliveryCallbackSecret != "" {
		t.Fatalf("DeliveryCallbackSecret = %q, want empty", cfg.DeliveryCallbackSecret)
	}
}

func TestLoadDefaultsDeliveryWorkerDisabled(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_MODE", "disabled")
	t.Setenv("NOTIFICATION_DELIVERY_WORKER_ENABLED", "")
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if cfg.DeliveryWorkerEnabled {
		t.Fatal("DeliveryWorkerEnabled = true, want false by default")
	}
}

func TestLoadRejectsDeliveryWorkerWithoutProvider(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_MODE", "disabled")
	t.Setenv("NOTIFICATION_DELIVERY_WORKER_ENABLED", "true")
	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "requires NOTIFICATION_DELIVERY_MODE=novu") {
		t.Fatalf("Load() error = %v, want disabled-provider rejection", err)
	}
}

func TestLoadRejectsDeliveryWorkerWithoutCallbackVerifier(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_MODE", "novu")
	t.Setenv("NOVU_SECRET_KEY", "provider-test-key")
	t.Setenv("NOTIFICATION_DELIVERY_WORKER_ENABLED", "true")
	t.Setenv("NOTIFICATION_DELIVERY_CALLBACK_SECRET", "")
	if _, err := Load(); err == nil || !strings.Contains(err.Error(), "requires NOTIFICATION_DELIVERY_CALLBACK_SECRET") {
		t.Fatalf("Load() error = %v, want callback-verifier rejection", err)
	}
}

func TestLoadAcceptsDeliveryWorkerWithExplicitProviderAndVerifier(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_MODE", "novu")
	t.Setenv("NOVU_SECRET_KEY", "provider-test-key")
	t.Setenv("NOTIFICATION_DELIVERY_WORKER_ENABLED", "true")
	t.Setenv("NOTIFICATION_DELIVERY_CALLBACK_SECRET", "delivery-callback-secret-32-bytes-minimum")
	t.Setenv("NOTIFICATION_DELIVERY_WORKER_POLL_INTERVAL", "250ms")
	t.Setenv("NOTIFICATION_DELIVERY_WORKER_LEASE", "2s")
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if !cfg.DeliveryWorkerEnabled || cfg.DeliveryWorkerPollInterval.String() != "250ms" || cfg.DeliveryWorkerLease.String() != "2s" {
		t.Fatalf("worker config = enabled:%v poll:%s lease:%s", cfg.DeliveryWorkerEnabled, cfg.DeliveryWorkerPollInterval, cfg.DeliveryWorkerLease)
	}
}
