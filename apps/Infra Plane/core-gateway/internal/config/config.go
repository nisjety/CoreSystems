package config

import (
	"crypto/tls"
	"fmt"
	"strings"
	"time"

	"github.com/spf13/viper"
)

// Config holds all configuration for the integration gateway
type Config struct {
	Server         ServerConfig
	GRPC           GRPCConfig
	Redis          RedisConfig
	RateLimit      RateLimitConfig
	CircuitBreaker CircuitBreakerConfig
	Docker         DockerConfig
	Auth           AuthConfig
	Integrations   IntegrationsConfig
	Logging        LoggingConfig
	Whitelist      WhitelistConfig
	Database       DatabaseConfig
}

// ServerConfig holds HTTP server configuration
type ServerConfig struct {
	Host            string
	Port            int
	GRPCPort        int // Port for gRPC server (default 50051)
	ReadTimeout     time.Duration
	WriteTimeout    time.Duration
	ShutdownTimeout time.Duration
	APIKey          string
}

// GRPCConfig holds gRPC server configuration
type GRPCConfig struct {
	Host string
	Port int
	TLS  GRPCTLSConfig
}

// GRPCTLSConfig holds gRPC TLS configuration
type GRPCTLSConfig struct {
	Enabled  bool
	CertFile string
	KeyFile  string
	CAFile   string
}

// RedisConfig holds Redis configuration with TLS
type RedisConfig struct {
	Enabled  bool
	Host     string
	Port     int
	Password string
	DB       int
	TLS      TLSConfig
	PoolSize int
	Timeout  time.Duration
}

// TLSConfig holds TLS configuration
type TLSConfig struct {
	Enabled            bool
	InsecureSkipVerify bool
	MinVersion         uint16
}

// RateLimitConfig holds rate limiting configuration
type RateLimitConfig struct {
	GlobalRPS   int
	AdminRPS    int
	BurstSize   int
	Distributed bool
}

// CircuitBreakerConfig holds circuit breaker settings
type CircuitBreakerConfig struct {
	MaxRequests      uint32
	Interval         time.Duration
	Timeout          time.Duration
	FailureThreshold uint32
}

// DockerConfig holds Docker socket proxy configuration
type DockerConfig struct {
	Host    string
	Version string
	Timeout time.Duration
}

// AuthConfig holds authentication configuration
type AuthConfig struct {
	TokenRefreshInterval time.Duration
	TokenEncryptionKey   string
}

// IntegrationsConfig holds external API configurations
type IntegrationsConfig struct {
	SuperOffice SuperOfficeConfig
	Visma       VismaConfig
}

// SuperOfficeConfig holds SuperOffice API configuration
type SuperOfficeConfig struct {
	BaseURL      string
	ClientID     string
	ClientSecret string
	TenantID     string
	Timeout      time.Duration
	RetryMax     int
}

// VismaConfig holds Visma.net API configuration
type VismaConfig struct {
	BaseURL      string
	ClientID     string
	ClientSecret string
	CompanyID    string
	Timeout      time.Duration
	RetryMax     int
}

// LoggingConfig holds logging configuration
type LoggingConfig struct {
	Level      string
	Format     string
	OutputPath string
}

// WhitelistConfig holds IP whitelist configuration
type WhitelistConfig struct {
	TraefikConfigPath string
}

// DatabaseConfig holds database connection configuration
type DatabaseConfig struct {
	PostgresURL string
}

// Load loads configuration from environment variables and config files
func Load() (*Config, error) {
	viper.SetConfigName("config")
	viper.SetConfigType("yaml")
	viper.AddConfigPath("/app/configs")
	viper.AddConfigPath("./configs")
	viper.AddConfigPath(".")

	// Set defaults
	setDefaults()

	// Auto-bind environment variables with underscore replacement
	viper.SetEnvKeyReplacer(strings.NewReplacer(".", "_"))
	viper.AutomaticEnv()

	// Read config file (optional)
	if err := viper.ReadInConfig(); err != nil {
		if _, ok := err.(viper.ConfigFileNotFoundError); !ok {
			return nil, fmt.Errorf("failed to read config: %w", err)
		}
		// Config file not found; use defaults and env vars
	}

	var config Config
	if err := viper.Unmarshal(&config); err != nil {
		return nil, fmt.Errorf("failed to unmarshal config: %w", err)
	}

	// Validate configuration
	if err := validate(&config); err != nil {
		return nil, fmt.Errorf("invalid configuration: %w", err)
	}

	return &config, nil
}

