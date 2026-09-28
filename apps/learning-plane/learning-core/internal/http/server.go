package learninghttp

import (
	"encoding/json"
	"errors"
	"net"
	"net/http"
	"strconv"
)

// Foundation listeners must not accidentally expose an unauthenticated future API.
func ValidateListenAddress(addr string) error {
	host, port, err := net.SplitHostPort(addr)
	if err != nil {
		return errors.New("LEARNING_LISTEN_ADDR must be a loopback IP and port")
	}
	ip := net.ParseIP(host)
	p, err := strconv.Atoi(port)
	if ip == nil || !ip.IsLoopback() || err != nil || p < 1 || p > 65535 {
		return errors.New("foundation requires a loopback IP and a valid port")
	}
	return nil
}

type Capability struct {
	Key   string `json:"key"`
	State string `json:"state"`
}

type Status struct {
	Service           string       `json:"service"`
	Phase             string       `json:"phase"`
	Ready             bool         `json:"ready"`
	ProcessingProfile string       `json:"processingProfile"`
	Capabilities      []Capability `json:"capabilities"`
}

func FoundationStatus() Status {
	return Status{Service: "learning-core", Phase: "foundation", Ready: false, ProcessingProfile: "strict-self-hosted", Capabilities: []Capability{
		{Key: "school-identity", State: "not-connected"},
		{Key: "course-persistence", State: "not-connected"},
		{Key: "learner-progress", State: "not-connected"},
		{Key: "self-hosted-speech", State: "not-connected"},
		{Key: "tutor-runtime", State: "not-connected"},
		{Key: "teacher-assistance", State: "not-connected"},
	}}
}

func NewHandler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /healthz", func(w http.ResponseWriter, r *http.Request) {
		write(w, 200, map[string]any{"data": map[string]string{"status": "alive"}})
	})
	mux.HandleFunc("GET /readyz", unavailable)
	mux.HandleFunc("GET /api/v1/learning/status", func(w http.ResponseWriter, r *http.Request) {
		write(w, 200, map[string]any{"data": FoundationStatus()})
	})
	mux.HandleFunc("/api/v1/learning/", unavailable)
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		w.Header().Set("X-Content-Type-Options", "nosniff")
		mux.ServeHTTP(w, r)
	})
}

func unavailable(w http.ResponseWriter, r *http.Request) {
	write(w, http.StatusServiceUnavailable, map[string]any{"error": map[string]any{
		"code": "LEARNING_NOT_READY", "message": "School authentication and durable learning records are not connected.",
		"details": map[string]any{"phase": "foundation", "retryable": false},
	}})
}

func write(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(body)
}
