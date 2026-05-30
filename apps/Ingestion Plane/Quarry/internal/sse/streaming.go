package sse

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"sync"
	"time"

	"github.com/triodelab/quarry/internal/jobs"
	structtypes "github.com/triodelab/quarry/internal/struct_types"
)

// EventType represents different types of SSE events
type EventType string

const (
	EventJobCreated    EventType = "job:created"
	EventJobStarted    EventType = "job:started"
	EventJobProgress   EventType = "job:progress"
	EventJobCompleted  EventType = "job:completed"
	EventJobFailed     EventType = "job:failed"
	EventCrawlStart    EventType = "crawl:start"
	EventCrawlStep     EventType = "crawl:step"
	EventCrawlData     EventType = "crawl:data"
	EventCrawlError    EventType = "crawl:error"
	EventAIAnalysis      EventType = "ai:analysis"
	EventPlanGenerated   EventType = "plan:generated"
	EventPageClassified  EventType = "page:classified"
	EventPageEnriched    EventType = "page:enriched"
	EventPageDiscovered  EventType = "page:discovered"
)

// Event represents an SSE event
type Event struct {
	Type      EventType   `json:"type"`
	Data      interface{} `json:"data"`
	Timestamp time.Time   `json:"timestamp"`
	JobID     string      `json:"job_id,omitempty"`
}

// StreamManager manages SSE connections and event broadcasting
type StreamManager struct {
	mu          sync.RWMutex
	connections map[string]map[chan Event]struct{} // jobID -> set of channels
	jobStore    *jobs.Store
}

// NewStreamManager creates a new stream manager
func NewStreamManager(jobStore *jobs.Store) *StreamManager {
	return &StreamManager{
		connections: make(map[string]map[chan Event]struct{}),
		jobStore:    jobStore,
	}
}

// Subscribe creates a new SSE connection for a job
func (sm *StreamManager) Subscribe(ctx context.Context, jobID string) (<-chan Event, func()) {
	ch := make(chan Event, 100) // Buffered to prevent blocking

	sm.mu.Lock()
	if sm.connections[jobID] == nil {
		sm.connections[jobID] = make(map[chan Event]struct{})
	}
	sm.connections[jobID][ch] = struct{}{}
	sm.mu.Unlock()

	// Cleanup function
	cleanup := func() {
		sm.mu.Lock()
		defer sm.mu.Unlock()

		if subs := sm.connections[jobID]; subs != nil {
			delete(subs, ch)
			if len(subs) == 0 {
				delete(sm.connections, jobID)
			}
		}
		close(ch)
	}

	// Send initial job state if it exists
	if job, exists := sm.jobStore.Get(jobID); exists {
		select {
		case ch <- Event{
			Type:      EventJobCreated,
			Data:      job,
			Timestamp: time.Now(),
			JobID:     jobID,
		}:
		default:
		}
	}

	return ch, cleanup
}

// Broadcast sends an event to all subscribers of a job
func (sm *StreamManager) Broadcast(jobID string, eventType EventType, data interface{}) {
	sm.mu.RLock()
	subs := sm.connections[jobID]
	sm.mu.RUnlock()

	if subs == nil {
		return
	}

	event := Event{
		Type:      eventType,
		Data:      data,
		Timestamp: time.Now(),
		JobID:     jobID,
	}

	sm.mu.RLock()
	for ch := range subs {
		select {
		case ch <- event:
		default:
			// Channel is full, skip
		}
	}
	sm.mu.RUnlock()
}

// ProgressReporter provides methods to report progress during operations
type ProgressReporter struct {
	jobID         string
	streamManager *StreamManager
	jobStore      *jobs.Store
}

// NewProgressReporter creates a new progress reporter
func NewProgressReporter(jobID string, streamManager *StreamManager, jobStore *jobs.Store) *ProgressReporter {
	return &ProgressReporter{
		jobID:         jobID,
		streamManager: streamManager,
		jobStore:      jobStore,
	}
}

// ReportProgress updates job progress and broadcasts to subscribers
func (pr *ProgressReporter) ReportProgress(progress int, message string) {
	// Update job in store
	pr.jobStore.Update(pr.jobID, func(job *jobs.Job) {
		job.Progress = progress
		if job.Status == jobs.StatusPending {
			job.Status = jobs.StatusRunning
		}
	})

	// Broadcast to subscribers
	pr.streamManager.Broadcast(pr.jobID, EventJobProgress, map[string]interface{}{
		"progress": progress,
		"message":  message,
	})
}

// ReportCrawlStart reports the start of a crawling operation
func (pr *ProgressReporter) ReportCrawlStart(url string) {
	pr.streamManager.Broadcast(pr.jobID, EventCrawlStart, map[string]interface{}{
		"url":       url,
		"timestamp": time.Now(),
	})
}

// ReportCrawlStep reports a step in the crawling process
func (pr *ProgressReporter) ReportCrawlStep(step string, details interface{}) {
	pr.streamManager.Broadcast(pr.jobID, EventCrawlStep, map[string]interface{}{
		"step":      step,
		"details":   details,
		"timestamp": time.Now(),
	})
}

// ReportCrawlData reports crawled data
func (pr *ProgressReporter) ReportCrawlData(data interface{}) {
	pr.streamManager.Broadcast(pr.jobID, EventCrawlData, map[string]interface{}{
		"data":      data,
		"timestamp": time.Now(),
	})
}

// ReportAIAnalysis reports AI analysis results
func (pr *ProgressReporter) ReportAIAnalysis(url string, analysis structtypes.AIAnalysis) {
	pr.streamManager.Broadcast(pr.jobID, EventAIAnalysis, map[string]interface{}{
		"url":       url,
		"analysis":  analysis,
		"timestamp": time.Now(),
	})
}

// ReportError reports an error during operation
func (pr *ProgressReporter) ReportError(err error) {
	// Update job in store
	pr.jobStore.Update(pr.jobID, func(job *jobs.Job) {
		job.Status = jobs.StatusFailed
		job.Error = err.Error()
	})

	// Broadcast to subscribers
	pr.streamManager.Broadcast(pr.jobID, EventCrawlError, map[string]interface{}{
		"error":     err.Error(),
		"timestamp": time.Now(),
	})
}

// ReportCompletion reports successful completion
func (pr *ProgressReporter) ReportCompletion(result interface{}) {
	// Update job in store
	pr.jobStore.Update(pr.jobID, func(job *jobs.Job) {
		job.Status = jobs.StatusReady
		job.Progress = 100
		job.Result = map[string]interface{}{
			"data": result,
		}
	})

	// Broadcast to subscribers
	pr.streamManager.Broadcast(pr.jobID, EventJobCompleted, map[string]interface{}{
		"result":    result,
		"timestamp": time.Now(),
	})
}

// WriteSSE writes an SSE event to an HTTP response writer
func WriteSSE(w http.ResponseWriter, event Event) error {
	data, err := json.Marshal(event)
	if err != nil {
		return fmt.Errorf("failed to marshal event: %w", err)
	}

	fmt.Fprintf(w, "event: %s\ndata: %s\n\n", event.Type, data)

	if flusher, ok := w.(http.Flusher); ok {
		flusher.Flush()
	}

	return nil
}

// SetupSSEHeaders configures the response headers for SSE
func SetupSSEHeaders(w http.ResponseWriter) {
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Access-Control-Allow-Headers", "Cache-Control")
}
