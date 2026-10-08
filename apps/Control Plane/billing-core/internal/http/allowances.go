package http

import (
	"errors"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
	"github.com/gin-gonic/gin"
	"net/http"
)

func allowanceError(c *gin.Context, err error) {
	code, status, message := "allowance_unavailable", http.StatusServiceUnavailable, "Usage allowances are temporarily unavailable."
	switch {
	case errors.Is(err, billing.ErrAllowanceInvalid):
		code, status, message = "invalid_allowance", 400, err.Error()
	case errors.Is(err, billing.ErrAllowanceConflict):
		code, status, message = "allowance_conflict", 409, "This operation conflicts with an existing allowance record."
	case errors.Is(err, billing.ErrNotFound):
		code, status, message = "allowance_not_found", 404, "The billing account or allowance was not found."
	case errors.Is(err, billing.ErrOrganizationDeleted):
		code, status, message = "organization_inactive", 403, "This billing organization is inactive."
	}
	c.JSON(status, gin.H{"error": gin.H{"code": code, "message": message}})
}
func (s *Server) getAllowances(c *gin.Context) {
	result, err := s.billingCore.Allowances(c.Request.Context(), c.Param("orgId"))
	if err != nil {
		allowanceError(c, err)
		return
	}
	c.JSON(200, gin.H{"data": result})
}
func (s *Server) configureAllowance(c *gin.Context) {
	var p billing.AllowancePolicy
	if err := c.ShouldBindJSON(&p); err != nil {
		allowanceError(c, billing.ErrAllowanceInvalid)
		return
	}
	if err := s.billingCore.ConfigureAllowance(c.Request.Context(), c.Param("orgId"), p); err != nil {
		allowanceError(c, err)
		return
	}
	c.JSON(200, gin.H{"data": p})
}
func (s *Server) grantAllowance(c *gin.Context) {
	var e billing.AllowanceEntry
	if err := c.ShouldBindJSON(&e); err != nil {
		allowanceError(c, billing.ErrAllowanceInvalid)
		return
	}
	if err := s.billingCore.GrantAllowance(c.Request.Context(), c.Param("orgId"), c.Param("metric"), e); err != nil {
		allowanceError(c, err)
		return
	}
	c.JSON(200, gin.H{"data": e})
}

func (s *Server) scheduleAllowanceRevision(c *gin.Context) {
	var revision billing.AllowanceRevision
	if err := c.ShouldBindJSON(&revision); err != nil {
		allowanceError(c, billing.ErrAllowanceInvalid)
		return
	}
	if err := s.billingCore.ScheduleAllowanceRevision(c.Request.Context(), c.Param("orgId"), c.Param("metric"), revision); err != nil {
		allowanceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": revision})
}
