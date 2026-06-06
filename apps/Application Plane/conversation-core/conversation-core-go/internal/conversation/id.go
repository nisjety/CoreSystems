package conversation

import (
	"crypto/rand"
	"crypto/sha1"
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

func stableInboxID(orgID, channel string) string {
	hash := sha1.Sum([]byte(strings.TrimSpace(orgID) + ":" + strings.TrimSpace(channel)))
	return "inbox_" + hex.EncodeToString(hash[:])[:16]
}

func stableContactID(orgID, email string) string {
	hash := sha1.Sum([]byte(strings.TrimSpace(orgID) + ":" + strings.ToLower(strings.TrimSpace(email))))
	return "contact_" + hex.EncodeToString(hash[:])[:16]
}
