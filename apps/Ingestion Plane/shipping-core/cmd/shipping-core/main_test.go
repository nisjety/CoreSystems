package main

import (
	"io"
	"log/slog"
	"strings"
	"testing"

	"shipping-core/internal/carrier"
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
	// Real Bring replaces mock-bring, and the remaining mocks are dropped:
	// once ANY real carrier is configured, invented prices must not compete
	// in the same cheapest-first comparison. See dropMocksWhenRealCarriersExist.
	if len(adapters) != 1 {
		t.Fatalf("got %d adapters, want 1 (real bring only)", len(adapters))
	}
	codes := carrierCodes(t)
	if codes["mock-bring"] {
		t.Error("mock-bring should be removed once real Bring credentials are configured")
	}
	if !codes["bring"] {
		t.Error("expected the real bring adapter to be present")
	}
	for code := range codes {
		if strings.HasPrefix(code, "mock-") {
			t.Errorf("mock carrier %q must not survive alongside a real carrier", code)
		}
	}
}

// The failure this encodes: on 2026-09-14 a real 66 kg quote returned
// "DSV" 669 NOK and "PostNord" 808 NOK ahead of a real UPS quote of
// 1 364,60 NOK. Both cheap options were mock adapters, so the two carriers
// the customer would have been offered were fabricated.
func TestBuildCarriers_MocksNeverCompeteWithRealCarriers(t *testing.T) {
	clearCarrierEnv(t)
	t.Setenv("UPS_CLIENT_ID", "id")
	t.Setenv("UPS_CLIENT_SECRET", "secret")

	for _, a := range buildCarriers(silentLogger()) {
		if a.Info().Mode == carrier.ModeMock || strings.HasPrefix(a.Info().Code, "mock-") {
			t.Fatalf("mock carrier %q served alongside a real one", a.Info().Code)
		}
	}
}

// A local stack with no credentials must still answer with something.
func TestBuildCarriers_MocksKeptWhenNoRealCarrierExists(t *testing.T) {
	clearCarrierEnv(t)

	adapters := buildCarriers(silentLogger())
	if len(adapters) != 6 {
		t.Fatalf("got %d adapters, want the 6 mocks when nothing real is configured", len(adapters))
	}
}

// Keeping mocks alongside real carriers stays possible, but only by saying so.
func TestBuildCarriers_MocksKeptWhenExplicitlyAllowed(t *testing.T) {
	clearCarrierEnv(t)
	t.Setenv("UPS_CLIENT_ID", "id")
	t.Setenv("UPS_CLIENT_SECRET", "secret")
	t.Setenv("SHIPPING_ALLOW_MOCK_CARRIERS", "true")

	codes := carrierCodes(t)
	if !codes["mock-bring"] || !codes["ups"] {
		t.Errorf("expected mocks and real ups together under the opt-in, got %v", codes)
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
	// ups + fedex only: the mocks are dropped now that real carriers exist,
	// including mock-bring even though Bring itself has no credentials. A
	// carrier with no integration is absent from the comparison rather than
	// represented by an invented price.
	if len(adapters) != 2 {
		t.Fatalf("got %d adapters, want 2 (ups + fedex)", len(adapters))
	}
	codes := carrierCodes(t)
	if !codes["ups"] || !codes["fedex"] {
		t.Errorf("expected real ups and fedex adapters, got %v", codes)
	}
	if codes["mock-bring"] {
		t.Error("mock-bring must not be served once real carriers are configured")
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
