package config

import (
	"os"
	"testing"
)

func TestPublisherNATSSettingsPreferVelion(t *testing.T) {
	cfg := &Config{
		NATSSharedURL:        "nats://local:4222",
		NATSSharedToken:      "local-token",
		VelionNATSURL:        "nats://shared:4222",
		VelionNATSToken:      "shared-token",
		velionNATSConfigured: true,
	}

	if got := cfg.PublisherNATSURL(); got != "nats://shared:4222" {
		t.Fatalf("PublisherNATSURL() = %q, want %q", got, "nats://shared:4222")
	}
	if got := cfg.PublisherNATSToken(); got != "shared-token" {
		t.Fatalf("PublisherNATSToken() = %q, want %q", got, "shared-token")
	}
}

func TestPublisherNATSSettingsFallBackToSharedQueue(t *testing.T) {
	cfg := &Config{
		NATSSharedURL:   "nats://local:4222",
		NATSSharedToken: "local-token",
	}

	if got := cfg.PublisherNATSURL(); got != "nats://local:4222" {
		t.Fatalf("PublisherNATSURL() = %q, want %q", got, "nats://local:4222")
	}
	if got := cfg.PublisherNATSToken(); got != "local-token" {
		t.Fatalf("PublisherNATSToken() = %q, want %q", got, "local-token")
	}
}

func TestLoadRejectsPartialVelionPublisherOverrides(t *testing.T) {
	t.Run("url_without_token", func(t *testing.T) {
		setEnvState(t, "VELION_NATS_URL", stringPtr("nats://shared:4222"))
		setEnvState(t, "VELION_NATS_TOKEN", nil)

		_, err := Load()
		if err == nil {
			t.Fatal("Load() error = nil, want validation error")
		}
		if got, want := err.Error(), "VELION_NATS_TOKEN must be set when VELION_NATS_URL is present"; got != want {
			t.Fatalf("Load() error = %q, want %q", got, want)
		}
	})

	t.Run("token_without_url", func(t *testing.T) {
		setEnvState(t, "VELION_NATS_URL", nil)
		setEnvState(t, "VELION_NATS_TOKEN", stringPtr("shared-token"))

		_, err := Load()
		if err == nil {
			t.Fatal("Load() error = nil, want validation error")
		}
		if got, want := err.Error(), "VELION_NATS_URL must be set when VELION_NATS_TOKEN is present"; got != want {
			t.Fatalf("Load() error = %q, want %q", got, want)
		}
	})
}

func TestLoadRejectsEmptyVelionPublisherURL(t *testing.T) {
	setEnvState(t, "VELION_NATS_URL", stringPtr("   "))
	setEnvState(t, "VELION_NATS_TOKEN", stringPtr("shared-token"))

	_, err := Load()
	if err == nil {
		t.Fatal("Load() error = nil, want validation error")
	}
	if got, want := err.Error(), "VELION_NATS_URL must be non-empty when publisher override is configured"; got != want {
		t.Fatalf("Load() error = %q, want %q", got, want)
	}
}

func TestLoadAllowsExplicitEmptyVelionToken(t *testing.T) {
	setEnvState(t, "NATS_SHARED_TOKEN", stringPtr("local-token"))
	setEnvState(t, "VELION_NATS_URL", stringPtr("nats://shared:4222"))
	setEnvState(t, "VELION_NATS_TOKEN", stringPtr(""))

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v, want nil", err)
	}
	if got := cfg.PublisherNATSURL(); got != "nats://shared:4222" {
		t.Fatalf("PublisherNATSURL() = %q, want %q", got, "nats://shared:4222")
	}
	if got := cfg.PublisherNATSToken(); got != "" {
		t.Fatalf("PublisherNATSToken() = %q, want empty string", got)
	}
}

func setEnvState(t *testing.T, key string, value *string) {
	t.Helper()
	originalValue, originalSet := os.LookupEnv(key)
	if value == nil {
		if err := os.Unsetenv(key); err != nil {
			t.Fatalf("Unsetenv(%q) error = %v", key, err)
		}
	} else {
		if err := os.Setenv(key, *value); err != nil {
			t.Fatalf("Setenv(%q) error = %v", key, err)
		}
	}
	t.Cleanup(func() {
		var err error
		if originalSet {
			err = os.Setenv(key, originalValue)
		} else {
			err = os.Unsetenv(key)
		}
		if err != nil {
			t.Fatalf("restore env %q error = %v", key, err)
		}
	})
}

func stringPtr(value string) *string {
	return &value
}