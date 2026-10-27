package http

import "github.com/gin-gonic/gin"

// typedErrorBody is the envelope of the org-policy routes (and of any later
// route that wants a machine-readable refusal): {"error":{"code","message",
// "details"}}. Details is always an object, never null, and must never carry
// attestation values, tokens or content.
func typedErrorBody(code, message string, details map[string]any) gin.H {
	if details == nil {
		details = map[string]any{}
	}
	return gin.H{"error": gin.H{"code": code, "message": message, "details": details}}
}

// abortTypedError writes a typed error and stops the handler chain.
func abortTypedError(c *gin.Context, status int, code, message string, details map[string]any) {
	c.AbortWithStatusJSON(status, typedErrorBody(code, message, details))
}
