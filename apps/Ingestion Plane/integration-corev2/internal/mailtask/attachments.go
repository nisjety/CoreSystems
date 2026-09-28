package mailtask

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"path/filepath"
	"strings"
	"time"
)

// allowedAttachmentExtensions mirrors imports-core's own allow-list
// (imports-core/app/config.py: pdf,docx,txt,md,csv,json,html,htm -- no
// xlsx/pptx, a known limit carried over unmodified from Wave 1, spec §6).
var allowedAttachmentExtensions = map[string]bool{
	"pdf": true, "docx": true, "txt": true, "md": true,
	"csv": true, "json": true, "html": true, "htm": true,
}

// maxAttachmentBytes matches imports-core's own decoded-size cap
// (chat_extract.py: "Choose a non-empty attachment smaller than 1 MB").
// Checked here too so an oversized attachment never reaches the network call.
const maxAttachmentBytes = 1_000_000

// AttachmentExtractor turns one attachment's raw bytes into extracted text.
// extracted=false (with err=nil) means "could not extract, not an error" --
// e.g. the extractor understood the file but found no readable text.
type AttachmentExtractor interface {
	Extract(ctx context.Context, filename, contentType string, content []byte) (text string, extracted bool, err error)
}

// ExtractAttachments applies the allow-list and size cap, then asks extractor
// for each attachment that passes. An attachment that fails any check --
// wrong extension, empty, oversized, extractor error, or no readable text --
// is recorded with Extracted=false and its filename kept, never dropped
// silently and never failing the whole message (spec §3.2, §6). A nil
// extractor degrades every attachment to Extracted=false rather than
// panicking, so a deployment that has not wired one yet still processes the
// email's own text.
func ExtractAttachments(ctx context.Context, extractor AttachmentExtractor, attachments []InboundAttachment) []ExtractedAttachment {
	results := make([]ExtractedAttachment, 0, len(attachments))
	for _, attachment := range attachments {
		filename := strings.TrimSpace(attachment.Filename)
		ext := strings.ToLower(strings.TrimPrefix(filepath.Ext(filename), "."))
		if extractor == nil || !allowedAttachmentExtensions[ext] || len(attachment.Content) == 0 || len(attachment.Content) > maxAttachmentBytes {
			results = append(results, ExtractedAttachment{Filename: filename, Extracted: false})
			continue
		}
		text, extracted, err := extractor.Extract(ctx, filename, attachment.ContentType, attachment.Content)
		if err != nil || !extracted || strings.TrimSpace(text) == "" {
			results = append(results, ExtractedAttachment{Filename: filename, Extracted: false})
			continue
		}
		results = append(results, ExtractedAttachment{Filename: filename, Extracted: true, Text: text})
	}
	return results
}

// HTTPImportsExtractor calls imports-core's existing ephemeral extraction
// endpoint (POST /api/v1/import/extract, imports-core/app/main.py:204-212),
// the same one the live chat composer's attachment path uses -- so a
// mail-task attachment gets identical handling (no storage, same 1MB/type
// limits) rather than a parallel implementation.
//
// AUTHENTICATION GAP (deliberately not resolved here, see this change's
// rollout report): imports-core's require_internal_auth
// (imports-core/app/auth_middleware.py:214) accepts only an RS256 bearer
// token verified against auth-core's JWKS for the "ingestion" audience, or a
// legacy shared key that is hard-gated to
// ALLOW_LEGACY_TENANT_KEY+ALLOW_INSECURE_DEV_DEFAULTS+ISOLATED_E2E (never
// true in production). integration-corev2 today only VERIFIES such tokens
// (internal/controlplane/clients.go's AuthClient), it does not MINT one to
// call another ingestion-audience service the way the gateway mints
// audience-scoped tokens via auth-core (audience_tokens.rs). Token carries
// MailTaskImportsCoreToken as a pre-provisioned bearer for now; before this
// ships, ops must either (a) mint and rotate a long-lived service JWT for
// principal "integration-mail-task-worker" the same way auth-core issues
// other service tokens, or (b) add a narrow scoped-service-credential path to
// imports-core's auth_middleware.py mirroring user-core's
// X-Service-Id/X-Service-Token scheme (internal/http/service_auth.go). Either
// is Control/Ingestion Plane auth work, not mail-task-specific, so it is
// intentionally out of scope for this change.
type HTTPImportsExtractor struct {
	BaseURL     string
	BearerToken string
	Client      *http.Client
}

type importsExtractRequest struct {
	Filename      string `json:"filename"`
	ContentBase64 string `json:"content_base64"`
	ContentType   string `json:"content_type,omitempty"`
}

type importsExtractResponse struct {
	Data struct {
		Name      string `json:"name"`
		Content   string `json:"content"`
		Persisted bool   `json:"persisted"`
	} `json:"data"`
}

func (e *HTTPImportsExtractor) Extract(ctx context.Context, filename, contentType string, content []byte) (string, bool, error) {
	if e == nil || strings.TrimSpace(e.BaseURL) == "" {
		return "", false, nil
	}
	client := e.Client
	if client == nil {
		client = &http.Client{Timeout: 20 * time.Second}
	}
	payload := importsExtractRequest{
		Filename:      filename,
		ContentBase64: base64.StdEncoding.EncodeToString(content),
		ContentType:   contentType,
	}
	body, err := json.Marshal(payload)
	if err != nil {
		return "", false, fmt.Errorf("encode imports-core extract request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimRight(e.BaseURL, "/")+"/api/v1/import/extract", bytes.NewReader(body))
	if err != nil {
		return "", false, err
	}
	req.Header.Set("Content-Type", "application/json")
	if token := strings.TrimSpace(e.BearerToken); token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	resp, err := client.Do(req)
	if err != nil {
		return "", false, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		// imports-core answers 400/413/415/422 for an unsupported, oversized,
		// or unreadable attachment -- all of these are "could not extract",
		// never a hard error for the whole message.
		_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 2048))
		return "", false, nil
	}
	var decoded importsExtractResponse
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&decoded); err != nil {
		return "", false, fmt.Errorf("decode imports-core extract response: %w", err)
	}
	if strings.TrimSpace(decoded.Data.Content) == "" {
		return "", false, nil
	}
	return decoded.Data.Content, true, nil
}
