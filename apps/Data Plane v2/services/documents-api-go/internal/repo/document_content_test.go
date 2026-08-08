package repo

import (
	"testing"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
)

func TestDocumentContentUnchanged(t *testing.T) {
	existing := &model.Document{Title: "Quarterly Report", Content: "revenue up 12%"}

	cases := []struct {
		name  string
		input model.CreateDocumentInput
		want  bool
	}{
		{
			name:  "identical content and title is a no-op",
			input: model.CreateDocumentInput{Title: "Quarterly Report", Content: "revenue up 12%"},
			want:  true,
		},
		{
			name:  "changed content is not a no-op",
			input: model.CreateDocumentInput{Title: "Quarterly Report", Content: "revenue up 18%"},
			want:  false,
		},
		{
			name:  "changed title is not a no-op",
			input: model.CreateDocumentInput{Title: "Q3 Report", Content: "revenue up 12%"},
			want:  false,
		},
		{
			name:  "both changed is not a no-op",
			input: model.CreateDocumentInput{Title: "Q3 Report", Content: "revenue up 18%"},
			want:  false,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := documentContentUnchanged(existing, tc.input); got != tc.want {
				t.Errorf("documentContentUnchanged = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestSourceVisibilityUpdate(t *testing.T) {
	private := &model.Document{Visibility: "private"}
	org := &model.Document{Visibility: "org"}

	cases := []struct {
		name     string
		existing *model.Document
		input    model.CreateDocumentInput
		want     string // "" means no update
	}{
		{
			// The case this exists for: a connector reports the SharePoint ACL
			// as org-wide on a document that first landed private.
			name:     "connector-reported org promotes a private document",
			existing: private,
			input:    model.CreateDocumentInput{Visibility: "org", VisibilityFromSource: true},
			want:     "org",
		},
		{
			// Equally important, and the reason this must not be one-way: a
			// document restricted upstream has to come back down.
			name:     "connector-reported private demotes an org document",
			existing: org,
			input:    model.CreateDocumentInput{Visibility: "private", VisibilityFromSource: true},
			want:     "private",
		},
		{
			name:     "no change when the ACL already matches",
			existing: org,
			input:    model.CreateDocumentInput{Visibility: "org", VisibilityFromSource: true},
			want:     "",
		},
		{
			// A caller preference (not an ACL reading) must never move an
			// existing document — the original re-open guard.
			name:     "non-source visibility is ignored",
			existing: private,
			input:    model.CreateDocumentInput{Visibility: "org", VisibilityFromSource: false},
			want:     "",
		},
		{
			name:     "garbage from source is clamped, not trusted verbatim",
			existing: org,
			input:    model.CreateDocumentInput{Visibility: "public", VisibilityFromSource: true},
			want:     "private",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := sourceVisibilityUpdate(tc.existing, tc.input)
			if tc.want == "" {
				if got != nil {
					t.Fatalf("expected no visibility update, got %q", *got)
				}
				return
			}
			if got == nil {
				t.Fatalf("expected visibility update to %q, got none", tc.want)
			}
			if *got != tc.want {
				t.Fatalf("visibility update = %q, want %q", *got, tc.want)
			}
		})
	}
}
