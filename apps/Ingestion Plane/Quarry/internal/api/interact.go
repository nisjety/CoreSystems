package api

import (
	"context"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/gofiber/contrib/websocket"
	"github.com/gofiber/fiber/v2"
	zlog "github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/ai"
	quarrybrowser "github.com/triodelab/quarry/internal/browser"
	"github.com/triodelab/quarry/internal/session"
)

// ---------- Request / Response types ----------

type V2ScrapeRequest struct {
	URL      string                  `json:"url"`
	Viewport *session.ViewportConfig `json:"viewport,omitempty"`
	Mobile   bool                    `json:"mobile,omitempty"`
}

type V2ScrapeResponse struct {
	Success   bool                `json:"success"`
	ScrapeID  string              `json:"scrape_id"`
	URL       string              `json:"url"`
	HTML      string              `json:"html,omitempty"`
	Session   session.SessionInfo `json:"session"`
	RequestID string              `json:"request_id,omitempty"`
}

type InteractRequest struct {
	// Either explicit actions OR a natural-language prompt (AI picks the actions).
	Actions []session.Action `json:"actions,omitempty"`
	Prompt  string           `json:"prompt,omitempty"`

	// AI-assisted navigation
	Goal            string `json:"goal,omitempty"`
	Schema          string `json:"schema,omitempty"` // extraction schema
	ContinueOnError bool   `json:"continue_on_error,omitempty"`
	MaxSteps        int    `json:"max_steps,omitempty"` // for AI navigation loop

	// If true, return page HTML after executing all actions.
	ReturnHTML bool `json:"return_html,omitempty"`
}

type InteractResponse struct {
	Success   bool                   `json:"success"`
	ScrapeID  string                 `json:"scrape_id"`
	Results   []session.ActionResult `json:"results"`
	HTML      string                 `json:"html,omitempty"`
	Session   session.SessionInfo    `json:"session"`
	RequestID string                 `json:"request_id,omitempty"`
}

type SessionListResponse struct {
	Success  bool                  `json:"success"`
	Sessions []session.SessionInfo `json:"sessions"`
	Count    int                   `json:"count"`
}

// ---------- Route Registration ----------

func (h *Handler) RegisterV2(app *fiber.App) {
	v2 := app.Group("/v2")

	// Session-based scraping
	v2.Post("/scrape", h.v2Scrape)
	v2.Post("/scrape/:scrapeId/interact", h.v2Interact)
	v2.Get("/scrape/:scrapeId", h.v2SessionInfo)
	v2.Delete("/scrape/:scrapeId", h.v2DestroySession)
	v2.Get("/sessions", h.v2ListSessions)

	// Phase 4: enhanced extract + search
	h.registerV2ExtractAndSearch(v2)

	// Phase 4: crawl enhancements
	h.registerV2Crawl(v2)

	// Agent LiveCast — FC SDK compatible WS endpoint
	v2.Get("/agent-livecast", h.ensureWebsocketUpgrade, websocket.New(h.agentLivecastWS))
}

// ---------- POST /v2/scrape ----------

func (h *Handler) v2Scrape(c *fiber.Ctx) error {
	var req V2ScrapeRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}

	req.URL = strings.TrimSpace(req.URL)
	if req.URL == "" {
		return writeError(c, http.StatusBadRequest, "url is required", nil)
	}

	if !isValidURL(req.URL) {
		return writeError(c, http.StatusBadRequest, "invalid url format", nil)
	}

	if h.browserRuntime == nil {
		return writeError(c, http.StatusServiceUnavailable, "browser runtime not initialized", nil)
	}

	ctx, cancel := context.WithTimeout(c.UserContext(), 30*time.Second)
	defer cancel()

	// Security check
	if h.security != nil {
		assessment, err := h.security.AssessURL(ctx, req.URL)
		if err == nil && assessment != nil && assessment.Blocked {
			return writeError(c, http.StatusForbidden, "url blocked by security policy", map[string]interface{}{
				"reason": assessment.BlockReason,
			})
		}
	}

	resp, err := h.browserRuntime.Create(ctx, quarrybrowser.CreateRequest{
		URL:      req.URL,
		Viewport: req.Viewport,
		Mobile:   req.Mobile,
	})
	if err != nil {
		zlog.Error().Err(err).Str("url", req.URL).Msg("v2: failed to create session")
		return writeError(c, http.StatusInternalServerError, fmt.Sprintf("session creation failed: %v", err), nil)
	}
	state := resp.State

	// Publish event
	if h.sharedPublisher != nil {
		_ = h.sharedPublisher.PublishCrawlStarted(ctx, "", req.URL, state.Session.ID, map[string]interface{}{
			"api_version": "v2",
			"interactive": true,
		})
	}

	requestID, _ := c.Locals("requestid").(string)
	return c.Status(http.StatusCreated).JSON(V2ScrapeResponse{
		Success:   true,
		ScrapeID:  state.Session.ID,
		URL:       req.URL,
		HTML:      truncateHTML(resp.HTML, 50000),
		Session:   sessionInfoFromState(&state),
		RequestID: requestID,
	})
}

