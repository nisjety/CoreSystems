package api

import (
	"encoding/json"
	"errors"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/auth"
	"github.com/triodelab/anbud/internal/demo"
)

// maxDemoBody bounds a demo request body. A profile is a few kilobytes.
const maxDemoBody = 64 << 10

// retryAfterBusy is what a caller is told to wait, in seconds, when the scan slots are taken.
const retryAfterBusy = "2"

// registerDemo mounts the demo routes. They are a demo surface, not a frozen contract:
//
//	POST /demo/matches             match a profile against the stored tenders
//	POST /demo/matches/:notice_id  the audit sheet of one tender for a profile
//	POST /demo/company             what the register holds about an organisation
//	POST /demo/retenders           contracts likely to be tendered again soon
//
// All four are POST because the body is a company's own business data, and none of it belongs in
// a URL, which ends up in access logs. Nothing in a request is stored.
//
// A request that carries X-Org-ID (the organisation the gateway authenticated) is also judged by the
// tender documents that organisation uploaded, when uploads are configured. The organisation is never
// read from the body.
func registerDemo(r fiber.Router, svc *demo.Service, logger *zerolog.Logger) {
	r.Post("/notices/search", catalogueHandler(svc, logger))
	r.Post("/notices/:notice_id", catalogueDetailHandler(svc, logger))
	r.Post("/demo/matches", matchesHandler(svc, logger))
	r.Post("/demo/matches/:notice_id", auditHandler(svc, logger))
	r.Post("/demo/company", companyHandler(svc, logger))
	r.Post("/demo/retenders", reTendersHandler(svc, logger))
}

func reTendersHandler(svc *demo.Service, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		if tooLarge(c) {
			return writeError(c, fiber.StatusRequestEntityTooLarge, "payload_too_large", "the request body is too large")
		}
		req, problems := demo.DecodeReTenderRequest(c.BodyRaw())
		if len(problems) > 0 {
			return writeValidation(c, problems)
		}
		out, err := svc.ReTenders(c.UserContext(), req)
		if err != nil {
			return demoError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": out})
	}
}

func matchesHandler(svc *demo.Service, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		if tooLarge(c) {
			return writeError(c, fiber.StatusRequestEntityTooLarge, "payload_too_large", "the request body is too large")
		}
		req, problems := demo.DecodeMatchRequest(c.BodyRaw())
		if len(problems) > 0 {
			return writeValidation(c, problems)
		}
		req.Org = auth.OrganizationID(c)
		req.User = auth.UserID(c)
		out, err := svc.Matches(c.UserContext(), req)
		if err != nil {
			return demoError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": out})
	}
}

func auditHandler(svc *demo.Service, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		if tooLarge(c) {
			return writeError(c, fiber.StatusRequestEntityTooLarge, "payload_too_large", "the request body is too large")
		}
		req, problems := demo.DecodeMatchRequest(c.BodyRaw())
		if len(problems) > 0 {
			return writeValidation(c, problems)
		}
		req.Org = auth.OrganizationID(c)
		out, err := svc.Audit(c.UserContext(), c.Params("notice_id"), req)
		if err != nil {
			return demoError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": out})
	}
}

func companyHandler(svc *demo.Service, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		if tooLarge(c) {
			return writeError(c, fiber.StatusRequestEntityTooLarge, "payload_too_large", "the request body is too large")
		}
		number, problems := demo.DecodeCompanyRequest(c.BodyRaw())
		if len(problems) > 0 {
			return writeValidation(c, problems)
		}
		out, err := svc.Company(c.UserContext(), number)
		if err != nil {
			return demoError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": out})
	}
}

func tooLarge(c *fiber.Ctx) bool { return len(c.BodyRaw()) > maxDemoBody }

// writeValidation answers a request that failed validation with the fields that are wrong and why.
// The reasons are fixed text: no value of the request is in them.
func writeValidation(c *fiber.Ctx, problems []demo.FieldError) error {
	return writeErrorDetails(c, fiber.StatusBadRequest, "validation_failed", "the request is not valid", problems)
}

