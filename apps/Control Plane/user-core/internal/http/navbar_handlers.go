package http

import (
	"encoding/json"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

type navbarCalendarEvent struct {
	ID        string `json:"id"`
	Title     string `json:"title"`
	Start     string `json:"start"`
	End       string `json:"end"`
	Type      string `json:"type"`
	Status    string `json:"status"`
	CreatedAt string `json:"createdAt"`
}

type navbarCalendarNote struct {
	ID        string `json:"id"`
	Text      string `json:"text"`
	Date      string `json:"date"`
	CreatedAt string `json:"createdAt"`
}

type navbarCalendarState struct {
	Events []navbarCalendarEvent `json:"events"`
	Notes  []navbarCalendarNote  `json:"notes"`
}

type createNavbarCalendarEventRequest struct {
	Title string `json:"title"`
	Start string `json:"start"`
	End   string `json:"end"`
	Type  string `json:"type"`
}

type createNavbarCalendarNoteRequest struct {
	Text string `json:"text"`
	Date string `json:"date"`
}

type createNavbarSupportRequest struct {
	Subject string `json:"subject"`
	Message string `json:"message"`
	Context string `json:"context"`
}

type navbarSupportRequest struct {
	ID        string `json:"id"`
	Subject   string `json:"subject"`
	Message   string `json:"message"`
	Context   string `json:"context"`
	Status    string `json:"status"`
	CreatedAt string `json:"createdAt"`
}

func decodeCalendarState(settings map[string]any) navbarCalendarState {
	state := navbarCalendarState{Events: []navbarCalendarEvent{}, Notes: []navbarCalendarNote{}}
	data, err := json.Marshal(settings)
	if err != nil {
		return state
	}
	_ = json.Unmarshal(data, &state)
	return state
}

func decodeSupportRequests(settings map[string]any) []navbarSupportRequest {
	out := struct {
		Requests []navbarSupportRequest `json:"requests"`
	}{Requests: []navbarSupportRequest{}}
	data, err := json.Marshal(settings)
	if err != nil {
		return out.Requests
	}
	_ = json.Unmarshal(data, &out)
	return out.Requests
}

func trimBounded(value string, max int) string {
	value = strings.TrimSpace(value)
	if len(value) <= max {
		return value
	}
	return value[:max]
}

func parseOptionalRFC3339(value string) bool {
	if strings.TrimSpace(value) == "" {
		return false
	}
	_, err := time.Parse(time.RFC3339, value)
	return err == nil
}

func (s *Server) listNavbarCalendarState(c *gin.Context) {
	userID, ok := getUserIDFromContext(c)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "calendar", map[string]any{
		"events": []any{},
		"notes":  []any{},
	})
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to get calendar settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve calendar"})
		return
	}

	c.JSON(http.StatusOK, decodeCalendarState(settings))
}

func (s *Server) createNavbarCalendarEvent(c *gin.Context) {
	userID, ok := getUserIDFromContext(c)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	var req createNavbarCalendarEventRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	title := trimBounded(req.Title, 160)
	if title == "" || !parseOptionalRFC3339(req.Start) {
		c.JSON(http.StatusBadRequest, gin.H{"error": "title and valid start are required"})
		return
	}

	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "calendar", map[string]any{
		"events": []any{},
		"notes":  []any{},
	})
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to read calendar before append")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save calendar event"})
		return
	}

	state := decodeCalendarState(settings)
	event := navbarCalendarEvent{
		ID:        "cal_" + newCorrelationID(),
		Title:     title,
		Start:     strings.TrimSpace(req.Start),
		End:       strings.TrimSpace(req.End),
		Type:      trimBounded(req.Type, 32),
		Status:    "scheduled",
		CreatedAt: time.Now().UTC().Format(time.RFC3339),
	}
	if event.End == "" {
		event.End = event.Start
	}
	if event.Type == "" {
		event.Type = "event"
	}

	nextState := navbarCalendarState{
		Events: append([]navbarCalendarEvent{event}, state.Events...),
		Notes:  state.Notes,
	}
	if len(nextState.Events) > 100 {
		nextState.Events = nextState.Events[:100]
	}

	if _, err := s.userService.UpsertSettings(c.Request.Context(), userID, "calendar", map[string]any{
		"events": nextState.Events,
		"notes":  nextState.Notes,
	}); err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to persist calendar event")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save calendar event"})
		return
	}

	c.JSON(http.StatusCreated, gin.H{"event": event})
}

func (s *Server) createNavbarCalendarNote(c *gin.Context) {
	userID, ok := getUserIDFromContext(c)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	var req createNavbarCalendarNoteRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	text := trimBounded(req.Text, 600)
	if text == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "note text is required"})
		return
	}

	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "calendar", map[string]any{
		"events": []any{},
		"notes":  []any{},
	})
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to read calendar notes before append")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save calendar note"})
		return
	}

	state := decodeCalendarState(settings)
	date := strings.TrimSpace(req.Date)
	if date == "" {
		date = time.Now().UTC().Format("2006-01-02")
	}
	note := navbarCalendarNote{
		ID:        "note_" + newCorrelationID(),
		Text:      text,
		Date:      date,
		CreatedAt: time.Now().UTC().Format(time.RFC3339),
	}

	nextState := navbarCalendarState{
		Events: state.Events,
		Notes:  append([]navbarCalendarNote{note}, state.Notes...),
	}
	if len(nextState.Notes) > 100 {
		nextState.Notes = nextState.Notes[:100]
	}

	if _, err := s.userService.UpsertSettings(c.Request.Context(), userID, "calendar", map[string]any{
		"events": nextState.Events,
		"notes":  nextState.Notes,
	}); err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to persist calendar note")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save calendar note"})
		return
	}

	c.JSON(http.StatusCreated, gin.H{"note": note})
}

func (s *Server) createNavbarSupportRequest(c *gin.Context) {
	userID, ok := getUserIDFromContext(c)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}

	var req createNavbarSupportRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	subject := trimBounded(req.Subject, 160)
	message := trimBounded(req.Message, 2000)
	if subject == "" || message == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "subject and message are required"})
		return
	}

	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "support", map[string]any{
		"requests": []any{},
	})
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to read support requests before append")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save support request"})
		return
	}

	requests := decodeSupportRequests(settings)
	request := navbarSupportRequest{
		ID:        "support_" + newCorrelationID(),
		Subject:   subject,
		Message:   message,
		Context:   trimBounded(req.Context, 200),
		Status:    "open",
		CreatedAt: time.Now().UTC().Format(time.RFC3339),
	}

	nextRequests := append([]navbarSupportRequest{request}, requests...)
	if len(nextRequests) > 100 {
		nextRequests = nextRequests[:100]
	}

	if _, err := s.userService.UpsertSettings(c.Request.Context(), userID, "support", map[string]any{
		"requests": nextRequests,
	}); err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to persist support request")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save support request"})
		return
	}

	c.JSON(http.StatusCreated, gin.H{"request": request})
}
