package pagespeed

import (
	"bytes"
	"encoding/json"
)

// Check projects only a fixed Lighthouse diagnostic ID and machine outcome.
// Provider descriptions, debug strings, details and resource URLs never cross
// this boundary. A missing outcome is not evidence that a page passed.
type Check struct {
	ID     string   `json:"id"`
	Status string   `json:"status"`
	Score  *float64 `json:"score"`
}

// IDs follow Lighthouse's SEO audit configuration. Keep order stable for
// clients; unknown provider audits cannot expand the response contract.
var checkIDs = [...]string{
	"document-title", "meta-description", "http-status-code", "is-crawlable",
	"robots-txt", "canonical", "hreflang", "link-text", "crawlable-anchors",
	"image-alt", "structured-data",
}

func projectChecks(audits map[string]json.RawMessage) []Check {
	checks := make([]Check, 0, len(checkIDs))
	for _, id := range checkIDs {
		checks = append(checks, projectCheck(id, audits[id]))
	}
	return checks
}

func projectCheck(id string, raw json.RawMessage) Check {
	check := Check{ID: id, Status: "not_reported"}
	var audit struct {
		Mode  string          `json:"scoreDisplayMode"`
		Score json.RawMessage `json:"score"`
	}
	if json.Unmarshal(raw, &audit) != nil || len(audit.Score) == 0 {
		return check
	}
	if bytes.Equal(bytes.TrimSpace(audit.Score), []byte("null")) {
		switch audit.Mode {
		case "notApplicable":
			check.Status = "not_applicable"
		case "manual", "informative", "error":
			check.Status = audit.Mode
		}
		return check
	}
	var score float64
	if audit.Mode != "binary" || json.Unmarshal(audit.Score, &score) != nil {
		return check
	}
	switch score {
	case 0:
		check.Status, check.Score = "failed", &score
	case 1:
		check.Status, check.Score = "passed", &score
	}
	return check
}

func numericAudit(raw json.RawMessage) (*float64, bool) {
	if len(raw) == 0 {
		return nil, true
	}
	var audit struct {
		Value *float64 `json:"numericValue"`
	}
	if json.Unmarshal(raw, &audit) != nil || audit.Value != nil && !finiteBetween(*audit.Value, 0, 1e12) {
		return nil, false
	}
	return audit.Value, true
}

func cloneChecks(checks []Check) []Check {
	if checks == nil {
		return nil
	}
	copied := make([]Check, len(checks))
	for i, check := range checks {
		copied[i] = Check{ID: check.ID, Status: check.Status, Score: copyNumber(check.Score)}
	}
	return copied
}
