package secret

import (
	"bytes"
	"encoding"
	"encoding/json"
	"fmt"
	"log/slog"
	"strings"
	"testing"
)

const raw = "super-secret-value-0123456789"

func TestValueNeverRendersTheRawSecret(t *testing.T) {
	t.Parallel()

	v := Value(raw)
	type holder struct {
		Key  Value
		Name string
	}
	h := holder{Key: v, Name: "doffin"}

	tests := []struct {
		name string
		got  string
	}{
		{"%s", fmt.Sprintf("%s", v)},
		{"%v", fmt.Sprintf("%v", v)},
		{"%+v", fmt.Sprintf("%+v", v)},
		{"%#v", fmt.Sprintf("%#v", v)},
		{"%q", fmt.Sprintf("%q", v)},
		{"%x", fmt.Sprintf("%x", v)},
		{"struct %v", fmt.Sprintf("%v", h)},
		{"struct %+v", fmt.Sprintf("%+v", h)},
		{"struct %#v", fmt.Sprintf("%#v", h)},
		{"pointer-to-struct %+v", fmt.Sprintf("%+v", &h)},
		{"%d", fmt.Sprintf("%d", v)},
		{"%o", fmt.Sprintf("%o", v)},
		{"%t", fmt.Sprintf("%t", v)},
		{"%5.3s", fmt.Sprintf("%5.3s", v)},
		{"%-40v", fmt.Sprintf("%-40v", v)},
		{"%+q", fmt.Sprintf("%+q", v)},
		{"Sprint", fmt.Sprint(v)},
		{"Error wrap", fmt.Errorf("boom: %v", v).Error()},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			if strings.Contains(tc.got, raw) {
				t.Fatalf("%s leaked the secret: %q", tc.name, tc.got)
			}
		})
	}
}

func TestValueMarshalsRedacted(t *testing.T) {
	t.Parallel()

	type payload struct {
		APIKey Value  `json:"api_key"`
		Name   string `json:"name"`
	}
	body, err := json.Marshal(payload{APIKey: Value(raw), Name: "doffin"})
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}
	if bytes.Contains(body, []byte(raw)) {
		t.Fatalf("JSON leaked the secret: %s", body)
	}
	if !bytes.Contains(body, []byte(redacted)) {
		t.Fatalf("JSON should carry the redaction marker, got %s", body)
	}

	var tm encoding.TextMarshaler = Value(raw)
	text, err := tm.MarshalText()
	if err != nil {
		t.Fatalf("MarshalText: %v", err)
	}
	if strings.Contains(string(text), raw) {
		t.Fatalf("MarshalText leaked the secret: %q", text)
	}
}

func TestValueIsRedactedInStructuredLogs(t *testing.T) {
	t.Parallel()

	for name, handler := range map[string]func(*bytes.Buffer) slog.Handler{
		"text": func(b *bytes.Buffer) slog.Handler { return slog.NewTextHandler(b, nil) },
		"json": func(b *bytes.Buffer) slog.Handler { return slog.NewJSONHandler(b, nil) },
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			var buf bytes.Buffer
			slog.New(handler(&buf)).Info("config loaded", "doffin_key", Value(raw))
			if strings.Contains(buf.String(), raw) {
				t.Fatalf("slog %s handler leaked the secret: %s", name, buf.String())
			}
		})
	}
}

func TestRevealAndIsSet(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		value   Value
		wantSet bool
	}{
		{"set", Value(raw), true},
		{"empty", Value(""), false},
		{"blank", Value("  \t\n"), false},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			if got := tc.value.IsSet(); got != tc.wantSet {
				t.Fatalf("IsSet() = %v, want %v", got, tc.wantSet)
			}
			if got := tc.value.Reveal(); got != string(tc.value) {
				t.Fatalf("Reveal() = %q, want %q", got, string(tc.value))
			}
		})
	}
}

func TestUnsetValueIsDistinguishableFromSet(t *testing.T) {
	t.Parallel()

	if got := Value("").String(); got == Value(raw).String() {
		t.Fatalf("an unset value must not look like a set one, both rendered %q", got)
	}
}
