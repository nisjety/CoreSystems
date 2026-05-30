package config

import (
	"fmt"
	"os"
	"strings"

	"github.com/joho/godotenv"
)

type Config struct {
	Server   ServerConfig
	Database DatabaseConfig
	Redis    RedisConfig
	Auth     AuthConfig
	Session  SessionConfig
	Logging  LoggingConfig
	Security SecurityConfig
	CORS     CORSConfig
	NATS     NATSConfig
}

type ServerConfig struct {
	GRPCPort    int
	HTTPPort    int
	Environment string
}

type DatabaseConfig struct {
	URL            string
	Host           string
	Port           int
	User           string
	Password       string
	Name           string
	SSLMode        string
	MaxConnections int
	MinConnections int
}

type RedisConfig struct {
	Host     string
	Port     string
	Password string
	DB       int
	Enabled  bool
}

type NATSConfig struct {
	URL         string
	Token       string
	SharedURL   string
	SharedToken string
	ClusterID   string
	ClientID    string
}

type AuthConfig struct {
	ServiceURL string
}

type SessionConfig struct {
	ExpireSeconds int
	MaxPerUser    int
}

type LoggingConfig struct {
	Level  string
	Format string
}

type SecurityConfig struct {
	BCryptCost int
	JWTSecret  string
}

type CORSConfig struct {
	AllowedOrigins []string
	AllowedMethods []string
	AllowedHeaders []string
}

// Load loads configuration from environment variables and .env file
func Load() (*Config, error) {
	// Load .env file if it exists (ignore error if file doesn't exist)
	_ = godotenv.Load()

	config := &Config{
		Server: ServerConfig{
			GRPCPort:    getEnvAsInt("GRPC_PORT", 50012),
			HTTPPort:    getEnvAsInt("HTTP_PORT", 3012),
			Environment: getEnv("ENVIRONMENT", "development"),
		},
		Database: DatabaseConfig{
			URL:            getEnv("DATABASE_URL", ""),
			Host:           getEnv("DB_HOST", "localhost"),
			Port:           getEnvAsInt("DB_PORT", 5432),
			User:           getEnv("DB_USER", "postgres"),
			Password:       getEnv("DB_PASSWORD", ""),
			Name:           getEnv("DB_NAME", "aquatiq_users"),
			SSLMode:        getEnv("DB_SSL_MODE", "disable"),
			MaxConnections: getEnvAsInt("DB_MAX_OPEN_CONNS", 25),
			MinConnections: getEnvAsInt("DB_MAX_IDLE_CONNS", 5),
		},
		Redis: RedisConfig{
			Host:     getEnv("REDIS_HOST", "localhost"),
			Port:     getEnv("REDIS_PORT", "6379"),
			Password: getEnv("REDIS_PASSWORD", ""),
			DB:       getEnvAsInt("REDIS_DB", 0),
			Enabled:  getEnvAsBool("REDIS_ENABLED", false),
		},
		NATS: NATSConfig{
			URL:         getEnv("NATS_URL", "nats://localhost:4222"),
			Token:       getEnv("NATS_TOKEN", ""),
			SharedURL:   getEnv("VELION_NATS_URL", getEnv("NATS_SHARED_URL", "")),
			SharedToken: getEnv("VELION_NATS_TOKEN", getEnv("NATS_SHARED_TOKEN", "")),
			ClusterID:   getEnv("NATS_CLUSTER_ID", "aquatiq-cluster"),
			ClientID:    getEnv("NATS_CLIENT_ID", "user-service"),
		},
		Auth: AuthConfig{
			ServiceURL: getEnv("AUTH_SERVICE_GRPC_URL", "localhost:50011"),
		},
		Session: SessionConfig{
			ExpireSeconds: getEnvAsInt("SESSION_EXPIRE_SECONDS", 86400),
			MaxPerUser:    getEnvAsInt("SESSION_MAX_PER_USER", 10),
		},
		Logging: LoggingConfig{
			Level:  getEnv("LOG_LEVEL", "info"),
			Format: getEnv("LOG_FORMAT", "json"),
		},
		Security: SecurityConfig{
			BCryptCost: getEnvAsInt("BCRYPT_COST", 10),
			JWTSecret:  getEnv("JWT_SECRET", ""),
		},
		CORS: CORSConfig{
			AllowedOrigins: getEnvAsSlice("CORS_ALLOWED_ORIGINS", []string{"http://localhost:3000"}),
			AllowedMethods: getEnvAsSlice("CORS_ALLOWED_METHODS", []string{"GET", "POST", "PUT", "DELETE", "OPTIONS"}),
			AllowedHeaders: getEnvAsSlice("CORS_ALLOWED_HEADERS", []string{"Content-Type", "Authorization"}),
		},
	}

	if err := config.Validate(); err != nil {
		return nil, fmt.Errorf("config validation failed: %w", err)
	}

	return config, nil
}

// Validate validates the configuration
func (c *Config) Validate() error {
	if c.Database.URL == "" && c.Database.Host == "" {
		return fmt.Errorf("database URL or host must be provided")
	}

	if c.Security.JWTSecret == "" {
		return fmt.Errorf("JWT secret must be provided")
	}

	return nil
}

// GetDatabaseDSN returns the database connection string
func (c *DatabaseConfig) GetDatabaseDSN() string {
	if c.URL != "" {
		return c.URL
	}

	return fmt.Sprintf(
		"host=%s port=%d user=%s password=%s dbname=%s sslmode=%s",
		c.Host, c.Port, c.User, c.Password, c.Name, c.SSLMode,
	)
}

// Helper functions
func getEnv(key, defaultValue string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return defaultValue
}

func getEnvAsInt(key string, defaultValue int) int {
	if value := os.Getenv(key); value != "" {
		var intValue int
		if _, err := fmt.Sscanf(value, "%d", &intValue); err == nil {
			return intValue
		}
	}
	return defaultValue
}

func getEnvAsBool(key string, defaultValue bool) bool {
	if value := os.Getenv(key); value != "" {
		return value == "true" || value == "1"
	}
	return defaultValue
}

func getEnvAsSlice(key string, defaultValue []string) []string {
	if value := os.Getenv(key); value != "" {
		parts := strings.Split(value, ",")
		var result []string
		for _, p := range parts {
			if trimmed := strings.TrimSpace(p); trimmed != "" {
				result = append(result, trimmed)
			}
		}
		return result
	}
	return defaultValue
}
