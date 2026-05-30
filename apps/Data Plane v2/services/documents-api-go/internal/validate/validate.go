// Package validate provides input validators that run at the API boundary —
// where untrusted client data first enters the system. Catches malformed
// input before it reaches the repo layer.
package validate

import (
	"errors"
	"fmt"
	"regexp"
	"unicode/utf8"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
)

// Reasonable per-field caps. These are *upper bounds* designed to reject
// pathological input (e.g. a 100MB "title"); legitimate documents fit
// comfortably below them.
const (
	MaxOrgIDLength       = 128
	MaxSourceLength      = 256
	MaxTypeLength        = 64
	MaxTitleLength       = 1024
	MaxContentLength     = 5 * 1024 * 1024 // 5 MiB
	MaxIdempotencyKeyLen = 256
	MaxBulkBatchSize     = 500
	MaxConnectorLength   = 64
	MaxExternalIDLength  = 512
	MaxPathLength        = 4096
	MaxMimeTypeLength    = 256
)

// org_id format: alphanumeric, hyphens, underscores. No spaces, no slashes,
// no shell metacharacters. This is intentionally strict — org_id flows into
// log labels, metric labels, and Redis cache keys.
var orgIDPattern = regexp.MustCompile(`^[A-Za-z0-9_\-]+$`)

// idempotency_key: same alphabet as org_id plus dots and colons (UUIDs, tuples).
var idempotencyKeyPattern = regexp.MustCompile(`^[A-Za-z0-9_\-\.\:]+$`)

// validZDR is the allow-list for zdr_classification. Empty string is allowed
// (interpreted as "internal" downstream).
var validZDR = map[string]bool{
	"":           true,
	"internal":   true,
	"public":     true,
	"sensitive":  true,
	"restricted": true,
}

func OrgID(orgID string) error {
	if orgID == "" {
		return errors.New("org_id is required")
	}
	if len(orgID) > MaxOrgIDLength {
		return fmt.Errorf("org_id exceeds max length of %d", MaxOrgIDLength)
	}
	if !orgIDPattern.MatchString(orgID) {
		return errors.New("org_id must be alphanumeric, hyphens, or underscores only")
	}
	return nil
}

func CreateDocument(input *model.CreateDocumentInput) error {
	if input.Source == "" {
		return errors.New("source is required")
	}
	if len(input.Source) > MaxSourceLength {
		return fmt.Errorf("source exceeds max length of %d", MaxSourceLength)
	}

	if input.Type == "" {
		return errors.New("type is required")
	}
	if len(input.Type) > MaxTypeLength {
		return fmt.Errorf("type exceeds max length of %d", MaxTypeLength)
	}

	if input.Title == "" {
		return errors.New("title is required")
	}
	if len(input.Title) > MaxTitleLength {
		return fmt.Errorf("title exceeds max length of %d bytes", MaxTitleLength)
	}
	if !utf8.ValidString(input.Title) {
		return errors.New("title is not valid UTF-8")
	}

	if input.Content == "" {
		return errors.New("content is required")
	}
	if len(input.Content) > MaxContentLength {
		return fmt.Errorf("content exceeds max size of %d bytes", MaxContentLength)
	}
	if !utf8.ValidString(input.Content) {
		return errors.New("content is not valid UTF-8")
	}

	if !validZDR[input.ZDRClassification] {
		return fmt.Errorf("zdr_classification must be one of: internal, public, sensitive, restricted (got %q)", input.ZDRClassification)
	}

	if input.IdempotencyKey != "" {
		if len(input.IdempotencyKey) > MaxIdempotencyKeyLen {
			return fmt.Errorf("idempotency_key exceeds max length of %d", MaxIdempotencyKeyLen)
		}
		if !idempotencyKeyPattern.MatchString(input.IdempotencyKey) {
			return errors.New("idempotency_key must contain only alphanumerics, hyphens, underscores, dots, or colons")
		}
	}

	return nil
}

// BulkBatchSize bounds the number of documents in a single BulkIngest request.
// Larger batches should be chunked client-side.
func BulkBatchSize(n int) error {
	if n == 0 {
		return errors.New("documents list is empty")
	}
	if n > MaxBulkBatchSize {
		return fmt.Errorf("bulk batch size %d exceeds max %d; chunk client-side", n, MaxBulkBatchSize)
	}
	return nil
}

func UpsertSourceObject(input *model.UpsertSourceObjectInput) error {
	if input.Connector == "" {
		return errors.New("connector is required")
	}
	if len(input.Connector) > MaxConnectorLength {
		return fmt.Errorf("connector exceeds max length of %d", MaxConnectorLength)
	}
	if input.Source == "" {
		return errors.New("source is required")
	}
	if len(input.Source) > MaxSourceLength {
		return fmt.Errorf("source exceeds max length of %d", MaxSourceLength)
	}
	if input.ExternalID == "" {
		return errors.New("external_id is required")
	}
	if len(input.ExternalID) > MaxExternalIDLength {
		return fmt.Errorf("external_id exceeds max length of %d", MaxExternalIDLength)
	}
	if input.Name == "" {
		return errors.New("name is required")
	}
	if len(input.Name) > MaxTitleLength {
		return fmt.Errorf("name exceeds max length of %d", MaxTitleLength)
	}
	if !utf8.ValidString(input.Name) {
		return errors.New("name is not valid UTF-8")
	}
	if len(input.Path) > MaxPathLength {
		return fmt.Errorf("path exceeds max length of %d", MaxPathLength)
	}
	if len(input.MimeType) > MaxMimeTypeLength {
		return fmt.Errorf("mime_type exceeds max length of %d", MaxMimeTypeLength)
	}
	return nil
}

func DeleteSourceObject(input *model.DeleteSourceObjectInput) error {
	if input.SourceObjectID != "" {
		return nil
	}
	if input.Connector == "" || input.ExternalID == "" {
		return errors.New("source_object_id or connector + external_id are required")
	}
	if len(input.Connector) > MaxConnectorLength {
		return fmt.Errorf("connector exceeds max length of %d", MaxConnectorLength)
	}
	if len(input.ExternalID) > MaxExternalIDLength {
		return fmt.Errorf("external_id exceeds max length of %d", MaxExternalIDLength)
	}
	return nil
}
