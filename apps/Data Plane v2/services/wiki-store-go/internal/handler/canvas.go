package handler

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strconv"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/authctx"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/canvas"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/spacedecision"
)

// canvasDecisionHeader carries the Control-signed Space decision for one canvas request.
const canvasDecisionHeader = "X-Space-Decision"

// CanvasHandler serves the room canvas (WS19-S3). While the flag is off every canvas route answers 404 space_canvas_disabled.
// With the flag on, a request needs a verified bearer that carries no ZDR posture and a Control decision bound to the route.
// The repository trusts no author or provenance it is handed: both come from the verified decision.
type CanvasHandler struct {
	repo    *canvas.Repo
	enabled bool
	keys    spacedecision.KeySet
}

// NewCanvasHandler returns the canvas handler. keys is read only when the flag is on.
func NewCanvasHandler(repo *canvas.Repo, enabled bool, keys spacedecision.KeySet) *CanvasHandler {
	return &CanvasHandler{repo: repo, enabled: enabled, keys: keys}
}

// MountCanvasRoutes installs the canvas routes beside the wiki routes. The flag gate runs before the credential, so with the
// flag off every canvas path answers 404 space_canvas_disabled. Reads need wiki.read and writes need wiki.write. Both refuse a
// ZDR token before any handler runs.
func MountCanvasRoutes(r chi.Router, authMiddleware func(http.Handler) http.Handler, h *CanvasHandler) {
	r.Route("/v1/space-canvas", func(r chi.Router) {
		r.Use(h.flagGate)
		r.Use(authMiddleware)
		read := r.With(authctx.RequireScope("wiki.read"), refuseCanvasZDR)
		write := r.With(authctx.RequireScope("wiki.write"), refuseCanvasZDR)
		read.Method(http.MethodGet, "/spaces/{space_ref}", http.HandlerFunc(h.GetCanvas))
		read.Method(http.MethodGet, "/spaces/{space_ref}/versions", http.HandlerFunc(h.ListVersions))
		read.Method(http.MethodGet, "/spaces/{space_ref}/versions/{version_id}", http.HandlerFunc(h.GetVersion))
		write.Method(http.MethodPost, "/spaces/{space_ref}/versions", http.HandlerFunc(h.SaveVersion))
		write.Method(http.MethodPost, "/spaces/{space_ref}/versions/{version_id}/restore", http.HandlerFunc(h.RestoreVersion))
	})
}

// flagGate answers every canvas route 404 while WIKISTORE_SPACE_CANVAS is off. It runs before the credential is read.
func (h *CanvasHandler) flagGate(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !h.enabled {
			writeCanvasError(w, http.StatusNotFound, "space_canvas_disabled", "the room canvas is not enabled on this service", nil)
			return
		}
		next.ServeHTTP(w, r)
	})
}

// refuseCanvasZDR refuses every canvas route, reads included, for a token that carries zdr=true (D-5). The refusal is the
// Model Plane catalogue's zdr_durable_mutation_forbidden (412), the answer the gateway gives a ZDR caller too.
func refuseCanvasZDR(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		claims, ok := authctx.FromContext(r.Context())
		if !ok || claims.ZDR == nil {
			writeCanvasError(w, http.StatusUnauthorized, "unauthorized", "authentication required", nil)
			return
		}
		if claims.RestrictiveZDR() {
			canvasZDRRefusal().write(w)
			return
		}
		next.ServeHTTP(w, r)
	})
}

// GetCanvas answers GET /spaces/{space_ref}: the head with its content, or unchanged when known_version_id is the head.
func (h *CanvasHandler) GetCanvas(w http.ResponseWriter, r *http.Request) {
	bearer, ok := bearerOf(w, r)
	if !ok {
		return
	}
	spaceRef := chi.URLParam(r, "space_ref")
	var known *string
	if raw := r.URL.Query().Get("known_version_id"); raw != "" {
		if err := canvas.ValidateVersionID(raw); err != nil {
			writeCanvasStoreError(w, err)
			return
		}
		known = &raw
	}
	caller, refused := h.authorizeCaller(r, bearer, spaceRef, false, true)
	if refused != nil {
		refused.write(w)
		return
	}
	if digestRefusal := caller.checkRead(); digestRefusal != nil {
		digestRefusal.write(w)
		return
	}
	view, err := h.repo.Head(r.Context(), bearer.OrgID, spaceRef, known)
	if err != nil {
		writeCanvasStoreError(w, err)
		return
	}
	if view.Unchanged {
		writeJSON(w, http.StatusOK, map[string]any{"data": map[string]any{
			"unchanged":       true,
			"head_version_id": view.Canvas.HeadVersionID,
		}})
		return
	}
	var head any
	if view.Head != nil {
		head = versionFullDTO{versionDTO: newVersionDTO(*view.Head), Content: view.Head.Content}
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": map[string]any{
		"canvas": newCanvasDTO(view.Canvas),
		"head":   head,
	}})
}

