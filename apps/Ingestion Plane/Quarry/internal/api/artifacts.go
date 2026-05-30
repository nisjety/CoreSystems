package api

import (
	"context"
	"encoding/base64"
	"net/url"
	"path"
	"strings"
	"time"

	"github.com/google/uuid"
	zlog "github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/objectstore"
)

func (h *Handler) persistResponseArtifacts(ctx context.Context, sourceURL string, outputs map[string]interface{}, actions []models.ActionExecutionResult) (map[string]interface{}, []models.ActionExecutionResult) {
	if h == nil || h.artifactStore == nil || !h.artifactStore.Enabled() {
		return outputs, actions
	}

	persistedOutputs := cloneAnyMap(outputs)
	persistedActions := cloneActionExecutionResults(actions)

	for format, spec := range map[string]struct {
		kind        string
		contentType string
		ext         string
	}{
		"screenshot": {kind: "screenshot", contentType: "image/png", ext: ".png"},
		"pdf":        {kind: "pdf", contentType: "application/pdf", ext: ".pdf"},
	} {
		raw, ok := outputs[format].([]byte)
		if !ok || len(raw) == 0 {
			continue
		}
		artifact, err := h.storeArtifactBytes(ctx, sourceURL, spec.kind, spec.contentType, spec.ext, raw)
		if err != nil {
			zlog.Warn().Err(err).Str("format", format).Msg("failed to persist response artifact; keeping inline output")
			continue
		}
		persistedOutputs[format] = artifact
	}

	for index := range persistedActions {
		if !strings.EqualFold(strings.TrimSpace(persistedActions[index].Type), "generatepdf") {
			continue
		}
		encoded, ok := persistedActions[index].Output.(string)
		if !ok || strings.TrimSpace(encoded) == "" {
			continue
		}

		raw, err := base64.StdEncoding.DecodeString(encoded)
		if err != nil {
			zlog.Warn().Err(err).Msg("failed to decode action PDF output; keeping inline output")
			continue
		}

		artifact, err := h.storeArtifactBytes(ctx, sourceURL, "action-pdf", "application/pdf", ".pdf", raw)
		if err != nil {
			zlog.Warn().Err(err).Msg("failed to persist action PDF artifact; keeping inline output")
			continue
		}
		persistedActions[index].Output = artifact
	}

	return persistedOutputs, persistedActions
}

// storeArtifactBytes uploads binary response data to object storage so Quarry
// can return a small metadata object instead of embedding large blobs inline.
func (h *Handler) storeArtifactBytes(ctx context.Context, sourceURL, kind, contentType, ext string, data []byte) (*objectstore.Artifact, error) {
	return h.artifactStore.PutBytes(ctx, buildArtifactKey(sourceURL, kind, ext), data, objectstore.PutOptions{
		Kind:        kind,
		ContentType: contentType,
		Metadata: map[string]string{
			"source_url": sourceURL,
			"artifact":   kind,
		},
	})
}

func buildArtifactKey(sourceURL, kind, ext string) string {
	host := "unknown-host"
	if parsed, err := url.Parse(strings.TrimSpace(sourceURL)); err == nil && parsed.Hostname() != "" {
		host = sanitizeArtifactSegment(parsed.Hostname())
	}

	return path.Join(
		"quarry",
		time.Now().UTC().Format("2006/01/02"),
		host,
		sanitizeArtifactSegment(kind),
		uuid.NewString()+ext,
	)
}

func sanitizeArtifactSegment(input string) string {
	trimmed := strings.ToLower(strings.TrimSpace(input))
	if trimmed == "" {
		return "unknown"
	}

	var out strings.Builder
	out.Grow(len(trimmed))
	for _, r := range trimmed {
		switch {
		case r >= 'a' && r <= 'z':
			out.WriteRune(r)
		case r >= '0' && r <= '9':
			out.WriteRune(r)
		case r == '.', r == '-', r == '_':
			out.WriteRune(r)
		default:
			out.WriteByte('-')
		}
	}

	sanitized := strings.Trim(out.String(), "-")
	if sanitized == "" {
		return "unknown"
	}
	return sanitized
}

func cloneAnyMap(src map[string]interface{}) map[string]interface{} {
	if src == nil {
		return nil
	}
	out := make(map[string]interface{}, len(src))
	for key, value := range src {
		out[key] = value
	}
	return out
}

func cloneActionExecutionResults(src []models.ActionExecutionResult) []models.ActionExecutionResult {
	if len(src) == 0 {
		return src
	}
	out := make([]models.ActionExecutionResult, len(src))
	copy(out, src)
	return out
}
