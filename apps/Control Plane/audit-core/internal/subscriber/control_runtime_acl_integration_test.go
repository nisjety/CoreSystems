package subscriber

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"

	server "github.com/nats-io/nats-server/v2/server"
	"github.com/nats-io/nats.go"
	"github.com/triodelab/controlplane/audit-core/internal/provisioner"
)

func TestUserRuntimePrincipalUsesProvisionedStreamsWithoutAdminAuthority(t *testing.T) {
	const (
		adminUser = "observability-provisioner-control"
		adminPass = "abcdef0123456789abcdef0123456789"
		userName  = "user-core-control"
		userPass  = "0123456789abcdef0123456789abcdef"
	)
	configPath := filepath.Join(t.TempDir(), "nats.conf")
	config := fmt.Sprintf(`
authorization {
  users = [
    {user: %q, password: %q, permissions: {publish: ">", subscribe: ">"}}
    {
      user: %q
      password: %q
      permissions: {
        publish: ["service.authenticate", "user.>", "velion.audit.v2.control.user-core.>"]
        subscribe: ["auth.>", "organization.member.>", "_INBOX.USER_CONTROL.>"]
      }
    }
  ]
}
`, adminUser, adminPass, userName, userPass)
	if err := os.WriteFile(configPath, []byte(config), 0o600); err != nil {
		t.Fatal(err)
	}
	opts, err := server.ProcessConfigFile(configPath)
	if err != nil {
		t.Fatal(err)
	}
	opts.Port = -1
	opts.JetStream = true
	opts.StoreDir = t.TempDir()
	instance, err := server.NewServer(opts)
	if err != nil {
		t.Fatal(err)
	}
	go instance.Start()
	if !instance.ReadyForConnections(10 * time.Second) {
		t.Fatal("scoped control NATS server did not become ready")
	}
	t.Cleanup(func() { instance.Shutdown(); instance.WaitForShutdown() })

	admin := connectScopedUser(t, instance.ClientURL(), adminUser, adminPass)
	adminJS, err := admin.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if err := provisioner.ProvisionControlPlaneRuntime(context.Background(), adminJS); err != nil {
		t.Fatal(err)
	}

	permissionErrors := make(chan error, 8)
	userRuntime, err := nats.Connect(
		instance.ClientURL(),
		nats.UserInfo(userName, userPass),
		nats.CustomInboxPrefix("_INBOX.USER_CONTROL"),
		nats.ErrorHandler(func(_ *nats.Conn, _ *nats.Subscription, err error) {
			permissionErrors <- err
		}),
	)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(userRuntime.Close)
	received := make(chan struct{}, 1)
	if _, err := userRuntime.Subscribe("auth.user.registered", func(*nats.Msg) {
		received <- struct{}{}
	}); err != nil {
		t.Fatalf("subscribe with scoped runtime principal: %v", err)
	}
	if err := userRuntime.Flush(); err != nil {
		t.Fatal(err)
	}
	if _, err := adminJS.Publish("auth.user.registered", []byte(`{"user_id":"user-1"}`)); err != nil {
		t.Fatal(err)
	}
	select {
	case <-received:
	case <-time.After(3 * time.Second):
		t.Fatal("scoped runtime principal did not receive auth event")
	}

	userJS, err := userRuntime.JetStream()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := userJS.Publish("user.updated", []byte(`{"user_id":"user-1"}`)); err != nil {
		t.Fatalf("publish to pre-provisioned stream: %v", err)
	}
	assertPermissionDenied(t, userRuntime, permissionErrors, "runtime stream administration", func() error {
		return userRuntime.Publish("$JS.API.STREAM.CREATE.FORGED", []byte(`{"name":"FORGED","subjects":["forged.>"]}`))
	})
}
