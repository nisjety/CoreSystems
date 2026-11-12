package server

import (
	"context"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// fillOtpServer is a signed-in user's server over store with the fill_otp
// switch set to enabled.
func fillOtpServer(store grant.Repository, enabled bool) *Server {
	cfg := DefaultConfig()
	cfg.FillOtpEnabled = enabled
	srv := NewServerWithConfig(store, cfg)
	srv.principal = func(context.Context) (authctx.Principal, error) { return userPrincipal(false), nil }
	return srv
}

// BROWSER_BROKER_FILL_OTP_ENABLED is also the kill switch: a fill_otp approval
// already issued validates as revoked once the switch is off, so its OTP-typing
// authority ends at the next validation rather than at its TTL.
func TestFillOtp_SwitchOffValidatesAnIssuedApprovalAsRevoked(t *testing.T) {
	store := grant.NewMemoryRepository()
	on := fillOtpServer(store, true)
	parent := acquire(t, on, &AcquireGrantRequest{})
	otp, err := on.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "session1", AllowedDomains: []string{"docs.example.com"},
		AllowedActions: []string{"fill_otp"}, AllowedFrameIds: []string{"frame-1"},
		ParentGrantId: parent.GetGrantId(), MailConnectionId: "mail-1", GrantOrigin: "https://docs.example.com",
	})
	if err != nil {
		t.Fatalf("fill_otp acquire with the switch on: %v", err)
	}
	val, err := on.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: otp.GetGrantId()})
	if err != nil || val.GetMailConnectionId() != "mail-1" || val.GetGrantOrigin() != "https://docs.example.com" {
		t.Fatalf("validate with the switch on = %#v, %v", val, err)
	}
	off := fillOtpServer(store, false)
	if _, err := off.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: otp.GetGrantId()}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("validate with the switch off: code = %v (%v), want FailedPrecondition", status.Code(err), err)
	}
	if _, err := off.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: parent.GetGrantId()}); err != nil {
		t.Fatalf("the parent session must still validate with the fill_otp switch off: %v", err)
	}
}

// The switch gates fill_otp only: another sensitive approval under the same
// parent still validates while the fill_otp switch is off.
func TestFillOtp_SwitchOffLeavesOtherApprovalsValid(t *testing.T) {
	store := grant.NewMemoryRepository()
	on := fillOtpServer(store, true)
	parent := acquire(t, on, &AcquireGrantRequest{})
	download, err := on.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "session1", AllowedDomains: []string{"example.com"},
		AllowedActions: []string{"download_ref"}, AllowedFrameIds: []string{"frame-1"},
		ParentGrantId: parent.GetGrantId(),
	})
	if err != nil {
		t.Fatal(err)
	}
	off := fillOtpServer(store, false)
	if _, err := off.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: download.GetGrantId()}); err != nil {
		t.Fatalf("download_ref approval with the fill_otp switch off: %v", err)
	}
}
