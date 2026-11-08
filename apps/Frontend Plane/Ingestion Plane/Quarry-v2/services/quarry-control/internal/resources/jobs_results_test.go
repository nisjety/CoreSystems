package resources

import (
	"encoding/json"
	"fmt"
	"net/http"
	"testing"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

type resultsEnv struct {
	Data jobResultsView `json:"data"`
}

func getResults(t *testing.T, h http.Handler, id quarrycontracts.ID, query string) (int, resultsEnv) {
	t.Helper()
	rec := lcDo(h, http.MethodGet, "/v1/jobs/"+string(id)+"/results?"+query, nil)
	var env resultsEnv
	if rec.Code == http.StatusOK {
		if err := json.Unmarshal(rec.Body.Bytes(), &env); err != nil {
			t.Fatalf("decode: %v: %s", err, rec.Body.String())
		}
	}
	return rec.Code, env
}

func fetchedPayload(url string, extra map[string]any) map[string]any {
	p := map[string]any{"url": url, "status": float64(200), "fingerprint": "blake3:x", "links": float64(3), "content_type": "text/html", "title": "T " + url}
	for k, v := range extra {
		p[k] = v
	}
	return p
}

func TestResults_RowsComeFromPageFetchedInSeqOrder(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "running", nil)
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 1, fetchedPayload("https://e.test/a", map[string]any{
		"markdown_artifact_id": "art_01MD", "source_artifact_id": "art_01SRC", "bytes": float64(900), "word_count": float64(120), "driver": "static",
	}))
	lcAppend(t, db, id, quarrycontracts.EvtPageFailed, 2, map[string]any{"url": "https://e.test/bad", "final": true, "category": "client_4xx"})
	lcAppend(t, db, id, quarrycontracts.EvtPageBlocked, 3, map[string]any{"url": "https://blocked.test/", "category": "host_blocked"})
	lcAppend(t, db, id, quarrycontracts.EvtPageExtracted, 4, map[string]any{"url": "https://e.test/a", "title": "x"})
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 5, fetchedPayload("https://e.test/b", nil))

	code, env := getResults(t, h, id, "org_id=org_a")
	if code != http.StatusOK {
		t.Fatalf("status %d", code)
	}
	items := env.Data.Items
	if len(items) != 2 {
		t.Fatalf("items = %#v (failed, blocked and extracted rows are not results)", items)
	}
	first := items[0]
	if first.Seq != 1 || first.URL != "https://e.test/a" || first.Status != 200 || first.ContentType != "text/html" {
		t.Fatalf("first = %#v", first)
	}
	if first.MarkdownArtifactID != "art_01MD" || first.SourceArtifactID != "art_01SRC" || first.Bytes != 900 || first.WordCount != 120 || first.Driver != "static" {
		t.Fatalf("first result fields = %#v", first)
	}
	if first.FetchedAt == "" {
		t.Fatal("fetched_at missing")
	}
	// A row from before the new payload fields tolerates their absence.
	second := items[1]
	if second.Seq != 5 || second.URL != "https://e.test/b" || second.MarkdownArtifactID != "" || second.SourceArtifactID != "" || second.Bytes != 0 {
		t.Fatalf("second = %#v", second)
	}
	if env.Data.NextSeq != nil {
		t.Fatalf("a short page must not carry next_seq: %d", *env.Data.NextSeq)
	}
}

