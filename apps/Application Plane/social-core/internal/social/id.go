package social

import (
	"crypto/rand"
	"encoding/hex"
	"strings"
)

func newID(prefix string) string {
	var buf [12]byte
	if _, err := rand.Read(buf[:]); err != nil {
		return strings.TrimSpace(prefix) + "_fallback"
	}
	return strings.TrimSpace(prefix) + "_" + hex.EncodeToString(buf[:])
}
