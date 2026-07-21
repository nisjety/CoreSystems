package config

import (
	"os"
	"strconv"
	"strings"
	"time"
)

type Config struct {
	Port            int
	ServiceName     string
	InternalAPIKey  string
	UserAgent       string
	LovdataAPIKey   string
	EnturClientName string
	DatexURL        string
	DatexUsername   string
	DatexPassword   string
	FrostURL        string
	FrostClientID   string
}

func Load() Config {
	return Config{
		Port:            getInt("PORT", 3190),
		ServiceName:     getString("SERVICE_NAME", "information-core"),
		InternalAPIKey:  strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")),
		UserAgent:       getString("INFORMATION_CORE_USER_AGENT", "VelionInformationCore/1.0 (ops@velion.local)"),
		LovdataAPIKey:   strings.TrimSpace(os.Getenv("LOVDATA_API_KEY")),
		EnturClientName: getString("ENTUR_CLIENT_NAME", "coresystem-information-core"),
		DatexURL:        strings.TrimSpace(os.Getenv("DATEX_URL")),
		DatexUsername:   strings.TrimSpace(os.Getenv("DATEX_USERNAME")),
		DatexPassword:   os.Getenv("DATEX_PASSWORD"),
		FrostURL:        strings.TrimSpace(os.Getenv("FROST_URL")),
		FrostClientID:   strings.TrimSpace(os.Getenv("FROST_CLIENT_ID")),
	}
}

func getString(key, fallback string) string {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	return value
}

func getInt(key string, fallback int) int {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	parsed, err := strconv.Atoi(value)
	if err != nil {
		return fallback
	}
	return parsed
}

func TTL(seconds int) time.Duration {
	return time.Duration(seconds) * time.Second
}
