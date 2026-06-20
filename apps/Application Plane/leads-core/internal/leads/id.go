package leads

import (
	"crypto/rand"
	"encoding/hex"
)

// newID returns a prefixed, random, URL-safe id (e.g. "list_a1b2c3...").
func newID(prefix string) string {
	var b [12]byte
	_, _ = rand.Read(b[:])
	return prefix + "_" + hex.EncodeToString(b[:])
}
