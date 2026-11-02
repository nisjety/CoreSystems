package sitesources

import (
	"bytes"
	"encoding/json"
	"io"
	"unicode/utf8"
)

// DecodeReceipt validates the minimal Integration-owned machine receipt. Raw
// provider JSON, keys, query logs and unrecognized fields never enter storage.
func DecodeReceipt(raw []byte) (Report, error) {
	if len(raw) > MaxReceiptBytes || !utf8.Valid(raw) || !uniqueJSON(raw) {
		return Report{}, ErrUnavailable
	}
	outer, ok := object(raw, []string{"success", "data"}, nil)
	if !ok || !bytes.Equal(bytes.TrimSpace(outer["success"]), []byte("true")) {
		return Report{}, ErrUnavailable
	}
	return decodeReport(outer["data"])
}
func decodeReport(raw []byte) (Report, error) {
	var r Report
	if !reportShape(raw) || json.Unmarshal(raw, &r) != nil || !validReport(r) {
		return Report{}, ErrUnavailable
	}
	return r, nil
}
func object(raw []byte, required, optional []string) (map[string]json.RawMessage, bool) {
	var m map[string]json.RawMessage
	if json.Unmarshal(raw, &m) != nil || m == nil {
		return nil, false
	}
	allowed := map[string]bool{}
	for _, key := range required {
		if m[key] == nil {
			return nil, false
		}
		allowed[key] = true
	}
	for _, key := range optional {
		allowed[key] = true
	}
	for key, value := range m {
		if !allowed[key] || key != "noResultRate" && bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return nil, false
		}
	}
	return m, true
}
func reportShape(raw []byte) bool {
	var header struct {
		Status   string `json:"status"`
		Provider string `json:"provider"`
	}
	if json.Unmarshal(raw, &header) != nil {
		return false
	}
	fields := []string{"websiteId", "site", "websiteRevision", "provider", "observedAt", "window", "status"}
	if header.Status != "not_loaded" && header.Status != "not_configured" {
		fields = append(fields, header.Provider)
	}
	m, ok := object(raw, fields, nil)
	if !ok {
		return false
	}
	if _, ok = object(m["window"], []string{"startDate", "endDate"}, nil); !ok {
		return false
	}
	if header.Status == "not_loaded" || header.Status == "not_configured" {
		return true
	}
	if header.Provider == "cookiebot" {
		c, ok := object(m["cookiebot"], []string{"reportingDomain", "consents", "inventory"}, nil)
		if !ok {
			return false
		}
		return consentsShape(c["consents"]) && inventoryShape(c["inventory"])
	}
	a, ok := object(m["algolia"], []string{"indexName", "region", "analytics"}, []string{"topSearches", "noResultTerms"})
	if !ok {
		return false
	}
	var status struct {
		Status string `json:"status"`
	}
	if json.Unmarshal(a["analytics"], &status) != nil {
		return false
	}
	fields = []string{"status", "reportedDays"}
	if status.Status == "connected" {
		fields = append(fields, "searches", "noResultSearches", "noResultRate")
	}
	_, ok = object(a["analytics"], fields, nil)
	if !ok {
		return false
	}
	for _, key := range []string{"topSearches", "noResultTerms"} {
		if raw, exists := a[key]; exists && !termsShape(raw) {
			return false
		}
	}
	return true
}
func termsShape(raw []byte) bool {
	var header struct {
		Status string `json:"status"`
	}
	if json.Unmarshal(raw, &header) != nil {
		return false
	}
	fields := []string{"status", "limit"}
	if header.Status == "connected" {
		fields = append(fields, "rows")
	}
	m, ok := object(raw, fields, nil)
	if !ok || header.Status != "connected" && header.Status != "unavailable" {
		return false
	}
	if header.Status == "unavailable" {
		return true
	}
	var rows []json.RawMessage
	if json.Unmarshal(m["rows"], &rows) != nil || rows == nil || len(rows) > 20 {
		return false
	}
	for _, row := range rows {
		if _, ok := object(row, []string{"query", "count"}, []string{"averageResults"}); !ok {
			return false
		}
	}
	return true
}
func consentsShape(raw []byte) bool {
	var status struct {
		Status string `json:"status"`
	}
	if json.Unmarshal(raw, &status) != nil {
		return false
	}
	fields := []string{"status", "reportedDays"}
	if status.Status == "connected" {
		fields = append(fields, "totals")
	}
	c, ok := object(raw, fields, nil)
	if !ok {
		return false
	}
	if status.Status != "connected" {
		return true
	}
	_, ok = object(c["totals"], []string{"optIn", "optOut", "preferences", "statistics", "marketing"}, nil)
	return ok
}
func inventoryShape(raw []byte) bool {
	var status struct {
		Status string `json:"status"`
	}
	if json.Unmarshal(raw, &status) != nil {
		return false
	}
	if status.Status != "connected" {
		_, ok := object(raw, []string{"status"}, nil)
		return ok
	}
	i, ok := object(raw, []string{"status", "scannedAt", "total", "categories", "thirdParty", "secure", "httpOnly"}, nil)
	if !ok {
		return false
	}
	_, ok = object(i["categories"], []string{"necessary", "preferences", "statistics", "marketing", "unclassified"}, nil)
	return ok
}
func uniqueJSON(raw []byte) bool {
	d := json.NewDecoder(bytes.NewReader(raw))
	if !jsonValue(d, 0) {
		return false
	}
	_, err := d.Token()
	return err == io.EOF
}
func jsonValue(d *json.Decoder, depth int) bool {
	if depth > 8 {
		return false
	}
	token, err := d.Token()
	if err != nil {
		return false
	}
	delim, ok := token.(json.Delim)
	if !ok {
		return true
	}
	switch delim {
	case '{':
		keys := map[string]bool{}
		for d.More() {
			key, err := d.Token()
			s, ok := key.(string)
			if err != nil || !ok || keys[s] {
				return false
			}
			keys[s] = true
			if !jsonValue(d, depth+1) {
				return false
			}
		}
		end, err := d.Token()
		return err == nil && end == json.Delim('}')
	case '[':
		for d.More() {
			if !jsonValue(d, depth+1) {
				return false
			}
		}
		end, err := d.Token()
		return err == nil && end == json.Delim(']')
	}
	return false
}
