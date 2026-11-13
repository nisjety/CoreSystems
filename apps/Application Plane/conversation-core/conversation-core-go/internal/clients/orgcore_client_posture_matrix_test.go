package clients

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

// The vendored zdr-posture-matrix.json is a byte copy of the Model Plane
// contract; its .sha256 pins it so a drifted copy fails here instead of
// silently testing an old rule.
type postureMatrix struct {
	OrgPostureRows []struct {
		ID     string `json:"id"`
		Inputs struct {
			Intent   *bool  `json:"intent"`
			Entitled *bool  `json:"entitled"`
			Lookup   string `json:"lookup"`
		} `json:"inputs"`
		Expect struct {
			State  string `json:"state"`
			Code   string `json:"code"`
			Lapsed bool   `json:"lapsed"`
		} `json:"expect"`
	} `json:"org_posture_rows"`
}

func loadPostureMatrix(t *testing.T) postureMatrix {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("testdata", "zdr-posture-matrix.json"))
	if err != nil {
		t.Fatalf("read matrix: %v", err)
	}
	pin, err := os.ReadFile(filepath.Join("testdata", "zdr-posture-matrix.json.sha256"))
	if err != nil {
		t.Fatalf("read matrix sha256: %v", err)
	}
	sum := sha256.Sum256(raw)
	want := strings.Fields(string(pin))[0]
	if got := hex.EncodeToString(sum[:]); got != want {
		t.Fatalf("vendored matrix sha256 = %s, pinned %s", got, want)
	}
	var matrix postureMatrix
	if err := json.Unmarshal(raw, &matrix); err != nil {
		t.Fatalf("decode matrix: %v", err)
	}
	if len(matrix.OrgPostureRows) == 0 {
		t.Fatal("matrix has no org_posture_rows")
	}
	return matrix
}

