package dhl

import (
	"fmt"
	"os"
)

// Config holds MyDHL API credentials from the DHL Developer Portal
// (developer.dhl.com → your app → "API Key"/"API Secret", shown under
// Credentials once the app is Approved). MyDHL API authenticates with plain
// HTTP Basic Auth: the API Key is the username, the API Secret is the
// password — there is no OAuth2 flow for this API.
type Config struct {
	APIKey    string // Mybring calls this "username"; DHL portal calls it "API Key (Username / site ID)"
	APISecret string // DHL portal calls it "API Secret (Password)"
	// AccountNumber is the DHL Express shipper account number. Optional for
	// list-rate shopping (a rate request omitting productCode/account
	// returns all available products); required for account/negotiated
	// rates and for booking.
	AccountNumber string
	// BaseURL defaults to the MyDHL API production host. Set
	// DHL_API_BASE_URL to https://express.api.dhl.com/mydhlapi/test for the
	// sandbox environment (this is what a freshly-approved DHL app is
	// scoped to — e.g. an app named "...-sandbox-..." in the portal), or a
	// test server URL in unit tests.
	BaseURL string
}

// NewConfigFromEnv reads DHL_API_KEY and DHL_API_SECRET (both required)
// plus optional DHL_ACCOUNT_NUMBER and DHL_API_BASE_URL. Missing required
// vars are all named in one error; main.go treats that as "not configured"
// (skip the real adapter, keep the mock) rather than a startup failure —
// DHL Express access arrives piecemeal as the customer account is approved
// (docs/TASKS.md Fase 0; DHL was previously blocked on account approval,
// not code).
func NewConfigFromEnv() (Config, error) {
	cfg := Config{
		APIKey:        os.Getenv("DHL_API_KEY"),
		APISecret:     os.Getenv("DHL_API_SECRET"),
		AccountNumber: os.Getenv("DHL_ACCOUNT_NUMBER"),
		BaseURL:       os.Getenv("DHL_API_BASE_URL"),
	}

	var missing []string
	if cfg.APIKey == "" {
		missing = append(missing, "DHL_API_KEY")
	}
	if cfg.APISecret == "" {
		missing = append(missing, "DHL_API_SECRET")
	}
	if len(missing) > 0 {
		return Config{}, fmt.Errorf("dhl: missing required environment variables: %v", missing)
	}
	return cfg, nil
}
