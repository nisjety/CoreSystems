package resources

import (
	"encoding/json"
	"net/http"
	"testing"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// setJobCredits flips QUARRY_CONTROL__JOB_CREDITS for one test. Tests that call
// it are not parallel: they share the package-level getenv hook.
func setJobCredits(t *testing.T, value string) {
	t.Helper()
	prev := jobCreditsGetenv
	jobCreditsGetenv = func(k string) string {
		if k == jobCreditsEnv {
			return value
		}
		return ""
	}
	t.Cleanup(func() { jobCreditsGetenv = prev })
}

func f64(v float64) *float64 { return &v }

func TestCreditsFor(t *testing.T) {
	t.Parallel()
	cases := []struct {
		name    string
		kind    string
		status  string
		fetched int64
		want    *float64 // nil: no credits object
		final   bool
	}{
		{"crawl 5 fetched, running", "crawl", "running", 5, f64(5), false},
		{"crawl 5 fetched, completed", "crawl", "completed", 5, f64(5), true},
		{"crawl no pages yet", "crawl", "accepted", 0, f64(0), false},
		{"crawl cancelled keeps what it fetched", "crawl", "cancelled", 2, f64(2), true},
		{"crawl timed out", "crawl", "timed_out", 1, f64(1), true},
		{"paused is not final", "crawl", "paused", 3, f64(3), false},
		{"batch", "batch", "completed", 12, f64(12), true},
		{"scrape", "scrape", "completed", 1, f64(1), true},
		{"extract", "extract", "running", 1, f64(1), false},
		{"search is not page priced", "search", "completed", 4, nil, false},
		{"research is not page priced", "research", "completed", 4, nil, false},
		{"agent is not page priced", "agent", "completed", 4, nil, false},
		{"unknown kind", "bogus", "completed", 4, nil, false},
		{"negative count clamps to zero", "crawl", "running", -3, f64(0), false},
	}
	for _, c := range cases {
		c := c
		t.Run(c.name, func(t *testing.T) {
			t.Parallel()
			got := creditsFor(c.kind, c.status, c.fetched)
			if c.want == nil {
				if got != nil {
					t.Fatalf("got %+v want nil", got)
				}
				return
			}
			if got == nil {
				t.Fatal("got nil credits")
			}
			if got.Used != *c.want {
				t.Fatalf("used=%v want %v", got.Used, *c.want)
			}
			if got.Final != c.final {
				t.Fatalf("final=%v want %v", got.Final, c.final)
			}
			if got.Unit != "credit" || got.Basis != "pages_fetched" || !got.Estimated {
				t.Fatalf("labels wrong: %+v", got)
			}
			if got.PricePerPage != store.CreditsPerPage {
				t.Fatalf("price_per_page=%v want the shared constant %v", got.PricePerPage, store.CreditsPerPage)
			}
		})
	}
}

func TestCreditsJSONShape(t *testing.T) {
	t.Parallel()
	b, err := json.Marshal(creditsFor("crawl", "running", 12))
	if err != nil {
		t.Fatal(err)
	}
	want := `{"used":12,"unit":"credit","basis":"pages_fetched","price_per_page":1,"estimated":true,"final":false}`
	if string(b) != want {
		t.Fatalf("json=%s want %s", b, want)
	}
}

func TestJobCreditsEnabled(t *testing.T) {
	prev := jobCreditsGetenv
	t.Cleanup(func() { jobCreditsGetenv = prev })
	for v, want := range map[string]bool{
		"": false, "0": false, "false": false, "off": false, "garbage": false,
		"1": true, "true": true, "TRUE": true, " on ": true, "yes": true,
	} {
		v := v
		jobCreditsGetenv = func(string) string { return v }
		if got := jobCreditsEnabled(); got != want {
			t.Errorf("flag %q: got %v want %v", v, got, want)
		}
	}
}

func TestBuildSummary_CarriesCredits(t *testing.T) {
	t.Parallel()
	job := store.Job{ID: "job_1", OrgID: "o", Kind: "crawl", Status: "running"}
	sum := map[quarrycontracts.EventType]store.EventTypeSummary{
		quarrycontracts.EvtPageFetched: {Count: 4},
		quarrycontracts.EvtPageFailed:  {Count: 9},
	}
	view := buildSummary(job, sum)
	if view.Credits == nil || view.Credits.Used != 4 || view.Credits.Final {
		t.Fatalf("running crawl credits=%+v want used 4, not final (failed pages are free)", view.Credits)
	}
	job.Status = "completed"
	if v := buildSummary(job, sum); v.Credits == nil || !v.Credits.Final {
		t.Fatalf("completed crawl credits=%+v want final", v.Credits)
	}
	job.Kind = "agent"
	if v := buildSummary(job, sum); v.Credits != nil {
		t.Fatalf("agent job credits=%+v want nil", v.Credits)
	}
}

// creditsOfSummary reads GET /v1/jobs/{id}/summary and returns the raw credits
// object (nil when the key is absent) and the status code.
func creditsOfSummary(t *testing.T, h http.Handler, org string, id quarrycontracts.ID) (map[string]any, int) {
	t.Helper()
	rec := lcDo(h, http.MethodGet, "/v1/jobs/"+string(id)+"/summary?org_id="+org, nil)
	if rec.Code != http.StatusOK {
		return nil, rec.Code
	}
	var env struct {
		Data map[string]any `json:"data"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &env); err != nil {
		t.Fatalf("decode: %v: %s", err, rec.Body.String())
	}
	raw, present := env.Data["credits"]
	if !present {
		return nil, rec.Code
	}
	c, ok := raw.(map[string]any)
	if !ok {
		t.Fatalf("credits key present but not an object: %v", raw)
	}
	return c, rec.Code
}

func TestJobSummary_CreditsOnlyWhenFlagOn(t *testing.T) {
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "running", map[string]any{"max_pages": 10.0})
	for i := 0; i < 3; i++ {
		lcAppend(t, db, id, quarrycontracts.EvtPageFetched, uint64(i+1), map[string]any{"url": "https://e.example/p"})
	}
	lcAppend(t, db, id, quarrycontracts.EvtPageFailed, 4, map[string]any{"url": "https://e.example/x", "final": true})

	setJobCredits(t, "")
	if c, code := creditsOfSummary(t, h, "org_a", id); code != http.StatusOK || c != nil {
		t.Fatalf("flag off: code=%d credits=%v want absent", code, c)
	}

	setJobCredits(t, "1")
	c, code := creditsOfSummary(t, h, "org_a", id)
	if code != http.StatusOK || c == nil {
		t.Fatalf("flag on: code=%d credits=%v", code, c)
	}
	if c["used"] != 3.0 || c["final"] != false || c["estimated"] != true ||
		c["unit"] != "credit" || c["basis"] != "pages_fetched" || c["price_per_page"] != 1.0 {
		t.Fatalf("running credits=%v", c)
	}

	// Terminal: final flips true, and a late page_fetched still counts (the
	// figure is derived at read time, never frozen).
	job := mustJob(t, db, "org_a", id)
	job.Status = "completed"
	if err := db.Jobs().Update(job); err != nil {
		t.Fatal(err)
	}
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 5, map[string]any{"url": "https://e.example/late"})
	c, _ = creditsOfSummary(t, h, "org_a", id)
	if c == nil || c["used"] != 4.0 || c["final"] != true {
		t.Fatalf("terminal credits=%v want used 4, final", c)
	}
}

func TestJobSummary_CreditsAbsentForNonPageKinds(t *testing.T) {
	h, db := lcRouter(t)
	setJobCredits(t, "1")
	id := lcSeedJob(t, db, "org_a", "agent", "completed", nil)
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 1, map[string]any{"url": "https://e.example/p"})
	if c, code := creditsOfSummary(t, h, "org_a", id); code != http.StatusOK || c != nil {
		t.Fatalf("agent: code=%d credits=%v want absent", code, c)
	}
}

func TestJobSummary_CreditsOrgScoped(t *testing.T) {
	h, db := lcRouter(t)
	setJobCredits(t, "1")
	id := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 1, map[string]any{"url": "https://e.example/p"})
	if _, code := creditsOfSummary(t, h, "org_b", id); code != http.StatusNotFound {
		t.Fatalf("foreign org read status=%d want 404", code)
	}
}

func TestJobLists_CarryCreditsAndStillDecode(t *testing.T) {
	h, db := lcRouter(t)
	done := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	for i := 0; i < 2; i++ {
		lcAppend(t, db, done, quarrycontracts.EvtPageFetched, uint64(i+1), map[string]any{"url": "https://e.example/p"})
	}
	running := lcSeedJob(t, db, "org_a", "crawl", "running", nil)
	search := lcSeedJob(t, db, "org_a", "search", "running", nil)
	lcAppend(t, db, search, quarrycontracts.EvtPageFetched, 1, map[string]any{"url": "https://e.example/s"})

	ongoing := func() []jobWire {
		rec := lcDo(h, http.MethodGet, "/v1/jobs/ongoing?org_id=org_a", nil)
		if rec.Code != http.StatusOK {
			t.Fatalf("ongoing status=%d", rec.Code)
		}
		var env struct {
			Data []jobWire `json:"data"`
		}
		if err := json.Unmarshal(rec.Body.Bytes(), &env); err != nil {
			t.Fatalf("decode: %v: %s", err, rec.Body.String())
		}
		return env.Data
	}

	setJobCredits(t, "")
	if rows := ongoing(); len(rows) != 2 {
		t.Fatalf("flag off ongoing rows=%d want 2", len(rows))
	} else {
		for _, row := range rows {
			if row.Credits != nil {
				t.Fatalf("flag off: row %s carries credits %+v", row.JobID, row.Credits)
			}
		}
	}

	setJobCredits(t, "true")
	byID := map[string]jobWire{}
	for _, row := range ongoing() {
		byID[row.JobID] = row
	}
	if c := byID[string(running)].Credits; c == nil || c.Used != 0 || c.Final {
		t.Fatalf("running crawl row credits=%+v want used 0, not final", c)
	}
	if c := byID[string(search)].Credits; c != nil {
		t.Fatalf("search row credits=%+v want none", c)
	}

	// The by-kind list returns every status: the completed crawl carries 2.
	rec := lcDo(h, http.MethodGet, "/v1/crawl/jobs?org_id=org_a", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("by-kind list status=%d: %s", rec.Code, rec.Body.String())
	}
	var env struct {
		Data struct {
			Items []jobWire `json:"items"`
		} `json:"data"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &env); err != nil {
		t.Fatalf("decode by-kind: %v: %s", err, rec.Body.String())
	}
	var found bool
	for _, row := range env.Data.Items {
		if row.JobID == string(done) {
			found = true
			if row.Credits == nil || row.Credits.Used != 2 || !row.Credits.Final {
				t.Fatalf("completed crawl row credits=%+v want used 2, final", row.Credits)
			}
		}
	}
	if !found {
		t.Fatalf("completed crawl missing from by-kind list: %s", rec.Body.String())
	}
}

// The per-job credits and the team figure agree: same price, same event rule.
func TestJobCredits_AgreeWithTeamCreditUsage(t *testing.T) {
	_, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	for i := 0; i < 5; i++ {
		lcAppend(t, db, id, quarrycontracts.EvtPageFetched, uint64(i+1), map[string]any{"url": "https://e.example/p"})
	}
	lcAppend(t, db, id, quarrycontracts.EvtRunStarted, 6, map[string]any{})
	lcAppend(t, db, id, quarrycontracts.EvtPageQueued, 7, map[string]any{"url": "https://e.example/q"})

	view := buildSummary(mustJob(t, db, "org_a", id), db.Events().SummaryForJob(id))
	team, err := db.TeamUsage().CreditUsage("org_a", "7d")
	if err != nil {
		t.Fatal(err)
	}
	if view.Credits == nil || view.Credits.Used != team.CreditsUsed {
		t.Fatalf("job credits=%+v team=%v: the two figures must agree", view.Credits, team.CreditsUsed)
	}
}

func mustJob(t *testing.T, db store.DB, org string, id quarrycontracts.ID) store.Job {
	t.Helper()
	j, ok := db.Jobs().GetByOrg(org, id)
	if !ok {
		t.Fatal("job missing")
	}
	return j
}
