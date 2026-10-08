package actions

import (
	"context"
	"fmt"
	"net/url"
	"regexp"
	"strings"
	"unicode"
)

// Files and SharePoint for the connected user: OneDrive and SharePoint document
// libraries through Microsoft Graph, My Drive and shared drives through Google
// Drive. The operations are drive.files, drive.file.content, and, Microsoft
// only, sharepoint.sites and sharepoint.drives. The composer picker and the
// agent's read tool (planned in F6) both call them.
//
// Microsoft
//
//	drive.files          query, folderId, driveId, maxResults, pageToken
//	drive.file.content   itemId (required), driveId
//	sharepoint.sites     query (default "*": every site the user can see), pageToken
//	sharepoint.drives    siteId (required), pageToken
//
// Without driveId the own OneDrive is read (/me/drive), exactly as before. With a
// driveId (a SharePoint document library, or any drive the user can open) the
// same requests go to /drives/{driveId}. A library's driveId comes from
// sharepoint.drives; the siteId for that from sharepoint.sites. A search takes
// priority over a folder, because Graph's search already covers the whole drive
// and v1.0 has no folder-scoped search to combine it with.
//
// Google Drive
//
//	drive.files          query, match (name | text), folderId, driveId, maxResults, pageToken
//	drive.file.content   fileId (required), mimeType, exportAs (text)
//
// Shared-drive items are always allowed in (supportsAllDrives,
// includeItemsFromAllDrives); driveId narrows the listing to one shared drive
// (corpora=drive). match=name, the default, is the picker's own file-name
// search; match=text searches inside the files (fullText). A listing that is
// narrowed in any way leaves trashed files out. Each listed file carries its
// parents and, in a shared drive, its driveId, so the id a later call needs is
// in the listing.
//
// drive.file.content returns the bytes as base64 and refuses anything over
// maxContentBytes outright (never a partial body). A Google Docs Editors file
// has no bytes of its own: it is exported, as .docx, .xlsx or .pptx by default,
// or as text with exportAs=text (Docs and Slides as text/plain, Sheets as
// text/csv, first sheet only). That is the extractor-free first slice of agent
// reading; the default stays the Office formats the picker attaches. exportAs
// does nothing for a file that is not a Docs Editors file. The Docs Editors
// type is the caller's mimeType (the picker sends the one from its own listing);
// when it is absent one metadata request finds it, because asking for the bytes
// of a native file is answered 403 fileNotDownloadable, which would read as a
// credential rejection.
//
// Paging. A Graph listing's nextPageToken is its @odata.nextLink, handed back
// as pageToken and replayed as is (it carries the whole previous query), so
// nothing else sent beside a pageToken is applied. A link that does not look
// like one of the links these operations produce is refused before any request:
// the token comes back from the caller, who may be an agent steered by file
// content, and the request that follows carries the user's bearer token. A link
// Graph hands out in a shape that is not recognised produces no nextPageToken
// and moreAvailable=true instead, so a cut-short list can be told from one that
// ended. Google's own nextPageToken is passed through untouched.
//
// What is not verified against a live provider yet (no tenant with a SharePoint
// site and no Google account were available when this was written; the live
// proof is task F10): that Graph takes a path-escaped siteId (contoso.sharepoint.com,
// guid, guid with the commas as %2C) and a path-escaped driveId (the "!" as %21),
// that its $select is honoured on /sites?search=, that the nextLink of a drive
// listing or a search keeps one of the shapes in graphPagePatterns, that a search
// text with a slash inside the OData literal is accepted as %2F, and that
// Drive's files.list with corpora=drive and trashed=false behaves as documented.
// Each of those is isolated in one helper or one table below, so a real response
// can change one line.

