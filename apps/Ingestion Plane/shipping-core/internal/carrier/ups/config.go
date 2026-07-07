package ups

import (
	"fmt"
	"os"
)

// Config holds UPS OAuth2 client credentials from the UPS Developer
// Portal (developer.ups.com → your app → Client ID/Secret).
type Config struct {
	ClientID     string
	ClientSecret string
	// AccountNumber (the 6-char UPS shipper number) is optional for
	// list-rate shopping; required later for negotiated rates and booking.
	AccountNumber string
	// BaseURL defaults to production. Set to https://wwwcie.ups.com for
	// UPS's Customer Integration Environment (test), or a test server URL
	// in unit tests. Both the OAuth token endpoint and the Rating API
	// hang off this same host.
	BaseURL string
}

// NewConfigFromEnv reads UPS_CLIENT_ID and UPS_CLIENT_SECRET (both
// required) plus optional UPS_ACCOUNT_NUMBER and UPS_API_BASE_URL.
// Missing required vars are all named in one error, and main.go treats
// that as "not configured" (skip adapter), never a startup failure.
func NewConfigFromEnv() (Config, error) {
	cfg := Config{
		ClientID:      os.Getenv("UPS_CLIENT_ID"),
		ClientSecret:  os.Getenv("UPS_CLIENT_SECRET"),
		AccountNumber: os.Getenv("UPS_ACCOUNT_NUMBER"),
		BaseURL:       os.Getenv("UPS_API_BASE_URL"),
	}

	var missing []string
	if cfg.ClientID == "" {
		missing = append(missing, "UPS_CLIENT_ID")
	}
	if cfg.ClientSecret == "" {
		missing = append(missing, "UPS_CLIENT_SECRET")
	}
	if len(missing) > 0 {
		return Config{}, fmt.Errorf("ups: missing required environment variables: %v", missing)
	}
	return cfg, nil
}
