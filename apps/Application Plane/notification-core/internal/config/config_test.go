package config

import "testing"

func TestLoadSuccess(t *testing.T) {
	t.Setenv("DATABASE_URL", "postgres://appuser:secret@localhost:5432/application_plane?sslmode=disable")
	t.Setenv("INTERNAL_API_KEY", "internal-secret")
	t.Setenv("PORT", "")
	t.Setenv("SERVICE_NAME", "")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	if cfg.InternalAPIKey != "internal-secret" {
		t.Fatalf("InternalAPIKey = %q, want %q", cfg.InternalAPIKey, "internal-secret")
	}
	if cfg.HTTPPort != 3140 {
		t.Fatalf("HTTPPort = %d, want %d", cfg.HTTPPort, 3140)
	}
	if cfg.ServiceName != "notification-core" {
		t.Fatalf("ServiceName = %q, want %q", cfg.ServiceName, "notification-core")
	}
	if cfg.DatabaseURL == "" {
		t.Fatal("DatabaseURL = empty, want non-empty")
	}
}

func TestLoadCustomValues(t *testing.T) {
	t.Setenv("DATABASE_URL", "postgres://appuser:secret@localhost:5432/custom_notifications?sslmode=disable")
	t.Setenv("INTERNAL_API_KEY", "internal-secret")
	t.Setenv("PORT", "8080")
	t.Setenv("SERVICE_NAME", "custom-notification-core")
	t.Setenv("VELION_NATS_URL", "nats://custom-velion-nats:4222")
	t.Setenv("VELION_NATS_TOKEN", "nats-token")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	if cfg.HTTPPort != 8080 {
		t.Fatalf("HTTPPort = %d, want %d", cfg.HTTPPort, 8080)
	}
	if cfg.ServiceName != "custom-notification-core" {
		t.Fatalf("ServiceName = %q, want %q", cfg.ServiceName, "custom-notification-core")
	}
	if cfg.DatabaseURL != "postgres://appuser:secret@localhost:5432/custom_notifications?sslmode=disable" {
		t.Fatalf("DatabaseURL = %q, want custom value", cfg.DatabaseURL)
	}
	if cfg.NATSURL != "nats://custom-velion-nats:4222" {
		t.Fatalf("NATSURL = %q, want %q", cfg.NATSURL, "nats://custom-velion-nats:4222")
	}
	if cfg.NATSToken != "nats-token" {
		t.Fatalf("NATSToken = %q, want %q", cfg.NATSToken, "nats-token")
	}
}



func TestLoadMissingDatabaseURL(t *testing.T) {
	t.Setenv("DATABASE_URL", "")
	t.Setenv("INTERNAL_API_KEY", "internal-secret")

	cfg, err := Load()
	if err == nil {
		t.Fatal("Load() error = nil, want error")
	}
	if cfg != nil {
		t.Fatalf("Load() config = %#v, want nil", cfg)
	}
	if err.Error() != "DATABASE_URL is required" {
		t.Fatalf("Load() error = %q, want %q", err.Error(), "DATABASE_URL is required")
	}
}

func TestLoadMissingInternalKey(t *testing.T) {
	t.Setenv("DATABASE_URL", "postgres://appuser:secret@localhost:5432/application_plane?sslmode=disable")
	t.Setenv("INTERNAL_API_KEY", "")

	cfg, err := Load()
	if err == nil {
		t.Fatal("Load() error = nil, want error")
	}
	if cfg != nil {
		t.Fatalf("Load() config = %#v, want nil", cfg)
	}
	if err.Error() != "INTERNAL_API_KEY is required" {
		t.Fatalf("Load() error = %q, want %q", err.Error(), "INTERNAL_API_KEY is required")
	}
}

func TestLoadWhitespaceInternalKey(t *testing.T) {
	t.Setenv("DATABASE_URL", "postgres://appuser:secret@localhost:5432/application_plane?sslmode=disable")
	t.Setenv("INTERNAL_API_KEY", "   ")

	cfg, err := Load()
	if err == nil {
		t.Fatal("Load() error = nil, want error")
	}
	if cfg != nil {
		t.Fatalf("Load() config = %#v, want nil", cfg)
	}
}
