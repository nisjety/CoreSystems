package config

import (
	"encoding/base64"
	"testing"
)

// The ZDR acknowledgement routes are dark by default: a deployment must opt in explicitly, and
// the switch is read from the environment like every other feature switch in this service.
func TestZDRAcknowledgementIsOffUnlessTheEnvironmentTurnsItOn(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))

	t.Setenv("ZDR_ACKNOWLEDGEMENT_ENABLED", "")
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if cfg.ZDRAcknowledgementEnabled {
		t.Fatal("ZDR acknowledgement must be off when the switch is not set")
	}

	t.Setenv("ZDR_ACKNOWLEDGEMENT_ENABLED", "true")
	cfg, err = Load()
	if err != nil {
		t.Fatalf("Load with the switch on: %v", err)
	}
	if !cfg.ZDRAcknowledgementEnabled {
		t.Fatal("ZDR_ACKNOWLEDGEMENT_ENABLED=true must enable the acknowledgement routes")
	}
}
