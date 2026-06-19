package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/joho/godotenv"
)

type Config struct {
	Server      ServerConfig
	Database    DatabaseConfig
	Redis       RedisConfig
	NATS        NATSConfig
	Auth        AuthConfig
	Convex      ConvexConfig
	Logging     LoggingConfig
	OrgCore     OrgCoreConfig
	UserCore    UserCoreConfig
	BillingCore BillingCoreConfig
}

// UserCoreConfig — upstream user-core for the Control Session aggregator (G10).
type UserCoreConfig struct {
	URL string
}

// BillingCoreConfig — upstream billing-core for the Control Session aggregator (G10).
type BillingCoreConfig struct {
	URL string
}

type ServerConfig struct {
	HTTPPort    string
	GRPCPort    string
	Environment string
}

type DatabaseConfig struct {
	URL            string
	Host           string
	Port           string
	User           string
	Password       string
	Name           string
	SSLMode        string
	MaxConnections int
	MinConnections int
}

type RedisConfig struct {
	Addr     string
	Password string
	DB       int
}

type NATSConfig struct {
	LocalURL    string
	Token       string
	SharedURL   string
	SharedToken string
	// ModelPlaneV2RolloutPct controls what percentage (0-100) of new
	// sessions are routed to Model Plane v2 when the client does not
	// explicitly request a version. 0 = all traffic to v1 (default).
	ModelPlaneV2RolloutPct int
}

type AuthConfig struct {
	InternalAPIKey string
	ServiceSecret  string
	NATSSubject    string
}

// ConvexConfig holds connection details for writing reactive state into
// the Application Plane (convex-core) after session mutations.
// Both fields must be set for Convex sync to be active; if either is
// empty, the integration is silently skipped.
type ConvexConfig struct {
	// URL is the convex-backend HTTP actions base URL, e.g.
	// http://convex-backend:3211
	URL string
	// ServiceKey must match CONVEX_INTERNAL_SERVICE_KEY in convex-core.
	ServiceKey string
}

type LoggingConfig struct {
	Level  string
	Format string
}

type OrgCoreConfig struct {
	URL string
}

func Load() *Config {
	_ = godotenv.Load()

	cfg := &Config{
		Server: ServerConfig{
			HTTPPort:    getEnv("SERVER_HTTP_PORT", "3017"),
			GRPCPort:    getEnv("SERVER_GRPC_PORT", "50017"),
			Environment: getEnv("ENVIRONMENT", "development"),
		},
		Database: DatabaseConfig{
			URL:            getEnv("DATABASE_URL", ""),
			Host:           getEnv("DATABASE_HOST", "controlplane-postgres"),
			Port:           getEnv("DATABASE_PORT", "5432"),
			User:           getEnv("DATABASE_USER", "controlplane_user"),
			// No baked-in default: DATABASE_PASSWORD is the single source of truth,
			// injected from the root Control Plane .env via compose ($DB_PASSWORD).
			// A stale literal here drifts from the live hex and breaks DB auth.
			Password:       getEnv("DATABASE_PASSWORD", ""),
			Name:           getEnv("DATABASE_NAME", "session_core"),
			SSLMode:        getEnv("DATABASE_SSL_MODE", "disable"),
			MaxConnections: getEnvAsInt("DATABASE_MAX_CONNS", 20),
			MinConnections: getEnvAsInt("DATABASE_MIN_CONNS", 5),
		},
		Redis: RedisConfig{
			Addr:     getEnv("DRAGONFLY_ADDR", getEnv("CACHE_ADDR", getEnv("REDIS_ADDR", "controlplane-dragonfly:6379"))),
			Password: getEnv("DRAGONFLY_PASSWORD", getEnv("CACHE_PASSWORD", getEnv("REDIS_PASSWORD", ""))),
			DB:       getEnvAsInt("DRAGONFLY_DB", getEnvAsInt("CACHE_DB", getEnvAsInt("REDIS_DB", 1))),
		},
		NATS: NATSConfig{
			LocalURL:               getEnv("NATS_LOCAL_URL", "nats://controlplane-nats:4222"),
			Token:                  getEnv("NATS_TOKEN", ""),
			SharedURL:              getEnv("NATS_SHARED_URL", ""),
			SharedToken:            getEnv("NATS_SHARED_TOKEN", ""),
			ModelPlaneV2RolloutPct: getEnvAsInt("MODEL_PLANE_V2_ROLLOUT_PCT", 0),
		},
		Auth: AuthConfig{
			InternalAPIKey: getEnv("INTERNAL_API_KEY", ""),
			ServiceSecret:  getEnv("INTERNAL_SERVICE_SECRET", ""),
			NATSSubject:    getEnv("AUTH_NATS_SUBJECT", "session.validate"),
		},
		Convex: ConvexConfig{
			URL:        getEnv("CONVEX_URL", ""),
			ServiceKey: getEnv("CONVEX_SERVICE_KEY", ""),
		},
		Logging: LoggingConfig{
			Level:  getEnv("LOGGING_LEVEL", "info"),
			Format: getEnv("LOGGING_FORMAT", "json"),
		},
		OrgCore: OrgCoreConfig{
			URL: getEnv("ORG_CORE_URL", "http://org-core:8080"),
		},
		UserCore: UserCoreConfig{
			URL: getEnv("USER_CORE_URL", "http://user-core:3012"),
		},
		BillingCore: BillingCoreConfig{
			URL: getEnv("BILLING_CORE_URL", "http://billing-core:3014"),
		},
	}

	return cfg
}

func (c *DatabaseConfig) URL_DSN() string {
	if c.URL != "" {
		return c.URL
	}
	return fmt.Sprintf(
		"postgres://%s:%s@%s:%s/%s?sslmode=%s",
		c.User, c.Password, c.Host, c.Port, c.Name, c.SSLMode,
	)
}

func getEnv(key, defaultVal string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return defaultVal
}

func getEnvAsInt(key string, defaultVal int) int {
	if v := os.Getenv(key); v != "" {
		if i, err := strconv.Atoi(v); err == nil {
			return i
		}
	}
	return defaultVal
}

func getEnvAsBool(key string, defaultVal bool) bool {
	if v := os.Getenv(key); v != "" {
		return strings.EqualFold(v, "true") || v == "1"
	}
	return defaultVal
}
