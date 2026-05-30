package transform

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"encoding/xml"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"path"
	"sort"
	"strconv"
	"strings"

	"github.com/ledongthuc/pdf"
)

type MediaResult struct {
	Type     string            `json:"type"`
	Text     string            `json:"text,omitempty"`
	Metadata map[string]string `json:"metadata,omitempty"`
}

type pdfNativeExtractor func(reader io.ReaderAt, size int64) (string, int, error)
type pdfOCRExtractor func(ctx context.Context, fileName string, pdfBytes []byte) (string, int, error)

func ParseMediaURL(ctx context.Context, mediaURL string, headers map[string]string, parserMode string) (*MediaResult, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, mediaURL, nil)
	if err != nil {
		return nil, err
	}
	for k, v := range headers {
		req.Header.Set(k, v)
	}

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("download media failed: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, 20*1024*1024))
	if err != nil {
		return nil, fmt.Errorf("read media body failed: %w", err)
	}

	ext := strings.ToLower(path.Ext(mediaURL))
	mode := normalizeParserMode(parserMode)
	switch ext {
	case ".pdf":
		return parsePDFDocument(ctx, mediaURL, body, mode, extractPDFText, nil)
	case ".docx":
		text, parseErr := extractDOCXText(body)
		if parseErr != nil {
			return nil, parseErr
		}
		return &MediaResult{Type: "docx", Text: text, Metadata: map[string]string{"url": mediaURL, "parserMode": mode}}, nil
	case ".xlsx":
		text, parseErr := extractXLSXMarkdown(body)
		if parseErr != nil {
			return nil, parseErr
		}
		return &MediaResult{Type: "xlsx", Text: text, Metadata: map[string]string{"url": mediaURL, "parserMode": mode}}, nil
	default:
		return nil, fmt.Errorf("unsupported media format: %s", ext)
	}
}

func parsePDFDocument(
	ctx context.Context,
	mediaURL string,
	body []byte,
	mode string,
	nativeExtractor pdfNativeExtractor,
	ocrExtractor pdfOCRExtractor,
) (*MediaResult, error) {
	text, pageCount, parseErr := nativeExtractor(bytes.NewReader(body), int64(len(body)))
	if parseErr != nil {
		return nil, parseErr
	}

	trimmedText := strings.TrimSpace(text)
	metadata := map[string]string{
		"url":                 mediaURL,
		"parserMode":          mode,
		"parserEngine":        "native_text",
		"pageCount":           strconv.Itoa(pageCount),
		"ocrAttempted":        "false",
		"ocrAvailable":        "false",
		"nativeTextExtracted": boolString(trimmedText != ""),
	}

	shouldAttemptOCR := mode == "ocr" || (mode == "auto" && trimmedText == "")
	if !shouldAttemptOCR {
		return &MediaResult{Type: "pdf", Text: trimmedText, Metadata: metadata}, nil
	}

	metadata["ocrAttempted"] = "true"
	if ocrExtractor == nil {
		if mode == "ocr" {
			return nil, fmt.Errorf("ocr parser mode is not configured")
		}
		return &MediaResult{Type: "pdf", Text: trimmedText, Metadata: metadata}, nil
	}

	ocrText, ocrPages, ocrErr := ocrExtractor(ctx, inferMediaFilename(mediaURL, "document.pdf"), body)
	if ocrErr != nil {
		metadata["ocrError"] = ocrErr.Error()
		if mode == "ocr" {
			return nil, fmt.Errorf("ocr parse pdf failed: %w", ocrErr)
		}
		return &MediaResult{Type: "pdf", Text: trimmedText, Metadata: metadata}, nil
	}

	ocrText = strings.TrimSpace(ocrText)
	if ocrText == "" {
		metadata["ocrError"] = "ocr response contained no text"
		if mode == "ocr" {
			return nil, fmt.Errorf("ocr parse pdf failed: ocr response contained no text")
		}
		return &MediaResult{Type: "pdf", Text: trimmedText, Metadata: metadata}, nil
	}

	metadata["ocrAvailable"] = "true"
	metadata["parserEngine"] = "ai_core_read"
	if ocrPages > 0 {
		metadata["pageCount"] = strconv.Itoa(ocrPages)
	}
	return &MediaResult{Type: "pdf", Text: ocrText, Metadata: metadata}, nil
}

