//go:build integration

package repo_test

// WS14-S4: ListSharedWith against a real Postgres (TEST_DATABASE_URL). The ids
// come from Control's grant store; the repository only resolves them to
// headers inside the viewer's org.

import (
	"context"
	"testing"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
)

func TestListSharedWithExcludesOwnDeletedAndOtherOrgAndReturnsNoContent(t *testing.T) {
	r, _, org, cleanup := setupSpaceFilesDB(t)
	defer cleanup()
	ctx := context.Background()
	mk := func(orgID, owner, title string) string {
		res, err := r.Create(ctx, model.CreateDocumentInput{OrgID: orgID, Source: "s", Type: "note", Title: title, Content: "SECRET BODY", OwnerID: owner, Visibility: "private"})
		if err != nil {
			t.Fatal(err)
		}
		return res.Document.DocumentID
	}
	byA := mk(org, "user-a", "shared by A")
	byB := mk(org, "user-b", "B own doc")
	deleted := mk(org, "user-a", "deleted")
	other := mk(org+"-other", "user-a", "other org")
	if err := r.SoftDelete(ctx, org, deleted, "user-a"); err != nil {
		t.Fatal(err)
	}

	got, err := r.ListSharedWith(ctx, org, "user-b", []string{byA, byB, deleted, other, "", "missing"}, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 1 || got[0].DocumentID != byA || got[0].OwnerID != "user-a" || got[0].Title != "shared by A" {
		t.Fatalf("shared = %+v, want only A's live document", got)
	}
	// SharedDocumentSummary has no content field by construction; the header
	// must not smuggle the body into the title.
	if got[0].Title == "SECRET BODY" {
		t.Fatal("content leaked into the header")
	}
	if empty, err := r.ListSharedWith(ctx, org, "user-b", nil, 10); err != nil || len(empty) != 0 {
		t.Fatalf("no grants = %+v err=%v", empty, err)
	}
}