const (
	driveDefaultMaxResults = 10
	driveMaxResultsCap     = 100
	driveQueryMaxRunes     = 500
	driveIDMaxBytes        = 512

	// parentReference is selected so that a search hit says which folder (and,
	// when it lives in another drive, which driveId) it is in.
	graphDriveItemSelect = "id,name,webUrl,file,folder,lastModifiedDateTime,size,parentReference"
	graphSiteSelect      = "id,displayName,webUrl"
	graphSiteDriveSelect = "id,name,driveType,webUrl"

	googleDriveFilesFields = "nextPageToken,files(id,name,mimeType,webViewLink,modifiedTime,size,parents,driveId)"
)

// driveStringParam reads an optional text param. A value of another type is an
// error rather than ignored: an ignored scope would widen a read the caller
// thought was narrowed.
func driveStringParam(params map[string]any, key string) (string, error) {
	return mailStringParam(params, key)
}

// driveIDValueParam reads an optional id that is sent as a value (a query
// parameter, or inside a Drive q expression), where the only thing to refuse is
// text that is not an id at all.
func driveIDValueParam(params map[string]any, key string) (string, error) {
	raw, err := driveStringParam(params, key)
	if err != nil || raw == "" {
		return "", err
	}
	if err := driveCheckID(key, raw); err != nil {
		return "", err
	}
	return raw, nil
}

// drivePathIDParam reads an id that is placed inside a URL path and returns it
// percent-escaped. The two dot segments are refused outright: PathEscape leaves
// them alone, and a dot segment would walk out of the path the operation was
// authorised for.
func drivePathIDParam(params map[string]any, key string, required bool) (string, error) {
	raw, err := driveStringParam(params, key)
	if err != nil {
		return "", err
	}
	if raw == "" {
		if required {
			return "", fmt.Errorf("%s is required", key)
		}
		return "", nil
	}
	if err := driveCheckID(key, raw); err != nil {
		return "", err
	}
	return url.PathEscape(raw), nil
}

func driveCheckID(key, raw string) error {
	if raw == "." || raw == ".." || len(raw) > driveIDMaxBytes {
		return fmt.Errorf("%s is not a valid id", key)
	}
	for _, r := range raw {
		if unicode.IsControl(r) {
			return fmt.Errorf("%s is not a valid id", key)
		}
	}
	return nil
}

// graphQueryString is Encode with spaces as %20 rather than "+": a literal plus
// is already %2B, so this is the same query for any decoder, and it does not
// rely on Graph reading "+" as a space.
func graphQueryString(values url.Values) string {
	return strings.ReplaceAll(values.Encode(), "+", "%20")
}

// graphDriveRoot is the drive the request addresses: the connected user's own
// OneDrive, or the drive with the given (already path-escaped) id.
func graphDriveRoot(base, escapedDriveID string) string {
	if escapedDriveID == "" {
		return base + "/me/drive"
	}
	return base + "/drives/" + escapedDriveID
}

// graphSearchLiteral renders text for the inside of search(q='...'). The text
// lives in the URL path, so it is percent-escaped: a question mark or hash
// would otherwise end the path, a percent sign would not parse, and a slash
// would start another segment. The single quote stays literal (it is the OData
// literal's own delimiter and is doubled by odataEscape), which is also what
// the unescaped form always sent.
func graphSearchLiteral(query string) string {
	return strings.ReplaceAll(url.PathEscape(odataEscape(query)), "%27", "'")
}

// --- Microsoft Graph ------------------------------------------------------------

