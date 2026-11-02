package handler

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"regexp"
	"strings"
	"unicode/utf8"

	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/spacegrant"
	"github.com/triodelab/dataplane/services/documents-api-go/pkg/authctx"
)

// WS14-S3: room files. A room file is a documents row of type "space_upload"
// accepted only under a verified Control Space import decision whose
// import_source_type is "space_upload". documents-api then:
//   - owns the row as the decision's signed subject (the uploader), not as the
//     imports-core service principal, so the uploader can see and delete it;
//   - forces visibility "shared" (owner-only outside a Space read; the Space
//     audience arrives through Control's Space grant, never through this field);
//   - enforces the per-Space file and text-byte quota and the content-hash
//     dedupe under a per-Space lock (repo.checkSpaceFileAdmissionTx);
//   - asks Control to attach the Space grant, and only then sets the
//     X-Space-Import-Authority-Accepted header, so imports-core's existing
//     fail-closed check covers a missed grant.
// Everything is behind DOCUMENTS_SPACE_FILES (off: room files are refused and
// Space imports keep the previous owner rule). The flag is a rollout control,
// never an authorization input.

const spaceUploadDocumentType = repo.SpaceFileType

var sha256HexPattern = regexp.MustCompile(`^[0-9a-f]{64}$`)

// SpaceGrantClient is the Control surface the handler needs; spacegrant.Client
// implements it.
type SpaceGrantClient interface {
	AttachFile(ctx context.Context, decisionToken, documentID string) error
	RevokeFile(ctx context.Context, orgID, userID, authorization, documentID, spaceRef string) error
}

// SpaceFilesConfig carries the DOCUMENTS_SPACE_FILES switch, the per-Space
// limits and the Control client. The zero value is "off".
type SpaceFilesConfig struct {
	Enabled             bool
	MaxPerRoom          int
	MaxTextBytesPerRoom int64
	Grants              SpaceGrantClient
}

// WithSpaceFiles wires the room-file rules. It returns the handler for chaining.
func (h *DocumentHandler) WithSpaceFiles(cfg SpaceFilesConfig) *DocumentHandler {
	h.spaceFiles = cfg
	return h
}

// writeCodedError is writeError plus a stable machine-readable `code`; the
// `error` field stays a string so existing callers keep working.
func writeCodedError(w http.ResponseWriter, status int, code, msg string, details map[string]any) {
	body := map[string]any{"error": msg, "code": code}
	if len(details) > 0 {
		body["details"] = details
	}
	writeJSON(w, status, body)
}

// prepareSpaceFile applies the room-file rules to a create that carries a
// verified space_upload decision. It returns false after writing the response.
func (h *DocumentHandler) prepareSpaceFile(w http.ResponseWriter, input *model.CreateDocumentInput, decision *spaceImportDecisionClaims) bool {
	if !h.spaceFiles.Enabled {
		writeCodedError(w, http.StatusNotFound, "room_files_disabled", "room files are not enabled", nil)
		return false
	}
	if strings.TrimSpace(input.Type) != spaceUploadDocumentType || strings.TrimSpace(input.SpaceRef) == "" {
		writeCodedError(w, http.StatusForbidden, "space_files_not_authorized", "Space import authority does not match document source type", nil)
		return false
	}
	var metadata map[string]any
	if len(input.Metadata) > 0 {
		if err := json.Unmarshal(input.Metadata, &metadata); err != nil {
			metadata = nil
		}
	}
	if metadata == nil {
		writeCodedError(w, http.StatusBadRequest, "room_file_metadata_invalid", "room file metadata is required", nil)
		return false
	}
	sha, _ := metadata["sha256"].(string)
	sha = strings.ToLower(strings.TrimSpace(sha))
	if !sha256HexPattern.MatchString(sha) {
		writeCodedError(w, http.StatusBadRequest, "room_file_metadata_invalid", "room file metadata needs the sha256 of the uploaded bytes", nil)
		return false
	}
	// Server-stamped fields: never taken from the caller.
	metadata["sha256"] = sha
	metadata["text_chars"] = utf8.RuneCountInString(input.Content)
	metadata["created_by_kind"] = "human"
	metadata["scan_state"] = "not_scanned_text_only"
	encoded, err := json.Marshal(metadata)
	if err != nil {
		writeCodedError(w, http.StatusBadRequest, "room_file_metadata_invalid", "room file metadata is invalid", nil)
		return false
	}
	input.Metadata = encoded
	input.SpaceFile = &model.SpaceFileConstraints{
		SHA256:       sha,
		MaxFiles:     h.spaceFiles.MaxPerRoom,
		MaxTextBytes: h.spaceFiles.MaxTextBytesPerRoom,
	}
	return true
}

