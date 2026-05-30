package platform

import (
	"io"
	"os"
	"strings"
	"time"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
)

// Logger wraps zerolog functionality
type Logger struct {
	logger zerolog.Logger
}

// NewLogger creates a new logger with the given configuration
func NewLogger(cfg LogConfig) *Logger {
	// Set global log level
	level, err := zerolog.ParseLevel(strings.ToLower(cfg.Level))
	if err != nil {
		level = zerolog.InfoLevel
	}
	zerolog.SetGlobalLevel(level)

	// Configure output format
	var output io.Writer = os.Stdout

	if cfg.Format == "console" {
		output = zerolog.ConsoleWriter{
			Out:        os.Stdout,
			TimeFormat: time.RFC3339,
		}
	}

	logger := zerolog.New(output).
		With().
		Timestamp().
		Caller().
		Logger()

	return &Logger{logger: logger}
}

// Info logs an info message
func (l *Logger) Info(msg string) *zerolog.Event {
	return l.logger.Info().Str("component", "quarry")
}

// Error logs an error message
func (l *Logger) Error(msg string) *zerolog.Event {
	return l.logger.Error().Str("component", "quarry")
}

// Debug logs a debug message
func (l *Logger) Debug(msg string) *zerolog.Event {
	return l.logger.Debug().Str("component", "quarry")
}

// Warn logs a warning message
func (l *Logger) Warn(msg string) *zerolog.Event {
	return l.logger.Warn().Str("component", "quarry")
}

// With returns a new logger with additional context
func (l *Logger) With() zerolog.Context {
	return l.logger.With()
}

// GetZerolog returns the underlying zerolog logger for advanced usage
func (l *Logger) GetZerolog() zerolog.Logger {
	return l.logger
}

// SetGlobalLogger sets the logger as the global logger
func (l *Logger) SetGlobalLogger() {
	log.Logger = l.logger
}