func (s *Service) microsoftDriveFiles(ctx context.Context, token, base string, params map[string]any) (any, error) {
	pageToken, err := mailPageTokenParam(params)
	if err != nil {
		return nil, err
	}
	if pageToken != "" {
		endpoint, err := s.graphReadPageURL(pageToken, graphPageDriveItems)
		if err != nil {
			return nil, err
		}
		return s.getGraphPage(ctx, token, endpoint, graphPageDriveItems)
	}
	query, err := mailTextParam(params, "query", driveQueryMaxRunes)
	if err != nil {
		return nil, err
	}
	driveID, err := drivePathIDParam(params, "driveId", false)
	if err != nil {
		return nil, err
	}
	folderID, err := drivePathIDParam(params, "folderId", false)
	if err != nil {
		return nil, err
	}
	values := url.Values{
		"$top":    {limitParam(params, "maxResults", driveDefaultMaxResults, driveMaxResultsCap)},
		"$select": {graphDriveItemSelect},
	}
	root := graphDriveRoot(base, driveID)
	endpoint := root + "/root/children"
	if query != "" {
		endpoint = root + "/root/search(q='" + graphSearchLiteral(query) + "')"
	} else if folderID != "" {
		endpoint = root + "/items/" + folderID + "/children"
	}
	return s.getGraphPage(ctx, token, endpoint+"?"+values.Encode(), graphPageDriveItems)
}

func (s *Service) microsoftDriveFileContent(ctx context.Context, token, base string, params map[string]any) (any, error) {
	itemID, err := drivePathIDParam(params, "itemId", true)
	if err != nil {
		return nil, err
	}
	driveID, err := drivePathIDParam(params, "driveId", false)
	if err != nil {
		return nil, err
	}
	contentB64, mimeType, size, truncated, err := s.getBearerBinaryBase64(
		ctx, token, graphDriveRoot(base, driveID)+"/items/"+itemID+"/content", maxContentBytes, nil,
	)
	if err != nil {
		return nil, err
	}
	return map[string]any{
		"content_base64": contentB64,
		"mime_type":      mimeType,
		"size":           size,
		"truncated":      truncated,
	}, nil
}

func (s *Service) microsoftSharePointSites(ctx context.Context, token, base string, params map[string]any) (any, error) {
	pageToken, err := mailPageTokenParam(params)
	if err != nil {
		return nil, err
	}
	if pageToken != "" {
		endpoint, err := s.graphReadPageURL(pageToken, graphPageSites)
		if err != nil {
			return nil, err
		}
		return s.getGraphPage(ctx, token, endpoint, graphPageSites)
	}
	query, err := mailTextParam(params, "query", driveQueryMaxRunes)
	if err != nil {
		return nil, err
	}
	if query == "" {
		query = "*"
	}
	values := url.Values{"search": {query}, "$select": {graphSiteSelect}}
	return s.getGraphPage(ctx, token, base+"/sites?"+graphQueryString(values), graphPageSites)
}

func (s *Service) microsoftSharePointDrives(ctx context.Context, token, base string, params map[string]any) (any, error) {
	pageToken, err := mailPageTokenParam(params)
	if err != nil {
		return nil, err
	}
	if pageToken != "" {
		endpoint, err := s.graphReadPageURL(pageToken, graphPageSiteDrives)
		if err != nil {
			return nil, err
		}
		return s.getGraphPage(ctx, token, endpoint, graphPageSiteDrives)
	}
	siteID, err := drivePathIDParam(params, "siteId", true)
	if err != nil {
		return nil, err
	}
	values := url.Values{"$select": {graphSiteDriveSelect}}
	return s.getGraphPage(ctx, token, base+"/sites/"+siteID+"/drives?"+graphQueryString(values), graphPageSiteDrives)
}

// getGraphPage runs a listing request and adds the paging fields to its result.
func (s *Service) getGraphPage(ctx context.Context, token, endpoint, kind string) (any, error) {
	result, err := s.getBearer(ctx, token, endpoint, nil)
	if err != nil {
		return nil, err
	}
	return s.finishGraphPage(result, kind), nil
}

// finishGraphPage adds nextPageToken beside the Graph `value` when the
// response's @odata.nextLink is one this operation would accept back, and
// moreAvailable when there is a next page that cannot be handed on.
func (s *Service) finishGraphPage(result any, kind string) any {
	list, ok := result.(map[string]any)
	if !ok {
		return result
	}
	if next := stringFromAny(list["@odata.nextLink"]); next != "" {
		if _, err := s.graphReadPageURL(next, kind); err == nil {
			list["nextPageToken"] = next
		} else {
			list["moreAvailable"] = true
		}
	}
	return list
}