// ---------- POST /v2/scrape/:scrapeId/interact ----------

func (h *Handler) v2Interact(c *fiber.Ctx) error {
	scrapeID := c.Params("scrapeId")
	if scrapeID == "" {
		return writeError(c, http.StatusBadRequest, "scrape_id is required", nil)
	}

	if h.browserRuntime == nil {
		return writeError(c, http.StatusServiceUnavailable, "browser runtime not initialized", nil)
	}

	var req InteractRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}

	ctx, cancel := context.WithTimeout(c.UserContext(), 90*time.Second)
	defer cancel()

	results, state, err := h.runBrowserInteraction(ctx, scrapeID, &req)
	if err != nil {
		status := mapBrowserInteractionError(err)
		if status == http.StatusInternalServerError {
			zlog.Warn().Err(err).Str("session_id", scrapeID).Msg("v2: interaction failed")
		}
		return writeError(c, status, err.Error(), nil)
	}

	var html string
	if req.ReturnHTML {
		htmlResp, htmlErr := h.browserRuntime.HTML(ctx, scrapeID)
		if htmlErr == nil {
			html = truncateHTML(htmlResp.HTML, 50000)
			if state == nil {
				current := sessionInfoFromState(&quarrybrowser.SessionState{Session: session.SessionInfo{ID: scrapeID}, CurrentURL: htmlResp.CurrentURL})
				state = &quarrybrowser.SessionState{Session: current, CurrentURL: htmlResp.CurrentURL}
			}
		}
	}

	requestID, _ := c.Locals("requestid").(string)
	return c.JSON(InteractResponse{
		Success:   true,
		ScrapeID:  scrapeID,
		Results:   results,
		HTML:      html,
		Session:   sessionInfoFromState(state),
		RequestID: requestID,
	})
}

// ---------- GET /v2/scrape/:scrapeId ----------

func (h *Handler) v2SessionInfo(c *fiber.Ctx) error {
	scrapeID := c.Params("scrapeId")

	if h.browserRuntime == nil {
		return writeError(c, http.StatusServiceUnavailable, "browser runtime not initialized", nil)
	}

	state, err := h.browserRuntime.Get(c.UserContext(), scrapeID)
	if err != nil {
		return writeError(c, http.StatusNotFound, err.Error(), nil)
	}

	return c.JSON(fiber.Map{
		"success": true,
		"session": sessionInfoFromState(state),
	})
}

// ---------- DELETE /v2/scrape/:scrapeId ----------

func (h *Handler) v2DestroySession(c *fiber.Ctx) error {
	scrapeID := c.Params("scrapeId")

	if h.browserRuntime == nil {
		return writeError(c, http.StatusServiceUnavailable, "browser runtime not initialized", nil)
	}

	if err := h.browserRuntime.Delete(c.UserContext(), scrapeID); err != nil {
		return writeError(c, http.StatusNotFound, err.Error(), nil)
	}
	return c.JSON(fiber.Map{
		"success": true,
		"message": "session destroyed",
	})
}

// ---------- GET /v2/sessions ----------

func (h *Handler) v2ListSessions(c *fiber.Ctx) error {
	if h.browserRuntime == nil {
		return writeError(c, http.StatusServiceUnavailable, "browser runtime not initialized", nil)
	}

	sessions, err := h.browserRuntime.List(c.UserContext())
	if err != nil {
		return writeError(c, http.StatusServiceUnavailable, err.Error(), nil)
	}
	return c.JSON(SessionListResponse{
		Success:  true,
		Sessions: sessions,
		Count:    len(sessions),
	})
}

// ---------- AI Interaction ----------

