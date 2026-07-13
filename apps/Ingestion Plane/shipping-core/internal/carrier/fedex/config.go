package fedex

import (
	"fmt"
	"os"
)

// Config holds FedEx OAuth2 client credentials from the FedEx Developer
// Portal (developer.fedex.com → your project → API Key/Secret Key).
type Config struct {
	ClientID     string
	ClientSecret string
	// AccountNumber is required: FedEx's Rate API quotes against a
	// specific account (rateRequestType ACCOUNT).
	AccountNumber string
	// BaseURL defaults to production (https://apis.fedex.com). New
	// developer-portal projects get SANDBOX credentials that only work
	// against https://apis-sandbox.fedex.com until the project is moved
	// to production — set FEDEX_API_BASE_URL accordingly.
	BaseURL string
	// LiveBooking must be explicitly true to allow Book/Label/Track against
	// a non-sandbox BaseURL. Like DHL/UPS, FedEx has no per-request test
	// flag — sandbox vs production is purely which host you call.
	LiveBooking bool
}

// NewConfigFromEnv reads FEDEX_CLIENT_ID, FEDEX_CLIENT_SECRET, and
// FEDEX_ACCOUNT_NUMBER (all required) plus optional FEDEX_API_BASE_URL and
// FEDEX_LIVE_BOOKING. Missing vars are all named in one error, and main.go
// treats that as "not configured" (skip adapter), never a startup failure.
func NewConfigFromEnv() (Config, error) {
	cfg := Config{
		ClientID:      os.Getenv("FEDEX_CLIENT_ID"),
		ClientSecret:  os.Getenv("FEDEX_CLIENT_SECRET"),
		AccountNumber: os.Getenv("FEDEX_ACCOUNT_NUMBER"),
		BaseURL:       os.Getenv("FEDEX_API_BASE_URL"),
		LiveBooking:   os.Getenv("FEDEX_LIVE_BOOKING") == "true",
	}

	var missing []string
	if cfg.ClientID == "" {
		missing = append(missing, "FEDEX_CLIENT_ID")
	}
	if cfg.ClientSecret == "" {
		missing = append(missing, "FEDEX_CLIENT_SECRET")
	}
	if cfg.AccountNumber == "" {
		missing = append(missing, "FEDEX_ACCOUNT_NUMBER")
	}
	if len(missing) > 0 {
		return Config{}, fmt.Errorf("fedex: missing required environment variables: %v", missing)
	}
	return cfg, nil
}
