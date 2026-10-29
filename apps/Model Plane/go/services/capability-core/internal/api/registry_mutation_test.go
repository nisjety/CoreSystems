package api

import (
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestRoutingAndSafetyMutationsDoNotClaimSuccessWhenNoTenantRowChanged(t *testing.T) {
	for _, test := range []struct {
		name    string
		path    string
		body    string
		handler func(*recordingDatabase) http.Handler
	}{
		{
			name: "routing patch", path: "/api/v1/routing/rp-foreign", body: `{"enabled":false}`,
			handler: func(database *recordingDatabase) http.Handler {
				routing := NewRoutingHandler(nil)
				routing.pool = database
				mux := http.NewServeMux()
				routing.Register(mux)
				return mux
			},
		},
		{
			name: "routing delete", path: "/api/v1/routing/rp-foreign",
			handler: func(database *recordingDatabase) http.Handler {
				routing := NewRoutingHandler(nil)
				routing.pool = database
				mux := http.NewServeMux()
				routing.Register(mux)
				return mux
			},
		},
		{
			name: "safety patch", path: "/api/v1/safety/sp-foreign", body: `{"enabled":false}`,
			handler: func(database *recordingDatabase) http.Handler {
				safety := NewSafetyHandler(nil)
				safety.pool = database
				mux := http.NewServeMux()
				safety.Register(mux)
				return mux
			},
		},
		{
			name: "safety delete", path: "/api/v1/safety/sp-foreign",
			handler: func(database *recordingDatabase) http.Handler {
				safety := NewSafetyHandler(nil)
				safety.pool = database
				mux := http.NewServeMux()
				safety.Register(mux)
				return mux
			},
		},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := &recordingDatabase{execTag: "UPDATE 0"}
			handler, _, writeToken := mcpAuthenticatedHandler(t, test.handler(database))
			method := http.MethodPatch
			if test.body == "" {
				method = http.MethodDelete
			}
			request := httptest.NewRequest(method, test.path, strings.NewReader(test.body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusNotFound {
				t.Fatalf("status = %d, body=%s, want 404", response.Code, response.Body.String())
			}
		})
	}
}

func TestRoutingAndSafetyMutationErrorsAreGeneric(t *testing.T) {
	for _, test := range []struct {
		name    string
		path    string
		handler func(*recordingDatabase) http.Handler
	}{
		{
			name: "routing", path: "/api/v1/routing/rp-1",
			handler: func(database *recordingDatabase) http.Handler {
				routing := NewRoutingHandler(nil)
				routing.pool = database
				mux := http.NewServeMux()
				routing.Register(mux)
				return mux
			},
		},
		{
			name: "safety", path: "/api/v1/safety/sp-1",
			handler: func(database *recordingDatabase) http.Handler {
				safety := NewSafetyHandler(nil)
				safety.pool = database
				mux := http.NewServeMux()
				safety.Register(mux)
				return mux
			},
		},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := &recordingDatabase{execErr: errors.New("postgres connection refused")}
			handler, _, writeToken := mcpAuthenticatedHandler(t, test.handler(database))
			request := httptest.NewRequest(http.MethodPatch, test.path, strings.NewReader(`{"enabled":false}`))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusInternalServerError {
				t.Fatalf("status = %d, body=%s, want 500", response.Code, response.Body.String())
			}
			if strings.Contains(response.Body.String(), "postgres connection refused") {
				t.Fatalf("response leaked database error: %s", response.Body.String())
			}
		})
	}
}

func TestRoutingAndSafetyPatchWritesAllRequestedFieldsAtomically(t *testing.T) {
	for _, test := range []struct {
		name    string
		path    string
		handler func(*recordingDatabase) http.Handler
	}{
		{
			name: "routing", path: "/api/v1/routing/rp-1",
			handler: func(database *recordingDatabase) http.Handler {
				routing := NewRoutingHandler(nil)
				routing.pool = database
				mux := http.NewServeMux()
				routing.Register(mux)
				return mux
			},
		},
		{
			name: "safety", path: "/api/v1/safety/sp-1",
			handler: func(database *recordingDatabase) http.Handler {
				safety := NewSafetyHandler(nil)
				safety.pool = database
				mux := http.NewServeMux()
				safety.Register(mux)
				return mux
			},
		},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := &recordingDatabase{}
			handler, _, writeToken := mcpAuthenticatedHandler(t, test.handler(database))
			request := httptest.NewRequest(http.MethodPatch, test.path, strings.NewReader(`{"enabled":false,"priority":7}`))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusOK {
				t.Fatalf("status = %d, body=%s, want 200", response.Code, response.Body.String())
			}
			if len(database.execs) != 1 {
				t.Fatalf("writes = %d, want one atomic update", len(database.execs))
			}
			if got := database.execs[0].args; len(got) != 5 || got[0] != false || got[1] != 7 || got[4] != "org-a" {
				t.Fatalf("update args = %#v, want values scoped to org-a", got)
			}
		})
	}
}

func TestSkillsMutationsDoNotClaimSuccessWithoutOneTenantRow(t *testing.T) {
	for _, test := range []struct {
		name   string
		method string
		body   string
	}{
		{name: "patch", method: http.MethodPatch, body: `{"enabled":false}`},
		{name: "delete", method: http.MethodDelete},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := &recordingDatabase{execTag: "UPDATE 0"}
			skills := NewSkillsHandler(nil)
			skills.pool = database
			mux := http.NewServeMux()
			skills.Register(mux)
			handler, _, writeToken := mcpAuthenticatedHandler(t, mux)
			request := httptest.NewRequest(test.method, "/api/v1/skills/skill-foreign", strings.NewReader(test.body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusNotFound {
				t.Fatalf("status = %d, body=%s, want 404", response.Code, response.Body.String())
			}
		})
	}
}

