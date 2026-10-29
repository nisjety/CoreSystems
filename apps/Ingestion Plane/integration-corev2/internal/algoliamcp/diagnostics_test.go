package algoliamcp

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"testing"
)

func TestDiagnosticsExposeOnlyStageAndMachineCodes(t *testing.T) {
	for _, tc := range []struct {
		phase, body string
		status      int
		code        bool
	}{
		{"initialize", `{"error":"private-message access_token=private-token"}`, 403, false},
		{"initialize", `{"jsonrpc":"2.0","id":1,"error":{"code":-32001,"message":"private-message","data":{"secret":"private-token"}}}`, 200, true},
	} {
		var events []Diagnostic
		s, _, _ := fixtureService(t)
		s.diagnostic = func(d Diagnostic) { events = append(events, d) }
		s.http = &http.Client{Transport: fixtureTransport(func(*http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: tc.status, Header: http.Header{"Content-Type": []string{"application/json"}}, Body: io.NopCloser(strings.NewReader(tc.body))}, nil
		})}
		if _, err := s.Applications(context.Background(), "org-1", "conn_one"); err == nil {
			t.Fatal("failed initialization accepted")
		}
		if len(events) != 1 || events[0].Phase != tc.phase || events[0].HTTPStatus != tc.status || (events[0].RPCCode != nil) != tc.code {
			t.Fatalf("diagnostic %+v", events)
		}
		raw, _ := json.Marshal(events)
		if strings.Contains(string(raw), "private-") || strings.Contains(string(raw), "secret") || strings.Contains(string(raw), "token") {
			t.Fatal("sensitive diagnostic")
		}
	}
}
func TestApplicationSchemaFailureIsDiagnosedWithoutRawSchema(t *testing.T) {
	s, _, _ := fixtureService(t)
	var events []Diagnostic
	s.diagnostic = func(d Diagnostic) { events = append(events, d) }
	original := s.http.Transport
	s.http.Transport = fixtureTransport(func(r *http.Request) (*http.Response, error) {
		res, err := original.RoundTrip(r)
		if err != nil {
			return res, err
		}
		raw, _ := io.ReadAll(res.Body)
		_ = res.Body.Close()
		raw = []byte(strings.ReplaceAll(string(raw), `"enum":["APP1234567"]`, `"description":"private-schema-description"`))
		res.Body = io.NopCloser(strings.NewReader(string(raw)))
		return res, nil
	})
	if _, err := s.Applications(context.Background(), "org-1", "conn_one"); err == nil {
		t.Fatal("unrecognized application schema accepted")
	}
	if len(events) != 1 || events[0].Phase != "application-schema" {
		t.Fatalf("missing schema stage %+v", events)
	}
	raw, _ := json.Marshal(events)
	if strings.Contains(string(raw), "private-schema") || strings.Contains(string(raw), "APP1234567") {
		t.Fatal("raw schema logged")
	}
}
