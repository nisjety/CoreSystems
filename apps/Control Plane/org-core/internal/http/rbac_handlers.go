package http

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/rbac"
	"github.com/gin-gonic/gin"
)

// U6-3 (ui-ux-velion-gap.md §10): handlers for the role/permission editor.
//
// All handlers expect `:id` (org id) in the path. The internal-auth
// middleware (server.go) gates the entire surface — no per-user auth here.
// Velion's /settings/permissions page is responsible for verifying that
// the caller has `roles:manage` before exposing these UIs.

func (s *Server) rbacContext(c *gin.Context) (context.Context, context.CancelFunc) {
	return context.WithTimeout(c.Request.Context(), 10*time.Second)
}

func (s *Server) listCapabilityCatalog(c *gin.Context) {
	// Catalog is global today — the `:id` param is required for symmetry
	// with the rest of the surface but the response is the same per org.
	c.JSON(http.StatusOK, gin.H{
		"capabilities": rbac.Catalog(),
	})
}

func (s *Server) listRoles(c *gin.Context) {
	if s.rbacRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "rbac repository not configured"})
		return
	}
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "org id required"})
		return
	}
	ctx, cancel := s.rbacContext(c)
	defer cancel()

	roles, err := s.rbacRepo.List(ctx, orgID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"roles": roles})
}

type createRoleBody struct {
	RoleName    string   `json:"role_name"`
	Permissions []string `json:"permissions"`
}

func (s *Server) createRole(c *gin.Context) {
	if s.rbacRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "rbac repository not configured"})
		return
	}
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "org id required"})
		return
	}
	var body createRoleBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}
	body.RoleName = strings.TrimSpace(body.RoleName)
	if body.RoleName == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "role_name required"})
		return
	}
	ctx, cancel := s.rbacContext(c)
	defer cancel()

	role, err := s.rbacRepo.Create(ctx, rbac.CreateParams{
		OrgID:       orgID,
		RoleName:    body.RoleName,
		Permissions: body.Permissions,
	})
	if err != nil {
		switch {
		case errors.Is(err, rbac.ErrAlreadyExists):
			c.JSON(http.StatusConflict, gin.H{"error": err.Error()})
		case errors.Is(err, rbac.ErrInvalidCapability):
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		default:
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		}
		return
	}
	c.JSON(http.StatusCreated, role)
}

type updateRoleBody struct {
	Permissions []string `json:"permissions"`
}

func (s *Server) updateRole(c *gin.Context) {
	if s.rbacRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "rbac repository not configured"})
		return
	}
	orgID := strings.TrimSpace(c.Param("id"))
	roleName := strings.TrimSpace(c.Param("roleName"))
	if orgID == "" || roleName == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "org id and role name required"})
		return
	}
	var body updateRoleBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}
	ctx, cancel := s.rbacContext(c)
	defer cancel()

	role, err := s.rbacRepo.Update(ctx, rbac.UpdateParams{
		OrgID:       orgID,
		RoleName:    roleName,
		Permissions: body.Permissions,
	})
	if err != nil {
		switch {
		case errors.Is(err, rbac.ErrNotFound):
			c.JSON(http.StatusNotFound, gin.H{"error": err.Error()})
		case errors.Is(err, rbac.ErrInvalidCapability):
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		default:
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		}
		return
	}
	c.JSON(http.StatusOK, role)
}

func (s *Server) deleteRole(c *gin.Context) {
	if s.rbacRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "rbac repository not configured"})
		return
	}
	orgID := strings.TrimSpace(c.Param("id"))
	roleName := strings.TrimSpace(c.Param("roleName"))
	if orgID == "" || roleName == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "org id and role name required"})
		return
	}
	ctx, cancel := s.rbacContext(c)
	defer cancel()

	if err := s.rbacRepo.Delete(ctx, orgID, roleName); err != nil {
		switch {
		case errors.Is(err, rbac.ErrNotFound):
			c.JSON(http.StatusNotFound, gin.H{"error": err.Error()})
		case errors.Is(err, rbac.ErrCannotDelete):
			c.JSON(http.StatusConflict, gin.H{"error": err.Error()})
		default:
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		}
		return
	}
	c.Status(http.StatusNoContent)
}

type assignRoleBody struct {
	Role string `json:"role"`
}

func (s *Server) assignMemberRole(c *gin.Context) {
	if s.rbacRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "rbac repository not configured"})
		return
	}
	orgID := strings.TrimSpace(c.Param("id"))
	userID := strings.TrimSpace(c.Param("userId"))
	if orgID == "" || userID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "org id and user id required"})
		return
	}
	var body assignRoleBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}
	body.Role = strings.TrimSpace(body.Role)
	if body.Role == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "role required"})
		return
	}
	ctx, cancel := s.rbacContext(c)
	defer cancel()

	assignment, err := s.rbacRepo.AssignMemberRole(ctx, orgID, userID, body.Role)
	if err != nil {
		if errors.Is(err, rbac.ErrNotFound) {
			c.JSON(http.StatusNotFound, gin.H{"error": err.Error()})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, assignment)
}
