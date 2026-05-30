package platform

import (
	"fmt"
	"time"

	"github.com/kelseyhightower/envconfig"
)

// Config represents the application configuration
type Config struct {
	// Server configuration
	Server ServerConfig `envconfig:"SERVER"`

	// AI configuration
	AI AIConfig `envconfig:"AI"`

	// Logging configuration
	Log LogConfig `envconfig:"LOG"`

	// Health check configuration
	Health HealthConfig `envconfig:"HEALTH"`
}

// ServerConfig contains HTTP server settings
type ServerConfig struct {
	Host         string        `envconfig:"HOST" default:"0.0.0.0"`
	Port         int           `envconfig:"PORT" default:"8080"`
	ReadTimeout  time.Duration `envconfig:"READ_TIMEOUT" default:"30s"`
	WriteTimeout time.Duration `envconfig:"WRITE_TIMEOUT" default:"30s"`
	IdleTimeout  time.Duration `envconfig:"IDLE_TIMEOUT" default:"120s"`
}

// AIConfig contains AI and OpenAI settings
type AIConfig struct {
	OpenAIAPIKey   string        `envconfig:"OPENAI_API_KEY"`
	Model          string        `envconfig:"MODEL" default:"gpt-4o-mini"`
	PlanTimeout    time.Duration `envconfig:"PLAN_TIMEOUT" default:"30s"`
	MaxSteps       int           `envconfig:"MAX_STEPS" default:"10"`
	AllowedModules []string      `envconfig:"ALLOWED_MODULES" default:"single,multi,seo,map,interaction,search"`
	Temperature    float32       `envconfig:"TEMPERATURE" default:"0.1"`
	MaxTokens      int           `envconfig:"MAX_TOKENS" default:"4096"`
}

// LogConfig contains logging settings
type LogConfig struct {
	Level  string `envconfig:"LEVEL" default:"info"`
	Format string `envconfig:"FORMAT" default:"json"`
}

// HealthConfig contains health check settings
type HealthConfig struct {
	Enabled  bool          `envconfig:"ENABLED" default:"true"`
	Endpoint string        `envconfig:"ENDPOINT" default:"/healthz"`
	Timeout  time.Duration `envconfig:"TIMEOUT" default:"5s"`
}

// LoadConfig loads configuration from environment variables
func LoadConfig() (*Config, error) {
	var cfg Config

	err := envconfig.Process("QUARRY", &cfg)
	if err != nil {
		return nil, fmt.Errorf("failed to load config: %w", err)
	}

	return &cfg, nil
}

// Validate validates the configuration
func (c *Config) Validate() error {
	if c.AI.MaxSteps <= 0 {
		return fmt.Errorf("AI max steps must be positive")
	}

	if c.Server.Port <= 0 || c.Server.Port > 65535 {
		return fmt.Errorf("server port must be between 1 and 65535")
	}

	return nil
}
