package http

import (
	"encoding/json"
	"errors"
	"io"
	"mime"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/providerleads"
	"github.com/gin-gonic/gin"
)

func (h *Handler) SetWebsiteInquiries(service *providerleads.WebsiteService) {
	h.websiteInquiries = service
}

// Independent installation credentials, never the all-service API key. These
// routes are server-to-server only; browser clients cannot supply tenant scope.
func registerWebsiteInquiries(router *gin.Engine, h *Handler) {
	var mu sync.Mutex
	windows := map[string]struct {
		start time.Time
		count int
	}{}
	limited := func(c *gin.Context, b providerleads.WebsiteInstallation) bool {
		mu.Lock()
		defer mu.Unlock()
		now := time.Now()
		key := b.OrganizationID + "/" + b.WebsiteID + "/" + b.Environment
		w := windows[key]
		if now.Sub(w.start) >= time.Minute {
			w.start = now
			w.count = 0
		}
		w.count++
		windows[key] = w
		if w.count > 60 {
			c.Header("Retry-After", "60")
			c.JSON(429, errorPayload("rate_limited", "Wait before submitting another inquiry"))
			return true
		}
		return false
	}
	authorize := func(c *gin.Context, write bool) (providerleads.WebsiteInstallation, bool) {
		c.Header("Cache-Control", "no-store")
		authorization := c.GetHeader("Authorization")
		token := ""
		if strings.HasPrefix(authorization, "Bearer ") {
			token = strings.TrimPrefix(authorization, "Bearer ")
		}
		b, ok := h.websiteInquiries.Authorize(token, write)
		if !ok || c.GetHeader("Origin") != "" {
			c.JSON(401, errorPayload("unauthorized", "An authorized website installation is required"))
			return b, false
		}
		return b, true
	}
	router.POST("/api/v1/website-inquiries", func(c *gin.Context) {
		b, ok := authorize(c, true)
		if !ok || limited(c, b) {
			return
		}
		media, _, err := mime.ParseMediaType(c.GetHeader("Content-Type"))
		if err != nil || media != "application/json" {
			c.JSON(415, errorPayload("invalid_content_type", "JSON is required"))
			return
		}
		var v providerleads.WebsiteInquiry
		d := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, 16<<10))
		d.DisallowUnknownFields()
		if d.Decode(&v) != nil || d.Decode(new(any)) != io.EOF {
			c.JSON(400, errorPayload("invalid_inquiry", "Check the inquiry fields"))
			return
		}
		receipt, err := h.websiteInquiries.Receive(c.Request.Context(), b, v)
		if err != nil {
			status, code := 503, "inquiry_unavailable"
			if errors.Is(err, providerleads.ErrWebsiteInvalid) {
				status, code = 400, "invalid_inquiry"
			}
			if errors.Is(err, providerleads.ErrWebsiteConflict) {
				status, code = 409, "submission_conflict"
			}
			c.JSON(status, errorPayload(code, "The inquiry could not be confirmed"))
			return
		}
		c.JSON(200, gin.H{"data": receipt})
	})
	router.GET("/api/v1/website-outcomes", func(c *gin.Context) {
		b, ok := authorize(c, false)
		if !ok {
			return
		}
		q, err := urlQuery(c.Request)
		if err != nil || len(q) != 6 || q.Get("websiteId") != b.WebsiteID || q.Get("site") != b.Site || q.Get("websiteRevision") != strconv.FormatInt(b.WebsiteRevision, 10) || q.Get("environment") != b.Environment {
			c.JSON(400, errorPayload("invalid_website", "Choose the authorized website installation"))
			return
		}
		result, err := h.websiteInquiries.Report(c.Request.Context(), b, q.Get("startDate"), q.Get("endDate"))
		if err != nil {
			status := 503
			if errors.Is(err, providerleads.ErrWebsiteInvalid) {
				status = 400
			}
			c.JSON(status, errorPayload("outcomes_unavailable", "Website outcomes are unavailable"))
			return
		}
		c.JSON(200, gin.H{"data": result})
	})
}

func urlQuery(r *http.Request) (url.Values, error) {
	q, err := url.ParseQuery(r.URL.RawQuery)
	if err != nil {
		return nil, err
	}
	allowed := map[string]bool{"websiteId": true, "site": true, "websiteRevision": true, "environment": true, "startDate": true, "endDate": true}
	for key, v := range q {
		if !allowed[key] || len(v) != 1 {
			return nil, errors.New("invalid query")
		}
	}
	if r.ContentLength != 0 || len(r.TransferEncoding) > 0 {
		return nil, errors.New("unexpected body")
	}
	return q, nil
}
