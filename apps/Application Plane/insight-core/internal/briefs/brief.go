// Package briefs assembles a daily brief from insight-core's own real per-org
// metric rollups and delivers it through the EXISTING notification-core Novu
// adapter (this package does NOT talk to Novu directly — it POSTs a
// `daily_brief` notification request and notification-core triggers the Novu
// workflow). The brief carries the SAME Preview gate the gateway briefs surface
// uses (apps/.../gateway/src/domains/briefs.rs): below a minimum number of real
// recorded events the brief is labelled `preview` with an honest disclosure and
// shows only the real counts collected so far — never a fabricated trend.
package briefs

import (
	"fmt"
	"sort"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

// BriefMinEvents mirrors gateway briefs.rs BRIEF_MIN_EVENTS: at least this many
// real recorded metric events before a brief is `live` rather than `preview`.
const BriefMinEvents = 5

// PreviewDisclosure mirrors gateway briefs.rs PREVIEW_DISCLOSURE so the in-app
// + email surfaces and the gateway surface tell the user the same honest story.
const PreviewDisclosure = "Preview — not enough recorded activity to brief on yet. Showing the real counts collected so far; no trends are inferred until more events accrue."

const (
	// StateLive labels a brief whose recorded activity meets the gate.
	StateLive = "live"
	// StatePreview labels a brief below the gate — real counts only, no trends.
	StatePreview = "preview"

	// WorkflowDailyBrief is the notification-core request `type`, which is also
	// the Novu WorkflowID the adapter triggers (runtime/client.go: req.Type →
	// Novu WorkflowID). Registering the brief = using this canonical identifier
	// and the in_app+email channel set below; Novu workflow templates live in the
	// Novu dashboard, not in code (do NOT build Novu).
	WorkflowDailyBrief = "daily_brief"
)

// Brief is the assembled per-org daily brief. It is the structured shape that
// becomes the notification payload; the Preview gate (State + Disclosure) is
// always present so every channel renders the honesty label.
type Brief struct {
	OrgID       string         `json:"org_id"`
	State       string         `json:"state"`
	Title       string         `json:"title"`
	Body        string         `json:"body"`
	Disclosure  string         `json:"disclosure,omitempty"`
	TotalEvents int            `json:"total_events"`
	Surfaces    []SurfaceCount `json:"surfaces"`
	// Sources cites the real producer services behind the brief's counts
	// (e.g. ["conversation-core", "social-core"]), distinct and sorted. Empty
	// when nothing was attributed — never a fabricated source.
	Sources []string `json:"sources,omitempty"`
}

// SurfaceCount is one surface's real recorded total in the brief window.
type SurfaceCount struct {
	Surface     string `json:"surface"`
	TotalEvents int    `json:"total_events"`
	// Source cites the real producer(s) behind this surface's counts, threaded
	// from the overview scorecards. Empty when unattributed — never fabricated.
	Source string `json:"source,omitempty"`
}

// AssembleBrief builds a brief from an insight-core overview. It sums the real
// per-surface total_events; below BriefMinEvents the brief is `preview` with the
// disclosure. It NEVER invents counts — an empty overview yields a preview over
// zero, the honest empty-state. Pure, so the gate is unit-testable.
func AssembleBrief(overview *insights.Overview) Brief {
	brief := Brief{State: StatePreview, Surfaces: []SurfaceCount{}}
	if overview == nil {
		brief.Title = "Daily brief (preview)"
		brief.Body = "No recorded activity yet."
		brief.Disclosure = PreviewDisclosure
		return brief
	}
	brief.OrgID = overview.OrgID

	// Citation: gather the real producer(s) per surface from the overview
	// scorecards (which carry MetricRollup.Source). Never fabricated — a surface
	// with no attributed scorecard simply has an empty source.
	sourcesBySurface := sourcesBySurfaceFrom(overview.Scorecards)
	allSources := map[string]struct{}{}

	total := 0
	for _, surface := range overview.Surfaces {
		if surface.TotalEvents <= 0 {
			continue
		}
		total += surface.TotalEvents
		source := joinSources(sourcesBySurface[surface.Surface])
		brief.Surfaces = append(brief.Surfaces, SurfaceCount{
			Surface:     surface.Surface,
			TotalEvents: surface.TotalEvents,
			Source:      source,
		})
		for src := range sourcesBySurface[surface.Surface] {
			allSources[src] = struct{}{}
		}
	}
	brief.TotalEvents = total
	brief.Sources = sortedSources(allSources)

	if total >= BriefMinEvents {
		brief.State = StateLive
		brief.Title = "Daily brief"
		brief.Body = fmt.Sprintf("%d recorded events across %d active surface(s).", total, len(brief.Surfaces))
		return brief
	}

	brief.State = StatePreview
	brief.Title = "Daily brief (preview)"
	brief.Body = fmt.Sprintf("%d recorded event(s) so far.", total)
	brief.Disclosure = PreviewDisclosure
	return brief
}

// sourcesBySurfaceFrom collects the distinct, non-empty producer sources per
// surface from the overview scorecards. The map values are sets so the same
// producer is never double-counted across metrics on one surface.
func sourcesBySurfaceFrom(scorecards []insights.Scorecard) map[string]map[string]struct{} {
	bySurface := map[string]map[string]struct{}{}
	for _, card := range scorecards {
		source := card.Source
		if source == "" {
			continue
		}
		if bySurface[card.Surface] == nil {
			bySurface[card.Surface] = map[string]struct{}{}
		}
		// Scorecard.Source may already be a joined "a, b" string; split on the
		// same separator so each producer is a distinct set member.
		for _, part := range splitSources(source) {
			bySurface[card.Surface][part] = struct{}{}
		}
	}
	return bySurface
}

// joinSources renders a producer set as a stable, comma-separated citation.
// Empty when the set is empty — the honest "unattributed" state.
func joinSources(sources map[string]struct{}) string {
	return strings.Join(sortedSources(sources), ", ")
}

// sortedSources returns the set's members as a sorted slice (deterministic).
func sortedSources(sources map[string]struct{}) []string {
	if len(sources) == 0 {
		return nil
	}
	list := make([]string, 0, len(sources))
	for source := range sources {
		list = append(list, source)
	}
	sort.Strings(list)
	return list
}

// splitSources splits a possibly-joined "a, b" citation back into producers,
// trimming whitespace and dropping empties.
func splitSources(joined string) []string {
	parts := []string{}
	for _, part := range strings.Split(joined, ",") {
		if trimmed := strings.TrimSpace(part); trimmed != "" {
			parts = append(parts, trimmed)
		}
	}
	return parts
}

// Payload converts the brief into the notification payload map. It carries the
// display fields notification-core's feed sink reads (title/body) plus the
// structured brief, INCLUDING the Preview gate (`state` + `preview` flag +
// disclosure) so in_app and email both render the honesty label.
func (b Brief) Payload() map[string]any {
	payload := map[string]any{
		"title":        b.Title,
		"body":         b.Body,
		"state":        b.State,
		"preview":      b.State == StatePreview,
		"total_events": b.TotalEvents,
		"surfaces":     b.Surfaces,
		"channels":     []string{"in_app", "email"},
		"org_id":       b.OrgID,
	}
	if b.Disclosure != "" {
		payload["disclosure"] = b.Disclosure
	}
	if len(b.Sources) > 0 {
		payload["sources"] = b.Sources
	}
	return payload
}
