package browsersites

import (
	"encoding/json"
	"os"
	"testing"
	"time"
)

// siteMatrix is the shared decision golden (browser-site-policy-matrix.json),
// authored with WS21-S3 and vendored to testdata/. Each row is one set, one
// caller's choices and one instant, with the answer Decide must give.
type siteMatrix struct {
	Rows []struct {
		ID    string `json:"id"`
		Mode  string `json:"mode"`
		Rules []struct {
			Host string `json:"host"`
			Rule string `json:"rule"`
		} `json:"rules"`
		User  bool `json:"user"`
		Prefs []struct {
			Host      string `json:"host"`
			Level     string `json:"level"`
			ExpiresAt string `json:"expires_at"`
		} `json:"prefs"`
		Now    string `json:"now"`
		Host   string `json:"host"`
		Expect struct {
			Effective string `json:"effective"`
			Cap       string `json:"cap"`
			Source    string `json:"source"`
		} `json:"expect"`
	} `json:"rows"`
}

func TestDecideMatchesTheSharedMatrix(t *testing.T) {
	raw, err := os.ReadFile("testdata/browser-site-policy-matrix.json")
	if err != nil {
		t.Fatalf("read vendored matrix: %v", err)
	}
	var matrix siteMatrix
	if err := json.Unmarshal(raw, &matrix); err != nil {
		t.Fatalf("decode matrix: %v", err)
	}
	if len(matrix.Rows) == 0 {
		t.Fatal("matrix has no rows")
	}
	for _, row := range matrix.Rows {
		now, err := time.Parse(time.RFC3339, row.Now)
		if err != nil {
			t.Fatalf("%s: bad now: %v", row.ID, err)
		}
		set := PolicySet{Mode: row.Mode}
		for _, rule := range row.Rules {
			set.Rules = append(set.Rules, Rule{Host: rule.Host, Rule: rule.Rule})
		}
		var prefs map[string]Pref
		if row.User {
			prefs = map[string]Pref{}
			for _, pref := range row.Prefs {
				expires, err := time.Parse(time.RFC3339, pref.ExpiresAt)
				if err != nil {
					t.Fatalf("%s: bad expires_at: %v", row.ID, err)
				}
				prefs[pref.Host] = Pref{Host: pref.Host, Level: pref.Level, ExpiresAt: expires}
			}
		}
		got := Decide(set, prefs, row.Host, now)
		if got.Effective != row.Expect.Effective || got.Cap != row.Expect.Cap || got.Source != row.Expect.Source {
			t.Errorf("%s: got effective=%s cap=%s source=%s; golden says effective=%s cap=%s source=%s",
				row.ID, got.Effective, got.Cap, got.Source, row.Expect.Effective, row.Expect.Cap, row.Expect.Source)
		}
	}
}

func TestResolveAnswersEveryHostInOrderAndCarriesTheRevision(t *testing.T) {
	now := time.Date(2026, 10, 24, 12, 0, 0, 0, time.UTC)
	set := PolicySet{Revision: 7, Mode: ModeOpen, Rules: []Rule{{Host: "blocked.example.com", Rule: RuleBlock}}}
	got := Resolve(set, nil, []string{"blocked.example.com", "open.example.com"}, now)
	if got.Revision != 7 || len(got.Decisions) != 2 {
		t.Fatalf("resolution must carry the revision and one decision per host: %+v", got)
	}
	if got.Decisions[0].Effective != EffectiveDeny || got.Decisions[0].Source != SourceOrgBlock {
		t.Errorf("blocked host: %+v", got.Decisions[0])
	}
	if got.Decisions[1].Effective != EffectiveAsk || got.Decisions[1].Cap != RuleAct {
		t.Errorf("open host with no choice must ask at act: %+v", got.Decisions[1])
	}
}
