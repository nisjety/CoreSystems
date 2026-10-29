package webvitals

import (
	"net/http"
	"testing"
)

func TestInstallationMetadataDoesNotFetchOrCrossContext(t *testing.T) {
	service, err := NewService(manifest(t, nil), "production", &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) { t.Fatal("metadata fetched samples"); return nil, nil })}, nil)
	if err != nil {
		t.Fatal(err)
	}
	in := input()
	if ok, err := service.Configured("org-1", in); err != nil || !ok {
		t.Fatal("missing installation", ok, err)
	}
	if ok, _ := service.Configured("org-2", in); ok {
		t.Fatal("cross-org")
	}
	in.WebsiteRevision++
	if ok, _ := service.Configured("org-1", in); ok {
		t.Fatal("stale revision")
	}
	in = input()
	in.Environment = "development"
	if ok, _ := service.Configured("org-1", in); ok {
		t.Fatal("cross-environment")
	}
}