// demoError maps what the demo service refuses to a status and a code. What it does not know is
// returned as it is, and the error handler answers 500 and logs it: a cause is for the log, never
// for the caller.
func demoError(c *fiber.Ctx, err error, logger *zerolog.Logger) error {
	var unsupported *demo.UnsupportedError
	switch {
	case errors.Is(err, demo.ErrMatchCursor):
		return writeError(c, 400, "invalid_matching_cursor", "the matching cursor is not valid for this search")
	case errors.Is(err, demo.ErrMatchSnapshotExpired):
		return writeError(c, 409, "matching_snapshot_expired", "the matching results changed; restart from the first page")
	case errors.Is(err, demo.ErrBusy):
		c.Set(fiber.HeaderRetryAfter, retryAfterBusy)
		return writeError(c, fiber.StatusTooManyRequests, "rate_limited", "too many scans are running; try again shortly")
	case errors.Is(err, demo.ErrTimeout):
		return writeError(c, fiber.StatusGatewayTimeout, "timeout", "the scan took too long")
	case errors.Is(err, demo.ErrNotFound):
		return writeError(c, fiber.StatusNotFound, "not_found", "notice not found")
	case errors.Is(err, demo.ErrInvalidNoticeID):
		return writeError(c, fiber.StatusBadRequest, "bad_request", "the notice id is not valid")
	case errors.Is(err, demo.ErrNotAnOpportunity):
		return writeError(c, fiber.StatusUnprocessableEntity, "not_an_opportunity", "the notice is a result or a prior information notice: there is nothing to bid on")
	case errors.Is(err, demo.ErrUnreadable):
		return writeError(c, fiber.StatusUnprocessableEntity, "unreadable_notice", "the stored body is not a notice this service can read")
	case errors.As(err, &unsupported):
		return writeErrorDetails(c, fiber.StatusUnprocessableEntity, "unsupported_notice", "no field map covers this notice's shape",
			fiber.Map{"root": unsupported.Root, "sdk_version": unsupported.SDKVersion, "flavour": unsupported.Flavour})
	case errors.Is(err, demo.ErrInvalidOrgNumber):
		return writeValidation(c, []demo.FieldError{{Field: "org_number", Reason: "is not a valid Norwegian organisation number"}})
	case errors.Is(err, demo.ErrCompanyNotConfigured):
		return writeError(c, fiber.StatusServiceUnavailable, "not_configured", "organisation lookup is not configured")
	case errors.Is(err, demo.ErrCompanyNotFound):
		return writeError(c, fiber.StatusNotFound, "not_found", "organisation not found in Enhetsregisteret")
	case errors.Is(err, demo.ErrCompanyUnavailable):
		if logger != nil {
			// The cause names org-core's address and why the call failed, and not the number.
			logger.Warn().Err(err).Msg("demo: organisation lookup failed")
		}
		return writeError(c, fiber.StatusBadGateway, "upstream_unavailable", "the organisation register could not be reached")
	}
	return err
}

func catalogueHandler(svc *demo.Service, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		if tooLarge(c) {
			return writeError(c, fiber.StatusRequestEntityTooLarge, "payload_too_large", "the request body is too large")
		}
		var req demo.CatalogueRequest
		if err := json.Unmarshal(c.BodyRaw(), &req); err != nil {
			return writeError(c, 400, "bad_request", "invalid catalogue request")
		}
		out, err := svc.Catalogue(c.UserContext(), req)
		if errors.Is(err, demo.ErrCatalogueRequest) {
			return writeError(c, 400, "bad_request", "invalid catalogue filters or cursor")
		}
		if err != nil {
			return demoError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": out})
	}
}

func catalogueDetailHandler(svc *demo.Service, logger *zerolog.Logger) fiber.Handler {
	return func(c *fiber.Ctx) error {
		if tooLarge(c) {
			return writeError(c, 413, "payload_too_large", "the request body is too large")
		}
		out, err := svc.CatalogueDetail(c.UserContext(), c.Params("notice_id"))
		if err != nil {
			return demoError(c, err, logger)
		}
		return c.JSON(fiber.Map{"data": out})
	}
}
