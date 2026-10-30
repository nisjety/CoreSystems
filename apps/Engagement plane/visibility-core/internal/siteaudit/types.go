// Package siteaudit owns compact, bounded marketing audit projections. Quarry
// owns every public network fetch; this package never fetches website URLs.
package siteaudit

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"regexp"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

const Limit = 100
const RuleVersion = "bounded_rules_v1"
const maxLinks = 100
const maxRuns = 20
const maxSiteRuns = 5
const maxFileBytes = 32 << 20

var (
	ErrInvalid         = errors.New("invalid site audit request")
	ErrConflict        = errors.New("site audit context changed")
	ErrNotFound        = errors.New("site audit evidence not found")
	ErrCapacity        = errors.New("site audit cooldown or capacity reached")
	ErrUnavailable     = errors.New("site audit evidence unavailable")
	websitePattern     = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
	auditPattern       = regexp.MustCompile(`^audit_[a-f0-9]{32}$`)
	findingPattern     = regexp.MustCompile(`^finding_[a-f0-9]{32}$`)
	runPattern         = regexp.MustCompile(`^run_[0-7][0-9A-HJKMNP-TV-Z]{25}$`)
	fingerprintPattern = regexp.MustCompile(`^blake3:[a-f0-9]{64}$`)
	orgPattern         = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
)

type Check struct {
	RuleID string `json:"ruleId"`
	Status string `json:"status"`
}
type Page struct {
	URL               string   `json:"url"`
	FinalURL          string   `json:"finalUrl"`
	Status            string   `json:"status"`
	HTTPStatus        int      `json:"httpStatus"`
	Title             string   `json:"title"`
	Description       string   `json:"description"`
	Canonical         string   `json:"canonical"`
	ObservedAt        string   `json:"observedAt"`
	RunID             string   `json:"runId"`
	Fingerprint       string   `json:"fingerprint"`
	Links             []string `json:"links"`
	Checks            []Check  `json:"checks"`
	SEO               *SEO     `json:"seo,omitempty"`
	ExtractionPartial bool     `json:"extractionPartial,omitempty"`
	Reason            string   `json:"reason,omitempty"`
}
type Finding struct {
	ID          string   `json:"id"`
	RuleID      string   `json:"ruleId"`
	Severity    string   `json:"severity"`
	PageURL     string   `json:"pageUrl"`
	ObservedAt  string   `json:"observedAt"`
	RunID       string   `json:"runId"`
	Fingerprint string   `json:"fingerprint"`
	RelatedURLs []string `json:"relatedUrls"`
}
type Receipt struct {
	ID           string `json:"id"`
	Status       string `json:"status"`
	StartedAt    string `json:"startedAt"`
	CompletedAt  string `json:"completedAt"`
	PageCount    int    `json:"pageCount"`
	FindingCount int    `json:"findingCount"`
}
type Snapshot struct {
	ID              string         `json:"id"`
	WebsiteID       string         `json:"websiteId"`
	Site            string         `json:"site"`
	WebsiteRevision int64          `json:"websiteRevision"`
	Source          string         `json:"source"`
	RuleVersion     string         `json:"ruleVersion"`
	Status          string         `json:"status"`
	Limit           int            `json:"limit"`
	Partial         bool           `json:"partial"`
	StartedAt       string         `json:"startedAt"`
	CompletedAt     string         `json:"completedAt"`
	Pages           []Page         `json:"pages"`
	Findings        []Finding      `json:"findings"`
	History         []Receipt      `json:"history"`
	Reason          string         `json:"reason,omitempty"`
	Coverage        *CrawlCoverage `json:"coverage,omitempty"`
}

type CrawlCoverage struct {
	Discovered         int                    `json:"discovered"`
	Attempted          int                    `json:"attempted"`
	Checked            int                    `json:"checked"`
	Failed             int                    `json:"failed"`
	Pending            int                    `json:"pending"`
	LimitReached       bool                   `json:"limitReached"`
	InventoryTruncated bool                   `json:"inventoryTruncated"`
	ExtractionPartial  bool                   `json:"extractionPartial"`
	StopReason         string                 `json:"stopReason,omitempty"`
	Sitemap            *discovery.MapCoverage `json:"sitemap,omitempty"`
}

// CheckEvidence contains only authoritative, persisted capture provenance.
type CheckEvidence struct {
	AuditID, FindingID, PageURL, RuleID, Status, ObservedAt, RunID, Fingerprint string
}
type Registry interface {
	Resolve(string, string) (websites.Record, error)
	OrganizationID() string
}
type CapturePage struct {
	URL, FinalURL, Title, Description, Canonical, ContentType, ObservedAt, RunID, Fingerprint string
	HTTPStatus                                                                                int
	Links                                                                                     []string
	SEO                                                                                       *SEO
	Partial                                                                                   bool
}
type Capture interface {
	Map(context.Context, string) ([]discovery.Page, error)
	Capture(context.Context, string, string) (CapturePage, error)
}
type CaptureError struct{ Reason string }

func (e *CaptureError) Error() string { return "bounded Quarry capture unavailable" }
func newID(prefix string) string {
	var raw [16]byte
	if _, err := rand.Read(raw[:]); err != nil {
		panic("cryptographic identifier source unavailable")
	}
	return prefix + hex.EncodeToString(raw[:])
}
func stamp(t time.Time) string { return t.UTC().Format(time.RFC3339Nano) }
func validStamp(value string) bool {
	_, err := time.Parse(time.RFC3339Nano, value)
	return len(value) <= 40 && err == nil
}
func copySnapshot(s Snapshot) Snapshot {
	if s.Coverage != nil {
		v := *s.Coverage
		if v.Sitemap != nil {
			m := *v.Sitemap
			v.Sitemap = &m
		}
		s.Coverage = &v
	}
	s.Pages = append([]Page{}, s.Pages...)
	for i, p := range s.Pages {
		p.Links = append([]string{}, p.Links...)
		p.Checks = append([]Check{}, p.Checks...)
		p.SEO = copySEO(p.SEO)
		s.Pages[i] = p
	}
	s.Findings = append([]Finding{}, s.Findings...)
	for i, f := range s.Findings {
		f.RelatedURLs = append([]string{}, f.RelatedURLs...)
		s.Findings[i] = f
	}
	s.History = append([]Receipt{}, s.History...)
	return s
}