// ListVersions answers GET /spaces/{space_ref}/versions: one page of history, newest first, without content.
func (h *CanvasHandler) ListVersions(w http.ResponseWriter, r *http.Request) {
	bearer, ok := bearerOf(w, r)
	if !ok {
		return
	}
	spaceRef := chi.URLParam(r, "space_ref")
	limit := canvas.DefaultListLimit
	if raw := r.URL.Query().Get("limit"); raw != "" {
		n, err := strconv.Atoi(raw)
		if err != nil {
			writeCanvasError(w, http.StatusBadRequest, canvas.CodeInvalidRequest, "limit must be a number", nil)
			return
		}
		limit = canvas.ClampListLimit(n)
	}
	var before *int
	if raw := r.URL.Query().Get("cursor"); raw != "" {
		n, err := strconv.Atoi(raw)
		if err != nil || n < 1 {
			writeCanvasError(w, http.StatusBadRequest, canvas.CodeInvalidCursor, "the cursor must be a version number", nil)
			return
		}
		before = &n
	}
	claims, refused := h.authorize(r, bearer, spaceRef, false)
	if refused != nil {
		refused.write(w)
		return
	}
	if digestRefusal := checkCanvasDigest(claims, readOp(claims)); digestRefusal != nil {
		digestRefusal.write(w)
		return
	}
	page, err := h.repo.ListVersions(r.Context(), bearer.OrgID, spaceRef, before, limit)
	if err != nil {
		writeCanvasStoreError(w, err)
		return
	}
	data := make([]versionDTO, 0, len(page.Versions))
	for _, v := range page.Versions {
		data = append(data, newVersionDTO(v))
	}
	var next any
	if page.NextBefore != nil {
		next = *page.NextBefore
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"data": data,
		"meta": map[string]any{"limit": limit, "next_cursor": next},
	})
}

