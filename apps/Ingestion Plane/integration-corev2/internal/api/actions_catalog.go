package api

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/auth"
)

// actionsCatalogVersion is the version of the served operation catalog document.
// Additive changes (rows, operations, fields) keep it; a breaking shape change bumps it.
const actionsCatalogVersion = 1

// catalogRow is one row of the static operation table. Provider and Operation are the
// exact strings the executor matches, Summary is agent-facing text, and Params names the
// params.* keys the operation reads. Kind, capability and sensitivity are not stored here:
// buildActionsCatalog computes them per request.
type catalogRow struct {
	Provider  string
	Operation string
	Summary   string
	Params    []string
}

// catalogRows is the served operation table. It is seeded from execution-core's curated
// CATALOG (apps/Model Plane/rust/services/execution-core/src/integration_tools.rs), plus the
// github read rows the executor serves and the notion page.content read. A row is served only
// when requiredCapabilityForOperation maps its operation, so no row can advertise an ungated
// operation. The notion page.content row is served only while INTEGRATION_NOTION_PAGE_CONTENT_READ
// is on (see buildActionsCatalog).
var catalogRows = []catalogRow{
	// meta: Facebook Pages, Instagram, WhatsApp, Messenger, Meta ads and Threads.
	{"meta", "pages.list", "List the connected Facebook Pages (+ linked Instagram accounts).", []string{"limit"}},
	{"meta", "pages.post", "Publish a post to a Facebook Page feed. params.pageId, body.message.", []string{"pageId"}},
	{"meta", "instagram.media.create", "Create an Instagram media container. params.igUserId, body.image_url|video_url, caption.", []string{"igUserId"}},
	{"meta", "instagram.media.publish", "Publish a prepared Instagram media container. params.igUserId, body.creation_id.", []string{"igUserId"}},
	{"meta", "whatsapp.messages.send", "Send a WhatsApp Cloud API message. params.phoneNumberId, body.{to,type,text}.", []string{"phoneNumberId"}},
	{"meta", "messenger.messages.send", "Send a Messenger message. params.pageId, body.{recipient,message}.", []string{"pageId"}},
	{"meta", "ads.adaccounts", "List the connected Meta ad accounts.", []string{"limit"}},
	{"meta", "ads.campaigns", "List campaigns for an ad account. params.adAccountId.", []string{"adAccountId", "limit"}},
	{"meta", "ads.insights", "Read ad performance metrics. params.adAccountId.", []string{"adAccountId", "limit", "datePreset"}},
	{"meta", "catalogs.list", "List commerce catalogs. params.businessId.", []string{"businessId", "limit"}},
	{"meta", "threads.profile", "Read the connected Threads profile.", nil},
	// linkedin
	{"linkedin", "posts.create", "Publish a LinkedIn post. body.commentary + author.", nil},
	{"linkedin", "ads.campaigns", "List LinkedIn ad campaigns.", []string{"accountId", "pageSize", "search", "sortOrder", "pageToken"}},
	{"linkedin", "lead.forms", "List LinkedIn lead-gen forms. params.owner (URN).", []string{"owner", "count", "start"}},
	{"linkedin", "lead.responses", "Read responses for a lead form. params.leadForm (URN).", []string{"leadForm", "count", "start"}},
	{"linkedin", "conversions.list", "List Conversions API rules for an ad account.", []string{"account", "count", "start"}},
	// google
	{"google", "gmail.send", "Send an email via Gmail. body.{to,subject,bodyText}.", nil},
	{"google", "gmail.messages", "Read-only: list Gmail messages (ids and threadIds; with headers=true also subject, from, to, cc, date, snippet, labelIds). params.query (Gmail search syntax), from, since and until (YYYY-MM-DD), folder (inbox | sent | drafts | archive | deleted), unread, headers, maxResults (default 10, max 100), pageToken.", []string{"query", "from", "since", "until", "folder", "unread", "headers", "maxResults", "pageToken"}},
	{"google", "gmail.messages.get", "Read-only: read one Gmail message (headers and body text). params.id, the message id from gmail.messages.", []string{"id"}},
	{"google", "calendar.events", "Read-only: list Google Calendar events. params.since and until (YYYY-MM-DD, or a local date-time in Europe/Oslo), query, maxResults (default 10, max 100), pageToken.", []string{"since", "until", "query", "maxResults", "pageToken"}},
	{"google", "drive.files", "Read-only: list Google Drive files. params.query, match (name | text, default name), folderId, driveId (one shared drive), maxResults (default 10, max 100), pageToken.", []string{"query", "match", "folderId", "driveId", "maxResults", "pageToken"}},
	{"google", "drive.file.content", "Read-only: read one Drive file as base64. Docs, Sheets and Slides export as text with exportAs=text. params.fileId (from drive.files), mimeType, exportAs.", []string{"fileId", "mimeType", "exportAs"}},
	{"google", "profile", "Read-only: the connected Google account profile (OpenID userinfo).", nil},
	// microsoft
	{"microsoft", "mail.send", "Send an Outlook email. body.{to,subject,bodyText}.", nil},
	{"microsoft", "mail.messages", "Read-only: list recent Outlook messages (id, subject, from, receivedDateTime, webLink, bodyPreview). params.query, from, since and until (YYYY-MM-DD), folder (inbox | sent | drafts | archive | deleted), unread, maxResults (default 10, max 100), pageToken.", []string{"query", "from", "since", "until", "folder", "unread", "maxResults", "pageToken"}},
	{"microsoft", "mail.messages.get", "Read-only: read one Outlook message (headers, body and attachment metadata). params.messageId, the id from mail.messages.", []string{"messageId"}},
	{"microsoft", "drive.files", "Read-only: list files in the connected OneDrive, or in one SharePoint library when driveId is given. params.query, folderId, driveId, maxResults (default 10, max 100), pageToken.", []string{"query", "folderId", "driveId", "maxResults", "pageToken"}},
	{"microsoft", "drive.file.content", "Read-only: read one OneDrive or SharePoint file as base64. params.itemId (from drive.files), driveId.", []string{"itemId", "driveId"}},
	{"microsoft", "sharepoint.sites", "Read-only: list the SharePoint sites the user can see. params.query (default all sites), pageToken.", []string{"query", "pageToken"}},
	{"microsoft", "sharepoint.drives", "Read-only: list the document libraries of one SharePoint site. params.siteId (from sharepoint.sites), pageToken.", []string{"siteId", "pageToken"}},
	{"microsoft", "profile", "Read-only: the connected Microsoft account profile (id, displayName, mail).", nil},
	{"microsoft", "calendar.events", "Read-only: list Outlook calendar events. params.since and until (YYYY-MM-DD, or a local date-time in Europe/Oslo), query, maxResults (default 10, max 100), pageToken.", []string{"since", "until", "query", "maxResults", "pageToken"}},
	// slack
	{"slack", "message.send", "Post a Slack message. body.channel and body.text (or body.blocks), forwarded verbatim.", nil},
	{"slack", "channels.list", "List Slack channels.", []string{"types", "limit"}},
	// github
	{"github", "issues.list", "List repository issues. params.owner, params.repo.", []string{"owner", "repo", "perPage", "state"}},
	{"github", "issues.create", "Open a GitHub issue. params.owner, params.repo, body.{title,body}.", []string{"owner", "repo"}},
	{"github", "contents.get", "Read-only: a file or directory listing at a repository path. params.owner, params.repo, params.path (no . or .. segments), params.ref (optional).", []string{"owner", "repo", "path", "ref"}},
	{"github", "readme.get", "Read-only: a repository README. params.owner, params.repo, params.ref (optional).", []string{"owner", "repo", "ref"}},
	{"github", "repos", "Read-only: the repositories of the connected user, or of params.org when given. perPage (default 30, max 100), sort (default updated), type (default owner).", []string{"org", "perPage", "sort", "type"}},
	{"github", "repo", "Read-only: one repository. params.owner, params.repo.", []string{"owner", "repo"}},
	{"github", "branches", "Read-only: the branches of a repository. params.owner, params.repo, perPage (default 30, max 100).", []string{"owner", "repo", "perPage"}},
	{"github", "commits", "Read-only: the commits of a repository. params.owner, params.repo, perPage (default 30, max 100), sha (optional).", []string{"owner", "repo", "perPage", "sha"}},
	{"github", "pulls.list", "Read-only: the pull requests of a repository. params.owner, params.repo, perPage (default 30, max 100), state (default open).", []string{"owner", "repo", "perPage", "state"}},
	// notion
	{"notion", "databases", "List the Notion databases shared with the connection.", nil},
	{"notion", "pages", "List pages; with params.databaseId, query that database's rows.", []string{"databaseId"}},
	{"notion", "user", "Read the connected Notion bot user (workspace identity).", nil},
	{"notion", "page.content", "Read-only: one page of a Notion page's block children (its content), with next_cursor. params.pageId (32 hex digits, dashes optional), pageSize (default 50, max 100), startCursor (optional).", []string{"pageId", "pageSize", "startCursor"}},
	// shopify
	{"shopify", "products", "List Shopify products.", []string{"limit"}},
	{"shopify", "orders", "List Shopify orders.", []string{"limit", "status"}},
}

