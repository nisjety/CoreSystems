package http

import (
	"errors"
	"net/http"
	"net/http/httputil"
	"net/url"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/controlplane"
	"github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/runtime"
	"github.com/I-Dacosta/AquatiqCMS/apps/affine-core/internal/workspace"
	"github.com/gin-gonic/gin"
)

type Handler struct {
	cfg              *config.Config
	controlPlane     *controlplane.Client
	runtimeClient    *runtime.Client
	workspaceService *workspace.Service
}

func NewHandler(
	cfg *config.Config,
	runtimeClient *runtime.Client,
	workspaceService *workspace.Service,
	controlPlane *controlplane.Client,
) *Handler {
	return &Handler{
		cfg:              cfg,
		controlPlane:     controlPlane,
		runtimeClient:    runtimeClient,
		workspaceService: workspaceService,
	}
}

func (h *Handler) Health(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{
		"status":             "ok",
		"service":            h.cfg.ServiceName,
		"runtime_configured": h.runtimeClient != nil && h.runtimeClient.Ready(),
	})
}

func (h *Handler) ExchangeSession(c *gin.Context) {
	actor, ok := h.resolveActor(c)
	if !ok {
		return
	}

	runtimeSession, err := h.runtimeClient.ExchangeAdminSession(c.Request.Context())
	if err != nil {
		c.JSON(http.StatusBadGateway, gin.H{"error": err.Error()})
		return
	}

	binding, err := h.workspaceService.ResolveWorkspace(
		c.Request.Context(),
		strings.TrimSpace(actor.OrgID),
		strings.TrimSpace(actor.UserID),
	)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"user_id":          fallback(runtimeSession.UserID, runtimeSession.UserEmail),
		"workspace_id":     binding.WorkspaceID,
		"org_id":           binding.OrgID,
		"scope":            workspaceScope(binding),
		"expires_at":       time.Now().Add(7 * 24 * time.Hour).UnixMilli(),
		"runtime_cookies":  runtimeSession.RuntimeCookies,
		"proxied_as_admin": true,
	})
}

func (h *Handler) ResolveWorkspace(c *gin.Context) {
	actor, ok := h.resolveActor(c)
	if !ok {
		return
	}

	binding, err := h.workspaceService.ResolveWorkspace(
		c.Request.Context(),
		strings.TrimSpace(actor.OrgID),
		strings.TrimSpace(actor.UserID),
	)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"org_id":       binding.OrgID,
		"workspace_id": binding.WorkspaceID,
		"created_by":   emptyToNil(binding.CreatedBy),
		"scope":        workspaceScope(binding),
	})
}

func (h *Handler) ProxyRuntime(c *gin.Context) {
	if h.runtimeClient == nil || !h.runtimeClient.Ready() {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "affine runtime is unavailable"})
		return
	}

	targetURL, err := url.Parse(h.runtimeClient.BaseURL())
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "invalid affine runtime url"})
		return
	}

	proxy := httputil.NewSingleHostReverseProxy(targetURL)
	originalDirector := proxy.Director
	proxy.Director = func(req *http.Request) {
		originalDirector(req)
		req.Host = targetURL.Host
	}
	proxy.ErrorHandler = func(writer http.ResponseWriter, request *http.Request, proxyErr error) {
		writer.Header().Set("Content-Type", "application/json")
		writer.WriteHeader(http.StatusBadGateway)
		_, _ = writer.Write([]byte(`{"error":"affine runtime proxy failed"}`))
	}

	proxy.ServeHTTP(c.Writer, c.Request)
}

func (h *Handler) resolveActor(c *gin.Context) (*controlplane.ActorContext, bool) {
	actor, err := h.controlPlane.ResolveActorContext(c.Request.Context(), c.GetHeader("Cookie"))
	if err == nil {
		return actor, true
	}
	if errors.Is(err, controlplane.ErrUnauthenticated) {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return nil, false
	}

	c.JSON(http.StatusBadGateway, gin.H{"error": err.Error()})
	return nil, false
}

func fallback(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return value
		}
	}
	return ""
}

func emptyToNil(value string) any {
	if strings.TrimSpace(value) == "" {
		return nil
	}
	return value
}

func workspaceScope(binding workspace.Binding) string {
	if strings.TrimSpace(binding.OrgID) != "" {
		return "organization"
	}
	if strings.TrimSpace(binding.CreatedBy) != "" {
		return "personal"
	}
	return "anonymous"
}