// The three kinds of Graph listing a page token can belong to. A token is only
// accepted by the operation that produces it.
const (
	graphPageDriveItems = "driveItems"
	graphPageSites      = "sites"
	graphPageSiteDrives = "siteDrives"
)

var graphPageNouns = map[string]string{
	graphPageDriveItems: "file",
	graphPageSites:      "site",
	graphPageSiteDrives: "document library",
}

// graphPagePatterns are the paths, below /v1.0/, a page token may point at.
// {id} is one non-empty segment and {search} the search(q='...') function
// segment; a name('key') segment counts as two, so drives('b!x')/items('y') is
// the same path as drives/b!x/items/y. Each pattern is a listing that is read
// only, never a content, sharing, permission or write endpoint.
var graphPagePatterns = map[string][][]string{
	graphPageDriveItems: {
		{"me", "drive", "root", "children"},
		{"me", "drive", "root", "{search}"},
		{"me", "drive", "items", "{id}", "children"},
		{"drives", "{id}", "root", "children"},
		{"drives", "{id}", "root", "{search}"},
		{"drives", "{id}", "items", "{id}", "children"},
	},
	graphPageSites:      {{"sites"}},
	graphPageSiteDrives: {{"sites", "{id}", "drives"}},
}

var (
	// graphKeySegment is the OData key form of a path segment: name('value').
	graphKeySegment = regexp.MustCompile(`^([A-Za-z]+)\('([^']*)'\)$`)
	// graphSearchSegment is the drive search function with its text.
	graphSearchSegment = regexp.MustCompile(`(?s)^(?:microsoft\.graph\.)?search\(q='(.*)'\)$`)
)

// graphReadPageURL admits a page token only if it is exactly the kind of link
// Graph hands out for the listing of this kind on the configured Graph host.
// The request that follows carries the user's bearer token, so a link to
// another host, a plaintext link, another user's drive, another listing, or a
// content, sharing or permission endpoint (which this operation's capability
// does not cover) is refused before any request is made.
func (s *Service) graphReadPageURL(raw, kind string) (string, error) {
	invalid := fmt.Errorf("pageToken is not a %s page token from this connection", graphPageNouns[kind])
	link, err := url.Parse(raw)
	if err != nil || link.Scheme != "https" || link.Opaque != "" || link.User != nil || link.Host == "" || strings.Contains(raw, "#") {
		return "", invalid
	}
	configured, err := url.Parse(strings.TrimRight(s.cfg.MicrosoftGraphBaseURL, "/"))
	if err != nil || configured.Host == "" || !strings.EqualFold(link.Host, configured.Host) {
		return "", invalid
	}
	prefix := strings.TrimRight(configured.EscapedPath(), "/") + "/v1.0/"
	escaped := link.EscapedPath()
	if !strings.HasPrefix(escaped, prefix) {
		return "", invalid
	}
	tokens, ok := graphPathTokens(strings.TrimPrefix(escaped, prefix))
	if !ok {
		return "", invalid
	}
	for _, pattern := range graphPagePatterns[kind] {
		if graphPathMatches(tokens, pattern) {
			return raw, nil
		}
	}
	return "", invalid
}

// graphPathTokens splits an escaped path on its literal slashes (an escaped
// slash inside a search text stays inside its segment), decodes each segment,
// and turns name('value') into the two tokens name and value. It refuses empty
// and dot segments, in either spelling.
func graphPathTokens(escapedRest string) ([]string, bool) {
	var tokens []string
	for _, rawSegment := range strings.Split(escapedRest, "/") {
		segment, err := url.PathUnescape(rawSegment)
		if err != nil || segment == "" || segment == "." || segment == ".." {
			return nil, false
		}
		if match := graphKeySegment.FindStringSubmatch(segment); match != nil {
			if match[2] == "" || match[2] == "." || match[2] == ".." {
				return nil, false
			}
			tokens = append(tokens, match[1], match[2])
			continue
		}
		tokens = append(tokens, segment)
	}
	return tokens, true
}

