package api

import (
	"context"
	"encoding/json"
	"errors"
	"strings"

	"github.com/gofiber/fiber/v2"
)

var errLegacyConnectionRequired = errors.New("connectionId or organizationId is required")

func registerLegacyIntegrationRoutes(app *fiber.App, internalAuth fiber.Handler, cfg ServerConfig) {
	registerLegacyGet(app, internalAuth, cfg, "microsoft-graph", "/integrations/ms-graph/me", "profile", nil)
	registerLegacyGet(app, internalAuth, cfg, "microsoft-graph", "/integrations/ms-graph/calendar/events", "calendar.events", nil)
	registerLegacyGet(app, internalAuth, cfg, "microsoft-graph", "/integrations/ms-graph/mail/messages", "mail.messages", nil)
	registerLegacyPost(app, internalAuth, cfg, "microsoft-graph", "/integrations/ms-graph/mail/send", "mail.send", nil)

	registerLegacyGet(app, internalAuth, cfg, "microsoft-graph", "/integrations/microsoft/graph/user/profile", "profile", nil)
	registerLegacyGet(app, internalAuth, cfg, "microsoft-graph", "/integrations/microsoft/graph/user/calendar", "calendar.events", nil)
	registerLegacyGet(app, internalAuth, cfg, "microsoft-graph", "/integrations/microsoft/graph/user/mail", "mail.messages", nil)
	registerLegacyPost(app, internalAuth, cfg, "microsoft-graph", "/integrations/microsoft/graph/mail/send", "mail.send", nil)

	registerLegacyGet(app, internalAuth, cfg, "google-workspace", "/integrations/google/profile", "profile", nil)
	registerLegacyGet(app, internalAuth, cfg, "google-workspace", "/integrations/google/gmail/messages", "gmail.messages", nil)
	registerLegacyPost(app, internalAuth, cfg, "google-workspace", "/integrations/google/gmail/send", "gmail.send", nil)
	registerLegacyGet(app, internalAuth, cfg, "google-workspace", "/integrations/google/calendar/events", "calendar.events", nil)
	registerLegacyGet(app, internalAuth, cfg, "google-workspace", "/integrations/google/drive/files", "drive.files", nil)

	registerLegacyGet(app, internalAuth, cfg, "slack", "/integrations/slack/channels", "channels.list", nil)
	registerLegacyGet(app, internalAuth, cfg, "slack", "/integrations/slack/users", "users.list", nil)
	registerLegacyGet(app, internalAuth, cfg, "slack", "/integrations/slack/user/:userId", "user", func(c *fiber.Ctx) map[string]any {
		return map[string]any{"userId": c.Params("userId")}
	})
	registerLegacyGet(app, internalAuth, cfg, "slack", "/integrations/slack/messages/:channelId", "messages.list", func(c *fiber.Ctx) map[string]any {
		return map[string]any{"channel": c.Params("channelId")}
	})
	registerLegacyPost(app, internalAuth, cfg, "slack", "/integrations/slack/message", "message.send", nil)
	registerLegacyPost(app, internalAuth, cfg, "slack", "/integrations/slack/messages", "message.send", nil)

	registerLegacyGet(app, internalAuth, cfg, "github", "/integrations/github/user", "user", nil)
	registerLegacyGet(app, internalAuth, cfg, "github", "/integrations/github/organizations", "orgs", nil)
	registerLegacyGet(app, internalAuth, cfg, "github", "/integrations/github/organizations/:org/teams", "teams", func(c *fiber.Ctx) map[string]any {
		return map[string]any{"org": c.Params("org")}
	})
	registerLegacyGet(app, internalAuth, cfg, "github", "/integrations/github/repositories", "repos", nil)
	registerLegacyGet(app, internalAuth, cfg, "github", "/integrations/github/repos", "repos", nil)
	registerLegacyGet(app, internalAuth, cfg, "github", "/integrations/github/repos/:owner/:repo", "repo", func(c *fiber.Ctx) map[string]any {
		return map[string]any{"owner": c.Params("owner"), "repo": c.Params("repo")}
	})

	registerLegacyGet(app, internalAuth, cfg, "notion", "/integrations/notion/user", "user", nil)
	registerLegacyGet(app, internalAuth, cfg, "notion", "/integrations/notion/databases", "databases", nil)
	registerLegacyGet(app, internalAuth, cfg, "notion", "/integrations/notion/pages", "pages", nil)
	registerLegacyGet(app, internalAuth, cfg, "notion", "/integrations/notion/pages/:databaseId", "pages", func(c *fiber.Ctx) map[string]any {
		return map[string]any{"databaseId": c.Params("databaseId")}
	})

	registerLegacyGet(app, internalAuth, cfg, "shopify", "/integrations/shopify/shop", "shop", nil)
	registerLegacyGet(app, internalAuth, cfg, "shopify", "/integrations/shopify/products", "products", nil)
	registerLegacyGet(app, internalAuth, cfg, "shopify", "/integrations/shopify/orders", "orders", nil)

	registerLegacyGet(app, internalAuth, cfg, "stripe", "/integrations/stripe/account", "account", nil)
	registerLegacyGet(app, internalAuth, cfg, "stripe", "/integrations/stripe/customers", "customers", nil)
	registerLegacyGet(app, internalAuth, cfg, "stripe", "/integrations/stripe/subscriptions", "subscriptions", nil)
	registerLegacyGet(app, internalAuth, cfg, "stripe", "/integrations/stripe/invoices", "invoices", nil)

	app.Post("/integrations/:provider/proxy", internalAuth, func(c *fiber.Ctx) error {
		return apiError(c, fiber.StatusGone, "proxy_not_supported", "Use whitelisted provider actions instead of raw provider proxying.")
	})
	app.Post("/integrations/microsoft/graph/proxy", internalAuth, func(c *fiber.Ctx) error {
		return apiError(c, fiber.StatusGone, "proxy_not_supported", "Use whitelisted provider actions instead of raw Microsoft Graph proxying.")
	})
}

