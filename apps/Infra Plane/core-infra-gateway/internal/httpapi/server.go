package httpapi

import (
	"crypto/subtle"
	"encoding/json"
	"net/http"
	"strings"
	"time"
)

const localTrafficOperatorRole = "local_traffic_operator"

// Config supplies the bounded runtime identity for the Core Infra gateway.
// It intentionally has no cross-plane database, cache, broker, or Docker
// connection settings.
type Config struct {
	ServiceName   string
	Version       string
	OperatorToken string
}

type status struct {
	ServiceName        string    `json:"service_name"`
	Version            string    `json:"version"`
	Status             string    `json:"status"`
	Role               string    `json:"role"`
	DatabaseAccess     bool      `json:"database_access"`
	CrossPlaneAccess   bool      `json:"cross_plane_access"`
	DockerSocketAccess bool      `json:"docker_socket_access"`
	ObservedAt         time.Time `json:"observed_at"`
}

type envelope struct {
	Data status `json:"data"`
}

type errorEnvelope struct {
	Error struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

// NewHandler exposes only the Core Infra component's own status. It is not a
// cross-plane BFF, data proxy, Docker controller, or service discovery API.
func NewHandler(config Config) http.Handler {
	serviceName := strings.TrimSpace(config.ServiceName)
	if serviceName == "" {
		serviceName = "core-infra-gateway"
	}
	version := strings.TrimSpace(config.Version)
	if version == "" {
		version = "dev"
	}

	currentStatus := func() status {
		return status{
			ServiceName:        serviceName,
			Version:            version,
			Status:             "ok",
			Role:               localTrafficOperatorRole,
			DatabaseAccess:     false,
			CrossPlaneAccess:   false,
			DockerSocketAccess: false,
			ObservedAt:         time.Now().UTC(),
		}
	}

	mux := http.NewServeMux()
	mux.HandleFunc("GET /health", func(writer http.ResponseWriter, _ *http.Request) {
		writeJSON(writer, http.StatusOK, envelope{Data: currentStatus()})
	})
	mux.HandleFunc("GET /ready", func(writer http.ResponseWriter, _ *http.Request) {
		writeJSON(writer, http.StatusOK, envelope{Data: currentStatus()})
	})
	mux.HandleFunc("GET /v1/operator/status", func(writer http.ResponseWriter, request *http.Request) {
		if !isAuthorized(request, config.OperatorToken) {
			writeError(writer, http.StatusUnauthorized, "unauthorized", "A local operator token is required.")
			return
		}
		writeJSON(writer, http.StatusOK, envelope{Data: currentStatus()})
	})

	return securityHeaders(mux)
}

func isAuthorized(request *http.Request, configuredToken string) bool {
	configuredToken = strings.TrimSpace(configuredToken)
	if configuredToken == "" {
		return false
	}

	provided := strings.TrimSpace(strings.TrimPrefix(request.Header.Get("Authorization"), "Bearer "))
	if provided == "" || len(provided) != len(configuredToken) {
		return false
	}

	return subtle.ConstantTimeCompare([]byte(provided), []byte(configuredToken)) == 1
}

func securityHeaders(next http.Handler) http.Handler {
	return http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		writer.Header().Set("Cache-Control", "no-store")
		writer.Header().Set("Content-Security-Policy", "default-src 'none'; frame-ancestors 'none'")
		writer.Header().Set("Referrer-Policy", "no-referrer")
		writer.Header().Set("X-Content-Type-Options", "nosniff")
		writer.Header().Set("X-Frame-Options", "DENY")
		next.ServeHTTP(writer, request)
	})
}

func writeJSON(writer http.ResponseWriter, statusCode int, value any) {
	writer.Header().Set("Content-Type", "application/json; charset=utf-8")
	writer.WriteHeader(statusCode)
	_ = json.NewEncoder(writer).Encode(value)
}

func writeError(writer http.ResponseWriter, statusCode int, code string, message string) {
	response := errorEnvelope{}
	response.Error.Code = code
	response.Error.Message = message
	writeJSON(writer, statusCode, response)
}