// GetVersion answers GET /spaces/{space_ref}/versions/{version_id} with one version and its content.
func (h *CanvasHandler) GetVersion(w http.ResponseWriter, r *http.Request) {
	bearer, ok := bearerOf(w, r)
	if !ok {
		return
	}
	spaceRef := chi.URLParam(r, "space_ref")
	versionID := chi.URLParam(r, "version_id")
	claims, refused := h.authorize(r, bearer, spaceRef, false)
	if refused != nil {
		refused.write(w)
		return
	}
	if digestRefusal := checkCanvasDigest(claims, readOp(claims)); digestRefusal != nil {
		digestRefusal.write(w)
		return
	}
	v, err := h.repo.GetVersion(r.Context(), bearer.OrgID, spaceRef, versionID)
	if err != nil {
		writeCanvasStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": versionFullDTO{versionDTO: newVersionDTO(v), Content: v.Content}})
}

// SaveVersion answers POST /spaces/{space_ref}/versions. The Space's active canvas decides which operation the decision must
// have signed: a save over the canvas when there is one, otherwise a create.
func (h *CanvasHandler) SaveVersion(w http.ResponseWriter, r *http.Request) {
	bearer, ok := bearerOf(w, r)
	if !ok {
		return
	}
	spaceRef := chi.URLParam(r, "space_ref")
	var body saveBody
	if !decodeCanvasBody(w, r, &body) {
		return
	}
	if err := validateSaveInput(body); err != nil {
		writeCanvasStoreError(w, err)
		return
	}
	summary, err := canvas.NormalizeSummary(body.Summary)
	if err != nil {
		writeCanvasStoreError(w, err)
		return
	}
	caller, refused := h.authorizeCaller(r, bearer, spaceRef, true, true)
	if refused != nil {
		refused.write(w)
		return
	}
	// The head read below returns no content and feeds only the operation and canvas id that the digest binds. Save runs
	// after the digest matches, and it refuses unless the canvas it finds under the Space lock is this same canvas (WS19-S8).
	activeID, err := h.repo.ActiveCanvasID(r.Context(), bearer.OrgID, spaceRef)
	if err != nil {
		writeCanvasStoreError(w, err)
		return
	}
	if activeID == "" && body.ExpectedVersionID != nil {
		writeCanvasError(w, http.StatusConflict, canvas.CodeHeadMoved, "the canvas head moved since you opened it",
			map[string]any{"head": nil})
		return
	}
	op := spacedecision.CanvasOp{
		Op:             spacedecision.OpCreate,
		Schema:         spacedecision.SchemaCanvasWrite,
		ContentDigest:  canvas.ContentDigest(body.Content),
		IdempotencyKey: body.IdempotencyKey,
	}
	if activeID != "" {
		op.Op = spacedecision.OpSave
		op.CanvasID = activeID
		op.ExpectedVersionID = derefString(body.ExpectedVersionID)
	}
	if digestRefusal := caller.checkWrite(op, runWriteArgs(body), body.IdempotencyKey); digestRefusal != nil {
		digestRefusal.write(w)
		return
	}
	res, err := h.repo.Save(r.Context(), canvas.SaveInput{
		OrgID:             bearer.OrgID,
		SpaceRef:          spaceRef,
		CanvasID:          activeID,
		ExpectedVersionID: body.ExpectedVersionID,
		Content:           body.Content,
		Summary:           summary,
		IdempotencyKey:    body.IdempotencyKey,
		Author:            caller.author(),
		Provenance:        caller.provenance(),
	})
	if err != nil {
		writeCanvasStoreError(w, err)
		return
	}
	writeCanvasWrite(w, res)
}

// RestoreVersion answers POST /spaces/{space_ref}/versions/{version_id}/restore, which copies that version's text into a new head.
func (h *CanvasHandler) RestoreVersion(w http.ResponseWriter, r *http.Request) {
	bearer, ok := bearerOf(w, r)
	if !ok {
		return
	}
	spaceRef := chi.URLParam(r, "space_ref")
	versionID := chi.URLParam(r, "version_id")
	var body restoreBody
	if !decodeCanvasBody(w, r, &body) {
		return
	}
	if err := validateRestoreInput(body, versionID); err != nil {
		writeCanvasStoreError(w, err)
		return
	}
	claims, refused := h.authorize(r, bearer, spaceRef, true)
	if refused != nil {
		refused.write(w)
		return
	}
	activeID, err := h.repo.ActiveCanvasID(r.Context(), bearer.OrgID, spaceRef)
	if err != nil {
		writeCanvasStoreError(w, err)
		return
	}
	if activeID == "" {
		writeCanvasError(w, http.StatusNotFound, canvas.CodeCanvasNotFound, "the Space has no canvas", nil)
		return
	}
	op := spacedecision.CanvasOp{
		Op:                   spacedecision.OpRestore,
		Schema:               spacedecision.SchemaCanvasWrite,
		CanvasID:             activeID,
		ExpectedVersionID:    derefString(body.ExpectedVersionID),
		RestoreFromVersionID: versionID,
		IdempotencyKey:       body.IdempotencyKey,
	}
	if digestRefusal := checkCanvasDigest(claims, op); digestRefusal != nil {
		digestRefusal.write(w)
		return
	}
	res, err := h.repo.Restore(r.Context(), canvas.RestoreInput{
		OrgID:                bearer.OrgID,
		SpaceRef:             spaceRef,
		CanvasID:             activeID,
		RestoreFromVersionID: versionID,
		ExpectedVersionID:    body.ExpectedVersionID,
		IdempotencyKey:       body.IdempotencyKey,
		Author:               canvasUserAuthor(claims),
		Provenance:           canvasProvenance(claims),
	})
	if err != nil {
		writeCanvasStoreError(w, err)
		return
	}
	writeCanvasWrite(w, res)
}

// saveBody is the POST body of a version. Unknown fields are refused, so no authority field can ride along.
type saveBody struct {
	ExpectedVersionID *string `json:"expected_version_id"`
	Content           string  `json:"content"`
	Summary           *string `json:"summary"`
	IdempotencyKey    string  `json:"idempotency_key"`
}

// restoreBody is the POST body of a restore.
type restoreBody struct {
	ExpectedVersionID *string `json:"expected_version_id"`
	IdempotencyKey    string  `json:"idempotency_key"`
}

// authorize verifies the Control decision a canvas request carries and binds it to the route. It checks the header, the
// signature and the life of the decision, then the claims against the caller and the path. It runs before any repository call.
func (h *CanvasHandler) authorize(r *http.Request, bearer *authctx.Claims, spaceRef string, write bool) (spacedecision.Claims, *canvasRefusal) {
	caller, refused := h.authorizeCaller(r, bearer, spaceRef, write, false)
	return caller.user, refused
}

// canvasRefusal is one typed refusal of a canvas request: the HTTP status, the wire code and the message. zdr marks the Model
// Plane catalogue refusal of zero data retention, which is written in the catalogue body (writeCanvasZDRRefusal).
type canvasRefusal struct {
	status  int
	code    string
	message string
	zdr     bool
}

func refusalOf(status int, code, message string) *canvasRefusal {
	return &canvasRefusal{status: status, code: code, message: message}
}

// canvasZDRRefusal is the catalogue refusal of a zero data retention token or decision on a canvas route.
func canvasZDRRefusal() *canvasRefusal {
	return &canvasRefusal{status: http.StatusPreconditionFailed, code: canvasZDRCode, message: canvasZDRMessage, zdr: true}
}

func (f *canvasRefusal) write(w http.ResponseWriter) {
	if f.zdr {
		writeCanvasZDRRefusal(w)
		return
	}
	writeCanvasError(w, f.status, f.code, f.message, nil)
}

func refusalFromErr(err error) *canvasRefusal {
	var typed *spacedecision.Refusal
	if errors.As(err, &typed) {
		return refusalOf(http.StatusForbidden, typed.Code, typed.Message)
	}
	return refusalOf(http.StatusForbidden, spacedecision.CodeDecisionInvalid, "the Space decision is not valid")
}

// checkCanvasDigest recomputes the payload digest from the verified claims and the operation, and compares it with the
// digest Control signed. A mismatch means the content, the head or the operation differs from what was authorized.
func checkCanvasDigest(claims spacedecision.Claims, op spacedecision.CanvasOp) *canvasRefusal {
	if spacedecision.CanvasPayloadDigest(claims, op) != claims.PayloadDigest {
		return refusalOf(http.StatusForbidden, spacedecision.CodeDigestMismatch, "the Space decision does not match this request")
	}
	return nil
}

// readOp is the operation of a read decision. Its idempotency key comes from the decision, because a read has no body.
func readOp(claims spacedecision.Claims) spacedecision.CanvasOp {
	return spacedecision.CanvasOp{
		Op:             spacedecision.OpRead,
		Schema:         spacedecision.SchemaCanvasRead,
		IdempotencyKey: claims.IdempotencyKey,
	}
}

func canvasUserAuthor(c spacedecision.Claims) canvas.Author {
	return canvas.Author{Kind: canvas.AuthorUser, ID: c.SubjectID}
}

func canvasProvenance(c spacedecision.Claims) canvas.Provenance {
	return canvas.Provenance{
		DecisionRef:       c.DecisionRef,
		AuthorityRevision: c.AuthorityRevision,
		AudienceRevision:  c.RecipientAudienceRevision,
	}
}

func validateSaveInput(body saveBody) error {
	if err := canvas.ValidateIdempotencyKey(body.IdempotencyKey); err != nil {
		return err
	}
	if body.ExpectedVersionID != nil {
		if err := canvas.ValidateVersionID(*body.ExpectedVersionID); err != nil {
			return err
		}
	}
	return canvas.ValidateContent(body.Content)
}

func validateRestoreInput(body restoreBody, versionID string) error {
	if err := canvas.ValidateIdempotencyKey(body.IdempotencyKey); err != nil {
		return err
	}
	if err := canvas.ValidateVersionID(versionID); err != nil {
		return err
	}
	if body.ExpectedVersionID != nil {
		return canvas.ValidateVersionID(*body.ExpectedVersionID)
	}
	return nil
}

// decodeCanvasBody reads one JSON object under the 128 KiB body cap. It refuses unknown fields and trailing data.
func decodeCanvasBody(w http.ResponseWriter, r *http.Request, dst any) bool {
	r.Body = http.MaxBytesReader(w, r.Body, canvas.MaxRequestBodyBytes)
	dec := json.NewDecoder(r.Body)
	dec.DisallowUnknownFields()
	if err := dec.Decode(dst); err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			writeCanvasError(w, http.StatusRequestEntityTooLarge, canvas.CodeTooLarge,
				"the request body is larger than the canvas limit", map[string]any{"max_bytes": canvas.MaxRequestBodyBytes})
			return false
		}
		writeCanvasError(w, http.StatusBadRequest, canvas.CodeInvalidRequest, "the request body is not a valid canvas request", nil)
		return false
	}
	if err := dec.Decode(&struct{}{}); !errors.Is(err, io.EOF) {
		writeCanvasError(w, http.StatusBadRequest, canvas.CodeInvalidRequest, "the request body has trailing data", nil)
		return false
	}
	return true
}

