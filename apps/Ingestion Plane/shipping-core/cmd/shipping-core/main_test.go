package main

import (
	"io"
	"log/slog"
	"testing"
)

func silentLogger() *slog.Logger {
	return slog.New(slog.NewTextHandler(io.Discard, nil))
}

// allCarrierEnvVars covers every credential buildCarriers reads, so each
// test starts from a clean, explicit environment.
var allCarrierEnvVars = []string{
	"BRING_API_UID", "BRING_API_KEY", "BRING_CUSTOMER_NUMBER", "BRING_API_BASE_URL",
	"UPS_CLIENT_ID", "UPS_CLIENT_SECRET", "UPS_ACCOUNT_NUMBER", "UPS_API_BASE_URL",
	"FEDEX_CLIENT_ID", "FEDEX_CLIENT_SECRET", "FEDEX_ACCOUNT_NUMBER", "FEDEX_API_BASE_URL",
}

func clearCarrierEnv(t *testing.T) {
	t.Helper()
	for _, key := range allCarrierEnvVars {
		t.Setenv(key, "")
	}
}

func carrierCodes(t *testing.T) map[string]bool {
	t.Helper()
	codes := map[string]bool{}
	for _, a := range buildCarriers(silentLogger()) {
		codes[a.Info().Code] = true
	}
	return codes
}

func TestBuildCarriers_NoCredentials_UsesSixMocks(t *testing.T) {
	clearCarrierEnv(t)

	adapters := buildCarriers(silentLogger())
	if len(adapters) != 6 {
		t.Fatalf("got %d adapters, want 6 (all mocks)", len(adapters))
	}
	codes := carrierCodes(t)
	if !codes["mock-bring"] {
		t.Error("expected mock-bring when no credentials are set")
	}
	if codes["ups"] || codes["fedex"] || codes["bring"] {
		t.Error("no real adapters should be present without credentials")
	}
}

func TestBuildCarriers_WithBringCredentials_ReplacesMockBring(t *testing.T) {
	clearCarrierEnv(t)
	t.Setenv("BRING_API_UID", "user@example.com")
	t.Setenv("BRING_API_KEY", "key")
	t.Setenv("BRING_CUSTOMER_NUMBER", "5")

	adapters := buildCarriers(silentLogger())
	if len(adapters) != 6 {
		t.Fatalf("got %d adapters, want 6 (5 mocks + real bring)", len(adapters))
	}
	codes := carrierCodes(t)
	if codes["mock-bring"] {
		t.Error("mock-bring should be removed once real Bring credentials are configured")
	}
	if !codes["bring"] {
		t.Error("expected the real bring adapter to be present")
	}
}

func TestBuildCarriers_WithUPSAndFedExCredentials_AppendsRealAdapters(t *testing.T) {
	clearCarrierEnv(t)
	t.Setenv("UPS_CLIENT_ID", "id")
	t.Setenv("UPS_CLIENT_SECRET", "secret")
	t.Setenv("FEDEX_CLIENT_ID", "id")
	t.Setenv("FEDEX_CLIENT_SECRET", "secret")
	t.Setenv("FEDEX_ACCOUNT_NUMBER", "740561073")

	adapters := buildCarriers(silentLogger())
	// 6 mocks + ups + fedex; no mock-ups/mock-fedex exists to replace.
	if len(adapters) != 8 {
		t.Fatalf("got %d adapters, want 8 (6 mocks + ups + fedex)", len(adapters))
	}
	codes := carrierCodes(t)
	if !codes["ups"] || !codes["fedex"] {
		t.Errorf("expected real ups and fedex adapters, got %v", codes)
	}
	if !codes["mock-bring"] {
		t.Error("mock-bring should still be present (no Bring credentials)")
	}
}

func TestBuildCarriers_FedExWithoutAccountNumber_Skipped(t *testing.T) {
	clearCarrierEnv(t)
	t.Setenv("FEDEX_CLIENT_ID", "id")
	t.Setenv("FEDEX_CLIENT_SECRET", "secret")
	// FEDEX_ACCOUNT_NUMBER intentionally unset — required for rate quotes.

	codes := carrierCodes(t)
	if codes["fedex"] {
		t.Error("fedex should be skipped when FEDEX_ACCOUNT_NUMBER is missing")
	}
}
