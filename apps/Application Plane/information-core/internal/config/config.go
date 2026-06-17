package config

import (
	"os"
	"strconv"
	"strings"
	"time"
)

type Config struct {
	Port           int
	ServiceName    string
	InternalAPIKey string
	UserAgent      string
	BringAPIUID    string
	BringAPIKey    string
}

func Load() Config {
	return Config{
		Port:           getInt("PORT", 3190),
		ServiceName:    getString("SERVICE_NAME", "information-core"),
		InternalAPIKey: strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")),
		UserAgent:      getString("INFORMATION_CORE_USER_AGENT", "VelionInformationCore/1.0 (ops@velion.local)"),
		BringAPIUID:    strings.TrimSpace(os.Getenv("BRING_API_UID")),
		BringAPIKey:    strings.TrimSpace(os.Getenv("BRING_API_KEY")),
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
