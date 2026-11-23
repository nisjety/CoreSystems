package repo

// WS14-S3: an idempotency key that already names a live row may be reused only on
// the same Space line. sameRoomFileKey decides that before the same-key UPDATE, so
// these cases need no database.

import (
	"testing"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
)

func TestSameRoomFileKeyKeepsEveryWriteOnItsOwnSpaceLine(t *testing.T) {
	spaceA, spaceB := "space-a", "space-b"
	room := &model.SpaceFileConstraints{}
	cases := []struct {
		name     string
		existing *model.Document
		input    model.CreateDocumentInput
		want     bool
	}{
		{"ordinary rewrites ordinary", &model.Document{Type: "text"}, model.CreateDocumentInput{Type: "text"}, true},
		{"ordinary never rewrites a room file", &model.Document{Type: SpaceFileType, SpaceRef: &spaceA}, model.CreateDocumentInput{Type: "text"}, false},
		{"import rewrites its own Space row", &model.Document{Type: "note", SpaceRef: &spaceA}, model.CreateDocumentInput{Type: "note", SpaceRef: spaceA}, true},
		{"unscoped write never rewrites a Space row", &model.Document{Type: "note", SpaceRef: &spaceA}, model.CreateDocumentInput{Type: "note"}, false},
		{"import never rewrites another Space's row", &model.Document{Type: "note", SpaceRef: &spaceA}, model.CreateDocumentInput{Type: "note", SpaceRef: spaceB}, false},
		{"import never rewrites an unscoped row", &model.Document{Type: "note"}, model.CreateDocumentInput{Type: "note", SpaceRef: spaceA}, false},
		{"room file refreshes its own Space", &model.Document{Type: SpaceFileType, SpaceRef: &spaceA}, model.CreateDocumentInput{Type: SpaceFileType, SpaceRef: spaceA, SpaceFile: room}, true},
		{"room file never crosses Spaces", &model.Document{Type: SpaceFileType, SpaceRef: &spaceA}, model.CreateDocumentInput{Type: SpaceFileType, SpaceRef: spaceB, SpaceFile: room}, false},
		{"room file never rewrites a Space note", &model.Document{Type: "note", SpaceRef: &spaceA}, model.CreateDocumentInput{Type: SpaceFileType, SpaceRef: spaceA, SpaceFile: room}, false},
	}
	for _, tc := range cases {
		if got := sameRoomFileKey(tc.existing, tc.input); got != tc.want {
			t.Errorf("%s: sameRoomFileKey = %v, want %v", tc.name, got, tc.want)
		}
	}
}