func registerLegacyGet(app *fiber.App, internalAuth fiber.Handler, cfg ServerConfig, connectorType, path, operation string, extraParams func(*fiber.Ctx) map[string]any) {
	app.Get(path, internalAuth, func(c *fiber.Ctx) error {
		params := legacyQueryParams(c, extraParams)
		connectionID, err := resolveLegacyConnectionID(c.UserContext(), cfg, c, connectorType, nil)
		if err != nil {
			return legacyActionError(c, err)
		}
		result, err := executeConnectionAction(c, cfg, connectionID, actionBody{Operation: operation, Params: params})
		if err != nil {
			return actionError(c, err)
		}
		return success(c, fiber.Map{"action": result, "compatibilityRoute": path})
	})
}

func registerLegacyPost(app *fiber.App, internalAuth fiber.Handler, cfg ServerConfig, connectorType, path, operation string, extraParams func(*fiber.Ctx) map[string]any) {
	app.Post(path, internalAuth, func(c *fiber.Ctx) error {
		parsed, err := parseLegacyPostBody(c, extraParams)
		if err != nil {
			return apiError(c, fiber.StatusBadRequest, "invalid_body", "Request body is invalid.")
		}
		connectionID, err := resolveLegacyConnectionID(c.UserContext(), cfg, c, connectorType, parsed.raw)
		if err != nil {
			return legacyActionError(c, err)
		}
		result, err := executeConnectionAction(c, cfg, connectionID, actionBody{
			Operation: operation,
			Params:    parsed.params,
			Body:      parsed.body,
		})
		if err != nil {
			return actionError(c, err)
		}
		return success(c, fiber.Map{"action": result, "compatibilityRoute": path})
	})
}

type legacyPostBody struct {
	raw    map[string]any
	params map[string]any
	body   map[string]any
}

func parseLegacyPostBody(c *fiber.Ctx, extraParams func(*fiber.Ctx) map[string]any) (legacyPostBody, error) {
	raw := map[string]any{}
	if len(c.Body()) > 0 {
		if err := json.Unmarshal(c.Body(), &raw); err != nil {
			return legacyPostBody{}, err
		}
	}
	params := legacyQueryParams(c, extraParams)
	if nestedParams, ok := raw["params"].(map[string]any); ok {
		params = mergeAnyMaps(params, nestedParams)
	}
	body := map[string]any{}
	if nestedBody, ok := raw["body"].(map[string]any); ok {
		body = cloneAnyMap(nestedBody)
	} else {
		for key, value := range raw {
			if legacyEnvelopeKey(key) {
				continue
			}
			body[key] = value
		}
	}
	return legacyPostBody{raw: raw, params: params, body: body}, nil
}

func resolveLegacyConnectionID(ctx context.Context, cfg ServerConfig, c *fiber.Ctx, connectorType string, body map[string]any) (string, error) {
	if connectionID := firstNonEmpty(
		c.Query("connectionId"),
		c.Get("X-Connection-ID"),
		c.Get("X-Velion-Connection-ID"),
		stringFromAny(body["connectionId"]),
	); connectionID != "" {
		return connectionID, nil
	}
	organizationID := firstNonEmpty(
		c.Query("organizationId"),
		c.Get("X-Org-ID"),
		c.Get("X-Organization-ID"),
		stringFromAny(body["organizationId"]),
	)
	if organizationID == "" {
		return "", errLegacyConnectionRequired
	}
	connection, err := cfg.Repo.FindActiveConnection(ctx, organizationID, connectorType)
	if err != nil {
		return "", err
	}
	return connection.ID, nil
}

func legacyQueryParams(c *fiber.Ctx, extraParams func(*fiber.Ctx) map[string]any) map[string]any {
	out := map[string]any{}
	for key, value := range c.Queries() {
		if legacyEnvelopeKey(key) {
			continue
		}
		out[key] = value
	}
	if extraParams != nil {
		out = mergeAnyMaps(out, extraParams(c))
	}
	return out
}

func legacyEnvelopeKey(key string) bool {
	switch strings.ToLower(strings.TrimSpace(key)) {
	case "connectionid", "organizationid", "operation", "params", "body":
		return true
	default:
		return false
	}
}

func legacyActionError(c *fiber.Ctx, err error) error {
	if errors.Is(err, errLegacyConnectionRequired) {
		return apiError(c, fiber.StatusBadRequest, "connection_required", "Pass connectionId, or organizationId/X-Org-ID for the route connector.")
	}
	return actionError(c, err)
}

func mergeAnyMaps(left, right map[string]any) map[string]any {
	out := cloneAnyMap(left)
	for key, value := range right {
		out[key] = value
	}
	return out
}

func cloneAnyMap(input map[string]any) map[string]any {
	out := map[string]any{}
	for key, value := range input {
		out[key] = value
	}
	return out
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if trimmed := strings.TrimSpace(value); trimmed != "" {
			return trimmed
		}
	}
	return ""
}

func stringFromAny(value any) string {
	if text, ok := value.(string); ok {
		return text
	}
	return ""
}
