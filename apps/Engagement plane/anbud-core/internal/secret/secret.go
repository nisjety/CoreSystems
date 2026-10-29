// Package secret provides a string type for credentials that must never be
// rendered by accident. A Value redacts itself in every fmt verb, in JSON and
// text encoding, and in structured logs; the raw credential is reachable only
// through Reveal, so each place a secret leaves the process is greppable.
package secret

import (
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"strings"
)

const (
	redacted = "[redacted]"
	unset    = "<unset>"
)

// Value is a credential. The zero value means "not configured".
type Value string

// IsSet reports whether a non-blank credential is present.
func (v Value) IsSet() bool { return strings.TrimSpace(string(v)) != "" }

// Reveal returns the raw credential. Call it only where the value is sent to
// the system it authenticates against.
func (v Value) Reveal() string { return string(v) }

// String implements fmt.Stringer; it never returns the credential.
func (v Value) String() string {
	if !v.IsSet() {
		return unset
	}
	return redacted
}

// GoString implements fmt.GoStringer so %#v cannot leak the credential.
func (v Value) GoString() string { return v.String() }

// Format implements fmt.Formatter, so every verb (%s %v %d %x %q ...) renders
// the redacted form. Without it a verb that does not apply to a string, such as
// %d, falls back to fmt's bad-verb output, which includes the raw value.
//
// fmt cannot call methods on UNEXPORTED struct fields, so a Value held in one
// is still printed raw when the enclosing struct is printed by value. Keep a
// Value in an exported field, or behind a pointer, if its struct may be printed.
func (v Value) Format(f fmt.State, _ rune) { _, _ = io.WriteString(f, v.String()) }

// MarshalText implements encoding.TextMarshaler.
func (v Value) MarshalText() ([]byte, error) { return []byte(v.String()), nil }

// MarshalJSON implements json.Marshaler.
func (v Value) MarshalJSON() ([]byte, error) { return json.Marshal(v.String()) }

// LogValue implements slog.LogValuer.
func (v Value) LogValue() slog.Value { return slog.StringValue(v.String()) }
