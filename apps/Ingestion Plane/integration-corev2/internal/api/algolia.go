package api

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/algoliamcp"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/sitesources"
	"github.com/triodelab/integration-corev2/internal/webvitals"
	"mime"
	"net/url"
	"strings"
)

type AlgoliaMCPReader interface {
	Access(context.Context, string, string) (algoliamcp.Access, error)
	Applications(context.Context, string, string) ([]algoliamcp.Application, error)
	Indices(context.Context, string, string, string) ([]algoliamcp.Index, error)
	Resources(context.Context, string, algoliamcp.ResourceInput) (algoliamcp.Resources, error)
	Report(context.Context, string, algoliamcp.ReportInput) (sitesources.Report, error)
}

func registerAlgolia(app *fiber.App, cfg ServerConfig, authenticate fiber.Handler, limits []fiber.Handler) {
	user := func(c *fiber.Ctx) error {
		c.Set("Cache-Control", "no-store")
		p, ok := auth.PrincipalFromContext(c)
		if !ok || auth.IsInternalCall(c) || p.PrincipalType != "user" || p.UserID == "" || p.OrganizationID == "" {
			return apiError(c, 403, "user_required", "Sign in to select an Algolia account.")
		}
		grant, err := cfg.Repo.GetConnection(c.UserContext(), c.Params("id"))
		if err != nil || grant.OrganizationID != p.OrganizationID || grant.ProviderKey != "algolia" || grant.DeletedAt != nil || grant.Status != "active" {
			return apiError(c, 404, "connection_not_found", "The Algolia grant is not available.")
		}
		if grant.UserID != p.UserID && p.Role != "admin" && p.Role != "owner" {
			return apiError(c, 403, "connection_forbidden", "Use your own grant or ask an organization administrator.")
		}
		if cfg.AlgoliaMCP == nil {
			return algoliaError(c, algoliamcp.ErrUnavailable)
		}
		return c.Next()
	}
	userHandlers := append(append([]fiber.Handler{}, limits...), authenticate, user)
	app.Get("/api/v1/providers/algolia/connections/:id/applications", append(append([]fiber.Handler{}, userHandlers...), func(c *fiber.Ctx) error {
		if c.Context().QueryArgs().Len() != 0 || len(c.Body()) != 0 {
			return invalidAlgolia(c)
		}
		p, _ := auth.PrincipalFromContext(c)
		items, err := cfg.AlgoliaMCP.Applications(c.UserContext(), strings.Clone(p.OrganizationID), c.Params("id"))
		if err != nil {
			return algoliaError(c, err)
		}
		return c.JSON(fiber.Map{"success": true, "data": fiber.Map{"applications": items}})
	})...)
	app.Get("/api/v1/providers/algolia/connections/:id/indices", append(append([]fiber.Handler{}, userHandlers...), func(c *fiber.Ctx) error {
		q, err := url.ParseQuery(string(c.Context().QueryArgs().QueryString()))
		if err != nil || len(q) != 1 || len(q["applicationId"]) != 1 || len(c.Body()) != 0 {
			return invalidAlgolia(c)
		}
		p, _ := auth.PrincipalFromContext(c)
		items, err := cfg.AlgoliaMCP.Indices(c.UserContext(), strings.Clone(p.OrganizationID), c.Params("id"), q.Get("applicationId"))
		if err != nil {
			return algoliaError(c, err)
		}
		return c.JSON(fiber.Map{"success": true, "data": fiber.Map{"indices": items}})
	})...)
	handlers := append(append([]fiber.Handler{}, limits...), authenticate, siteSourceReadScope)
	for _, operation := range []string{"access", "resources", "report"} {
		operation := operation
		app.Post("/api/v1/insights/algolia/"+operation, append(append([]fiber.Handler{}, handlers...), func(c *fiber.Ctx) error {
			c.Set("Cache-Control", "no-store")
			if cfg.AlgoliaMCP == nil {
				return algoliaError(c, algoliamcp.ErrUnavailable)
			}
			p, _ := auth.PrincipalFromContext(c)
			org := strings.Clone(p.OrganizationID)
			switch operation {
			case "access":
				var in struct {
					ConnectionID string `json:"connectionId"`
				}
				if !algoliaBody(c, &in, "connectionId") {
					return invalidAlgolia(c)
				}
				result, err := cfg.AlgoliaMCP.Access(c.UserContext(), org, in.ConnectionID)
				if err != nil {
					return algoliaError(c, err)
				}
				return c.JSON(fiber.Map{"success": true, "data": result})
			case "resources":
				var in algoliamcp.ResourceInput
				if !algoliaBody(c, &in, "connectionId", "applicationId", "indexName") {
					return invalidAlgolia(c)
				}
				result, err := cfg.AlgoliaMCP.Resources(c.UserContext(), org, in)
				if errors.Is(err, algoliamcp.ErrUnavailable) {
					return c.JSON(fiber.Map{"success": true, "data": result})
				}
				if err != nil {
					return algoliaError(c, err)
				}
				return c.JSON(fiber.Map{"success": true, "data": result})
			default:
				var in algoliamcp.ReportInput
				if !algoliaBody(c, &in, "websiteId", "site", "websiteRevision", "connectionId", "applicationId", "indexName") {
					return invalidAlgolia(c)
				}
				result, err := cfg.AlgoliaMCP.Report(c.UserContext(), org, in)
				if err != nil {
					return algoliaError(c, err)
				}
				return c.JSON(fiber.Map{"success": true, "data": result})
			}
		})...)
	}
}
func algoliaBody(c *fiber.Ctx, target any, keys ...string) bool {
	media, _, err := mime.ParseMediaType(c.Get("Content-Type"))
	raw := c.Body()
	if err != nil || media != "application/json" || len(raw) > 2048 || c.Context().QueryArgs().Len() != 0 || webvitals.UniqueJSON(raw) != nil {
		return false
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || len(fields) != len(keys) {
		return false
	}
	for _, key := range keys {
		if value, ok := fields[key]; !ok || strings.TrimSpace(string(value)) == "null" {
			return false
		}
	}
	return json.Unmarshal(raw, target) == nil
}
func invalidAlgolia(c *fiber.Ctx) error {
	return apiError(c, 400, "invalid_algolia_request", "Choose a current website and an explicitly authorized Algolia resource.")
}
func algoliaError(c *fiber.Ctx, err error) error {
	if errors.Is(err, algoliamcp.ErrAccess) {
		return apiError(c, 404, "algolia_connection_unavailable", "The Algolia resource is not available in this organization.")
	}
	if errors.Is(err, algoliamcp.ErrBusy) {
		c.Set("Retry-After", "60")
		return apiError(c, 429, "algolia_busy", "Algolia reporting capacity is busy.")
	}
	return apiError(c, 503, "algolia_unavailable", "Algolia discovery or aggregate reporting is unavailable. Check account consent, Productivity MCP and Analytics permissions.")
}
