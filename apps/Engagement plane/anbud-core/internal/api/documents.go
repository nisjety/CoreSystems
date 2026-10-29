package api

import (
	"bytes"
	"context"
	"errors"
	"mime"
	"net/url"
	"strconv"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/gofiber/fiber/v2"
	"github.com/valyala/fasthttp"

	"github.com/triodelab/anbud/internal/auth"
	"github.com/triodelab/anbud/internal/demo"
	"github.com/triodelab/anbud/internal/docstore"
	"github.com/triodelab/anbud/internal/tenderdocs"
)

const (
	// uploadReadTimeout is how long an upload may take to arrive: an archive of a hundred megabytes
	// over a slow line. The ordinary timeout is 15 seconds.
	uploadReadTimeout = 5 * time.Minute

	// Headers a gateway or a client sends with an upload.
	headerArchiveName = "X-Archive-Name"
	headerZDR         = "X-ZDR"
)

// Documents is what the document routes need of the tender document service.
type Documents interface {
	Upload(ctx context.Context, r tenderdocs.UploadRequest) (docstore.Upload, bool, error)
	List(ctx context.Context, org, noticeID string) ([]docstore.Upload, error)
	Delete(ctx context.Context, org, noticeID string, id int64) error
	// MaxArchiveBytes is the largest archive, and so the largest body, an upload may carry.
	MaxArchiveBytes() int64
}

// registerDocuments mounts the routes of the tender documents a company uploads for a notice. They
// are tenant routes: every one needs X-Org-ID, and what one organisation uploaded is not visible to
// another.
//
//	POST   /notices/:notice_id/documents      upload a ZIP archive: the body is the archive itself
//	GET    /notices/:notice_id/documents      the uploads of the notice: what was in each, and what was found
//	DELETE /notices/:notice_id/documents/:id  delete an upload
//
// An upload carries the archive as the request body, with Content-Type application/zip, and may name it
// in X-Archive-Name, percent-encoded. An organisation in zero-data-retention mode sends X-ZDR: true,
// and its uploads are refused: nothing is stored for it. The archive is read while the request is open
// and is not kept (internal/tenderdocs).
func registerDocuments(r fiber.Router, svc Documents) {
	g := r.Group("/notices/:notice_id/documents", auth.RequireOrganization())
	g.Post("/", uploadHandler(svc))
	g.Get("/", listHandler(svc))
	g.Delete("/:id", deleteHandler(svc))
}

func uploadHandler(svc Documents) fiber.Handler {
	return func(c *fiber.Ctx) error {
		if !isArchiveType(c.Get(fiber.HeaderContentType)) {
			return writeError(c, fiber.StatusUnsupportedMediaType, "unsupported_media_type",
				"send the ZIP archive itself as the request body, with Content-Type application/zip")
		}
		name, ok := archiveName(c)
		if !ok {
			return writeValidation(c, []demo.FieldError{{Field: headerArchiveName, Reason: "is not percent-encoded UTF-8 text"}})
		}
		up, created, err := svc.Upload(c.UserContext(), tenderdocs.UploadRequest{
			OrgID: auth.OrganizationID(c), UserID: auth.UserID(c), NoticeID: c.Params("notice_id"),
			ArchiveName: name, ZDR: zeroDataRetention(c),
			// The raw body, which nothing inflates: no route takes a compressed one (refuseEncodedBodies).
			Archive: c.BodyRaw(),
		})
		if err != nil {
			return documentsError(c, err)
		}
		status := fiber.StatusCreated
		if !created {
			status = fiber.StatusOK // the same archive was uploaded before: it is that upload
		}
		return c.Status(status).JSON(fiber.Map{"data": newUploadView(up)})
	}
}

func listHandler(svc Documents) fiber.Handler {
	return func(c *fiber.Ctx) error {
		uploads, err := svc.List(c.UserContext(), auth.OrganizationID(c), c.Params("notice_id"))
		if err != nil {
			return documentsError(c, err)
		}
		views := make([]uploadView, 0, len(uploads))
		for _, u := range uploads {
			views = append(views, newUploadView(u))
		}
		return c.JSON(fiber.Map{"data": fiber.Map{"uploads": views}})
	}
}