// bearerOf returns the verified bearer. Every canvas route sits behind the auth middleware, so a missing identity is a 401.
func bearerOf(w http.ResponseWriter, r *http.Request) (*authctx.Claims, bool) {
	claims, ok := authctx.FromContext(r.Context())
	if !ok || claims.OrgID == "" {
		writeCanvasError(w, http.StatusUnauthorized, "unauthorized", "authentication required", nil)
		return nil, false
	}
	return claims, true
}

// writeCanvasStoreError maps a repository refusal to its typed answer. Any other error is a store failure and is answered
// 503 canvas_unavailable without detail.
func writeCanvasStoreError(w http.ResponseWriter, err error) {
	var typed *canvas.Error
	if !errors.As(err, &typed) {
		writeCanvasError(w, http.StatusServiceUnavailable, "canvas_unavailable", "the canvas store is unavailable", nil)
		return
	}
	switch typed.Code {
	case canvas.CodeCanvasNotFound:
		writeCanvasError(w, http.StatusNotFound, typed.Code, "the Space has no canvas", nil)
	case canvas.CodeVersionNotFound:
		writeCanvasError(w, http.StatusNotFound, typed.Code, "the version is not in this canvas", nil)
	case canvas.CodeHeadMoved:
		writeCanvasError(w, http.StatusConflict, typed.Code, typed.Message, map[string]any{"head": headRefDTO(typed.Head)})
	case canvas.CodeIdempotencyKeyConflict:
		writeCanvasError(w, http.StatusConflict, typed.Code, typed.Message, nil)
	case canvas.CodeTooLarge:
		writeCanvasError(w, http.StatusRequestEntityTooLarge, typed.Code, typed.Message, map[string]any{"max_bytes": typed.MaxBytes})
	case canvas.CodeInvalidRequest, canvas.CodeInvalidCursor:
		writeCanvasError(w, http.StatusBadRequest, typed.Code, typed.Message, nil)
	case canvas.CodeRateLimited, canvas.CodeRunWriteLimit:
		writeCanvasError(w, http.StatusTooManyRequests, typed.Code, typed.Message, nil)
	default:
		writeCanvasError(w, http.StatusServiceUnavailable, "canvas_unavailable", "the canvas store is unavailable", nil)
	}
}

