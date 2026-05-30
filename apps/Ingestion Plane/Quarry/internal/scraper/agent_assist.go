package scraper

import (
	"context"
	"strings"

	zlog "github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/ai"
	"github.com/triodelab/quarry/internal/driver"
)

const (
	// agentMaxSteps is the upper bound on AI-driven interaction steps per page.
	// Keeps costs bounded and prevents infinite loops on adversarial pages.
	agentMaxSteps = 5
	// agentMinContentLen is the minimum visible-text length that ends the agent
	// loop early — once we have enough content, there's no need for more steps.
	agentMinContentLen = 500
	// agentSnapshotMaxBytes caps the HTML snapshot sent to ai-core per step.
	// LLMs don't need the full DOM; a trimmed snapshot is faster and cheaper.
	agentSnapshotMaxBytes = 12_000
)

// agentAssistedFetch runs an AI-driven browser interaction loop to extract
// content from pages that remain empty or insufficient after normal rendering.
//
// It uses ai-core's AgentNavigate method (Model Plane v2) to decide what
// browser action to take next (click, type, scroll, wait, navigate) and
// executes that action through the driver.PageDriver interface.
//
// Returns the final HTML after the agent loop completes or the content
// threshold is met. Falls back to the initial html on any unrecoverable error.
func agentAssistedFetch(
	ctx context.Context,
	drv driver.PageDriver,
	aiClient ai.AIClient,
	targetURL string,
	goal string, // e.g. "extract main article content" or user-provided prompt
	schema string, // JSON schema hint, may be empty
	orgID string,
	initialHTML string,
) string {
	if aiClient == nil || drv == nil {
		return initialHTML
	}
	if goal == "" {
		goal = "Extract the main content from this page. Click through any necessary navigation (cookie banners, load-more buttons, tab switches) to reveal the primary content."
	}

	visitedURLs := []string{targetURL}
	currentHTML := initialHTML

	for step := 1; step <= agentMaxSteps; step++ {
		if ctx.Err() != nil {
			break
		}

		// Refresh HTML from the live page at each step.
		if liveHTML, err := drv.HTML(ctx); err == nil && strings.TrimSpace(liveHTML) != "" {
			currentHTML = liveHTML
		}

		// Early exit: enough content is already present.
		if !isContentInsufficient(currentHTML) && len(strings.TrimSpace(visibleText(currentHTML))) >= agentMinContentLen {
			zlog.Debug().
				Str("url", targetURL).
				Int("step", step).
				Msg("agent_assist: sufficient content reached, stopping early")
			break
		}

		snapshot := truncateSnapshot(currentHTML, agentSnapshotMaxBytes)

		resp, err := aiClient.AgentNavigate(ctx, &ai.AgentNavigateRequest{
			Goal:         goal,
			StepNumber:   step,
			MaxSteps:     agentMaxSteps,
			CurrentURL:   targetURL,
			VisitedURLs:  visitedURLs,
			PageSnapshot: snapshot,
			Schema:       schema,
			OrgID:        orgID,
		})
		if err != nil {
			zlog.Warn().Err(err).Str("url", targetURL).Int("step", step).Msg("agent_assist: AgentNavigate failed")
			break
		}

		zlog.Debug().
			Str("url", targetURL).
			Int("step", step).
			Str("action", resp.Action.Type).
			Str("selector", resp.Action.Selector).
			Str("reasoning", resp.Reasoning).
			Msg("agent_assist: executing action")

		if resp.IsComplete {
			break
		}

		// Execute the recommended action through the driver.
		actionErr := executeAgentAction(ctx, drv, resp.Action)
		if actionErr != nil {
			zlog.Warn().Err(actionErr).
				Str("url", targetURL).
				Str("action", resp.Action.Type).
				Msg("agent_assist: action failed (continuing)")
		}

		// Brief stabilization pause (800 ms) after each action so the page can settle.
		_ = drv.Wait(ctx, 800)
	}

	// Final HTML read.
	if liveHTML, err := drv.HTML(ctx); err == nil && strings.TrimSpace(liveHTML) != "" {
		return liveHTML
	}
	return currentHTML
}

// executeAgentAction maps an AgentNavigate action to a PageDriver call.
func executeAgentAction(ctx context.Context, drv driver.PageDriver, action ai.NavigationAction) error {
	switch strings.ToLower(strings.TrimSpace(action.Type)) {
	case "click":
		if action.Selector != "" {
			return drv.Click(ctx, action.Selector)
		}
	case "type":
		if action.Selector != "" && action.Value != "" {
			return drv.Type(ctx, action.Selector, action.Value)
		}
	case "press":
		key := action.Value
		if key == "" {
			key = "Enter"
		}
		return drv.Press(ctx, key)
	case "scroll":
		dir := action.Value
		if dir == "" {
			dir = "down"
		}
		return drv.Scroll(ctx, dir)
	case "wait":
		ms := int(action.WaitMs)
		if ms <= 0 || ms > 5000 {
			ms = 1000
		}
		return drv.Wait(ctx, ms)
	}
	return nil
}

// truncateSnapshot trims the HTML to maxBytes for the AI snapshot, preferring
// to keep the <body> content rather than the <head>.
func truncateSnapshot(html string, maxBytes int) string {
	if len(html) <= maxBytes {
		return html
	}
	// Try to start from <body>.
	if idx := strings.Index(strings.ToLower(html), "<body"); idx >= 0 {
		body := html[idx:]
		if len(body) <= maxBytes {
			return body
		}
		return body[:maxBytes]
	}
	return html[:maxBytes]
}

// visibleText strips HTML tags to get a rough visible-text string.
// Used only for length comparisons, not for output.
func visibleText(html string) string {
	var b strings.Builder
	inTag := false
	for _, r := range html {
		switch {
		case r == '<':
			inTag = true
		case r == '>':
			inTag = false
		case !inTag:
			b.WriteRune(r)
		}
	}
	return b.String()
}
