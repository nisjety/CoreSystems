// Package auth authenticates service-to-service calls with a shared API key
// and carries the organization and user context the gateway forwards.
//
// The organization and user headers are only honoured after the key has been
// verified, so an unauthenticated caller can never assert a tenant.
package auth

import (
	"bytes"
	"crypto/subtle"
	"regexp"
	"strings"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/anbud/internal/secret"
)

const (
	// OrganizationIDHeader carries the tenant the gateway authenticated.
	OrganizationIDHeader = "X-Org-ID"
	// UserIDHeader carries the user the gateway authenticated. It says who did something; it never
	// decides what anyone may do.
	UserIDHeader = "X-User-ID"

	defaultKeyHeader    = "X-API-Key"
	internalKeyHeader   = "X-Internal-Api-Key"
	organizationIDLocal = "org_id"
	userIDLocal         = "user_id"
)

// identifierPattern is the shape of an organization or user id: it starts with a letter or digit and
// has at most 128 characters of letters, digits and _ . : -.
var identifierPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$`)

// Config configures Middleware.
type Config struct {
	APIKey       secret.Value
	APIKeyHeader string
}

// wanted is the configured key, trimmed.
func (cfg Config) wanted() []byte { return []byte(strings.TrimSpace(cfg.APIKey.Reveal())) }

// presented is the key a request carries: on the configured header, else on X-Internal-Api-Key.
// get reads a request header by name.
func (cfg Config) presented(get func(name string) string) string {
	header := strings.TrimSpace(cfg.APIKeyHeader)
	if header == "" {
		header = defaultKeyHeader
	}
	if provided := strings.TrimSpace(get(header)); provided != "" {
		return provided
	}
	return strings.TrimSpace(get(internalKeyHeader))
}

// Authenticated reports whether the headers carry the configured key: the check Middleware makes,
// for a caller that has to decide before there is a request to run it on. The HTTP server's header
// hook is one, which decides how large a body to accept. With no key configured it is false.
func (cfg Config) Authenticated(get func(name string) string) bool {
	want, provided := cfg.wanted(), cfg.presented(get)
	return len(want) > 0 && provided != "" && subtle.ConstantTimeCompare([]byte(provided), want) == 1
}

// Middleware rejects any request that does not carry the configured API key.
// With no key configured it fails closed. It records the organization and user
// context when it is supplied but does not require it; see RequireOrganization.
func Middleware(cfg Config) fiber.Handler {
	want := cfg.wanted()

	return func(c *fiber.Ctx) error {
		provided := cfg.presented(func(name string) string { return c.Get(name) })

		if len(want) == 0 || provided == "" {
			return writeError(c, fiber.StatusUnauthorized, "unauthorized", "authentication required")
		}
		if subtle.ConstantTimeCompare([]byte(provided), want) != 1 {
			return writeError(c, fiber.StatusUnauthorized, "unauthorized", "invalid API key")
		}

		// The tenant is the gateway's to say, and once, in the head of the request: a header sent twice is read
		// by the first line, which a gateway that adds its own line to the client's does not control, and a
		// trailer after a chunked body is merged into the same list as the head. A request that names a tenant
		// twice, or in a trailer, is refused, whatever the lines say.
		if named, once := headerOnce(c, OrganizationIDHeader); named && !once {
			return writeError(c, fiber.StatusBadRequest, "invalid_organization", "X-Org-ID must be sent once, in the head of the request")
		}
		if org := strings.TrimSpace(c.Get(OrganizationIDHeader)); org != "" {
			if !identifierPattern.MatchString(org) {
				return writeError(c, fiber.StatusBadRequest, "invalid_organization", "X-Org-ID is not a valid organization id")
			}
			// c.Get is backed by a buffer Fiber recycles after the request; clone
			// it so a goroutine that outlives the handler never reads reused bytes.
			c.Locals(organizationIDLocal, strings.Clone(org))
		}
		// The user says who acted, and nothing hangs on it, so a value that is not an identifier is
		// left out instead of refusing the request: one gateway sending another shape of id must not
		// take every route down.
		// A user named twice, or in a trailer, is not named: which line is the gateway's cannot be told.
		if _, once := headerOnce(c, UserIDHeader); once {
			if user := strings.TrimSpace(c.Get(UserIDHeader)); identifierPattern.MatchString(user) {
				c.Locals(userIDLocal, strings.Clone(user))
			}
		}
		return c.Next()
	}
}

// headerOnce says whether the request names a header, and whether it does so once, in its head. The server reads
// a header by its first line, and puts the lines of a trailer (what follows a chunked body) in the same list as
// those of the head, so a header that is in the list more often than it is in the head was named in a trailer, and
// one that is in the head more than once was named twice.
func headerOnce(c *fiber.Ctx, name string) (named, once bool) {
	merged := len(c.Request().Header.PeekAll(name))
	head := linesInHead(c.Request().Header.RawHeaders(), name)
	return merged > 0 || head > 0, merged == head && head <= 1
}

// linesInHead counts the lines of the head, as the server kept them, that are the header name. A line that begins
// with a blank continues the one before and is not a header of its own, and an empty line ends the head.
func linesInHead(raw []byte, name string) int {
	n := 0
	for len(raw) > 0 {
		var line []byte
		if i := bytes.IndexByte(raw, '\n'); i >= 0 {
			line, raw = raw[:i], raw[i+1:]
		} else {
			line, raw = raw, nil
		}
		line = bytes.TrimRight(line, "\r")
		if len(line) == 0 {
			break
		}
		if line[0] == ' ' || line[0] == '\t' {
			continue
		}
		if i := bytes.IndexByte(line, ':'); i > 0 && strings.EqualFold(strings.TrimSpace(string(line[:i])), name) {
			n++
		}
	}
	return n
}

// RequireOrganization rejects requests that carry no organization context.
// Mount it after Middleware on tenant-scoped routes.
func RequireOrganization() fiber.Handler {
	return func(c *fiber.Ctx) error {
		if OrganizationID(c) == "" {
			return writeError(c, fiber.StatusBadRequest, "organization_required", "organization context is required")
		}
		return c.Next()
	}
}

// OrganizationID returns the organization recorded by Middleware, or "".
func OrganizationID(c *fiber.Ctx) string {
	org, _ := c.Locals(organizationIDLocal).(string)
	return strings.TrimSpace(org)
}

// UserID returns the user recorded by Middleware, or "".
func UserID(c *fiber.Ctx) string {
	user, _ := c.Locals(userIDLocal).(string)
	return strings.TrimSpace(user)
}

func writeError(c *fiber.Ctx, status int, code, message string) error {
	return c.Status(status).JSON(fiber.Map{
		"error": fiber.Map{"code": code, "message": message},
	})
}