// ParseMediaURLWithOCR behaves identically to ParseMediaURL, except that when
// aiCoreBaseURL is non-empty PDFs are processed with OCR fallback via the
// Model Plane v2 unified analyze endpoint (POST /api/v1/analyze).
func ParseMediaURLWithOCR(ctx context.Context, mediaURL string, headers map[string]string, parserMode, aiCoreBaseURL string) (*MediaResult, error) {
	if aiCoreBaseURL == "" {
		return ParseMediaURL(ctx, mediaURL, headers, parserMode)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, mediaURL, nil)
	if err != nil {
		return nil, err
	}
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("download media failed: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, 20*1024*1024))
	if err != nil {
		return nil, fmt.Errorf("read media body failed: %w", err)
	}

	ext := strings.ToLower(path.Ext(mediaURL))
	mode := normalizeParserMode(parserMode)
	switch ext {
	case ".pdf":
		ocr := makeAICoreOCRExtractor(aiCoreBaseURL)
		return parsePDFDocument(ctx, mediaURL, body, mode, extractPDFText, ocr)
	case ".docx":
		text, parseErr := extractDOCXText(body)
		if parseErr != nil {
			return nil, parseErr
		}
		return &MediaResult{Type: "docx", Text: text, Metadata: map[string]string{"url": mediaURL, "parserMode": mode}}, nil
	case ".xlsx":
		text, parseErr := extractXLSXMarkdown(body)
		if parseErr != nil {
			return nil, parseErr
		}
		return &MediaResult{Type: "xlsx", Text: text, Metadata: map[string]string{"url": mediaURL, "parserMode": mode}}, nil
	default:
		return nil, fmt.Errorf("unsupported media format: %s", ext)
	}
}

// makeAICoreOCRExtractor returns a pdfOCRExtractor that calls the Model Plane v2
// unified analyze endpoint (POST /api/v1/analyze) to extract text from a PDF.
func makeAICoreOCRExtractor(baseURL string) pdfOCRExtractor {
	baseURL = strings.TrimRight(baseURL, "/")
	analyzeURL := baseURL + "/api/v1/analyze"

	return func(ctx context.Context, _ string, pdfBytes []byte) (string, int, error) {
		payload := map[string]interface{}{
			"content_base64": base64.StdEncoding.EncodeToString(pdfBytes),
			"mime_type":      "application/pdf",
			"model":          "layout",
		}
		payloadBytes, err := json.Marshal(payload)
		if err != nil {
			return "", 0, fmt.Errorf("ai_core analyze: marshal payload: %w", err)
		}

		analyzeReq, err := http.NewRequestWithContext(ctx, http.MethodPost, analyzeURL, bytes.NewReader(payloadBytes))
		if err != nil {
			return "", 0, fmt.Errorf("ai_core analyze: build request: %w", err)
		}
		analyzeReq.Header.Set("Content-Type", "application/json")

		analyzeResp, err := http.DefaultClient.Do(analyzeReq)
		if err != nil {
			return "", 0, fmt.Errorf("ai_core analyze: http: %w", err)
		}
		defer analyzeResp.Body.Close()

		if analyzeResp.StatusCode != http.StatusOK {
			body, _ := io.ReadAll(io.LimitReader(analyzeResp.Body, 4096))
			return "", 0, fmt.Errorf("ai_core analyze: status %d: %s", analyzeResp.StatusCode, strings.TrimSpace(string(body)))
		}

		var result struct {
			Content string     `json:"content"`
			Pages   []struct{} `json:"pages"`
		}
		if err := json.NewDecoder(analyzeResp.Body).Decode(&result); err != nil {
			return "", 0, fmt.Errorf("ai_core analyze: decode response: %w", err)
		}
		return result.Content, len(result.Pages), nil
	}
}

func extractPDFText(reader io.ReaderAt, size int64) (string, int, error) {
	pdfReader, err := pdf.NewReader(reader, size)
	if err != nil {
		return "", 0, fmt.Errorf("parse pdf failed: %w", err)
	}
	textBuilder := strings.Builder{}
	totalPage := pdfReader.NumPage()
	for pageIndex := 1; pageIndex <= totalPage; pageIndex++ {
		page := pdfReader.Page(pageIndex)
		if page.V.IsNull() {
			continue
		}
		content, err := page.GetPlainText(nil)
		if err != nil {
			continue
		}
		textBuilder.WriteString(content)
		textBuilder.WriteString("\n")
	}
	return strings.TrimSpace(textBuilder.String()), totalPage, nil
}

func inferMediaFilename(mediaURL, fallback string) string {
	if parsed, err := url.Parse(mediaURL); err == nil {
		name := path.Base(parsed.Path)
		if strings.TrimSpace(name) != "" && name != "/" && name != "." {
			return name
		}
	}
	return fallback
}

func extractDOCXText(doc []byte) (string, error) {
	archive, err := zip.NewReader(bytes.NewReader(doc), int64(len(doc)))
	if err != nil {
		return "", fmt.Errorf("open docx archive failed: %w", err)
	}

	var documentFile *zip.File
	for _, file := range archive.File {
		if file.Name == "word/document.xml" {
			documentFile = file
			break
		}
	}
	if documentFile == nil {
		return "", fmt.Errorf("word/document.xml not found in docx")
	}

	rc, err := documentFile.Open()
	if err != nil {
		return "", err
	}
	defer rc.Close()

	decoder := xml.NewDecoder(rc)
	var b strings.Builder

	for {
		tok, err := decoder.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			return "", err
		}

		switch t := tok.(type) {
		case xml.StartElement:
			if t.Name.Local == "p" { // Paragraph start
				if b.Len() > 0 {
					b.WriteString("\n")
				}
			} else if t.Name.Local == "tab" { // Tab character
				b.WriteString("\t")
			} else if t.Name.Local == "br" { // Line break
				b.WriteString("\n")
			}
		case xml.CharData:
			v := string(t)
			if len(v) > 0 {
				b.WriteString(v)
			}
		}
	}
	return strings.TrimSpace(b.String()), nil
}

