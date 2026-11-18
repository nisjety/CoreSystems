package canvas

// Postgres tests for the history window of one canvas (D18 and WS04-S11). Like the rest of this package they run only when
// WIKI_TEST_DATABASE_URL names a disposable local *_test database, and skip otherwise.

import (
	"context"
	"testing"
	"time"
)

// instantPtr returns a pointer to t, for a Floor's timestamp.
func instantPtr(t time.Time) *time.Time { return &t }

// TestCanvasWindowShowsACanvasCreatedInsideTheWindow: a canvas created after the window starts is shown whole, to the head, the
// unchanged answer, the version list and one version. Every version it holds was written inside the window.
func TestCanvasWindowShowsACanvasCreatedInsideTheWindow(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	created := createCanvas(ctx, t, repo, "org-a", "space-a", "# Room\n")
	// The window starts a minute before the canvas existed, under the audience revision the canvas was written under.
	floor := Floor{At: instantPtr(created.Version.CreatedAt.Add(-time.Minute)), Revision: 4}

	head, err := repo.Head(ctx, "org-a", "space-a", nil, floor)
	if err != nil || head.Head == nil || head.Head.Content != "# Room\n" {
		t.Fatalf("head inside the window = %+v, %v; want the canvas text", head, err)
	}
	known := created.Version.VersionID
	unchanged, err := repo.Head(ctx, "org-a", "space-a", &known, floor)
	if err != nil || !unchanged.Unchanged {
		t.Fatalf("unchanged head inside the window = %+v, %v; want Unchanged", unchanged, err)
	}
	page, err := repo.ListVersions(ctx, "org-a", "space-a", nil, 10, floor)
	if err != nil || len(page.Versions) != 1 {
		t.Fatalf("version list inside the window = %+v, %v; want the one version", page, err)
	}
	v, err := repo.GetVersion(ctx, "org-a", "space-a", created.Version.VersionID, floor)
	if err != nil || v.Content != "# Room\n" {
		t.Fatalf("version inside the window = %+v, %v; want its text", v, err)
	}
}

// TestCanvasWindowHidesACanvasCreatedBeforeTheWindow: a canvas created before the window starts is hidden from that reader on every
// read that would show its text or its history, the unchanged answer included. An unwindowed reader still sees it.
func TestCanvasWindowHidesACanvasCreatedBeforeTheWindow(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	created := createCanvas(ctx, t, repo, "org-a", "space-a", "# Room\n")
	floor := Floor{At: instantPtr(created.Version.CreatedAt.Add(time.Minute))}
	known := created.Version.VersionID

	_, err := repo.Head(ctx, "org-a", "space-a", nil, floor)
	requireCanvasError(t, err, CodeHistoryFloor)
	_, err = repo.Head(ctx, "org-a", "space-a", &known, floor)
	requireCanvasError(t, err, CodeHistoryFloor)
	_, err = repo.ListVersions(ctx, "org-a", "space-a", nil, 10, floor)
	requireCanvasError(t, err, CodeHistoryFloor)
	_, err = repo.GetVersion(ctx, "org-a", "space-a", created.Version.VersionID, floor)
	requireCanvasError(t, err, CodeHistoryFloor)

	whole, err := repo.Head(ctx, "org-a", "space-a", nil, Floor{})
	if err != nil || whole.Head == nil || whole.Head.Content != "# Room\n" {
		t.Fatalf("the unwindowed head = %+v, %v; want the canvas text", whole, err)
	}
}

