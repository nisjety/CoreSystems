package quarryotel

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"reflect"
	"testing"

	"go.opentelemetry.io/otel"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
)

// goldenPath is the repository golden; testdata/ holds the byte-identical copy this package tests against.
const goldenPath = "../../crates/quarry-runtime/contracts/telemetry-field-contract.json"

func TestExportOffIsNoop(t *testing.T) {
	t.Setenv(ExportEnv, "off")
	t.Setenv("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:4317")

	shutdown, err := Init(context.Background(), "quarry-test", "0.0.0")
	if err != nil {
		t.Fatalf("Init with export off must not fail: %v", err)
	}
	if err := shutdown(context.Background()); err != nil {
		t.Fatalf("the no-op shutdown returns nil: %v", err)
	}
	if _, ok := otel.GetTracerProvider().(*sdktrace.TracerProvider); ok {
		t.Fatal("export off builds no SDK tracer provider and no exporter")
	}

	for _, tc := range []struct {
		value string
		want  bool
	}{
		{"", true},
		{"on", true},
		{"off", false},
		{" FALSE ", false},
		{"0", false},
	} {
		t.Setenv(ExportEnv, tc.value)
		if got := ExportEnabled(); got != tc.want {
			t.Fatalf("ExportEnabled with %q = %v, want %v", tc.value, got, tc.want)
		}
	}
}

func TestFieldConstantsEqualGolden(t *testing.T) {
	copied, err := os.ReadFile("testdata/telemetry-field-contract.json")
	if err != nil {
		t.Fatalf("the vendored golden copy is missing: %v", err)
	}
	canonical, err := os.ReadFile(goldenPath)
	if err != nil {
		t.Fatalf("the golden is missing (a check that cannot run is a failure): %v", err)
	}
	if !bytes.Equal(copied, canonical) {
		t.Fatal("testdata copy differs from the golden; re-vendor it")
	}

	var golden struct {
		Version      int      `json:"version"`
		Safe         []string `json:"safe"`
		ZdrSensitive []string `json:"zdr_sensitive"`
		Never        []string `json:"never"`
	}
	if err := json.Unmarshal(copied, &golden); err != nil {
		t.Fatalf("golden parses: %v", err)
	}
	if golden.Version != 1 {
		t.Fatalf("golden version = %d, want 1", golden.Version)
	}
	if !reflect.DeepEqual(golden.Safe, SafeFields) {
		t.Fatal("SafeFields differs from the golden safe list")
	}
	if !reflect.DeepEqual(golden.ZdrSensitive, ZdrSensitiveFields) {
		t.Fatal("ZdrSensitiveFields differs from the golden zdr_sensitive list")
	}
	if !reflect.DeepEqual(golden.Never, NeverFields) {
		t.Fatal("NeverFields differs from the golden never list")
	}
}