// writeCanvasWrite answers a save or a restore: 201 with the version, or 200 unchanged when the content equals the head.
func writeCanvasWrite(w http.ResponseWriter, res canvas.WriteResult) {
	if res.Unchanged {
		writeJSON(w, http.StatusOK, map[string]any{"data": map[string]any{
			"canvas_id":       res.CanvasID,
			"head_version_id": res.HeadVersionID,
			"unchanged":       true,
		}})
		return
	}
	writeJSON(w, http.StatusCreated, map[string]any{"data": map[string]any{
		"canvas_id":    res.CanvasID,
		"version":      newVersionDTO(res.Version),
		"deduplicated": res.Deduplicated,
	}})
}

// writeCanvasError writes the typed error envelope {error:{code,message,details}}.
func writeCanvasError(w http.ResponseWriter, status int, code, message string, details any) {
	writeJSON(w, status, map[string]any{"error": map[string]any{
		"code":    code,
		"message": message,
		"details": details,
	}})
}

// The Model Plane catalogue refusal of zero data retention (zdr_refusal.rs DURABLE_MUTATION_FORBIDDEN, the sentence the gateway
// carries) and the surface label the gateway gives it on a room canvas.
const (
	canvasZDRCode    = "zdr_durable_mutation_forbidden"
	canvasZDRMessage = "Zero Data Retention credentials cannot create, change, or delete durable state."
	canvasSurface    = "room_canvas"
)