func deleteHandler(svc Documents) fiber.Handler {
	return func(c *fiber.Ctx) error {
		id, err := strconv.ParseInt(c.Params("id"), 10, 64)
		if err != nil || id <= 0 {
			return writeError(c, fiber.StatusBadRequest, "bad_request", "the upload id is not valid")
		}
		if err := svc.Delete(c.UserContext(), auth.OrganizationID(c), c.Params("notice_id"), id); err != nil {
			return documentsError(c, err)
		}
		return c.SendStatus(fiber.StatusNoContent)
	}
}

// isArchiveType reports whether a Content-Type is one an archive is sent as.
func isArchiveType(contentType string) bool {
	mediaType, _, err := mime.ParseMediaType(contentType)
	if err != nil {
		return false
	}
	switch mediaType {
	case "application/zip", "application/x-zip-compressed", "application/x-zip", "application/octet-stream":
		return true
	}
	return false
}

// archiveName reads X-Archive-Name, which is percent-encoded so that it can carry any character. It is
// empty when absent. The second result is false for a value that is not valid encoded text.
func archiveName(c *fiber.Ctx) (string, bool) {
	raw := c.Get(headerArchiveName)
	if raw == "" {
		return "", true
	}
	name, err := url.PathUnescape(raw)
	if err != nil || !utf8.ValidString(name) {
		return "", false
	}
	return name, true
}

// zeroDataRetention reads the posture the gateway forwards: x-zdr is true for "true" and "1", the same
// as everywhere on the platform.
//
// A header may be sent as several lines, a line may be a list ("true, false"), and a trailer after a chunked body
// joins the lines of the head. Of several lines the server reads the first, and which of them is the gateway's is
// not known, so every line counts, every item of a list counts, and one that says so is enough: an upload is
// stored only when nothing in the request asks for it not to be.
func zeroDataRetention(c *fiber.Ctx) bool {
	for _, line := range c.Request().Header.PeekAll(headerZDR) {
		items := strings.FieldsFunc(string(line), func(r rune) bool { return r == ',' || unicode.IsSpace(r) })
		for _, item := range items {
			if strings.EqualFold(item, "true") || item == "1" {
				return true
			}
		}
	}
	return false
}

// documentsError maps what the document service refuses to a status and a code. What it does not know
// is returned as it is: the error handler answers 500 and logs it, and the cause is for the log.
func documentsError(c *fiber.Ctx, err error) error {
	var archive *tenderdocs.ArchiveError
	var quota *tenderdocs.QuotaError
	switch {
	case errors.Is(err, tenderdocs.ErrZDR):
		return writeError(c, fiber.StatusConflict, "zdr_persistence_forbidden",
			"this organisation keeps nothing (zero data retention): tender documents are not stored")
	case errors.Is(err, tenderdocs.ErrInvalidNotice), errors.Is(err, docstore.ErrInvalid):
		return writeError(c, fiber.StatusBadRequest, "bad_request", "the notice id is not valid")
	case errors.Is(err, tenderdocs.ErrNoticeNotFound):
		return writeError(c, fiber.StatusNotFound, "not_found", "notice not found")
	case errors.Is(err, tenderdocs.ErrNotFound):
		return writeError(c, fiber.StatusNotFound, "not_found", "upload not found")
	case errors.Is(err, tenderdocs.ErrBusy):
		c.Set(fiber.HeaderRetryAfter, retryAfterBusy)
		return writeError(c, fiber.StatusTooManyRequests, "rate_limited", "too many archives are being read; try again shortly")
	case errors.Is(err, tenderdocs.ErrTimeout):
		return writeError(c, fiber.StatusGatewayTimeout, "timeout", "reading the archive took too long; upload fewer documents at a time")
	case errors.As(err, &quota):
		return writeErrorDetails(c, fiber.StatusConflict, "limit_reached",
			"this "+quota.Scope+" already holds as many uploads as it may; delete one first", fiber.Map{"scope": quota.Scope})
	case errors.As(err, &archive):
		switch archive.Reason {
		case tenderdocs.ReasonEmpty:
			return writeError(c, fiber.StatusBadRequest, "bad_request", "the request body is empty: send the archive")
		case tenderdocs.ReasonTooLarge:
			return writeError(c, fiber.StatusRequestEntityTooLarge, "payload_too_large", "the archive, or what it expands to, is too large")
		}
		return writeErrorDetails(c, fiber.StatusUnprocessableEntity, "invalid_archive", "the archive cannot be read",
			fiber.Map{"reason": archive.Reason})
	}
	return err
}

