package telemetry

import (
	"bytes"
	"context"
	"encoding/json"
	"strings"
	"testing"
)

func TestNewLogsServiceAndEnvironment(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	tel := New(&buf, "anbud-core", "dev", "")
	tel.Logger.Info().Msg("hello")

	var line map[string]any
	if err := json.Unmarshal(buf.Bytes(), &line); err != nil {
		t.Fatalf("log line is not JSON: %q (%v)", buf.String(), err)
	}
	if line["service"] != "anbud-core" || line["env"] != "dev" || line["message"] != "hello" {
		t.Fatalf("log line = %v", line)
	}
}

func TestLevelFollowsTheEnvironment(t *testing.T) {
	t.Parallel()

	tests := []struct {
		environment string
		wantDebug   bool
	}{
		{"dev", true},
		{"local", true},
		{"DEV", true},
		{"production", false},
		{"staging", false},
	}
	for _, tc := range tests {
		t.Run(tc.environment, func(t *testing.T) {
			t.Parallel()
			var buf bytes.Buffer
			tel := New(&buf, "anbud-core", tc.environment, "")
			tel.Logger.Debug().Msg("detail")
			if got := strings.Contains(buf.String(), "detail"); got != tc.wantDebug {
				t.Fatalf("debug emitted = %v, want %v", got, tc.wantDebug)
			}
		})
	}
}

func TestShutdownIsSafeToCallRepeatedlyAndOnNil(t *testing.T) {
	t.Parallel()

	var nilTel *Telemetry
	if err := nilTel.Shutdown(context.Background()); err != nil {
		t.Fatalf("nil Shutdown: %v", err)
	}
	tel := New(&bytes.Buffer{}, "anbud-core", "dev", "")
	for i := range 2 {
		if err := tel.Shutdown(context.Background()); err != nil {
			t.Fatalf("Shutdown #%d: %v", i+1, err)
		}
	}
}