// TestCanvasWindowHidesALaterVersionThatCarriesPreWindowText is the case the window is built for. The canvas predates the window, and
// an edit after the window starts keeps the line written before it. The edit is inside the window by its time, but the text it stores
// is not, so no read shows it: not the head, not the version, not the list.
func TestCanvasWindowHidesALaterVersionThatCarriesPreWindowText(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	const preWindowLine = "Secret plan: launch in March.\n"
	base := createCanvas(ctx, t, repo, "org-a", "space-a", preWindowLine)
	floor := Floor{At: instantPtr(base.Version.CreatedAt.Add(time.Millisecond))}
	// Let the edit's transaction start well after the window, so its own created_at is inside the window.
	time.Sleep(20 * time.Millisecond)

	edited, err := saveCanvas(ctx, repo, "org-a", "space-a", base.CanvasID, ptr(base.Version.VersionID),
		preWindowLine+"\nNew line after the window.\n", "edit-after-window", canvasAuthor())
	if err != nil || !edited.Inserted {
		t.Fatalf("the edit after the window = %+v, %v; want an inserted version", edited, err)
	}
	if !edited.Version.CreatedAt.After(*floor.At) {
		t.Fatalf("the edit began at %v, not after the window at %v; the test would prove nothing", edited.Version.CreatedAt, *floor.At)
	}

	_, err = repo.Head(ctx, "org-a", "space-a", nil, floor)
	requireCanvasError(t, err, CodeHistoryFloor)
	_, err = repo.GetVersion(ctx, "org-a", "space-a", edited.Version.VersionID, floor)
	requireCanvasError(t, err, CodeHistoryFloor)
	_, err = repo.ListVersions(ctx, "org-a", "space-a", nil, 10, floor)
	requireCanvasError(t, err, CodeHistoryFloor)

	// The unwindowed reader still reads the head, which now holds the edit.
	whole, err := repo.Head(ctx, "org-a", "space-a", nil, Floor{})
	if err != nil || whole.Head == nil || whole.Head.VersionID != edited.Version.VersionID {
		t.Fatalf("the unwindowed head = %+v, %v; want the edit", whole, err)
	}
}

// TestCanvasWindowRevisionFollowsTheAuditRevisionStamp: a version is stamped with the audience revision of its decision. A window at
// revision 4 admits a canvas whose versions are all stamped 4 or later, and a window at 5 hides it, because its first version is
// stamped 4, even though its head is stamped 6.
func TestCanvasWindowRevisionFollowsTheAuditRevisionStamp(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	created := createCanvas(ctx, t, repo, "org-a", "space-a", "first")
	head := created.Version.VersionID
	later, err := repo.Save(ctx, SaveInput{
		OrgID:             "org-a",
		SpaceRef:          "space-a",
		CanvasID:          created.CanvasID,
		ExpectedVersionID: ptr(head),
		Content:           "second",
		IdempotencyKey:    "edit-revision-6",
		Author:            canvasAuthor(),
		Provenance:        Provenance{DecisionRef: "decision-edit-revision-6", AuthorityRevision: 7, AudienceRevision: 6},
	})
	if err != nil || !later.Inserted {
		t.Fatalf("the revision-6 edit = %+v, %v; want an inserted version", later, err)
	}

	if _, err := repo.Head(ctx, "org-a", "space-a", nil, Floor{Revision: 4}); err != nil {
		t.Fatalf("a window at the first version's revision refused the canvas: %v", err)
	}
	_, err = repo.Head(ctx, "org-a", "space-a", nil, Floor{Revision: 5})
	requireCanvasError(t, err, CodeHistoryFloor)
}

// TestCanvasWindowLeavesTheScopeRefusalsAhead: a version of another org or another Space is not found, even under a window. The window
// is applied only to a canvas the caller may already read, so it never turns a scope refusal into a history refusal.
func TestCanvasWindowLeavesTheScopeRefusalsAhead(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	mine := createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")
	other := createCanvas(ctx, t, repo, "org-b", "space-b", "bravo")
	floor := Floor{At: instantPtr(mine.Version.CreatedAt.Add(time.Minute))}

	_, err := repo.GetVersion(ctx, "org-a", "space-a", other.Version.VersionID, floor)
	requireCanvasError(t, err, CodeVersionNotFound)
	_, err = repo.Head(ctx, "org-b", "space-a", nil, floor)
	requireCanvasError(t, err, CodeCanvasNotFound)
}
