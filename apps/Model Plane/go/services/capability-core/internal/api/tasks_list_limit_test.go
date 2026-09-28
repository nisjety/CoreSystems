package api

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

// The tasks list used to pass any limit straight to Postgres: a huge one read
// the org's whole table and a negative one failed the query with a raw
// database error. It is now bounded like the service's other list endpoints.
func TestTaskListLimitIsBoundedLikeOtherListEndpoints(t *testing.T) {
	for _, test := range []struct {
		query string
		want  int
	}{
		{query: "", want: 50},
		{query: "?limit=25", want: 25},
		{query: "?limit=200", want: 200},
		{query: "?limit=201", want: 50},
		{query: "?limit=1000000", want: 50},
		{query: "?limit=0", want: 50},
		{query: "?limit=-5", want: 50},
		{query: "?limit=abc", want: 50},
	} {
		t.Run(test.query, func(t *testing.T) {
			database := &recordingDatabase{}
			tasks := NewTasksHandler(database)
			mux := http.NewServeMux()
			tasks.Register(mux)
			handler, readToken, _ := mcpAuthenticatedHandler(t, mux)
			request := httptest.NewRequest(http.MethodGet, "/api/v1/tasks"+test.query, nil)
			request.Header.Set("Authorization", "Bearer "+readToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusOK {
				t.Fatalf("status = %d, body=%s, want 200", response.Code, response.Body.String())
			}
			if len(database.queries) != 1 {
				t.Fatalf("queries = %d, want 1", len(database.queries))
			}
			args := database.queries[0].args
			if len(args) != 3 || args[0] != "org-a" || args[2] != test.want {
				t.Fatalf("list args = %#v, want org-a scope and limit %d", args, test.want)
			}
		})
	}
}
