package org

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// ZDR-Z-03a: org-core states the org's effective ZDR posture through one
// resolver. The vendored matrix (a byte copy of
// apps/Model Plane/api/contracts/zdr-posture-matrix.json) is the authority;
// every org_posture_rows row with lookup ok is driven through
// resolveOrgPosture, NewRetentionPosture, RetentionPosture.Resolved and
// withRetentionView, which must all give the matrix's answer.

type matrixOrgRow struct {
	ID     string `json:"id"`
	Inputs struct {
		Intent   *bool  `json:"intent"`
		Entitled *bool  `json:"entitled"`
		Lookup   string `json:"lookup"`
	} `json:"inputs"`
	Expect struct {
		State  string `json:"state"`
		Lapsed bool   `json:"lapsed"`
	} `json:"expect"`
}

type postureMatrix struct {
	Policy struct {
		PlanLapse string `json:"plan_lapse"`
	} `json:"policy"`
	OrgPostureRows []matrixOrgRow `json:"org_posture_rows"`
}

func loadPostureMatrix(t *testing.T) postureMatrix {
	t.Helper()
	path := filepath.Join("testdata", "zdr-posture-matrix.json")
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read vendored matrix: %v", err)
	}
	pinRaw, err := os.ReadFile(path + ".sha256")
	if err != nil {
		t.Fatalf("read matrix pin: %v", err)
	}
	fields := strings.Fields(string(pinRaw))
	if len(fields) == 0 {
		t.Fatalf("matrix pin is empty")
	}
	normalised := strings.ReplaceAll(string(raw), "\r\n", "\n")
	sum := sha256.Sum256([]byte(normalised))
	if got := hex.EncodeToString(sum[:]); got != fields[0] {
		t.Fatalf("vendored matrix sha256 = %s, pin says %s", got, fields[0])
	}
	var matrix postureMatrix
	if err := json.Unmarshal(raw, &matrix); err != nil {
		t.Fatalf("decode matrix: %v", err)
	}
	return matrix
}

// A plan that entitles ZDR and one that does not, read from the real
// allowlist so the test does not restate it.
func planFor(t *testing.T, entitled bool) string {
	t.Helper()
	for _, plan := range []string{"enterprise", "pro", "free", "starter", "team"} {
		if PlanAllowsZeroDataRetention(plan) == entitled {
			return plan
		}
	}
	t.Fatalf("no plan with entitlement %v", entitled)
	return ""
}

func TestRetentionPostureMatrixOrgRows(t *testing.T) {
	matrix := loadPostureMatrix(t)
	if matrix.Policy.PlanLapse != PlanLapsePolicy {
		t.Fatalf("matrix policy.plan_lapse = %q, org-core PlanLapsePolicy = %q: flip both together (OD-LAPSE)",
			matrix.Policy.PlanLapse, PlanLapsePolicy)
	}

	okRows := 0
	sawLapsed := false
	for _, row := range matrix.OrgPostureRows {
		if row.Inputs.Lookup != "ok" {
			continue
		}
		okRows++
		row := row
		t.Run(row.ID, func(t *testing.T) {
			if row.Inputs.Intent == nil || row.Inputs.Entitled == nil {
				t.Fatalf("lookup ok row without intent/entitled inputs")
			}
			intent, entitled := *row.Inputs.Intent, *row.Inputs.Entitled
			wantZDR := row.Expect.State == "zdr"
			if !wantZDR && row.Expect.State != "not_zdr" {
				t.Fatalf("unexpected state %q for a lookup ok row", row.Expect.State)
			}
			if row.Expect.Lapsed {
				sawLapsed = true
			}

			effective, lapsed := resolveOrgPosture(intent, entitled)
			if effective != wantZDR || lapsed != row.Expect.Lapsed {
				t.Fatalf("resolveOrgPosture(%v, %v) = (%v, %v), matrix wants (%v, %v)",
					intent, entitled, effective, lapsed, wantZDR, row.Expect.Lapsed)
			}

			posture := NewRetentionPosture(planFor(t, entitled), intent)
			want := RetentionPosture{
				ZDRIntent:       intent,
				Entitled:        entitled,
				EffectiveZDR:    wantZDR,
				Lapsed:          row.Expect.Lapsed,
				PlanLapsePolicy: matrix.Policy.PlanLapse,
			}
			if posture != want {
				t.Fatalf("NewRetentionPosture = %+v, want %+v", posture, want)
			}

			// A hand-built posture with lying derived fields is corrected.
			lying := RetentionPosture{ZDRIntent: intent, Entitled: entitled, EffectiveZDR: !wantZDR, Lapsed: !row.Expect.Lapsed, PlanLapsePolicy: "other"}
			if got := lying.Resolved(); got != want {
				t.Fatalf("Resolved() = %+v, want %+v", got, want)
			}

			// The org-record projection agrees with the posture route.
			org := &Organization{
				Plan:     planFor(t, entitled),
				Metadata: map[string]any{"interactiveRetention": map[string]any{"zdr": intent}},
			}
			withRetentionView(org)
			if org.RetentionPosture == nil {
				t.Fatalf("withRetentionView left RetentionPosture nil")
			}
			if org.RetentionPosture.EffectiveZDR != posture.EffectiveZDR || org.RetentionPosture.Lapsed != posture.Lapsed {
				t.Fatalf("withRetentionView = %+v, NewRetentionPosture = %+v", *org.RetentionPosture, posture)
			}
		})
	}
	if okRows == 0 {
		t.Fatalf("matrix has no lookup ok org rows")
	}
	if !sawLapsed {
		t.Fatalf("matrix has no lapsed org row (o_plan_lapsed_keeps_zdr)")
	}
}

