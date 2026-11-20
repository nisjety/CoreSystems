package canvas

// Postgres tests for the history window of one canvas (D18, WS04-S11, D21m). Like the rest of this package they run only when
// WIKI_TEST_DATABASE_URL names a disposable local *_test database, and skip otherwise.

import (
	"context"
	"testing"
	"time"
)

// instantPtr returns a pointer to t, for a Floor's timestamp.
func instantPtr(t time.Time) *time.Time { return &t }

// windowedRestore restores from under floor, over the expected head.
func windowedRestore(ctx context.Context, repo *Repo, canvasID, from, expected, key string, floor Floor) (WriteResult, error) {
	return repo.Restore(ctx, RestoreInput{
		OrgID:                "org-a",
		SpaceRef:             "space-a",
		CanvasID:             canvasID,
		RestoreFromVersionID: from,
		ExpectedVersionID:    ptr(expected),
		IdempotencyKey:       key,
		Author:               canvasAuthor(),
		Provenance:           canvasProvenance(key),
		Floor:                floor,
	})
}

// listIDs returns the version ids of one page, newest first, and the ids marked current.
func listIDs(page VersionPage) (ids, current []string) {
	for _, v := range page.Versions {
		ids = append(ids, v.VersionID)
		if v.Current {
			current = append(current, v.VersionID)
		}
	}
	return ids, current
}

// TestCanvasWindowShowsACanvasCreatedInsideTheWindow: a canvas created after the window starts is shown whole, to the head, the
// unchanged answer, the version list and one version. Nothing is withheld.
func TestCanvasWindowShowsACanvasCreatedInsideTheWindow(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	created := createCanvas(ctx, t, repo, "org-a", "space-a", "# Room\n")
	// The window starts a minute before the canvas existed, under the audience revision the canvas was written under.
	floor := Floor{At: instantPtr(created.Version.CreatedAt.Add(-time.Minute)), Revision: 4}

	head, err := repo.Head(ctx, "org-a", "space-a", nil)
	if err != nil || head.Head == nil || head.Head.Content != "# Room\n" {
		t.Fatalf("head = %+v, %v; want the canvas text", head, err)
	}
	page, err := repo.ListVersions(ctx, "org-a", "space-a", nil, 10, floor)
	if err != nil || len(page.Versions) != 1 || page.HiddenBefore || !page.Versions[0].Current {
		t.Fatalf("version list inside the window = %+v, %v; want the one current version, nothing hidden", page, err)
	}
	v, err := repo.GetVersion(ctx, "org-a", "space-a", created.Version.VersionID, floor)
	if err != nil || v.Content != "# Room\n" {
		t.Fatalf("version inside the window = %+v, %v; want its text", v, err)
	}
}

// TestCanvasWindowShowsTheCurrentCanvasButNotItsEarlierHistory (D21m) is the windowed member's case: the canvas predates the window
// and was edited after it. The member reads the head (the edit), lists only the edit (marked current) with HiddenBefore set, and
// may not read or restore the pre-window version. An unwindowed reader still sees the whole history.
func TestCanvasWindowShowsTheCurrentCanvasButNotItsEarlierHistory(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	base := createCanvas(ctx, t, repo, "org-a", "space-a", "Before the window.\n")
	floor := Floor{At: instantPtr(base.Version.CreatedAt.Add(time.Millisecond))}
	// Let the edit's transaction start well after the window, so its own created_at is inside the window.
	time.Sleep(20 * time.Millisecond)
	edited, err := saveCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, ptr(base.Version.VersionID),
		"Before the window.\nAfter the window.\n", "edit-after-window", canvasAuthor())
	if err != nil || !edited.Inserted {
		t.Fatalf("the edit after the window = %+v, %v; want an inserted version", edited, err)
	}
	if !edited.Version.CreatedAt.After(*floor.At) {
		t.Fatalf("the edit began at %v, not after the window at %v; the test would prove nothing", edited.Version.CreatedAt, *floor.At)
	}

	head, err := repo.Head(ctx, "org-a", "space-a", nil)
	if err != nil || head.Head == nil || head.Head.VersionID != edited.Version.VersionID {
		t.Fatalf("the head = %+v, %v; want the edit", head, err)
	}
	page, err := repo.ListVersions(ctx, "org-a", "space-a", nil, 10, floor)
	ids, current := listIDs(page)
	if err != nil || len(ids) != 1 || ids[0] != edited.Version.VersionID || len(current) != 1 || !page.HiddenBefore {
		t.Fatalf("the windowed list = %v (current %v, hidden %v), %v; want only the edit, current, with hidden_before", ids, current,
			page.HiddenBefore, err)
	}
	_, err = repo.GetVersion(ctx, "org-a", "space-a", base.Version.VersionID, floor)
	requireCanvasError(t, err, CodeHistoryFloor)
	if _, err := repo.GetVersion(ctx, "org-a", "space-a", edited.Version.VersionID, floor); err != nil {
		t.Fatalf("the edit inside the window was refused: %v", err)
	}
	_, err = windowedRestore(ctx, repo, base.CanvasID, base.Version.VersionID, edited.Version.VersionID, "restore-pre-window", floor)
	requireCanvasError(t, err, CodeHistoryFloor)
	if n := versionsOfCanvas(ctx, t, pool, base.CanvasID); n != 2 {
		t.Fatalf("a refused restore left %d versions, want 2", n)
	}
	probe, err := repo.Probe(ctx, "org-a", "space-a", base.Version.VersionID, floor)
	if err != nil || !probe.VersionFound || probe.InWindow {
		t.Fatalf("probe of the pre-window version = %+v, %v; want found and outside the window", probe, err)
	}

	whole, err := repo.ListVersions(ctx, "org-a", "space-a", nil, 10, Floor{})
	if err != nil || len(whole.Versions) != 2 || whole.HiddenBefore {
		t.Fatalf("the unwindowed list = %+v, %v; want both versions", whole, err)
	}
}

