package api

import "github.com/gofiber/fiber/v2"

type ErrorEnvelope struct {
	Success   bool        `json:"success"`
	Error     string      `json:"error"`
	RequestID string      `json:"requestId,omitempty"`
	Details   interface{} `json:"details,omitempty"`
}

func WriteError(c *fiber.Ctx, status int, errMsg string, details interface{}) error {
	requestID, _ := c.Locals("requestid").(string)
	if requestID == "" {
		requestID = c.GetRespHeader("X-Request-ID")
	}
	return c.Status(status).JSON(ErrorEnvelope{
		Success:   false,
		Error:     errMsg,
		RequestID: requestID,
		Details:   details,
	})
}

func writeError(c *fiber.Ctx, status int, errMsg string, details interface{}) error {
	return WriteError(c, status, errMsg, details)
}