func TestResults_AbsentFieldsAreLeftOutOfTheWire(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "batch", "completed", nil)
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 1, fetchedPayload("https://e.test/a", nil))
	rec := lcDo(h, http.MethodGet, "/v1/jobs/"+string(id)+"/results?org_id=org_a", nil)
	var raw struct {
		Data struct {
			Items []map[string]any `json:"items"`
		} `json:"data"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &raw); err != nil {
		t.Fatal(err)
	}
	if len(raw.Data.Items) != 1 {
		t.Fatalf("body = %s", rec.Body.String())
	}
	for _, key := range []string{"markdown_artifact_id", "source_artifact_id", "bytes", "word_count", "driver"} {
		if _, ok := raw.Data.Items[0][key]; ok {
			t.Fatalf("%s must be absent, not empty: %s", key, rec.Body.String())
		}
	}
}

func TestResults_CursorPagingWithNextSeq(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	for i := 1; i <= 5; i++ {
		lcAppend(t, db, id, quarrycontracts.EvtPageFetched, uint64(i*2), fetchedPayload(fmt.Sprintf("https://e.test/%d", i), nil))
	}
	code, page1 := getResults(t, h, id, "org_id=org_a&limit=2")
	if code != http.StatusOK || len(page1.Data.Items) != 2 || page1.Data.NextSeq == nil || *page1.Data.NextSeq != 4 {
		t.Fatalf("page1 = %#v", page1.Data)
	}
	_, page2 := getResults(t, h, id, fmt.Sprintf("org_id=org_a&limit=2&after_seq=%d", *page1.Data.NextSeq))
	if len(page2.Data.Items) != 2 || page2.Data.Items[0].URL != "https://e.test/3" || page2.Data.NextSeq == nil || *page2.Data.NextSeq != 8 {
		t.Fatalf("page2 = %#v", page2.Data)
	}
	_, page3 := getResults(t, h, id, fmt.Sprintf("org_id=org_a&limit=2&after_seq=%d", *page2.Data.NextSeq))
	if len(page3.Data.Items) != 1 || page3.Data.Items[0].URL != "https://e.test/5" || page3.Data.NextSeq != nil {
		t.Fatalf("page3 = %#v", page3.Data)
	}
}

func TestResults_LimitIsClampedTo500AndDefaultsTo100(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	for i := 1; i <= 520; i++ {
		lcAppend(t, db, id, quarrycontracts.EvtPageFetched, uint64(i), fetchedPayload(fmt.Sprintf("https://e.test/%d", i), nil))
	}
	_, def := getResults(t, h, id, "org_id=org_a")
	if len(def.Data.Items) != 100 || def.Data.NextSeq == nil || *def.Data.NextSeq != 100 {
		t.Fatalf("default page = %d items, next %v", len(def.Data.Items), def.Data.NextSeq)
	}
	_, big := getResults(t, h, id, "org_id=org_a&limit=9999")
	if len(big.Data.Items) != 500 || big.Data.NextSeq == nil || *big.Data.NextSeq != 500 {
		t.Fatalf("clamped page = %d items, next %v", len(big.Data.Items), big.Data.NextSeq)
	}
	_, junk := getResults(t, h, id, "org_id=org_a&limit=abc")
	if len(junk.Data.Items) != 100 {
		t.Fatalf("a bad limit falls back to the default: %d", len(junk.Data.Items))
	}
}

func TestResults_AUrlFetchedTwiceAppearsTwiceUnlessLatest(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 1, fetchedPayload("https://e.test/a", map[string]any{"markdown_artifact_id": "art_old"}))
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 2, fetchedPayload("https://e.test/b", nil))
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 3, fetchedPayload("https://e.test/a", map[string]any{"markdown_artifact_id": "art_new"}))

	_, all := getResults(t, h, id, "org_id=org_a")
	if len(all.Data.Items) != 3 {
		t.Fatalf("all = %#v", all.Data.Items)
	}
	_, latest := getResults(t, h, id, "org_id=org_a&latest=true")
	if len(latest.Data.Items) != 2 {
		t.Fatalf("latest = %#v", latest.Data.Items)
	}
	if latest.Data.Items[0].URL != "https://e.test/b" || latest.Data.Items[0].Seq != 2 {
		t.Fatalf("latest[0] = %#v", latest.Data.Items[0])
	}
	if latest.Data.Items[1].URL != "https://e.test/a" || latest.Data.Items[1].MarkdownArtifactID != "art_new" || latest.Data.Items[1].Seq != 3 {
		t.Fatalf("latest[1] = %#v (the later fetch wins)", latest.Data.Items[1])
	}
	if latest.Data.ScanCapped {
		t.Fatal("a small job is never scan_capped")
	}
}

func TestResults_LatestPagesTheDedupedRows(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	// 6 fetches over 3 URLs: the latest rows are seq 4, 5, 6.
	for i := 1; i <= 6; i++ {
		lcAppend(t, db, id, quarrycontracts.EvtPageFetched, uint64(i), fetchedPayload(fmt.Sprintf("https://e.test/%d", (i-1)%3), nil))
	}
	_, p1 := getResults(t, h, id, "org_id=org_a&latest=1&limit=2")
	if len(p1.Data.Items) != 2 || p1.Data.Items[0].Seq != 4 || p1.Data.Items[1].Seq != 5 || p1.Data.NextSeq == nil || *p1.Data.NextSeq != 5 {
		t.Fatalf("p1 = %#v", p1.Data)
	}
	_, p2 := getResults(t, h, id, "org_id=org_a&latest=1&limit=2&after_seq=5")
	if len(p2.Data.Items) != 1 || p2.Data.Items[0].Seq != 6 || p2.Data.NextSeq != nil {
		t.Fatalf("p2 = %#v", p2.Data)
	}
}

func TestResults_OrgScopedAndUnknownIsNotFound(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	lcAppend(t, db, id, quarrycontracts.EvtPageFetched, 1, fetchedPayload("https://e.test/a", nil))

	if code, _ := getResults(t, h, id, "org_id=org_b"); code != http.StatusNotFound {
		t.Fatalf("foreign org status %d", code)
	}
	if code, _ := getResults(t, h, "job_01J9FFFFFFFFFFFFFFFFFFFFFF", "org_id=org_a"); code != http.StatusNotFound {
		t.Fatalf("unknown job status %d", code)
	}
	if code, _ := getResults(t, h, id, ""); code != http.StatusBadRequest {
		t.Fatalf("missing org status %d", code)
	}
}

func TestResults_AnotherJobsRowsNeverLeak(t *testing.T) {
	t.Parallel()
	h, db := lcRouter(t)
	mine := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	other := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	lcAppend(t, db, mine, quarrycontracts.EvtPageFetched, 1, fetchedPayload("https://e.test/mine", nil))
	lcAppend(t, db, other, quarrycontracts.EvtPageFetched, 2, fetchedPayload("https://e.test/other", nil))
	_, env := getResults(t, h, mine, "org_id=org_a")
	if len(env.Data.Items) != 1 || env.Data.Items[0].URL != "https://e.test/mine" {
		t.Fatalf("items = %#v", env.Data.Items)
	}
}

func TestResults_NumbersInProcessPayloadsAreRead(t *testing.T) {
	t.Parallel()
	// An event built in process (not read back from JSON) holds typed integers.
	row := resultFromEvent(quarrycontracts.Event{Seq: 7, Payload: map[string]any{
		"url": "https://e.test/a", "status": uint16(404), "bytes": uint64(10), "word_count": int(3),
	}})
	if row.Status != 404 || row.Bytes != 10 || row.WordCount != 3 {
		t.Fatalf("row = %#v", row)
	}
}