type workbookXML struct {
	Sheets []workbookSheet `xml:"sheets>sheet"`
}

type workbookSheet struct {
	Name string `xml:"name,attr"`
}

type sharedStringsXML struct {
	Items []sharedStringItem `xml:"si"`
}

type sharedStringItem struct {
	Text string          `xml:"t"`
	Runs []sharedTextRun `xml:"r"`
}

type sharedTextRun struct {
	Text string `xml:"t"`
}

type worksheetXML struct {
	Rows []worksheetRow `xml:"sheetData>row"`
}

type worksheetRow struct {
	Cells []worksheetCell `xml:"c"`
}

type worksheetCell struct {
	Type         string             `xml:"t,attr"`
	Value        string             `xml:"v"`
	InlineString *worksheetRichText `xml:"is"`
}

type worksheetRichText struct {
	Text string          `xml:"t"`
	Runs []sharedTextRun `xml:"r"`
}

func extractXLSXMarkdown(doc []byte) (string, error) {
	archive, err := zip.NewReader(bytes.NewReader(doc), int64(len(doc)))
	if err != nil {
		return "", fmt.Errorf("open xlsx archive failed: %w", err)
	}

	files := make(map[string]*zip.File, len(archive.File))
	for _, file := range archive.File {
		files[file.Name] = file
	}

	sheetNames := extractWorkbookSheetNames(files["xl/workbook.xml"])
	sharedStrings, err := extractSharedStrings(files["xl/sharedStrings.xml"])
	if err != nil {
		return "", err
	}

	sheetFiles := make([]string, 0)
	for name := range files {
		if strings.HasPrefix(name, "xl/worksheets/sheet") && strings.HasSuffix(name, ".xml") {
			sheetFiles = append(sheetFiles, name)
		}
	}
	sort.Strings(sheetFiles)
	if len(sheetFiles) == 0 {
		return "", fmt.Errorf("no worksheets found in xlsx")
	}

	sections := make([]string, 0, len(sheetFiles))
	for index, sheetFile := range sheetFiles {
		rows, err := extractWorksheetRows(files[sheetFile], sharedStrings)
		if err != nil {
			return "", err
		}
		if len(rows) == 0 {
			continue
		}
		sheetName := fmt.Sprintf("Sheet %d", index+1)
		if index < len(sheetNames) && strings.TrimSpace(sheetNames[index]) != "" {
			sheetName = sheetNames[index]
		}
		sections = append(sections, renderMarkdownTable(sheetName, rows))
	}

	if len(sections) == 0 {
		return "", fmt.Errorf("xlsx contained no readable rows")
	}
	return strings.TrimSpace(strings.Join(sections, "\n\n")), nil
}

func extractWorkbookSheetNames(file *zip.File) []string {
	if file == nil {
		return nil
	}
	reader, err := file.Open()
	if err != nil {
		return nil
	}
	defer reader.Close()

	var workbook workbookXML
	if err := xml.NewDecoder(reader).Decode(&workbook); err != nil {
		return nil
	}
	names := make([]string, 0, len(workbook.Sheets))
	for _, sheet := range workbook.Sheets {
		names = append(names, strings.TrimSpace(sheet.Name))
	}
	return names
}

