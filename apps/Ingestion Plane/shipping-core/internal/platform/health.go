package platform

import (
	"encoding/json"
	"net/http"

	"github.com/jackc/pgx/v5/pgxpool"
)

type healthResponse struct {
	Status string            `json:"status"`
	Checks map[string]string `json:"checks,omitempty"`
}

// HealthzHandler is a liveness probe: it returns 200 as soon as the
// process can handle HTTP requests, with no dependency checks. Orchestrators
// use this to decide whether to restart the container.
func HealthzHandler() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		writeHealth(w, http.StatusOK, healthResponse{Status: "ok"})
	}
}

// ReadyzHandler is a readiness probe: it verifies the database is reachable
// before reporting healthy. Orchestrators use this to decide whether to
// route traffic to the instance.
func ReadyzHandler(pool *pgxpool.Pool) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		checks := map[string]string{}
		status := http.StatusOK
		overall := "ok"

		if err := pool.Ping(r.Context()); err != nil {
			checks["db"] = err.Error()
			status = http.StatusServiceUnavailable
			overall = "down"
		} else {
			checks["db"] = "ok"
		}

		writeHealth(w, status, healthResponse{Status: overall, Checks: checks})
	}
}

func writeHealth(w http.ResponseWriter, status int, body healthResponse) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(body)
}
