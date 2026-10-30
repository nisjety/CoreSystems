package algoliamapping

import (
	"bytes"
	"encoding/json"
	"io"
)

func object(raw []byte, required, optional []string) (map[string]json.RawMessage, bool) {
	var m map[string]json.RawMessage
	if json.Unmarshal(raw, &m) != nil || m == nil {
		return nil, false
	}
	allowed := map[string]bool{}
	for _, k := range required {
		if m[k] == nil {
			return nil, false
		}
		allowed[k] = true
	}
	for _, k := range optional {
		allowed[k] = true
	}
	for k, v := range m {
		if !allowed[k] || bytes.Equal(bytes.TrimSpace(v), []byte("null")) {
			return nil, false
		}
	}
	return m, true
}
func receipt(raw []byte, fields []string) (json.RawMessage, error) {
	if len(raw) > 64<<10 || !unique(raw) {
		return nil, ErrUnavailable
	}
	m, ok := object(raw, []string{"success", "data"}, nil)
	if !ok || !bytes.Equal(bytes.TrimSpace(m["success"]), []byte("true")) {
		return nil, ErrUnavailable
	}
	if _, ok = object(m["data"], fields, nil); !ok {
		return nil, ErrUnavailable
	}
	return m["data"], nil
}
func DecodeAccess(raw []byte, connection string) (Access, error) {
	data, err := receipt(raw, []string{"connectionId", "status", "generation"})
	if err != nil {
		return Access{}, err
	}
	var a Access
	if json.Unmarshal(data, &a) != nil || a.ConnectionID != connection || a.Status != "active" && a.Status != "reconnect_required" || !generationPattern.MatchString(a.Generation) {
		return Access{}, ErrUnavailable
	}
	return a, nil
}
func DecodeAllowed(raw []byte, connection, app, index string) (bool, error) {
	data, err := receipt(raw, []string{"connectionId", "applicationId", "indexName", "status"})
	if err != nil {
		return false, err
	}
	var v struct {
		ConnectionID  string `json:"connectionId"`
		ApplicationID string `json:"applicationId"`
		IndexName     string `json:"indexName"`
		Status        string `json:"status"`
	}
	if json.Unmarshal(data, &v) != nil || v.ConnectionID != connection || v.ApplicationID != app || v.IndexName != index || v.Status != "allowed" && v.Status != "unavailable" {
		return false, ErrUnavailable
	}
	return v.Status == "allowed", nil
}
func unique(raw []byte) bool {
	d := json.NewDecoder(bytes.NewReader(raw))
	if !value(d, 0) {
		return false
	}
	_, err := d.Token()
	return err == io.EOF
}
func value(d *json.Decoder, depth int) bool {
	if depth > 8 {
		return false
	}
	t, err := d.Token()
	if err != nil {
		return false
	}
	mark, ok := t.(json.Delim)
	if !ok {
		return true
	}
	switch mark {
	case '{':
		keys := map[string]bool{}
		for d.More() {
			key, err := d.Token()
			s, ok := key.(string)
			if err != nil || !ok || keys[s] {
				return false
			}
			keys[s] = true
			if !value(d, depth+1) {
				return false
			}
		}
		t, err = d.Token()
		return err == nil && t == json.Delim('}')
	case '[':
		for d.More() {
			if !value(d, depth+1) {
				return false
			}
		}
		t, err = d.Token()
		return err == nil && t == json.Delim(']')
	}
	return false
}