// withRetentionView reads intent exactly like the repository SQL: only the JSON
// boolean true is intent. Anything else (string, number, missing, wrong shape)
// is not.
func TestWithRetentionViewReadsIntentLikeTheRepository(t *testing.T) {
	plan := planFor(t, true)
	cases := []struct {
		name     string
		metadata map[string]any
		want     bool
	}{
		{"boolean true", map[string]any{"interactiveRetention": map[string]any{"zdr": true}}, true},
		{"boolean false", map[string]any{"interactiveRetention": map[string]any{"zdr": false}}, false},
		{"string true", map[string]any{"interactiveRetention": map[string]any{"zdr": "true"}}, false},
		{"number 1", map[string]any{"interactiveRetention": map[string]any{"zdr": 1}}, false},
		{"missing key", map[string]any{"interactiveRetention": map[string]any{}}, false},
		{"not an object", map[string]any{"interactiveRetention": true}, false},
		{"no metadata", nil, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			org := withRetentionView(&Organization{Plan: plan, Metadata: tc.metadata})
			if org.RetentionPosture.EffectiveZDR != tc.want {
				t.Fatalf("EffectiveZDR = %v, want %v", org.RetentionPosture.EffectiveZDR, tc.want)
			}
		})
	}

	// Decoded from JSON, as the cache path and the repository produce it.
	var org Organization
	if err := json.Unmarshal([]byte(`{"plan":"`+planFor(t, false)+`","metadata":{"interactiveRetention":{"zdr":true}}}`), &org); err != nil {
		t.Fatalf("decode: %v", err)
	}
	withRetentionView(&org)
	if !org.RetentionPosture.EffectiveZDR || !org.RetentionPosture.Lapsed {
		t.Fatalf("lapsed org decoded from JSON = %+v, want effective and lapsed", *org.RetentionPosture)
	}
	if withRetentionView(nil) != nil {
		t.Fatalf("withRetentionView(nil) must stay nil")
	}
}

// The projection uses the record's snake_case JSON and is omitted when unset.
func TestOrgRetentionViewWireShape(t *testing.T) {
	org := withRetentionView(&Organization{Plan: planFor(t, false), Metadata: map[string]any{"interactiveRetention": map[string]any{"zdr": true}}})
	body, err := json.Marshal(org)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var decoded map[string]any
	if err := json.Unmarshal(body, &decoded); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	view, ok := decoded["retention_posture"].(map[string]any)
	if !ok {
		t.Fatalf("retention_posture missing: %s", body)
	}
	if view["effective_zdr"] != true || view["lapsed"] != true || len(view) != 2 {
		t.Fatalf("retention_posture = %v, want exactly {effective_zdr:true, lapsed:true}", view)
	}
	bare, _ := json.Marshal(Organization{})
	if strings.Contains(string(bare), "retention_posture") {
		t.Fatalf("an organization without the projection must not carry retention_posture: %s", bare)
	}
}
