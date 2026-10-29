package http

import (
	"bytes"
	"encoding/json"
	"io"
	"mime"
	"net/http"
	"net/url"
	"regexp"
)

var websiteIDPattern = regexp.MustCompile(`^web_[a-f0-9]{32}$`)

func strictQuery(r *http.Request) (url.Values, error) { return url.ParseQuery(r.URL.RawQuery) }
func websiteObject(r *http.Request, fields ...string) (map[string]json.RawMessage, error) {
	return websiteObjectOneOf(r, fields)
}

func websiteObjectOneOf(r *http.Request, variants ...[]string) (map[string]json.RawMessage, error) {
	media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
	if err != nil || media != "application/json" || r.URL.RawQuery != "" {
		return nil, io.ErrUnexpectedEOF
	}
	raw, err := io.ReadAll(io.LimitReader(r.Body, 8193))
	if err != nil || len(raw) > 8192 {
		return nil, io.ErrUnexpectedEOF
	}
	for _, fields := range variants {
		if value, err := strictObject(raw, fields...); err == nil {
			return value, nil
		}
	}
	return nil, io.ErrUnexpectedEOF
}
func strictObject(raw []byte, fields ...string) (map[string]json.RawMessage, error) {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	start, err := decoder.Token()
	if err != nil || start != json.Delim('{') {
		return nil, io.ErrUnexpectedEOF
	}
	result := map[string]json.RawMessage{}
	for decoder.More() {
		token, err := decoder.Token()
		if err != nil {
			return nil, err
		}
		key, ok := token.(string)
		if !ok {
			return nil, io.ErrUnexpectedEOF
		}
		allowed := false
		for _, field := range fields {
			if field == key {
				allowed = true
			}
		}
		if !allowed || result[key] != nil {
			return nil, io.ErrUnexpectedEOF
		}
		var value json.RawMessage
		if err := decoder.Decode(&value); err != nil {
			return nil, err
		}
		result[key] = value
	}
	if _, err := decoder.Token(); err != nil {
		return nil, err
	}
	if len(result) != len(fields) || decoder.Decode(new(any)) != io.EOF {
		return nil, io.ErrUnexpectedEOF
	}
	return result, nil
}
