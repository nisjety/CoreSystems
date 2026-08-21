package clients

import (
	"strings"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

func TestSessionRunActionAuthorityClientConfigFailsClosed(t *testing.T) {
	client, err := NewSessionRunActionAuthorityClient("", "", SessionRunActionAuthorityTransport{})
	if err != nil || client != nil {
		t.Fatalf("unset client config = %#v, %v; want nil, nil", client, err)
	}
	if _, err := NewSessionRunActionAuthorityClient("session-core:50051", "Bearer forbidden", SessionRunActionAuthorityTransport{}); err == nil {
		t.Fatal("authorization scheme prefix unexpectedly accepted")
	}
}

func TestSessionRunActionAuthorityTransportRequiresTLSExceptExplicitIPLoopbackDevelopment(t *testing.T) {
	tlsCredentials, err := sessionRunActionAuthorityTransportCredentials("session-core:9091", SessionRunActionAuthorityTransport{})
	if err != nil {
		t.Fatalf("default TLS credentials error = %v", err)
	}
	if protocol := tlsCredentials.Info().SecurityProtocol; protocol != "tls" {
		t.Fatalf("default protocol = %q, want TLS", protocol)
	}
	if _, err := sessionRunActionAuthorityTransportCredentials("session-core:9091", SessionRunActionAuthorityTransport{AllowInsecureLoopback: true}); err == nil {
		t.Fatal("non-loopback insecure development transport unexpectedly accepted")
	}
	insecureCredentials, err := sessionRunActionAuthorityTransportCredentials("127.0.0.1:9091", SessionRunActionAuthorityTransport{AllowInsecureLoopback: true})
	if err != nil {
		t.Fatalf("explicit loopback development credentials error = %v", err)
	}
	if protocol := insecureCredentials.Info().SecurityProtocol; protocol != "insecure" {
		t.Fatalf("loopback development protocol = %q, want insecure", protocol)
	}
}

func TestRunActionAuthorityMappingRejectsUnresolvedAndPartialResponses(t *testing.T) {
	if _, err := runActionAuthorityFromResponse(&mpv1.ResolveRunActionAuthorityResponse{}); err == nil {
		t.Fatal("unresolved Session Core response unexpectedly accepted")
	}
	valid := &mpv1.ResolveRunActionAuthorityResponse{
		Resolved: true, RunId: "run-1", OrgId: "org-1", SubjectId: "user-1", ThreadId: "thread-1", SpaceId: "space-1",
		RecipientAudienceRef: "audience-1", RecipientAudienceRevision: 2, RecipientAudienceHash: "sha256:audience",
		PrivacyPolicyRef: "privacy-1", ThreadResourceAuthorizationRef: "control:space-1:thread-create:7", AuthorityRevision: 7,
		RunStatus: "running",
	}
	authority, err := runActionAuthorityFromResponse(valid)
	if err != nil || authority.RunID != "run-1" || authority.RunContextAuthorizationRef != valid.ThreadResourceAuthorizationRef {
		t.Fatalf("valid authority mapping = %#v, %v", authority, err)
	}
	valid.ThreadResourceAuthorizationRef = ""
	if _, err := runActionAuthorityFromResponse(valid); err == nil || !strings.Contains(err.Error(), "run_context_authorization_ref") {
		t.Fatalf("partial Session Core response error = %v; want source reference failure", err)
	}
}

func TestScheduledStepAuthorityMappingRejectsUnresolvedAndBindsSessionSubject(t *testing.T) {
	if _, err := scheduledStepAuthorityFromResponse(&mpv1.ResolveScheduledStepAuthorityResponse{}); err == nil {
		t.Fatal("unresolved scheduled-step authority unexpectedly accepted")
	}
	valid := &mpv1.ResolveScheduledStepAuthorityResponse{
		Resolved: true, RunId: "run-1", ThreadId: "thread-1", OrgId: "org-1", SubjectId: "user-1", SpaceId: "space-1",
		ScheduleId: "schedule-1", FireKey: "fire-1",
		TemplateDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		PolicyDigest:   "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		StepId:         "run-1:step:0", StepIndex: 0, IdempotencyKey: "fire-1:step:0", RunStatus: "running",
	}
	authority, err := scheduledStepAuthorityFromResponse(valid)
	if err != nil || authority.SubjectID != "user-1" {
		t.Fatalf("valid scheduled-step authority mapping = %#v, %v", authority, err)
	}
	valid.StepId = "other-run:step:0"
	if _, err := scheduledStepAuthorityFromResponse(valid); err == nil {
		t.Fatal("scheduled-step authority accepted a retargeted step")
	}
}
