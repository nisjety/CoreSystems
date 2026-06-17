package config

import (
	"reflect"
	"testing"
)

func TestLoad_Defaults(t *testing.T) {
	t.Setenv("BRIDGE_WEBHOOK_URL", "")
	t.Setenv("BRIDGE_WEBHOOK_CHANNELS", "")
	t.Setenv("SESSION_CORE_ADDR", "")
	t.Setenv("MODEL_GATEWAY_ADDR", "")

	cfg := Load()
	if cfg.HTTPAddr != ":8091" {
		t.Fatalf("HTTPAddr default: got %q", cfg.HTTPAddr)
	}
	if cfg.GRPCAddr != ":9100" {
		t.Fatalf("GRPCAddr default: got %q", cfg.GRPCAddr)
	}
	if cfg.SessionCoreAddr != "session-core:9091" {
		t.Fatalf("SessionCoreAddr default: got %q", cfg.SessionCoreAddr)
	}
	if cfg.WebhookEnabled() {
		t.Fatal("WebhookEnabled should be false without URL")
	}
}

func TestLoad_WebhookEnabled(t *testing.T) {
	t.Setenv("BRIDGE_WEBHOOK_URL", "https://hooks.example.com/abc")
	t.Setenv("BRIDGE_WEBHOOK_CHANNELS", "web, api ,")
	t.Setenv("BRIDGE_WEBHOOK_MAX_ATTEMPTS", "7")

	cfg := Load()
	if !cfg.WebhookEnabled() {
		t.Fatal("expected WebhookEnabled true")
	}
	if !reflect.DeepEqual(cfg.WebhookChannels, []string{"web", "api"}) {
		t.Fatalf("channel parse: got %#v", cfg.WebhookChannels)
	}
	if cfg.WebhookMaxAttempts != 7 {
		t.Fatalf("max attempts: got %d", cfg.WebhookMaxAttempts)
	}
}

func TestEnvInt_Fallback(t *testing.T) {
	t.Setenv("BRIDGE_WEBHOOK_MAX_ATTEMPTS", "not-a-number")
	cfg := Load()
	if cfg.WebhookMaxAttempts != 5 {
		t.Fatalf("expected fallback 5, got %d", cfg.WebhookMaxAttempts)
	}
}

func TestSplitList(t *testing.T) {
	if got := splitList("a,b,c"); !reflect.DeepEqual(got, []string{"a", "b", "c"}) {
		t.Fatalf("got %#v", got)
	}
	if got := splitList(" x , , y "); !reflect.DeepEqual(got, []string{"x", "y"}) {
		t.Fatalf("got %#v", got)
	}
	if got := splitList(""); got != nil {
		t.Fatalf("empty should yield nil, got %#v", got)
	}
}