// writeSpaceFileRepoError maps the repository's room-file errors. It returns
// true when it handled the error.
func writeSpaceFileRepoError(w http.ResponseWriter, err error) bool {
	var duplicate *repo.SpaceFileDuplicateError
	if errors.As(err, &duplicate) {
		details := map[string]any{}
		if duplicate.DocumentID != "" {
			details["document_id"] = duplicate.DocumentID
		}
		writeCodedError(w, http.StatusConflict, "room_file_duplicate", "the room already holds this file", details)
		return true
	}
	var quota *repo.SpaceFileQuotaError
	if errors.As(err, &quota) {
		writeCodedError(w, http.StatusConflict, "room_files_quota_exceeded", "the room's file quota is used up", map[string]any{
			"limit": quota.Limit, "max_files": quota.MaxFiles, "max_text_bytes": quota.MaxTextBytes,
		})
		return true
	}
	return false
}

// attachSpaceFile asks Control to attach the Space grant. It returns false
// after writing the refusal, in which case the accepted header must NOT be set
// and imports-core treats the item as failed and retries (the document create
// is idempotent, so the retry reuses the row and re-attempts the grant).
func (h *DocumentHandler) attachSpaceFile(w http.ResponseWriter, r *http.Request, documentID string) bool {
	token := strings.TrimSpace(r.Header.Get(spaceImportDecisionHeader))
	err := h.spaceFiles.attach(r.Context(), token, documentID)
	if err == nil {
		return true
	}
	var denied *spacegrant.DeniedError
	if errors.As(err, &denied) {
		details := map[string]any{}
		if denied.Code != "" {
			details["reason_code"] = denied.Code
		}
		writeCodedError(w, http.StatusForbidden, "space_files_not_authorized", "Control did not authorize attaching this file to the room", details)
		return false
	}
	log.Warn().Err(err).Str("document_id", documentID).Msg("documents: room file grant could not be attached")
	writeCodedError(w, http.StatusServiceUnavailable, "space_grants_unavailable", "the room grant could not be recorded", nil)
	return false
}

func (c SpaceFilesConfig) attach(ctx context.Context, token, documentID string) error {
	if c.Grants == nil {
		return spacegrant.ErrNotConfigured
	}
	return c.Grants.AttachFile(ctx, token, documentID)
}

// revokeSpaceFile removes the Space grant after the uploader deleted a room
// file. Best effort: the grant is inert once the document is gone (every read
// joins the live document), so a failure is logged and the delete stands.
func (h *DocumentHandler) revokeSpaceFile(r *http.Request, document *model.Document, orgID string) {
	if document == nil || document.Type != spaceUploadDocumentType || document.SpaceRef == nil || strings.TrimSpace(*document.SpaceRef) == "" {
		return
	}
	if !h.spaceFiles.Enabled || h.spaceFiles.Grants == nil {
		return
	}
	claims := verifiedClaims(r)
	userID := eventUserID(r)
	authorization, ok := authctx.AuthorizationHeader(r.Context())
	if claims == nil || userID == "" || !ok {
		// A service caller (org:data:write_all) has no user to delegate for.
		log.Warn().Str("document_id", document.DocumentID).Msg("documents: room file deleted without a user bearer; Space grant left inert")
		return
	}
	if err := h.spaceFiles.Grants.RevokeFile(r.Context(), orgID, userID, authorization, document.DocumentID, *document.SpaceRef); err != nil {
		log.Warn().Err(err).Str("document_id", document.DocumentID).Msg("documents: room file Space grant could not be revoked; grant is inert without the document")
	}
}
