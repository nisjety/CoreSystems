package org

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
)

// The golden matrix for "what is this org's interactive retention posture".
// Z-03 makes every reader (Auth Core, the BFF, conversation-core) agree with
// this table; the row that matters is a lapsed plan, where the stored intent is
// still true but the entitlement is gone. Effective ZDR is intent AND
// entitlement, so that row is NOT zero-retention.
func TestNewRetentionPostureMatrix(t *testing.T) {
	cases := []struct {
		name          string
		plan          string
		intent        bool
		wantIntent    bool
		wantEntitled  bool
		wantEffective bool
	}{
		{"pro with intent", "pro", true, true, true, true},
		{"enterprise with intent", "enterprise", true, true, true, true},
		{"plan name is case and space insensitive", "  Enterprise ", true, true, true, true},
		{"pro without intent", "pro", false, false, true, false},
		{"plan lapsed: intent kept, entitlement gone", "free", true, true, false, false},
		{"trial with stale intent", "trial", true, true, false, false},
		{"hobby with stale intent", "hobby", true, true, false, false},
		{"standard with stale intent", "standard", true, true, false, false},
		{"unknown plan with stale intent", "mystery", true, true, false, false},
		{"empty plan with stale intent", "", true, true, false, false},
		{"free without intent", "free", false, false, false, false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got := NewRetentionPosture(c.plan, c.intent)
			if got.ZDRIntent != c.wantIntent || got.Entitled != c.wantEntitled {
				t.Fatalf("NewRetentionPosture(%q, %v)=%+v want intent=%v entitled=%v", c.plan, c.intent, got, c.wantIntent, c.wantEntitled)
			}
			if got.Effective() != c.wantEffective {
				t.Fatalf("Effective()=%v want %v for %+v", got.Effective(), c.wantEffective, got)
			}
		})
	}
}

// The wire shape is a contract with Auth Core
// (internal/http/testdata/auth-core-org-core-retention-posture.contract.json): two
// booleans, exact names, nothing else. Extra fields would hand a machine
// principal org data it was not granted a scope for.
func TestRetentionPostureWireShape(t *testing.T) {
	encoded, err := json.Marshal(NewRetentionPosture("pro", true))
	if err != nil {
		t.Fatal(err)
	}
	if string(encoded) != `{"zdrIntent":true,"entitled":true}` {
		t.Fatalf("wire shape=%s", encoded)
	}
	encoded, err = json.Marshal(NewRetentionPosture("free", false))
	if err != nil {
		t.Fatal(err)
	}
	if string(encoded) != `{"zdrIntent":false,"entitled":false}` {
		t.Fatalf("wire shape=%s", encoded)
	}
}

func TestGetRetentionPostureRequiresOrgID(t *testing.T) {
	service := NewService(&Repository{}, nil)
	for _, id := range []string{"", "   "} {
		if _, err := service.GetRetentionPosture(context.Background(), id); err == nil || !strings.Contains(err.Error(), "organization id is required") {
			t.Fatalf("id %q error=%v", id, err)
		}
	}
}
