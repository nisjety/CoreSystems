package http

import (
	"context"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
)

// memberAuthorizer is the seam the membership guard depends on. *org.Service
// satisfies it via IsActiveMember. Keeping the dependency narrow (one method)
// lets the guard be unit-tested with a fake — no database required.
type memberAuthorizer interface {
	IsActiveMember(ctx context.Context, orgID, userID string) (bool, error)
}

// membershipGuardTimeout bounds the active-membership lookup so a slow/unhealthy
// DB cannot hang the request indefinitely on the auth path.
const membershipGuardTimeout = 5 * time.Second

// requireActiveMembership is the tenant-isolation SECOND LAYER.
//
// org-core sits behind the internal-API-key gate (server.go), which proves the
// CALLER is the trusted gateway but says nothing about whether the path-supplied
// org (`:id`) actually belongs to the acting user. Every org-scoped MUTATION
// therefore re-verifies, against org-core's own membership table, that the
// request's `x-user-id` is an active member of `:id` before the handler runs.
// This means org-core no longer blindly trusts the path even when the internal
// key is presented — a defense-in-depth backstop if any future caller reaches
// org-core directly with the key.
//
// Reads stay gateway-trusted (this guard is only wired onto mutating routes) to
// avoid adding a membership round-trip to every GET; the gateway remains the
// authority for read authorization. The guard fails CLOSED: missing user id,
// non-membership, and lookup errors all deny.
func requireActiveMembership(authz memberAuthorizer) gin.HandlerFunc {
	return func(c *gin.Context) {
		orgID := strings.TrimSpace(c.Param("id"))
		if orgID == "" {
			// A mutation route without an :id in the path is a wiring mistake,
			// not a client error — but fail closed regardless.
			c.AbortWithStatusJSON(http.StatusBadRequest, gin.H{"error": "organization id required"})
			return
		}

		userID := strings.TrimSpace(c.GetHeader("x-user-id"))
		if userID == "" {
			c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "forbidden: caller identity required"})
			return
		}

		ctx, cancel := context.WithTimeout(c.Request.Context(), membershipGuardTimeout)
		defer cancel()

		ok, err := authz.IsActiveMember(ctx, orgID, userID)
		if err != nil {
			// Treat a lookup failure as a denial, not a 500: we cannot prove
			// membership, so we must not let the mutation through. Log so a
			// surge of 403s from an unhealthy DB is distinguishable from
			// genuine non-membership (the client never sees the cause).
			log.Printf("membership guard: lookup failed for org=%s user=%s: %v", orgID, userID, err)
			c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "forbidden: membership could not be verified"})
			return
		}
		if !ok {
			c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "forbidden: not a member of this organization"})
			return
		}

		c.Next()
	}
}
