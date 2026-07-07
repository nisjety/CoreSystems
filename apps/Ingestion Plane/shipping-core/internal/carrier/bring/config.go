package bring

import (
	"fmt"
	"os"
)

// Config holds the Mybring account credentials needed to call the real
// Bring Shipping Guide API. See docs/CARRIERS.md for how to obtain them
// (a Mybring account, an API key from the Mybring API settings page, and
// a customer number with financial rights).
type Config struct {
	APIUID         string // Mybring account email — X-Mybring-API-Uid header
	APIKey         string // Mybring API key — X-Mybring-API-Key header
	CustomerNumber string // Bring customer number the quote is requested on behalf of
	// ClientURL identifies the service calling the API — X-Bring-Client-URL
	// header, required on every request alongside the two above (per
	// developer.bring.com/api's "Connect to the APIs" section). Defaults to
	// velion.no; override with BRING_CLIENT_URL for a different environment.
	ClientURL string
	BaseURL   string // override for testing; empty uses defaultBaseURL
	// BookingBaseURL / TrackingBaseURL override the Booking and Tracking API
	// endpoints for testing; empty uses the production defaults in booking.go.
	BookingBaseURL  string
	TrackingBaseURL string
	// LiveBooking must be EXPLICITLY true (BRING_LIVE_BOOKING=true) for real
	// bookings: the Booking API request carries testIndicator=true otherwise,
	// so a misconfigured environment can never place a real freight order by
	// accident.
	LiveBooking bool
}

// NewConfigFromEnv reads BRING_API_UID, BRING_API_KEY, and
// BRING_CUSTOMER_NUMBER (all required) plus optional BRING_API_BASE_URL.
// Returns an error naming every missing variable at once rather than
// failing on the first one, and main.go uses this to decide whether to
// register the real adapter or fall back to the mock — see docs/TASKS.md
// Fase 2, since Mybring API access is a Fase 0 business prerequisite that
// may not exist yet.
func NewConfigFromEnv() (Config, error) {
	cfg := Config{
		APIUID:          os.Getenv("BRING_API_UID"),
		APIKey:          os.Getenv("BRING_API_KEY"),
		CustomerNumber:  os.Getenv("BRING_CUSTOMER_NUMBER"),
		ClientURL:       os.Getenv("BRING_CLIENT_URL"),
		BaseURL:         os.Getenv("BRING_API_BASE_URL"),
		BookingBaseURL:  os.Getenv("BRING_BOOKING_BASE_URL"),
		TrackingBaseURL: os.Getenv("BRING_TRACKING_BASE_URL"),
		LiveBooking:     os.Getenv("BRING_LIVE_BOOKING") == "true",
	}

	var missing []string
	if cfg.APIUID == "" {
		missing = append(missing, "BRING_API_UID")
	}
	if cfg.APIKey == "" {
		missing = append(missing, "BRING_API_KEY")
	}
	if cfg.CustomerNumber == "" {
		missing = append(missing, "BRING_CUSTOMER_NUMBER")
	}
	if len(missing) > 0 {
		return Config{}, fmt.Errorf("bring: missing required environment variables: %v", missing)
	}
	return cfg, nil
}
