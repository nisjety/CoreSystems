package platform

import (
	"log/slog"
	"os"
)

// NewLogger returns a JSON structured logger writing to stdout. The field
// shape (time/level/msg plus attrs) is deliberately plain slog defaults so
// it can be matched against the same shape agent-service (pino) emits,
// keeping cross-service log correlation via trace_id straightforward.
func NewLogger() *slog.Logger {
	handler := slog.NewJSONHandler(os.Stdout, &slog.HandlerOptions{
		Level: slog.LevelInfo,
	})
	return slog.New(handler)
}
