package api

import (
	"context"
	"encoding/base64"
	"testing"

	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/objectstore"
)

type fakeArtifactStore struct {
	puts []*objectstore.Artifact
}

func (f *fakeArtifactStore) Enabled() bool { return true }

func (f *fakeArtifactStore) PutBytes(ctx context.Context, key string, data []byte, opts objectstore.PutOptions) (*objectstore.Artifact, error) {
	artifact := &objectstore.Artifact{
		Kind:        opts.Kind,
		Provider:    "minio",
		Bucket:      "quarry-artifacts",
		Key:         key,
		URL:         "http://localhost:9010/quarry-artifacts/" + key,
		ContentType: opts.ContentType,
		Size:        int64(len(data)),
	}
	f.puts = append(f.puts, artifact)
	return artifact, nil
}

func (f *fakeArtifactStore) Close() error { return nil }

func TestPersistResponseArtifacts(t *testing.T) {
	t.Parallel()

	store := &fakeArtifactStore{}
	handler := &Handler{artifactStore: store}

	outputs := map[string]interface{}{
		"screenshot": []byte("png-bytes"),
		"pdf":        []byte("pdf-bytes"),
		"markdown":   "# hello",
	}
	actions := []models.ActionExecutionResult{
		{
			Type:       "generatepdf",
			Success:    true,
			DurationMs: 10,
			Output:     base64.StdEncoding.EncodeToString([]byte("action-pdf")),
		},
	}

	persistedOutputs, persistedActions := handler.persistResponseArtifacts(context.Background(), "https://example.com/path", outputs, actions)

	if len(store.puts) != 3 {
		t.Fatalf("len(puts) = %d, want 3", len(store.puts))
	}

	screenshotArtifact, ok := persistedOutputs["screenshot"].(*objectstore.Artifact)
	if !ok {
		t.Fatalf("screenshot output type = %T, want *objectstore.Artifact", persistedOutputs["screenshot"])
	}
	if screenshotArtifact.Kind != "screenshot" {
		t.Fatalf("screenshot kind = %q, want %q", screenshotArtifact.Kind, "screenshot")
	}

	pdfArtifact, ok := persistedOutputs["pdf"].(*objectstore.Artifact)
	if !ok {
		t.Fatalf("pdf output type = %T, want *objectstore.Artifact", persistedOutputs["pdf"])
	}
	if pdfArtifact.Kind != "pdf" {
		t.Fatalf("pdf kind = %q, want %q", pdfArtifact.Kind, "pdf")
	}

	if persistedOutputs["markdown"] != "# hello" {
		t.Fatalf("markdown output = %v, want unchanged markdown", persistedOutputs["markdown"])
	}

	actionArtifact, ok := persistedActions[0].Output.(*objectstore.Artifact)
	if !ok {
		t.Fatalf("action output type = %T, want *objectstore.Artifact", persistedActions[0].Output)
	}
	if actionArtifact.Kind != "action-pdf" {
		t.Fatalf("action artifact kind = %q, want %q", actionArtifact.Kind, "action-pdf")
	}
}