func graphPathMatches(tokens, pattern []string) bool {
	if len(tokens) != len(pattern) {
		return false
	}
	for index, want := range pattern {
		token := tokens[index]
		switch want {
		case "{id}":
			if strings.ContainsAny(token, `/\`) {
				return false
			}
		case "{search}":
			if !graphSearchSegmentOK(token) {
				return false
			}
		default:
			if !strings.EqualFold(token, want) {
				return false
			}
		}
	}
	return true
}

// graphSearchSegmentOK accepts search(q='text') where the text is one OData
// string literal (quotes only doubled) and does not look like a path walking
// out of it.
func graphSearchSegmentOK(token string) bool {
	match := graphSearchSegment.FindStringSubmatch(token)
	if match == nil {
		return false
	}
	text := match[1]
	if strings.Contains(strings.ReplaceAll(text, "''", ""), "'") {
		return false
	}
	return !strings.ContainsAny(text, `\`) && !strings.Contains(text, "../") && !strings.Contains(text, "/..")
}

// --- Google Drive ------------------------------------------------------------------

func (s *Service) googleDriveFiles(ctx context.Context, token, base string, params map[string]any) (any, error) {
	driveID, err := driveIDValueParam(params, "driveId")
	if err != nil {
		return nil, err
	}
	q, err := googleDriveQuery(params, driveID != "")
	if err != nil {
		return nil, err
	}
	pageToken, err := mailPageTokenParam(params)
	if err != nil {
		return nil, err
	}
	values := url.Values{
		"pageSize":                  {limitParam(params, "maxResults", driveDefaultMaxResults, driveMaxResultsCap)},
		"fields":                    {googleDriveFilesFields},
		"supportsAllDrives":         {"true"},
		"includeItemsFromAllDrives": {"true"},
	}
	if driveID != "" {
		values.Set("driveId", driveID)
		values.Set("corpora", "drive")
	}
	if q != "" {
		values.Set("q", q)
	}
	if pageToken != "" {
		values.Set("pageToken", pageToken)
	}
	return s.getBearer(ctx, token, base+"/drive/v3/files?"+values.Encode(), nil)
}

// googleDriveQuery builds Drive's `q` filter from the optional query, match and
// folderId params. It is "" when nothing narrows the listing, which keeps the
// unfiltered listing exactly as it was; once anything narrows it (a text, a
// folder or a shared drive) trashed files are left out too.
func googleDriveQuery(params map[string]any, scopedToDrive bool) (string, error) {
	query, err := mailTextParam(params, "query", driveQueryMaxRunes)
	if err != nil {
		return "", err
	}
	match, err := driveStringParam(params, "match")
	if err != nil {
		return "", err
	}
	field := "name"
	switch strings.ToLower(match) {
	case "", "name":
	case "text":
		field = "fullText"
	default:
		return "", fmt.Errorf("match must be name or text")
	}
	folderID, err := driveIDValueParam(params, "folderId")
	if err != nil {
		return "", err
	}
	var clauses []string
	if query != "" {
		clauses = append(clauses, field+" contains '"+driveEscape(query)+"'")
	}
	if query != "" || folderID != "" || scopedToDrive {
		clauses = append(clauses, "trashed=false")
	}
	if folderID != "" {
		clauses = append(clauses, "'"+driveEscape(folderID)+"' in parents")
	}
	return strings.Join(clauses, " and "), nil
}

func (s *Service) googleDriveFileContent(ctx context.Context, token, base string, params map[string]any) (any, error) {
	fileID, err := drivePathIDParam(params, "fileId", true)
	if err != nil {
		return nil, err
	}
	exportAs, err := driveExportAsParam(params)
	if err != nil {
		return nil, err
	}
	mimeType := stringParam(params, "mimeType", "")
	if mimeType == "" {
		if mimeType, err = s.googleDriveMimeType(ctx, token, base, fileID); err != nil {
			return nil, err
		}
	}
	endpoint := base + "/drive/v3/files/" + fileID + "?" + url.Values{"alt": {"media"}, "supportsAllDrives": {"true"}}.Encode()
	exportExtension := ""
	// Falls back to the mimeType (the caller's, or the one looked up) only when
	// the response carries no Content-Type — see getBearerBinaryBase64's doc.
	// For a native-Doc export this must be the EXPORT mime type (e.g. .docx or
	// text/plain), not the native application/vnd.google-apps.* type the file
	// actually is: reporting the native type on exported bytes would mislabel
	// the very bytes this call just returned.
	fallbackMimeType := mimeType
	if strings.HasPrefix(mimeType, "application/vnd.google-apps.") {
		exportMIME, extension, exportErr := googleExportTarget(mimeType, exportAs)
		if exportErr != nil {
			return nil, exportErr
		}
		// files.export has no supportsAllDrives parameter: it finds a shared-drive
		// file by id alone.
		endpoint = base + "/drive/v3/files/" + fileID + "/export?" + url.Values{"mimeType": {exportMIME}}.Encode()
		exportExtension = extension
		fallbackMimeType = exportMIME
	}
	contentB64, respMimeType, size, truncated, err := s.getBearerBinaryBase64(ctx, token, endpoint, maxContentBytes, nil)
	if err != nil {
		return nil, err
	}
	if respMimeType == "" {
		respMimeType = fallbackMimeType
	}
	result := map[string]any{
		"content_base64": contentB64,
		"mime_type":      respMimeType,
		"size":           size,
		"truncated":      truncated,
	}
	if exportExtension != "" {
		result["export_extension"] = exportExtension
	}
	return result, nil
}

// googleDriveMimeType reads one file's mime type, for a caller that did not
// send it. fileID is already path-escaped.
func (s *Service) googleDriveMimeType(ctx context.Context, token, base, fileID string) (string, error) {
	values := url.Values{"fields": {"mimeType"}, "supportsAllDrives": {"true"}}
	file, err := s.getBearerMap(ctx, token, base+"/drive/v3/files/"+fileID+"?"+values.Encode(), nil)
	if err != nil {
		return "", err
	}
	return stringFromAny(file["mimeType"]), nil
}

// driveExportAsParam reads exportAs: empty (the Office formats) or "text".
func driveExportAsParam(params map[string]any) (string, error) {
	raw, err := driveStringParam(params, "exportAs")
	if err != nil {
		return "", err
	}
	switch strings.ToLower(raw) {
	case "":
		return "", nil
	case "text":
		return "text", nil
	}
	return "", fmt.Errorf("exportAs must be text")
}

// googleExportTarget is the export a native file is read as: the Office format
// by default, text when exportAs=text.
func googleExportTarget(mimeType, exportAs string) (exportMIME, extension string, err error) {
	if exportAs == "text" {
		return googleNativeTextExportTarget(mimeType)
	}
	return googleNativeExportTarget(mimeType)
}

// googleNativeTextExportTarget maps a Google-native mime type to a plain-text
// export. A spreadsheet exports as CSV, which Drive limits to its first sheet.
func googleNativeTextExportTarget(mimeType string) (exportMIME, extension string, err error) {
	switch mimeType {
	case "application/vnd.google-apps.document", "application/vnd.google-apps.presentation":
		return "text/plain", ".txt", nil
	case "application/vnd.google-apps.spreadsheet":
		return "text/csv", ".csv", nil
	default:
		return "", "", fmt.Errorf("unsupported native Google file type %q for export", mimeType)
	}
}