func TestPluginMutationsDoNotClaimSuccessWithoutOneTenantRow(t *testing.T) {
	for _, test := range []struct {
		name   string
		method string
		body   string
	}{
		{name: "patch", method: http.MethodPatch, body: `{"enabled":false,"pinned":true}`},
		{name: "delete", method: http.MethodDelete},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := &recordingDatabase{execTag: "UPDATE 0"}
			plugins := NewPluginsHandler(nil)
			plugins.pool = database
			mux := http.NewServeMux()
			plugins.Register(mux)
			handler, _, writeToken := mcpAuthenticatedHandler(t, mux)
			request := httptest.NewRequest(test.method, "/api/v1/plugins/plugin-foreign", strings.NewReader(test.body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusNotFound {
				t.Fatalf("status = %d, body=%s, want 404", response.Code, response.Body.String())
			}
		})
	}
}

func TestTaskAndCronMutationsDoNotClaimSuccessWithoutOneTenantRow(t *testing.T) {
	for _, test := range []struct {
		name    string
		method  string
		path    string
		body    string
		handler func(*recordingDatabase) http.Handler
	}{
		{
			name: "task patch", method: http.MethodPatch, path: "/api/v1/tasks/task-foreign", body: `{"status":"running"}`,
			handler: func(database *recordingDatabase) http.Handler {
				tasks := NewTasksHandler(nil)
				tasks.pool = database
				mux := http.NewServeMux()
				tasks.Register(mux)
				return mux
			},
		},
		{
			name: "task cancel", method: http.MethodPost, path: "/api/v1/tasks/task-foreign/cancel",
			handler: func(database *recordingDatabase) http.Handler {
				tasks := NewTasksHandler(nil)
				tasks.pool = database
				mux := http.NewServeMux()
				tasks.Register(mux)
				return mux
			},
		},
		{
			name: "cron patch", method: http.MethodPatch, path: "/api/v1/cron/cron-foreign", body: `{"enabled":false}`,
			handler: func(database *recordingDatabase) http.Handler {
				cronHandler := NewCronHandler(nil)
				cronHandler.pool = database
				mux := http.NewServeMux()
				cronHandler.Register(mux)
				return mux
			},
		},
		{
			name: "cron delete", method: http.MethodDelete, path: "/api/v1/cron/cron-foreign",
			handler: func(database *recordingDatabase) http.Handler {
				cronHandler := NewCronHandler(nil)
				cronHandler.pool = database
				mux := http.NewServeMux()
				cronHandler.Register(mux)
				return mux
			},
		},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := &recordingDatabase{execTag: "UPDATE 0"}
			handler, _, writeToken := mcpAuthenticatedHandler(t, test.handler(database))
			request := httptest.NewRequest(test.method, test.path, strings.NewReader(test.body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusNotFound {
				t.Fatalf("status = %d, body=%s, want 404", response.Code, response.Body.String())
			}
		})
	}
}

func TestTaskAndCronPatchWriteRequestedFieldsAtomically(t *testing.T) {
	for _, test := range []struct {
		name    string
		path    string
		body    string
		handler func(*recordingDatabase) http.Handler
	}{
		{
			name: "task", path: "/api/v1/tasks/task-1", body: `{"status":"running","assignee":"user-1"}`,
			handler: func(database *recordingDatabase) http.Handler {
				tasks := NewTasksHandler(nil)
				tasks.pool = database
				mux := http.NewServeMux()
				tasks.Register(mux)
				return mux
			},
		},
		{
			name: "cron", path: "/api/v1/cron/cron-1", body: `{"enabled":false,"schedule_expr":"0 * * * *"}`,
			handler: func(database *recordingDatabase) http.Handler {
				cronHandler := NewCronHandler(nil)
				cronHandler.pool = database
				mux := http.NewServeMux()
				cronHandler.Register(mux)
				return mux
			},
		},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := &recordingDatabase{}
			if test.name == "cron" {
				database.expectUpsertReturningID(t, "UTC")
			}
			handler, _, writeToken := mcpAuthenticatedHandler(t, test.handler(database))
			request := httptest.NewRequest(http.MethodPatch, test.path, strings.NewReader(test.body))
			request.Header.Set("Authorization", "Bearer "+writeToken)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusOK {
				t.Fatalf("status = %d, body=%s, want 200", response.Code, response.Body.String())
			}
			if len(database.execs) != 1 {
				t.Fatalf("writes = %d, want one atomic update", len(database.execs))
			}
			if got := database.execs[0].args; got[len(got)-1] != "org-a" {
				t.Fatalf("update args = %#v, want organization scope org-a", got)
			}
		})
	}
}

func TestTaskCancellationRejectsTerminalTasksWithoutMaskingMissingTasks(t *testing.T) {
	database := &recordingDatabase{execTag: "UPDATE 0"}
	database.expectUpsertReturningID(t, "completed")
	tasks := NewTasksHandler(nil)
	tasks.pool = database
	mux := http.NewServeMux()
	tasks.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)
	request := httptest.NewRequest(http.MethodPost, "/api/v1/tasks/task-completed/cancel", nil)
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusConflict {
		t.Fatalf("status = %d, body=%s, want 409", response.Code, response.Body.String())
	}
}