// uploadView is an upload as the routes show it: what the archive held, file by file, and the passages
// of its documents that name something the rules know. It holds no text of a document other than those
// passages.
type uploadView struct {
	ID            int64     `json:"id"`
	NoticeID      string    `json:"notice_id"`
	ArchiveName   string    `json:"archive_name,omitempty"`
	ArchiveSHA256 string    `json:"archive_sha256"`
	ArchiveBytes  int64     `json:"archive_bytes"`
	UploadedBy    string    `json:"uploaded_by,omitempty"`
	CreatedAt     time.Time `json:"created_at"`
	ExpiresAt     time.Time `json:"expires_at"`
	// Read and Unread count the files that were read and the files that could not be: the manifest
	// says which and why.
	Read      int                 `json:"read"`
	Unread    int                 `json:"unread"`
	Documents []docstore.Document `json:"documents"`
	Findings  []docstore.Finding  `json:"findings"`
	// FindingsDropped is how many passages were left out of Findings, because an upload keeps only so
	// many of each kind.
	FindingsDropped int `json:"findings_dropped"`
}

func newUploadView(u docstore.Upload) uploadView {
	v := uploadView{
		ID: u.ID, NoticeID: u.NoticeID, ArchiveName: u.ArchiveName, ArchiveSHA256: u.ArchiveSHA256, ArchiveBytes: u.ArchiveBytes,
		UploadedBy: u.UploadedBy, CreatedAt: u.CreatedAt.UTC(), ExpiresAt: u.ExpiresAt.UTC(),
		Read: u.ReadCount, Unread: u.UnreadCount, Documents: u.Documents, Findings: u.Findings, FindingsDropped: u.FindingsDropped,
	}
	if v.Documents == nil {
		v.Documents = []docstore.Document{}
	}
	if v.Findings == nil {
		v.Findings = []docstore.Finding{}
	}
	return v
}

// uploadLimits is the server's header hook. The server takes small bodies, and an archive is not one:
// the hook gives an upload, and only an upload, the room of an archive, and the time to arrive. It does
// so only for a request that carries the key, so that nobody who is not the gateway can make the
// server take a large body, and it decides from the headers alone, before the body is read.
func uploadLimits(cfg auth.Config, maxArchive int64) func(h *fasthttp.RequestHeader) fasthttp.RequestConfig {
	return func(h *fasthttp.RequestHeader) fasthttp.RequestConfig {
		if !h.IsPost() || !isUploadPath(h.RequestURI()) {
			return fasthttp.RequestConfig{}
		}
		if !cfg.Authenticated(func(name string) string { return string(h.Peek(name)) }) {
			return fasthttp.RequestConfig{}
		}
		return fasthttp.RequestConfig{MaxRequestBodySize: int(maxArchive), ReadTimeout: uploadReadTimeout}
	}
}

// isUploadPath reports whether a request target is the upload route: /api/v1/notices/{id}/documents,
// with or without a trailing slash, and with or without a query. The router reads the path, which ends where a
// fragment begins, and this reads the target as it was sent, so a target with a fragment in its path is read one way
// by one and another by the other: a client never sends one, and none is taken for the upload route.
func isUploadPath(target []byte) bool {
	if i := bytes.IndexByte(target, '?'); i >= 0 {
		target = target[:i]
	}
	if bytes.IndexByte(target, '#') >= 0 {
		return false
	}
	target = bytes.TrimSuffix(target, []byte("/"))
	rest, ok := bytes.CutPrefix(target, []byte("/api/v1/notices/"))
	if !ok {
		return false
	}
	id, tail, ok := bytes.Cut(rest, []byte("/"))
	return ok && len(id) > 0 && string(tail) == "documents"
}

// refuseEncodedBodies answers 415 to a request whose body is compressed. Context.Body inflates a gzip,
// deflate or brotli body without a limit, so a small request would become a large allocation, past
// every limit that is checked after it. The gateway does not compress what it sends.
//
// Every Content-Encoding line counts, not the first: the framework goes by the last, so a request that
// says "identity" and then "gzip" would pass a check of the first. The handlers read the body raw
// (BodyRaw) as well, so that no route inflates anything whatever this check lets by.
func refuseEncodedBodies() fiber.Handler {
	return func(c *fiber.Ctx) error {
		for _, value := range c.Request().Header.PeekAll(fiber.HeaderContentEncoding) {
			if enc := strings.TrimSpace(string(value)); enc != "" && !strings.EqualFold(enc, "identity") {
				return writeError(c, fiber.StatusUnsupportedMediaType, "unsupported_media_type", "request bodies must not be compressed")
			}
		}
		return c.Next()
	}
}
