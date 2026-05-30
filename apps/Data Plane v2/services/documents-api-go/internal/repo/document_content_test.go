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