// TestCanvasWindowShowsAnOldHeadAsCurrent (D21m): a canvas written only before the window is still the room's current document, so
// the windowed member reads it, lists it marked current and may read it as a version. Restoring it is unchanged. Nothing older is
// withheld, so hidden_before is false.
func TestCanvasWindowShowsAnOldHeadAsCurrent(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	created := createCanvas(ctx, t, repo, "org-a", "space-a", "# Old room\n")
	floor := Floor{At: instantPtr(created.Version.CreatedAt.Add(time.Minute))}
	head := created.Version.VersionID

	known, err := repo.Head(ctx, "org-a", "space-a", &head)
	if err != nil || !known.Unchanged {
		t.Fatalf("the unchanged head = %+v, %v; want Unchanged", known, err)
	}
	page, err := repo.ListVersions(ctx, "org-a", "space-a", nil, 10, floor)
	ids, current := listIDs(page)
	if err != nil || len(ids) != 1 || len(current) != 1 || page.HiddenBefore {
		t.Fatalf("the windowed list = %v (current %v, hidden %v), %v; want the head alone, current, nothing hidden", ids, current,
			page.HiddenBefore, err)
	}
	if _, err := repo.GetVersion(ctx, "org-a", "space-a", head, floor); err != nil {
		t.Fatalf("the head read as a version under the window was refused: %v", err)
	}
	res, err := windowedRestore(ctx, repo, created.CanvasID, head, head, "restore-head", floor)
	if err != nil || !res.Unchanged {
		t.Fatalf("restoring the head under the window = %+v, %v; want unchanged", res, err)
	}
}

// TestCanvasWindowRevisionFollowsTheAuditRevisionStamp: a version is stamped with the audience revision of its decision. A window at
// revision 5 withholds the first version (stamped 4) and lists the head (stamped 6); a window at 4 lists both.
func TestCanvasWindowRevisionFollowsTheAuditRevisionStamp(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	created := createCanvas(ctx, t, repo, "org-a", "space-a", "first")
	later, err := repo.Save(ctx, SaveInput{
		OrgID:             "org-a",
		SpaceRef:          "space-a",
		CanvasID:          created.CanvasID,
		ExpectedVersionID: ptr(created.Version.VersionID),
		Content:           "second",
		IdempotencyKey:    "edit-revision-6",
		Author:            canvasAuthor(),
		Provenance:        Provenance{DecisionRef: "decision-edit-revision-6", AuthorityRevision: 7, AudienceRevision: 6},
	})
	if err != nil || !later.Inserted {
		t.Fatalf("the revision-6 edit = %+v, %v; want an inserted version", later, err)
	}

	page, err := repo.ListVersions(ctx, "org-a", "space-a", nil, 10, Floor{Revision: 4})
	if err != nil || len(page.Versions) != 2 || page.HiddenBefore {
		t.Fatalf("a window at the first version's revision = %+v, %v; want both versions", page, err)
	}
	page, err = repo.ListVersions(ctx, "org-a", "space-a", nil, 10, Floor{Revision: 5})
	ids, _ := listIDs(page)
	if err != nil || len(ids) != 1 || ids[0] != later.Version.VersionID || !page.HiddenBefore {
		t.Fatalf("a window at revision 5 = %v (hidden %v), %v; want the revision-6 head only, hidden_before", ids, page.HiddenBefore, err)
	}
	_, err = repo.GetVersion(ctx, "org-a", "space-a", created.Version.VersionID, Floor{Revision: 5})
	requireCanvasError(t, err, CodeHistoryFloor)
}

// TestCanvasWindowLeavesTheScopeRefusalsAhead: a version of another org or another Space is not found, even under a window. The window
// is applied only to a version the caller's canvas holds, so it never turns a scope refusal into a history refusal.
func TestCanvasWindowLeavesTheScopeRefusalsAhead(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	mine := createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")
	other := createCanvas(ctx, t, repo, "org-b", "space-b", "bravo")
	floor := Floor{At: instantPtr(mine.Version.CreatedAt.Add(time.Minute))}

	_, err := repo.GetVersion(ctx, "org-a", "space-a", other.Version.VersionID, floor)
	requireCanvasError(t, err, CodeVersionNotFound)
	_, err = repo.Head(ctx, "org-b", "space-a", nil)
	requireCanvasError(t, err, CodeCanvasNotFound)
	_, err = windowedRestore(ctx, repo, mine.CanvasID, other.Version.VersionID, mine.Version.VersionID, "restore-foreign", floor)
	requireCanvasError(t, err, CodeVersionNotFound)
}
