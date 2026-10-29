package siteaudit

// Comparisons are limited to recorded public-page observations, never a site-
// wide health score. Proof fields always identify the actual capture, including
// when a newer failed/omitted check makes its current state unknown.
type ChangeCounts struct {
	New       int `json:"new"`
	Unchanged int `json:"unchanged"`
	Resolved  int `json:"resolved"`
	Regressed int `json:"regressed"`
	Unknown   int `json:"unknown"`
}
type InsightItem struct {
	PageURL      string   `json:"pageUrl"`
	RuleID       string   `json:"ruleId"`
	Status       string   `json:"status"`
	CheckStatus  string   `json:"checkStatus"`
	Severity     string   `json:"severity"`
	ProofAuditID string   `json:"proofAuditId"`
	ObservedAt   string   `json:"observedAt"`
	RunID        string   `json:"runId"`
	Fingerprint  string   `json:"fingerprint"`
	RelatedURLs  []string `json:"relatedUrls"`
}
type InsightAlert struct {
	Kind       string `json:"kind"`
	AuditID    string `json:"auditId"`
	PageURL    string `json:"pageUrl"`
	RuleID     string `json:"ruleId"`
	Reason     string `json:"reason"`
	ObservedAt string `json:"observedAt"`
}
type InsightsSnapshot struct {
	WebsiteID           string         `json:"websiteId"`
	Site                string         `json:"site"`
	WebsiteRevision     int64          `json:"websiteRevision"`
	Source              string         `json:"source"`
	AsOf                string         `json:"asOf"`
	TrackingSince       string         `json:"trackingSince"`
	Partial             bool           `json:"partial"`
	HistoryLimited      bool           `json:"historyLimited"`
	Truncated           bool           `json:"truncated"`
	ComparisonAvailable bool           `json:"comparisonAvailable"`
	LatestAuditID       string         `json:"latestAuditId"`
	LatestStatus        string         `json:"latestStatus"`
	Summary             ChangeCounts   `json:"summary"`
	Items               []InsightItem  `json:"items"`
	Alerts              []InsightAlert `json:"alerts"`
	AvailableWeeks      []string       `json:"availableWeeks"`
}
type WeeklyAudits struct {
	Total    int `json:"total"`
	Complete int `json:"complete"`
	Failed   int `json:"failed"`
}
type WeeklyPages struct {
	Attempted int `json:"attempted"`
	Checked   int `json:"checked"`
	Failed    int `json:"failed"`
}
type WeeklyReceipt struct {
	AuditID      string `json:"auditId"`
	Status       string `json:"status"`
	StartedAt    string `json:"startedAt"`
	CompletedAt  string `json:"completedAt"`
	CheckedPages int    `json:"checkedPages"`
	FailedPages  int    `json:"failedPages"`
	FindingCount int    `json:"findingCount"`
	Reason       string `json:"reason,omitempty"`
}
type WeeklyReport struct {
	WebsiteID       string          `json:"websiteId"`
	Site            string          `json:"site"`
	WebsiteRevision int64           `json:"websiteRevision"`
	Source          string          `json:"source"`
	AsOf            string          `json:"asOf"`
	TrackingSince   string          `json:"trackingSince"`
	Partial         bool            `json:"partial"`
	HistoryLimited  bool            `json:"historyLimited"`
	Truncated       bool            `json:"truncated"`
	WeekStart       string          `json:"weekStart"`
	EndExclusive    string          `json:"endExclusive"`
	Status          string          `json:"status"`
	Observed        bool            `json:"observed"`
	Audits          WeeklyAudits    `json:"audits"`
	Pages           WeeklyPages     `json:"pages"`
	FailedChecks    int             `json:"failedChecks"`
	Changes         ChangeCounts    `json:"changes"`
	Receipts        []WeeklyReceipt `json:"receipts"`
	AvailableWeeks  []string        `json:"availableWeeks"`
}

const maxInsightSites = 64
const maxConditions = 2000
const maxInsightWeeks = 8
const maxWeeklyReceipts = 20
const maxReceiptIDs = 128

type conditionRecord struct {
	InsightItem
	FinalURL            string   `json:"finalUrl"`
	SeenIssue           bool     `json:"seenIssue"`
	Unresolved          bool     `json:"unresolved"`
	EverPassed          bool     `json:"everPassed"`
	DependencyLimited   bool     `json:"dependencyLimited,omitempty"`
	PendingDependencies []string `json:"pendingDependencies,omitempty"`
}
type weekRecord struct {
	WeekStart    string          `json:"weekStart"`
	Audits       WeeklyAudits    `json:"audits"`
	Pages        WeeklyPages     `json:"pages"`
	FailedChecks int             `json:"failedChecks"`
	Changes      ChangeCounts    `json:"changes"`
	Receipts     []WeeklyReceipt `json:"receipts"`
	Truncated    bool            `json:"truncated"`
}
type insightRecord struct {
	WebsiteID           string            `json:"websiteId"`
	Site                string            `json:"site"`
	WebsiteRevision     int64             `json:"websiteRevision"`
	RuleVersion         string            `json:"ruleVersion"`
	TrackingSince       string            `json:"trackingSince"`
	HistoryLimited      bool              `json:"historyLimited"`
	Truncated           bool              `json:"truncated"`
	ComparisonAvailable bool              `json:"comparisonAvailable"`
	LatestAuditID       string            `json:"latestAuditId"`
	LatestStatus        string            `json:"latestStatus"`
	LatestStartedAt     string            `json:"latestStartedAt"`
	LatestCompletedAt   string            `json:"latestCompletedAt"`
	LatestReason        string            `json:"latestReason"`
	LatestCoverageGap   bool              `json:"latestCoverageGap"`
	Conditions          []conditionRecord `json:"conditions"`
	Weeks               []weekRecord      `json:"weeks"`
	Processed           []string          `json:"processed"`
}