// actionsCatalogOperation is one operation in the served catalog.
type actionsCatalogOperation struct {
	Operation  string   `json:"operation"`
	Kind       string   `json:"kind"`
	Summary    string   `json:"summary"`
	Params     []string `json:"params"`
	Capability string   `json:"capability"`
	Sensitive  bool     `json:"sensitive"`
}

// actionsCatalogProvider groups the served operations of one provider key.
type actionsCatalogProvider struct {
	Provider   string                    `json:"provider"`
	Operations []actionsCatalogOperation `json:"operations"`
}

// actionsCatalogDocument is the served catalog. ETag is the SHA-256 hex digest of the
// document marshalled while ETag is empty (omitempty drops the field), so the digest covers
// every other field of the response.
type actionsCatalogDocument struct {
	Version   int                      `json:"version"`
	ETag      string                   `json:"etag,omitempty"`
	Providers []actionsCatalogProvider `json:"providers"`
}

// isNotionPageContentOperation names the Notion page.content read, bare or with the notion.
// prefix, under the same normalisation requiredCapabilityForOperation applies.
func isNotionPageContentOperation(operation string) bool {
	switch strings.TrimSpace(strings.ToLower(operation)) {
	case "page.content", "notion.page.content":
		return true
	}
	return false
}

// buildActionsCatalog derives the served document from catalogRows on every call. Kind is
// "write" exactly when actionRequiresApproval admits the operation and "read" otherwise;
// capability and sensitivity come from requiredCapabilityForOperation. A row whose operation
// has no capability entry is dropped: the executor leaves an unmapped operation ungated, so it
// is never advertised. While notionPageContentRead (INTEGRATION_NOTION_PAGE_CONTENT_READ) is off,
// the notion page.content row is dropped too, because the executor refuses that operation.
func buildActionsCatalog(notionPageContentRead bool) actionsCatalogDocument {
	doc := actionsCatalogDocument{Version: actionsCatalogVersion, Providers: []actionsCatalogProvider{}}
	position := map[string]int{}
	for _, row := range catalogRows {
		if row.Provider == "notion" && isNotionPageContentOperation(row.Operation) && !notionPageContentRead {
			continue
		}
		capability, sensitive := requiredCapabilityForOperation(row.Provider, row.Operation)
		if capability == "" {
			continue
		}
		kind := "read"
		if actionRequiresApproval(row.Provider, row.Operation) {
			kind = "write"
		}
		index, seen := position[row.Provider]
		if !seen {
			index = len(doc.Providers)
			position[row.Provider] = index
			doc.Providers = append(doc.Providers, actionsCatalogProvider{Provider: row.Provider, Operations: []actionsCatalogOperation{}})
		}
		params := row.Params
		if params == nil {
			params = []string{}
		}
		doc.Providers[index].Operations = append(doc.Providers[index].Operations, actionsCatalogOperation{
			Operation:  row.Operation,
			Kind:       kind,
			Summary:    row.Summary,
			Params:     params,
			Capability: capability,
			Sensitive:  sensitive,
		})
	}
	return doc
}