func (h *Handler) aiInteract(ctx context.Context, sessionID string, req *InteractRequest) ([]session.ActionResult, *quarrybrowser.SessionState, error) {
	if h.scraper == nil || h.browserRuntime == nil {
		return nil, nil, fmt.Errorf("browser AI interaction is not initialized")
	}

	aiClient := h.scraper.AIClient()
	if aiClient == nil {
		return nil, nil, fmt.Errorf("AI client not available")
	}

	goal := req.Goal
	if goal == "" {
		goal = req.Prompt
	}

	maxSteps := req.MaxSteps
	if maxSteps <= 0 {
		maxSteps = 10
	}
	if maxSteps > 30 {
		maxSteps = 30
	}

	state, err := h.browserRuntime.Get(ctx, sessionID)
	if err != nil {
		return nil, nil, err
	}

	currentURL := state.CurrentURL
	if currentURL == "" {
		currentURL = state.Session.URL
	}
	visitedURLs := appendUniqueURL(nil, currentURL)
	var allResults []session.ActionResult

	for step := 0; step < maxSteps; step++ {
		if err := ctx.Err(); err != nil {
			allResults = append(allResults, session.ActionResult{
				Type: session.ActionDone, Success: false, Error: err.Error(),
			})
			break
		}

		// Get current page snapshot (truncated HTML).
		htmlResp, err := h.browserRuntime.HTML(ctx, sessionID)
		if err != nil {
			allResults = append(allResults, session.ActionResult{
				Type: session.ActionExtract, Success: false, Error: err.Error(),
			})
			break
		}
		currentURL = htmlResp.CurrentURL
		if currentURL == "" {
			currentURL = state.Session.URL
		}

		// Ask AI what to do next.
		navResp, err := aiClient.AgentNavigate(ctx, &ai.AgentNavigateRequest{
			PageSnapshot: truncateHTML(htmlResp.HTML, 30000),
			CurrentURL:   currentURL,
			Goal:         goal,
			VisitedURLs:  visitedURLs,
			Schema:       req.Schema,
			StepNumber:   step + 1,
			MaxSteps:     maxSteps,
		})
		if err != nil {
			allResults = append(allResults, session.ActionResult{
				Type: "agent_navigate", Success: false, Error: fmt.Sprintf("AI navigate failed: %v", err),
			})
			break
		}

		// Convert AI action to session action.
		action := session.Action{
			Type:     session.ActionType(navResp.Action.Type),
			Selector: navResp.Action.Selector,
			Value:    navResp.Action.Value,
			WaitMs:   int(navResp.Action.WaitMs),
		}

		execResp, execErr := h.browserRuntime.Execute(ctx, sessionID, quarrybrowser.ExecuteRequest{
			Actions: []session.Action{action},
		})
		if execErr != nil {
			allResults = append(allResults, session.ActionResult{
				Type: action.Type, Success: false, Error: execErr.Error(),
			})
			break
		}

		state = &execResp.State
		result := session.ActionResult{Type: action.Type, Success: true}
		if len(execResp.Results) > 0 {
			result = execResp.Results[0]
		}
		result.Data = navResp.Reasoning // attach AI reasoning to the result
		allResults = append(allResults, result)

		// Track visited URLs.
		if action.Type == session.ActionNavigate && action.Value != "" {
			visitedURLs = appendUniqueURL(visitedURLs, action.Value)
		}
		if state != nil {
			if state.CurrentURL != "" {
				currentURL = state.CurrentURL
			} else if state.Session.URL != "" {
				currentURL = state.Session.URL
			}
			visitedURLs = appendUniqueURL(visitedURLs, currentURL)
		}

		// Check if AI says we're done.
		if navResp.IsComplete {
			if navResp.ExtractedData != "" {
				allResults = append(allResults, session.ActionResult{
					Type: session.ActionExtract, Success: true, Data: navResp.ExtractedData,
				})
			}
			break
		}

		// Brief pause between AI steps to avoid hammering.
		if action.WaitMs <= 0 {
			select {
			case <-ctx.Done():
			case <-time.After(500 * time.Millisecond):
			}
		}
	}

	finalState, finalErr := h.browserRuntime.Get(ctx, sessionID)
	if finalErr == nil {
		state = finalState
	}
	return allResults, state, nil
}

// ---------- Helpers ----------

func truncateHTML(html string, maxLen int) string {
	if len(html) <= maxLen {
		return html
	}
	return html[:maxLen] + "\n... [truncated]"
}

func isValidURL(u string) bool {
	return strings.HasPrefix(u, "http://") || strings.HasPrefix(u, "https://")
}

func appendUniqueURL(urls []string, candidate string) []string {
	candidate = strings.TrimSpace(candidate)
	if candidate == "" {
		return urls
	}
	for _, existing := range urls {
		if existing == candidate {
			return urls
		}
	}
	return append(urls, candidate)
}

func mapBrowserInteractionError(err error) int {
	if err == nil {
		return http.StatusOK
	}
	message := strings.ToLower(strings.TrimSpace(err.Error()))
	switch {
	case strings.Contains(message, "not found"), strings.Contains(message, "expired"), strings.Contains(message, "inactive"):
		return http.StatusNotFound
	case strings.Contains(message, "either 'actions'"), strings.Contains(message, "request is required"):
		return http.StatusBadRequest
	default:
		return http.StatusInternalServerError
	}
}