func extractSharedStrings(file *zip.File) ([]string, error) {
	if file == nil {
		return nil, nil
	}
	reader, err := file.Open()
	if err != nil {
		return nil, err
	}
	defer reader.Close()

	var sharedStrings sharedStringsXML
	if err := xml.NewDecoder(reader).Decode(&sharedStrings); err != nil {
		return nil, err
	}

	values := make([]string, 0, len(sharedStrings.Items))
	for _, item := range sharedStrings.Items {
		if strings.TrimSpace(item.Text) != "" {
			values = append(values, item.Text)
			continue
		}
		var builder strings.Builder
		for _, run := range item.Runs {
			builder.WriteString(run.Text)
		}
		values = append(values, builder.String())
	}
	return values, nil
}

func extractWorksheetRows(file *zip.File, sharedStrings []string) ([][]string, error) {
	if file == nil {
		return nil, fmt.Errorf("worksheet file is nil")
	}
	reader, err := file.Open()
	if err != nil {
		return nil, err
	}
	defer reader.Close()

	var worksheet worksheetXML
	if err := xml.NewDecoder(reader).Decode(&worksheet); err != nil {
		return nil, err
	}

	rows := make([][]string, 0, len(worksheet.Rows))
	for _, row := range worksheet.Rows {
		values := make([]string, 0, len(row.Cells))
		for _, cell := range row.Cells {
			values = append(values, resolveWorksheetCell(cell, sharedStrings))
		}
		rows = append(rows, values)
	}
	return rows, nil
}

func resolveWorksheetCell(cell worksheetCell, sharedStrings []string) string {
	switch cell.Type {
	case "s":
		index, err := strconv.Atoi(strings.TrimSpace(cell.Value))
		if err == nil && index >= 0 && index < len(sharedStrings) {
			return sharedStrings[index]
		}
	case "inlineStr":
		if cell.InlineString == nil {
			return ""
		}
		if strings.TrimSpace(cell.InlineString.Text) != "" {
			return cell.InlineString.Text
		}
		var builder strings.Builder
		for _, run := range cell.InlineString.Runs {
			builder.WriteString(run.Text)
		}
		return builder.String()
	}
	return strings.TrimSpace(cell.Value)
}

func renderMarkdownTable(sheetName string, rows [][]string) string {
	if len(rows) == 0 {
		return "## " + sheetName
	}
	columnCount := 0
	for _, row := range rows {
		if len(row) > columnCount {
			columnCount = len(row)
		}
	}
	if columnCount == 0 {
		return "## " + sheetName
	}

	normalizedRows := make([][]string, 0, len(rows))
	for _, row := range rows {
		current := make([]string, columnCount)
		copy(current, row)
		normalizedRows = append(normalizedRows, current)
	}

	header := normalizedRows[0]
	if isEmptyRow(header) {
		header = make([]string, columnCount)
		for index := range header {
			header[index] = fmt.Sprintf("Column %d", index+1)
		}
	}

	var builder strings.Builder
	builder.WriteString("## ")
	builder.WriteString(sheetName)
	builder.WriteString("\n\n| ")
	builder.WriteString(strings.Join(escapeMarkdownRow(header), " | "))
	builder.WriteString(" |\n| ")

	separators := make([]string, columnCount)
	for index := range separators {
		separators[index] = "---"
	}
	builder.WriteString(strings.Join(separators, " | "))
	builder.WriteString(" |")

	for _, row := range normalizedRows[1:] {
		builder.WriteString("\n| ")
		builder.WriteString(strings.Join(escapeMarkdownRow(row), " | "))
		builder.WriteString(" |")
	}

	return builder.String()
}

func escapeMarkdownRow(row []string) []string {
	escaped := make([]string, 0, len(row))
	for _, value := range row {
		current := strings.ReplaceAll(strings.TrimSpace(value), "\n", " ")
		current = strings.ReplaceAll(current, "|", "\\|")
		escaped = append(escaped, current)
	}
	return escaped
}

func isEmptyRow(row []string) bool {
	for _, value := range row {
		if strings.TrimSpace(value) != "" {
			return false
		}
	}
	return true
}

func normalizeParserMode(raw string) string {
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case "fast", "ocr":
		return strings.ToLower(strings.TrimSpace(raw))
	default:
		return "auto"
	}
}

func boolString(v bool) string {
	if v {
		return "true"
	}
	return "false"
}

func mustJSON(data any) string {
	encoded, _ := json.Marshal(data)
	return string(encoded)
}