// setDefaults sets default configuration values
func setDefaults() {
	// Server defaults
	viper.SetDefault("server.host", "0.0.0.0")
	viper.SetDefault("server.port", 5001)
	viper.SetDefault("server.grpcport", 50051)
	viper.SetDefault("server.readtimeout", "30s")
	viper.SetDefault("server.writetimeout", "30s")
	viper.SetDefault("server.shutdowntimeout", "30s")
	viper.SetDefault("server.apikey", "dev-api-key-change-in-production")

	// gRPC defaults
	viper.SetDefault("grpc.host", "0.0.0.0")
	viper.SetDefault("grpc.port", 50051)
	viper.SetDefault("grpc.tls.enabled", false)
	viper.SetDefault("grpc.tls.certfile", "/certs/server-cert.pem")
	viper.SetDefault("grpc.tls.keyfile", "/certs/server-key.pem")
	viper.SetDefault("grpc.tls.cafile", "/certs/ca-cert.pem")

	// Redis defaults
	viper.SetDefault("redis.enabled", false) // Disabled by default for local development
	viper.SetDefault("redis.host", "coresystem-redis")
	viper.SetDefault("redis.port", 6379)
	viper.SetDefault("redis.db", 0)
	viper.SetDefault("redis.poolsize", 10)
	viper.SetDefault("redis.timeout", "10s")
	viper.SetDefault("redis.tls.enabled", true)
	viper.SetDefault("redis.tls.minversion", tls.VersionTLS12)

	// Rate limiting defaults
	viper.SetDefault("ratelimit.globalrps", 100)
	viper.SetDefault("ratelimit.adminrps", 20)
	viper.SetDefault("ratelimit.burstsize", 50)
	viper.SetDefault("ratelimit.distributed", true)

	// Circuit breaker defaults
	viper.SetDefault("circuitbreaker.maxrequests", 100)
	viper.SetDefault("circuitbreaker.interval", "10s")
	viper.SetDefault("circuitbreaker.timeout", "30s")
	viper.SetDefault("circuitbreaker.failurethreshold", 5)

	// Docker defaults
	viper.SetDefault("docker.host", "tcp://docker-socket-proxy:2375")
	viper.SetDefault("docker.version", "1.41")
	viper.SetDefault("docker.timeout", "30s")

	// Auth defaults
	viper.SetDefault("auth.tokenrefreshinterval", "30m")

	// Whitelist defaults
	viper.SetDefault("whitelist.traefikconfigpath", "/app/configs/traefik-dynamic.yml")

	// Database defaults
	viper.SetDefault("database.postgresurl", "postgres://coresystem:password@postgres:5432/coresystem?sslmode=disable")

	// Logging defaults
	viper.SetDefault("logging.level", "info")
	viper.SetDefault("logging.format", "json")
	viper.SetDefault("logging.outputpath", "stdout")
}

// validate validates the configuration
func validate(cfg *Config) error {
	if cfg.Server.Port < 1 || cfg.Server.Port > 65535 {
		return fmt.Errorf("invalid server port: %d", cfg.Server.Port)
	}

	if cfg.Server.APIKey == "" {
		return fmt.Errorf("server.apikey is required")
	}

	if cfg.Redis.Host == "" {
		return fmt.Errorf("redis.host is required")
	}

	if cfg.RateLimit.GlobalRPS < 1 {
		return fmt.Errorf("ratelimit.globalrps must be positive")
	}

	if cfg.Docker.Host == "" {
		return fmt.Errorf("docker.host is required")
	}

	return nil
}

// GetRedisAddr returns the Redis address
func (c *RedisConfig) GetRedisAddr() string {
	return fmt.Sprintf("%s:%d", c.Host, c.Port)
}

// GetServerAddr returns the server address
func (c *ServerConfig) GetServerAddr() string {
	return fmt.Sprintf("%s:%d", c.Host, c.Port)
}
