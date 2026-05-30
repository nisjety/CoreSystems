package config

import (
	"fmt"
	"os"

	"github.com/joho/godotenv"
)

type Config struct {
	HTTPPort        int
	GRPCPort        int
	MetricsPort     int
	DatabaseURL     string
	NATSURL         string
	NATSToken       string
	NATSSharedURL   string
	NATSSharedToken string
	AuthServiceURL  string
	UserServiceURL  string
	ServiceName     string
	Redis           RedisConfig
}

type RedisConfig struct {
	Host     string
	Port     string
	Password string
	DB       int
	Enabled  bool
}

func Load() (*Config, error) {
	_ = godotenv.Load()

	cfg := &Config{
		HTTPPort:        getEnvInt("HTTP_PORT", 3013),
		GRPCPort:        getEnvInt("GRPC_PORT", 9090),
		MetricsPort:     getEnvInt("METRICS_PORT", 9091),
		DatabaseURL:     getEnv("DATABASE_URL", ""),
		NATSURL:         getEnv("NATS_URL", "nats://controlplane-nats:4222"),
		NATSToken:       getEnv("NATS_TOKEN", getEnv("NATS_AUTH_TOKEN", "")),
		NATSSharedURL:   getEnv("VELION_NATS_URL", getEnv("NATS_SHARED_URL", "")),
		NATSSharedToken: getEnv("VELION_NATS_TOKEN", getEnv("NATS_SHARED_TOKEN", "")),
		AuthServiceURL:  getEnv("AUTH_SERVICE_URL", "http://auth-service:3011"),
		UserServiceURL:  getEnv("USER_SERVICE_URL", "http://user-service:3012"),
		ServiceName:     getEnv("SERVICE_NAME", "org-core"),
		Redis: RedisConfig{
			Host:     getEnv("REDIS_HOST", "aquatiq-redis-local"),
			Port:     getEnv("REDIS_PORT", "6379"),
			Password: getEnv("REDIS_PASSWORD", ""),
			DB:       getEnvInt("REDIS_DB", 3),
			Enabled:  getEnvBool("REDIS_ENABLED", false),
		},
	}

	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}

	return cfg, nil
}

func getEnv(key, fallback string) string {
	v := os.Getenv(key)
	if v == "" {
		return fallback
	}
	return v
}

func getEnvInt(key string, fallback int) int {
	v := os.Getenv(key)
	if v == "" {
		return fallback
	}
	parsed := fallback
	_, _ = fmt.Sscanf(v, "%d", &parsed)
	return parsed
}

func getEnvBool(key string, fallback bool) bool {
	v := os.Getenv(key)
	if v == "" {
		return fallback
	}
	return v == "true" || v == "1" || v == "yes"
}
