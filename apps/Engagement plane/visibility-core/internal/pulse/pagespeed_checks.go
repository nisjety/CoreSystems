package pulse

// PageSpeedCheck is a bounded Lighthouse observation, never a whole-site result.
// Scores retain Lighthouse's binary 0/1 scale; unscored states remain null.
type PageSpeedCheck struct {
	ID     string   `json:"id"`
	Status string   `json:"status"`
	Score  *float64 `json:"score"`
}

var pageSpeedCheckIDs = map[string]bool{
	"document-title": true, "meta-description": true, "http-status-code": true,
	"is-crawlable": true, "robots-txt": true, "canonical": true, "hreflang": true,
	"link-text": true, "crawlable-anchors": true, "image-alt": true, "structured-data": true,
}

func validPageSpeedChecks(checks []PageSpeedCheck) bool {
	if len(checks) > len(pageSpeedCheckIDs) {
		return false
	}
	seen := make(map[string]bool, len(checks))
	for _, check := range checks {
		if !pageSpeedCheckIDs[check.ID] || seen[check.ID] {
			return false
		}
		seen[check.ID] = true
		switch check.Status {
		case "passed":
			if check.Score == nil || *check.Score != 1 {
				return false
			}
		case "failed":
			if check.Score == nil || *check.Score != 0 {
				return false
			}
		case "not_applicable", "manual", "informative", "error", "not_reported":
			if check.Score != nil {
				return false
			}
		default:
			return false
		}
	}
	return true
}
