package rawstore

import (
	"testing"

	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/noticeid"
)

// A stored TED search page dates its notices under their public ids, next to Doffin's.
func TestPublicationDatesCoverBothSources(t *testing.T) {
	doffinStore, pool := openStore(t)
	tedStore, err := NewForSource(pool, noticeid.SourceTED)
	if err != nil {
		t.Fatalf("NewForSource: %v", err)
	}
	ctx := testCtx(t)
	run := beginSyncRun(t, doffinStore, "2026-10-27", "2026-10-27")
	page := []byte(`{"notices":[{"publication-number":"741144-2026","publication-date":"2026-10-27+01:00"}],"totalNoticeCount":1}`)
	if err := tedStore.PutSearchPage(ctx, run, doffinsync.SearchPage{QueryKey: "ted-test", Page: 1, ContentType: "application/json", Body: page, FetchedAt: utcDay(t, "2026-10-27")}); err != nil {
		t.Fatalf("PutSearchPage: %s", err)
	}
	dates, err := doffinStore.PublicationDates(ctx)
	if err != nil {
		t.Fatalf("PublicationDates: %v", err)
	}
	if dates["TED-741144-2026"] != "2026-10-27" {
		t.Errorf("dates %v, want TED-741144-2026 dated 2026-10-27", dates)
	}
}
