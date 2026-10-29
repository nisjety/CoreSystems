package http

import (
	"crypto/rand"
	"encoding/hex"
	"strings"

	"github.com/gin-gonic/gin"
)

// correlationMiddleware honours an inbound X-Correlation-Id, mints one if
// absent, exposes it as `correlation_id` on the gin context, and echoes it
// back in the response header. G15 in verevon-gap.md.
func correlationMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		cid := strings.TrimSpace(c.GetHeader("X-Correlation-Id"))
		if cid == "" || len(cid) > 128 {
			cid = newCorrelationID()
		}
		c.Set("correlation_id", cid)
		c.Header("X-Correlation-Id", cid)
		c.Next()
	}
}

// newCorrelationID returns a UUID-v4 string without adding a uuid dep.
func newCorrelationID() string {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "00000000-0000-0000-0000-000000000000"
	}
	// RFC 4122 v4 layout
	b[6] = (b[6] & 0x0f) | 0x40
	b[8] = (b[8] & 0x3f) | 0x80
	return hex.EncodeToString(b[0:4]) + "-" +
		hex.EncodeToString(b[4:6]) + "-" +
		hex.EncodeToString(b[6:8]) + "-" +
		hex.EncodeToString(b[8:10]) + "-" +
		hex.EncodeToString(b[10:16])
}
