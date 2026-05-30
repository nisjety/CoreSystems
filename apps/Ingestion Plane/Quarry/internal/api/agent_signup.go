package api

import (
	"context"
	"net/http"
	"regexp"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/otp"
)

var emailRE = regexp.MustCompile(`^[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}$`)

// agentSignupRequest is the body for POST /v1/agent-signup.
type agentSignupRequest struct {
	Email string `json:"email"`
}

// agentSignupConfirmRequest is the body for POST /v1/agent-signup/confirm.
type agentSignupConfirmRequest struct {
	Email string `json:"email"`
	OTP   string `json:"otp"`
}

// v1AgentSignup handles POST /v1/agent-signup.
//
// Generates a 6-digit OTP, stores it in Redis for 10 minutes, and sends it to
// the supplied email address. Responds 200 regardless of whether the email
// address already exists so that enumeration is not possible.
func (h *Handler) v1AgentSignup(c *fiber.Ctx) error {
	var req agentSignupRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}

	req.Email = strings.TrimSpace(req.Email)
	if req.Email == "" || !emailRE.MatchString(req.Email) {
		return writeError(c, http.StatusBadRequest, "a valid email address is required", nil)
	}

	// Require OTP store (Redis). Without it, OTP signup is unavailable.
	if h.otpStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "OTP service is not configured (requires Redis)", nil)
	}

	code, err := otp.GenerateCode()
	if err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to generate verification code", nil)
	}

	if err := h.otpStore.Store(c.UserContext(), req.Email, code); err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to store verification code", nil)
	}

	// Best-effort delivery — do not reveal delivery errors to callers.
	if h.emailSender != nil {
		go func() {
			_ = h.emailSender.SendOTP(otp.NormalizeEmail(req.Email), code)
		}()
	}

	return c.Status(http.StatusOK).JSON(fiber.Map{
		"success": true,
		"message": "Verification code sent. Check your email.",
	})
}

// v1AgentSignupConfirm handles POST /v1/agent-signup/confirm.
//
// Verifies the OTP and, on success, calls auth-core to create a sandbox
// account (org + user + API key) for the caller.
func (h *Handler) v1AgentSignupConfirm(c *fiber.Ctx) error {
	var req agentSignupConfirmRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}

	req.Email = strings.TrimSpace(req.Email)
	req.OTP = strings.TrimSpace(req.OTP)

	if req.Email == "" || !emailRE.MatchString(req.Email) {
		return writeError(c, http.StatusBadRequest, "a valid email address is required", nil)
	}
	if len(req.OTP) != 6 {
		return writeError(c, http.StatusBadRequest, "otp must be a 6-digit code", nil)
	}

	if h.otpStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "OTP service is not configured (requires Redis)", nil)
	}

	ctx := c.UserContext()

	ok, err := h.otpStore.Verify(ctx, req.Email, req.OTP)
	if err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to verify code", nil)
	}
	if !ok {
		return writeError(c, http.StatusUnauthorized, "invalid or expired verification code", nil)
	}

	// OTP verified — provision sandbox account via auth-core when available.
	if h.authClient == nil {
		// Degraded mode: auth-core not configured. Return success without a key
		// so the caller can still test the OTP flow.
		return c.Status(http.StatusOK).JSON(fiber.Map{
			"success": true,
			"message": "Email verified. Auth-core provisioning is not configured.",
		})
	}

	provCtx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()

	account, err := h.authClient.CreateSandboxAccount(provCtx, otp.NormalizeEmail(req.Email))
	if err != nil {
		return writeError(c, http.StatusBadGateway, "failed to provision account", err.Error())
	}

	return c.Status(http.StatusCreated).JSON(fiber.Map{
		"success": true,
		"data": fiber.Map{
			"apiKey": account.APIKey,
			"orgId":  account.OrgID,
			"userId": account.UserID,
		},
	})
}
