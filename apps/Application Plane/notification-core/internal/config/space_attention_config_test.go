package config

import "testing"

// WS11-S9: NOTIFICATION_SPACE_ATTENTION_SERVICE_TOKEN registers the
// "space-attention" delegation key. Empty keeps the principal disabled.

func TestLoadRegistersSpaceAttentionKeyFromItsToken(t *testing.T) {
	baseEnvironment(t)
	secret := "space-attention-test-secret-at-least-32-bytes"
	t.Setenv("NOTIFICATION_SPACE_ATTENTION_SERVICE_TOKEN", secret)
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if got := cfg.DelegationKeys["space-attention"]; got != secret {
		t.Fatalf("space-attention key registered = %t, want the configured secret", got != "")
	}
	if cfg.DelegationKeys["verevon-gateway"] == secret {
		t.Fatal("the space-attention secret must not be reused as the gateway key")
	}
}

func TestLoadTrimsTheSpaceAttentionToken(t *testing.T) {
	baseEnvironment(t)
	secret := "space-attention-test-secret-at-least-32-bytes"
	t.Setenv("NOTIFICATION_SPACE_ATTENTION_SERVICE_TOKEN", "  "+secret+"\n")
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if got := cfg.DelegationKeys["space-attention"]; got != secret {
		t.Fatal("space-attention token was not trimmed")
	}
}

func TestLoadLeavesSpaceAttentionDisabledWhenUnset(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_SPACE_ATTENTION_SERVICE_TOKEN", "")
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if _, present := cfg.DelegationKeys["space-attention"]; present {
		t.Fatal("an empty token must not register the space-attention principal")
	}
	if cfg.DelegationKeys["verevon-gateway"] == "" {
		t.Fatal("the gateway key must stay registered")
	}
}

func TestLoadRejectsAShortSpaceAttentionToken(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_SPACE_ATTENTION_SERVICE_TOKEN", "too-short")
	if _, err := Load(); err == nil {
		t.Fatal("Load() error = nil, want a short space-attention token error")
	}
}

func TestLoadRejectsPlaceholderSpaceAttentionTokens(t *testing.T) {
	for _, placeholder := range []string{
		"replace-with-dedicated-random-32-byte-minimum-key",
		"change-me-space-attention-secret-at-least-32-bytes",
		"CHANGE-ME-space-attention-secret-at-least-32-bytes",
	} {
		t.Run(placeholder[:9], func(t *testing.T) {
			baseEnvironment(t)
			t.Setenv("NOTIFICATION_SPACE_ATTENTION_SERVICE_TOKEN", placeholder)
			if _, err := Load(); err == nil {
				t.Fatal("Load() error = nil, want placeholder token rejection")
			}
		})
	}
}