// orgServer serves one org record (or a 500 when body is empty) plus a
// capability list that grants recurrence, so only the ZDR answer varies.
func orgServer(t *testing.T, orgID, body string) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/internal/orgs/" + orgID:
			if body == "" {
				http.Error(w, `{"error":"unavailable"}`, http.StatusInternalServerError)
				return
			}
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(body))
		case "/internal/orgs/" + orgID + "/roles/member/capabilities":
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"capabilities":["support:recurrence:read"]}`))
		default:
			http.NotFound(w, r)
		}
	}))
}

func testClient(server *httptest.Server, effectivePosture bool) *OrgCoreClient {
	return (&OrgCoreClient{
		baseURL:          server.URL,
		servicePrincipal: "conversation-core",
		serviceToken:     "service-token",
		httpClient:       server.Client(),
	}).WithEffectivePostureRead(effectivePosture)
}

func TestPostureMatrixRowsThroughSupportPolicyAndAllowAIProposal(t *testing.T) {
	matrix := loadPostureMatrix(t)
	for _, row := range matrix.OrgPostureRows {
		t.Run(row.ID, func(t *testing.T) {
			orgID := "org-" + row.ID
			body := ""
			if row.Inputs.Lookup == "ok" {
				if row.Inputs.Intent == nil {
					t.Fatalf("ok row without intent")
				}
				// As org-core serves the record after ZDR-Z-03a: the stored
				// intent in metadata and the computed retention_posture.
				effective := row.Expect.State == "zdr"
				body = fmt.Sprintf(
					`{"id":%q,"metadata":{"interactiveRetention":{"zdr":%t},"supportAi":{"mode":"review"}},"retention_posture":{"effective_zdr":%t,"lapsed":%t}}`,
					orgID, *row.Inputs.Intent, effective, row.Expect.Lapsed,
				)
			}
			server := orgServer(t, orgID, body)
			defer server.Close()
			client := testClient(server, true)

			policy, policyErr := client.SupportPolicy(t.Context(), orgID, "member")
			proposalErr := client.AllowAIProposal(t.Context(), orgID)

			switch row.Expect.State {
			case "zdr":
				if policyErr != nil || !policy.ZDREnabled {
					t.Fatalf("SupportPolicy = %+v, %v; want ZDREnabled", policy, policyErr)
				}
				if !errors.Is(proposalErr, conversation.ErrZDRAIProposalForbidden) {
					t.Fatalf("AllowAIProposal = %v; want ZDR refusal", proposalErr)
				}
			case "not_zdr":
				if policyErr != nil || policy.ZDREnabled {
					t.Fatalf("SupportPolicy = %+v, %v; want not ZDR", policy, policyErr)
				}
				if proposalErr != nil {
					t.Fatalf("AllowAIProposal = %v; want allowed", proposalErr)
				}
			case "unknown":
				// Unknown is never read as not ZDR: both fail closed.
				if policyErr == nil {
					t.Fatalf("SupportPolicy = %+v, nil; want error for unknown posture", policy)
				}
				if !errors.Is(proposalErr, conversation.ErrPolicyUnavailable) {
					t.Fatalf("AllowAIProposal = %v; want policy unavailable", proposalErr)
				}
			default:
				t.Fatalf("unhandled matrix state %q", row.Expect.State)
			}
		})
	}
}

// With the switch on the answer comes from org-core's effective posture, not
// the raw metadata flag (a lapsed or otherwise divergent record must follow
// org-core's computed field).
func TestEffectivePostureOverridesRawMetadataWhenOn(t *testing.T) {
	const orgID = "org-divergent"
	body := `{"id":"org-divergent","metadata":{"interactiveRetention":{"zdr":false},"supportAi":{"mode":"review"}},"retention_posture":{"effective_zdr":true,"lapsed":false}}`
	server := orgServer(t, orgID, body)
	defer server.Close()

	if err := testClient(server, true).AllowAIProposal(t.Context(), orgID); !errors.Is(err, conversation.ErrZDRAIProposalForbidden) {
		t.Fatalf("switch on: AllowAIProposal = %v; want ZDR refusal from effective posture", err)
	}
	// Switch off: byte-for-byte the old metadata rule, posture ignored.
	if err := testClient(server, false).AllowAIProposal(t.Context(), orgID); err != nil {
		t.Fatalf("switch off: AllowAIProposal = %v; want the metadata answer (allowed)", err)
	}
}

func TestRecordWithoutRetentionPostureFailsClosedWhenOn(t *testing.T) {
	const orgID = "org-no-posture"
	body := `{"id":"org-no-posture","metadata":{"interactiveRetention":{"zdr":false},"supportAi":{"mode":"review"}}}`
	server := orgServer(t, orgID, body)
	defer server.Close()

	err := testClient(server, true).AllowAIProposal(t.Context(), orgID)
	if !errors.Is(err, conversation.ErrPolicyUnavailable) {
		t.Fatalf("AllowAIProposal = %v; want ErrPolicyUnavailable for a record without retention_posture", err)
	}
	if _, err := testClient(server, true).SupportPolicy(t.Context(), orgID, "member"); err == nil {
		t.Fatal("SupportPolicy error = nil; want error for a record without retention_posture")
	}

	// A present block with effective_zdr absent is still "no posture".
	partial := `{"id":"org-no-posture","metadata":{},"retention_posture":{"lapsed":false}}`
	server2 := orgServer(t, orgID, partial)
	defer server2.Close()
	if err := testClient(server2, true).AllowAIProposal(t.Context(), orgID); !errors.Is(err, conversation.ErrPolicyUnavailable) {
		t.Fatalf("AllowAIProposal (effective_zdr absent) = %v; want ErrPolicyUnavailable", err)
	}

	// Switch off: the missing posture is ignored and the old metadata rule answers.
	if err := testClient(server, false).AllowAIProposal(t.Context(), orgID); err != nil {
		t.Fatalf("switch off: AllowAIProposal = %v; want allowed (metadata zdr false)", err)
	}
}

func listServer(t *testing.T, pages []string) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/internal/orgs" {
			http.NotFound(w, r)
			return
		}
		offset := r.URL.Query().Get("offset")
		index := 0
		if offset == "500" {
			index = 1
		}
		if index >= len(pages) {
			http.Error(w, "no page", http.StatusInternalServerError)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(pages[index]))
	}))
}

func TestZDREnabledOrgIDsFailsWholeCallForRecordWithoutPostureWhenOn(t *testing.T) {
	// First page complete; the missing posture is on the second page so a
	// partial list (built from page one) would be observable.
	pages := []string{
		`{"organizations":[{"id":"org-a","metadata":{},"retention_posture":{"effective_zdr":true,"lapsed":false}},{"id":"org-b","metadata":{},"retention_posture":{"effective_zdr":false,"lapsed":false}}],"hasMore":true}`,
		`{"organizations":[{"id":"org-c","metadata":{"interactiveRetention":{"zdr":true}}}],"hasMore":false}`,
	}
	server := listServer(t, pages)
	defer server.Close()

	ids, err := testClient(server, true).ZDREnabledOrgIDs(t.Context())
	if err == nil {
		t.Fatalf("ZDREnabledOrgIDs = %v, nil; want error for a record without retention_posture", ids)
	}
	if ids != nil {
		t.Fatalf("ZDREnabledOrgIDs returned a partial exclusion list %v alongside the error", ids)
	}

	// Switch off: the raw metadata rule over every page.
	ids, err = testClient(server, false).ZDREnabledOrgIDs(t.Context())
	if err != nil {
		t.Fatalf("switch off: ZDREnabledOrgIDs error = %v", err)
	}
	if len(ids) != 1 || !ids["org-c"] {
		t.Fatalf("switch off: ZDREnabledOrgIDs = %v; want only org-c (metadata zdr true)", ids)
	}
}

func TestZDREnabledOrgIDsReadsEffectivePostureWhenOn(t *testing.T) {
	pages := []string{
		`{"organizations":[{"id":"org-a","metadata":{"interactiveRetention":{"zdr":false}},"retention_posture":{"effective_zdr":true,"lapsed":false}},{"id":"org-b","metadata":{"interactiveRetention":{"zdr":true}},"retention_posture":{"effective_zdr":false,"lapsed":false}}],"hasMore":false}`,
	}
	server := listServer(t, pages)
	defer server.Close()

	ids, err := testClient(server, true).ZDREnabledOrgIDs(t.Context())
	if err != nil {
		t.Fatalf("ZDREnabledOrgIDs error = %v", err)
	}
	if len(ids) != 1 || !ids["org-a"] {
		t.Fatalf("ZDREnabledOrgIDs = %v; want only org-a (effective_zdr true)", ids)
	}
}

func TestWithEffectivePostureReadOnNilClientStaysNil(t *testing.T) {
	var client *OrgCoreClient
	if got := client.WithEffectivePostureRead(true); got != nil {
		t.Fatalf("WithEffectivePostureRead on nil = %v; want nil", got)
	}
	if NewOrgCoreClient("", "p", "t").WithEffectivePostureRead(true) != nil {
		t.Fatal("unconfigured client must stay nil")
	}
	if err := NewOrgCoreClient("", "p", "t").WithEffectivePostureRead(true).AllowAIProposal(t.Context(), "org"); !errors.Is(err, conversation.ErrPolicyUnavailable) {
		t.Fatalf("nil client AllowAIProposal = %v; want ErrPolicyUnavailable", err)
	}
}
