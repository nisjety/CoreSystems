package conversation

import (
	"crypto/rand"
	"crypto/sha1"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

func newID(prefix string) string {
	var bytes [12]byte
	if _, err := rand.Read(bytes[:]); err == nil {
		return prefix + "_" + hex.EncodeToString(bytes[:])
	}
	return fmt.Sprintf("%s_%d", prefix, time.Now().UnixNano())
}

// stableOutboundIntentID lets an exact idempotency-key replay address the same
// durable authorization row without storing message content in the identifier.
func OutboundIntentID(orgID, idempotencyKey string) string {
	digest := sha256.Sum256([]byte(strings.TrimSpace(orgID) + ":" + strings.TrimSpace(idempotencyKey)))
	return "outintent_" + hex.EncodeToString(digest[:12])
}

func stableInboxID(orgID, channel string) string {
	hash := sha1.Sum([]byte(strings.TrimSpace(orgID) + ":" + strings.TrimSpace(channel)))
	return "inbox_" + hex.EncodeToString(hash[:])[:16]
}

func stableContactID(orgID, email string) string {
	hash := sha1.Sum([]byte(strings.TrimSpace(orgID) + ":" + strings.ToLower(strings.TrimSpace(email))))
	return "contact_" + hex.EncodeToString(hash[:])[:16]
}
