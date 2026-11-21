package http

import (
	"context"
	"log"
	"net/http"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/browsersites"
	"github.com/gin-gonic/gin"
)

// browserSiteRulesBody is the answer of GET /internal/orgs/:orgId/browser-sites/rules:
// the organisation's admin policy and nothing else. It is the browsersites.PolicySet
// without the admin's free-text notes, the actor id and the timestamps, so a caller
// that only needs to know which hosts the org allows or blocks never receives them.
type browserSiteRulesBody struct {
	OrganizationID string                `json:"organization_id"`
	Revision       int64                 `json:"revision"`
	Mode           string                `json:"mode"`
	Rules          []browserSiteRuleBody `json:"rules"`
}

type browserSiteRuleBody struct {
	Host string `json:"host"`
	Rule string `json:"rule"`
}

// getBrowserSiteRulesInternal serves GET /internal/orgs/:orgId/browser-sites/rules,
// the Model Plane's read of the org's web domain rules (EP-9 O-23): model-gateway
// hands them to the BFF, which carries them on a crawl request so Quarry can apply
// them to every link it follows. Access requires org:browser-site:read:any, the same
// scope as /resolve (held by browser-broker, model-gateway and execution-core only).
//
// The answer is the stored policy: mode, revision and the rules as host and rule.
// An organisation that never wrote a policy answers the open default with no rules
// and revision 0, never 404, because a reader that fails closed on a missing policy
// would shut that organisation's crawls off. The path is mounted whether or not the
// service is configured (no switch: it is a scope-pinned, content-free read that
// changes nothing for any existing caller); with no service it answers 503, which a
// reader treats as "unknown". The answer is never cached.
func (s *Server) getBrowserSiteRulesInternal(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("orgId"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	if s.browserSites == nil {
		log.Printf("browser sites: no service configured for org=%s", orgID)
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "browser site policy unavailable"})
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), browserSiteTimeout)
	defer cancel()
	set, err := s.browserSites.Get(ctx, orgID)
	if err != nil {
		log.Printf("browser sites: rules read failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to read browser site rules"})
		return
	}
	c.JSON(http.StatusOK, browserSiteRulesAnswer(orgID, set))
}

// browserSiteRulesAnswer shapes a stored policy for the internal read. A nil set or
// an empty mode reads as the open default.
func browserSiteRulesAnswer(orgID string, set *browsersites.PolicySet) browserSiteRulesBody {
	body := browserSiteRulesBody{
		OrganizationID: orgID,
		Mode:           browsersites.ModeOpen,
		Rules:          []browserSiteRuleBody{},
	}
	if set == nil {
		return body
	}
	body.Revision = set.Revision
	if set.Mode != "" {
		body.Mode = set.Mode
	}
	for _, rule := range set.Rules {
		body.Rules = append(body.Rules, browserSiteRuleBody{Host: rule.Host, Rule: rule.Rule})
	}
	return body
}
