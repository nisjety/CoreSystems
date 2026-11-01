package server

import (
	"context"
	"log/slog"
	"strings"

	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
)

// The broker writes one line per grant issued and one per grant revoked, so an
// operator can follow a grant across a restart (WS1.A1 and WS1.C evidence). The
// lines carry the kind, the ZDR flag and COUNTS only: never a host, a scope URL
// or an organization. A ZDR grant's id is left out as well, so the log holds
// nothing that names a Zero Data Retention session. ValidateGrant is not logged
// here: it runs once per browser action.

// zdrIDPrefix marks a memory-only ZDR grant id (grant.newID).
const zdrIDPrefix = "z_"

func logGrantIssued(ctx context.Context, g *grant.Grant) {
	attrs := []any{
		"method", "AcquireGrant",
		"kind", string(g.Kind),
		"zdr", g.Zdr,
		"domain_count", len(g.AllowedDomains),
	}
	if !g.Zdr {
		attrs = append(attrs, "grant_id", g.ID)
		// INNH-10: the API key id (an id, never the key). Left out under ZDR
		// with the grant id, so nothing names a ZDR session.
		if g.KeyID != "" {
			attrs = append(attrs, "key_id", g.KeyID)
		}
	}
	slog.InfoContext(ctx, "browser grant issued", attrs...)
}

func logGrantRevoked(ctx context.Context, grantID string) {
	zdr := strings.HasPrefix(grantID, zdrIDPrefix)
	attrs := []any{"method", "RevokeGrant", "zdr", zdr}
	if !zdr {
		attrs = append(attrs, "grant_id", grantID)
	}
	slog.InfoContext(ctx, "browser grant revoked", attrs...)
}
