package http

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// E-PRIO-01: getAgentSubject answers the agent read the gateway uses for the access
// list. These run the handler with a repository that has no database: a blank id must
// be a typed 404 before any query (it would otherwise match every visible agent), and
// a real id reaches the registry, which answers unavailable rather than an empty list.
func callGetAgentSubject(t *testing.T, id string) *httptest.ResponseRecorder {
	t.Helper()
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(http.MethodGet, "/api/v1/internal/agents/subjects/x", nil)
	c.Params = gin.Params{{Key: "agent_subject_id", Value: id}}
	c.Set("org_id", "org-1")
	c.Set("user_id", "user-1")
	(&Server{spaceRepo: &spaces.Repository{}}).getAgentSubject(c)
	return w
}

func TestGetAgentSubjectBlankIDIsATypedNotFound(t *testing.T) {
	for _, id := range []string{"", "  "} {
		w := callGetAgentSubject(t, id)
		if w.Code != http.StatusNotFound {
			t.Fatalf("id %q: status = %d, want 404: %s", id, w.Code, w.Body.String())
		}
		var body struct {
			Error struct {
				Code string `json:"code"`
			} `json:"error"`
		}
		if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil || body.Error.Code != spaces.CodeAgentNotFound {
			t.Fatalf("id %q: body = %s, want error.code %q", id, w.Body.String(), spaces.CodeAgentNotFound)
		}
	}
}

func TestGetAgentSubjectWithoutARegistryIsNotAnEmptyList(t *testing.T) {
	w := callGetAgentSubject(t, "agent-sample1")
	if w.Code == http.StatusOK || w.Code == http.StatusNotFound {
		t.Fatalf("status = %d, want a registry failure, not 200/404: %s", w.Code, w.Body.String())
	}
}
