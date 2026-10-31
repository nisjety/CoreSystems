package discovery

import (
	"context"
	"io"
	"net/http"
	"strings"
	"testing"
)

func TestDiscoverySeparatesPublicTransportPolicyFromServiceAccess(t *testing.T) {
	for _, body := range []string{`{"code":"FORBIDDEN","error":"private transport detail"}`, `{"code":"SECURITY_BLOCKED","error":"private transport detail"}`} {
		client, err := NewClient(ClientConfig{URL: "http://quarry-edge:8082", TokenFile: tokenFile(t, "org-one", "quarry"), OrganizationID: "org-one"}, &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: 403, Header: http.Header{"Content-Type": []string{"application/json"}}, Body: io.NopCloser(strings.NewReader(body))}, nil
		})})
		if err != nil {
			t.Fatal(err)
		}
		_, err = client.Map(context.Background(), testSite)
		failure, ok := err.(*CaptureError)
		if !ok || failure.Reason != "policy_blocked" || strings.Contains(err.Error(), "private") {
			t.Fatal("policy denial confused with a credential failure", err)
		}
	}
}