// writeCanvasZDRRefusal answers the catalogue refusal of zero data retention: 412 with {error:{code,message,retryable,action,details}},
// the body the gateway's own refusal has for a ZDR caller. The catalogue fixes retryable false and action none for this code.
func writeCanvasZDRRefusal(w http.ResponseWriter) {
	writeJSON(w, http.StatusPreconditionFailed, map[string]any{"error": map[string]any{
		"code":      canvasZDRCode,
		"message":   canvasZDRMessage,
		"retryable": false,
		"action":    "none",
		"details":   map[string]any{"surface": canvasSurface},
	}})
}

// canvasDTO is the metadata of a canvas, without content.
type canvasDTO struct {
	CanvasID      string    `json:"canvas_id"`
	SpaceRef      string    `json:"space_ref"`
	State         string    `json:"state"`
	HeadVersionID *string   `json:"head_version_id"`
	HeadVersionNo int       `json:"head_version_no"`
	CreatedAt     time.Time `json:"created_at"`
	UpdatedAt     time.Time `json:"updated_at"`
}

// versionDTO is one version without content.
type versionDTO struct {
	VersionID             string    `json:"version_id"`
	VersionNo             int       `json:"version_no"`
	ParentVersionID       *string   `json:"parent_version_id"`
	AuthorKind            string    `json:"author_kind"`
	AuthorID              string    `json:"author_id"`
	OnBehalfOf            *string   `json:"on_behalf_of"`
	RunID                 *string   `json:"run_id"`
	ChangeKind            string    `json:"change_kind"`
	RestoredFromVersionID *string   `json:"restored_from_version_id"`
	Summary               *string   `json:"summary"`
	ContentBytes          int       `json:"content_bytes"`
	ContentSHA256         string    `json:"content_sha256"`
	CreatedAt             time.Time `json:"created_at"`
}

// versionFullDTO is one version with its content.
type versionFullDTO struct {
	versionDTO
	Content string `json:"content"`
}

func newCanvasDTO(c canvas.Canvas) canvasDTO {
	return canvasDTO{
		CanvasID:      c.CanvasID,
		SpaceRef:      c.SpaceRef,
		State:         c.State,
		HeadVersionID: c.HeadVersionID,
		HeadVersionNo: c.HeadVersionNo,
		CreatedAt:     c.CreatedAt,
		UpdatedAt:     c.UpdatedAt,
	}
}

func newVersionDTO(v canvas.Version) versionDTO {
	return versionDTO{
		VersionID:             v.VersionID,
		VersionNo:             v.VersionNo,
		ParentVersionID:       v.ParentVersionID,
		AuthorKind:            v.AuthorKind,
		AuthorID:              v.AuthorID,
		OnBehalfOf:            v.OnBehalfOf,
		RunID:                 v.RunID,
		ChangeKind:            v.ChangeKind,
		RestoredFromVersionID: v.RestoredFromVersionID,
		Summary:               v.Summary,
		ContentBytes:          v.ContentBytes,
		ContentSHA256:         v.ContentSHA256,
		CreatedAt:             v.CreatedAt,
	}
}

// headRefDTO names the head in a 409 conflict. It is null when the Space has no canvas yet.
func headRefDTO(head *canvas.HeadRef) any {
	if head == nil {
		return nil
	}
	return map[string]any{
		"version_id":  head.VersionID,
		"version_no":  head.VersionNo,
		"author_kind": head.AuthorKind,
		"created_at":  head.CreatedAt,
	}
}

func derefString(s *string) string {
	if s == nil {
		return ""
	}
	return *s
}
