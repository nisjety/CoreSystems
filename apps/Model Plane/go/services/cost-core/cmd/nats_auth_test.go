package main

import (
	"testing"

	"github.com/nats-io/nats.go"
)

func TestNATSAuthOptionsUseCostCoreInbox(t *testing.T) {
	tests := []struct {
		name         string
		user         string
		password     string
		token        string
		allowToken   string
		wantUser     string
		wantPassword string
		wantToken    string
	}{
		{name: "no credentials"},
		{name: "user password", user: "cost-core-runtime", password: "scoped-password", wantUser: "cost-core-runtime", wantPassword: "scoped-password"},
		{name: "token is ignored by default", token: "legacy-token"},
		{name: "explicit token fallback", token: "legacy-token", allowToken: "1", wantToken: "legacy-token"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			t.Setenv("NATS_USER", test.user)
			t.Setenv("NATS_PASSWORD", test.password)
			t.Setenv("NATS_AUTH_TOKEN", test.token)
			t.Setenv("NATS_ALLOW_TOKEN_FALLBACK", test.allowToken)
			options := nats.GetDefaultOptions()
			for _, option := range natsAuthOptions() {
				if err := option(&options); err != nil {
					t.Fatalf("apply NATS option: %v", err)
				}
			}
			if options.InboxPrefix != "_INBOX.COST_CORE_RUNTIME" {
				t.Fatalf("inbox prefix = %q, want cost-core-specific prefix", options.InboxPrefix)
			}
			if options.User != test.wantUser || options.Password != test.wantPassword || options.Token != test.wantToken {
				t.Fatalf("credentials = user=%q password=%q token=%q, want user=%q password=%q token=%q", options.User, options.Password, options.Token, test.wantUser, test.wantPassword, test.wantToken)
			}
		})
	}
}