// withActionsCatalogETag fills doc.ETag with the SHA-256 hex digest of the document as
// marshalled with ETag empty.
func withActionsCatalogETag(doc actionsCatalogDocument) (actionsCatalogDocument, error) {
	raw, err := json.Marshal(doc)
	if err != nil {
		return doc, err
	}
	sum := sha256.Sum256(raw)
	doc.ETag = hex.EncodeToString(sum[:])
	return doc, nil
}

// actionsCatalogReadScope gates the catalog on integration:read for service principals, the
// same rule pageSpeedReadScope applies to read-only diagnostics. The internal key (a synthetic
// service principal holding both scopes) and user bearers pass.
func actionsCatalogReadScope(c *fiber.Ctx) error {
	principal, ok := auth.PrincipalFromContext(c)
	if !ok {
		return apiError(c, fiber.StatusUnauthorized, "unauthorized", "Authentication required.")
	}
	if principal.PrincipalType == "service" && !principal.HasScope(auth.IntegrationReadScope) {
		return apiError(c, fiber.StatusForbidden, "integration_read_scope_required", "The provider operation catalog requires integration:read.")
	}
	return c.Next()
}

// actionsCatalogHandler serves GET /api/v1/actions/catalog. The body is the catalog document
// itself, so execution-core can parse {version, etag, providers} directly. When If-None-Match
// names the current etag the answer is 304 with no body. notionPageContentRead is the
// INTEGRATION_NOTION_PAGE_CONTENT_READ switch the document is built with.
func actionsCatalogHandler(notionPageContentRead bool) fiber.Handler {
	return func(c *fiber.Ctx) error {
		doc, err := withActionsCatalogETag(buildActionsCatalog(notionPageContentRead))
		if err != nil {
			return apiError(c, fiber.StatusServiceUnavailable, "actions_catalog_unavailable", "The provider operation catalog is unavailable.")
		}
		c.Set("ETag", `"`+doc.ETag+`"`)
		c.Set("Cache-Control", "private, no-cache")
		if actionsCatalogNoneMatch(c.Get("If-None-Match"), doc.ETag) {
			return c.SendStatus(fiber.StatusNotModified)
		}
		return c.JSON(doc)
	}
}

// actionsCatalogNoneMatch reports whether an If-None-Match header names the current etag.
// Quoted, weak (W/) and comma-separated forms match, and "*" matches any current
// representation. Only If-None-Match is read here: fiber's Fresh would also answer 304 to an
// If-Modified-Since-only request, which has no Last-Modified to compare against.
func actionsCatalogNoneMatch(header, etag string) bool {
	for _, candidate := range strings.Split(header, ",") {
		candidate = strings.TrimPrefix(strings.TrimSpace(candidate), "W/")
		if candidate == "*" || strings.Trim(candidate, `"`) == etag {
			return true
		}
	}
	return false
}
