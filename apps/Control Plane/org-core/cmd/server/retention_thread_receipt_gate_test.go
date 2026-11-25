package main

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/clients"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
)

// Fixtures are low-entropy fakes. The API key stands for the launcher-minted service key of org-core's principal.
const (
	receiptTestAPIKey  = "fixture-org-core-service-api-key-0000000000000000"
	receiptTestAddr    = "model-session-core:9091"
	receiptTestAuthURL = "http://auth-core:3011"
)

// threadReceiptEnv is an environment that holds the given variables and reads every other name as unset.
func threadReceiptEnv(values map[string]string) func(string) string {
	return func(key string) string { return values[key] }
}

// receiptEnv is an environment with the switch set to switchValue and the extra variables alongside it.
func receiptEnv(switchValue string, extra map[string]string) func(string) string {
	values := map[string]string{retention.EnvThreadReceiptCheck: switchValue}
	for key, value := range extra {
		values[key] = value
	}
	return threadReceiptEnv(values)
}

// fullReceiptConfig is a complete configuration for the switch on: a private address, the Auth Core URL and
// the service API key.
func fullReceiptConfig() map[string]string {
	return map[string]string{
		envSessionCoreRetentionAddr: receiptTestAddr,
		envAuthCoreURL:              receiptTestAuthURL,
		envOrgCoreSessionAPIKey:     receiptTestAPIKey,
	}
}

// fakeTokenSource is a TokenSource for the boot tests, which never ask it for a token.
type fakeTokenSource struct{}

func (fakeTokenSource) Token(context.Context, string) (string, error) { return "fixture-token", nil }

// TestThreadReceiptSwitchOffBuildsNoReader pins switch-off behaviour: with the switch anything but the literal
// true, no reader is built, no token source is built and no call is made, even when the address and the URL are
// set to values that would be refused if they were used.
func TestThreadReceiptSwitchOffBuildsNoReader(t *testing.T) {
	unused := map[string]string{
		envSessionCoreRetentionAddr: "8.8.8.8:9091",
		envAuthCoreURL:              "not a url",
	}
	for _, value := range []string{"", "false", "1", "TRUE", " true"} {
		getenv := receiptEnv(value, unused)
		reader, err := threadReceiptFromEnvironment(getenv)
		if err != nil || reader != nil {
			t.Fatalf("%q leaves the switch off, and an off switch builds no reader: reader=%v err=%v", value, reader, err)
		}
		tokenBuilds, clientBuilds := 0, 0
		reader, err = threadReceiptReader(getenv,
			func(string, string, string) (clients.TokenSource, error) {
				tokenBuilds++
				return nil, errors.New("a token source must not be built while the switch is off")
			},
			func(string, clients.TokenSource) (*clients.SessionThreadRetentionClient, error) {
				clientBuilds++
				return nil, errors.New("a client must not be built while the switch is off")
			},
		)
		if err != nil || reader != nil || tokenBuilds != 0 || clientBuilds != 0 {
			t.Fatalf("%q: no token source and no client are built: reader=%v err=%v tokens=%d clients=%d", value, reader, err, tokenBuilds, clientBuilds)
		}
	}
}

// TestThreadReceiptSwitchOnRefusesToStartWithoutItsConfiguration pins fail-closed boot. The literal true refuses
// to start while the address, the Auth Core URL or the service API key is unset. Every refusal names the switch
// and never the key.
func TestThreadReceiptSwitchOnRefusesToStartWithoutItsConfiguration(t *testing.T) {
	without := func(key string) map[string]string {
		values := fullReceiptConfig()
		delete(values, key)
		return values
	}
	cases := map[string]map[string]string{
		"nothing configured": {},
		"no address":         without(envSessionCoreRetentionAddr),
		"no Auth Core URL":   without(envAuthCoreURL),
		"no service API key": without(envOrgCoreSessionAPIKey),
	}
	for name, extra := range cases {
		reader, err := threadReceiptFromEnvironment(receiptEnv("true", extra))
		if err == nil || reader != nil || !strings.Contains(err.Error(), retention.EnvThreadReceiptCheck) {
			t.Fatalf("%s: the literal true refuses to start, naming the switch: reader=%v err=%v", name, reader, err)
		}
		if strings.Contains(err.Error(), receiptTestAPIKey) {
			t.Fatalf("%s: the refusal must not carry the service API key", name)
		}
	}
}

// TestThreadReceiptPlaintextTargetsOnlyThePrivateNetwork pins the transport at boot. The hop is plaintext, as
// session-core serves it, so a configured address on the private network starts with a reader, and an address on
// a public network refuses to start, naming the switch.
func TestThreadReceiptPlaintextTargetsOnlyThePrivateNetwork(t *testing.T) {
	for _, addr := range []string{receiptTestAddr, "10.0.0.5:9091"} {
		values := fullReceiptConfig()
		values[envSessionCoreRetentionAddr] = addr
		reader, err := threadReceiptFromEnvironment(receiptEnv("true", values))
		if err != nil || reader == nil {
			t.Fatalf("a private-network address starts with a reader (%s): reader=%v err=%v", addr, reader, err)
		}
		client, ok := reader.(*clients.SessionThreadRetentionClient)
		if !ok {
			t.Fatalf("the reader is the session-core thread retention client, got %T", reader)
		}
		if err := client.Close(); err != nil {
			t.Fatalf("closing the reader's connection: %v", err)
		}
	}
	values := fullReceiptConfig()
	values[envSessionCoreRetentionAddr] = "8.8.8.8:9091"
	reader, err := threadReceiptFromEnvironment(receiptEnv("true", values))
	if err == nil || reader != nil || !strings.Contains(err.Error(), retention.EnvThreadReceiptCheck) {
		t.Fatalf("a public-network address refuses to start, naming the switch: reader=%v err=%v", reader, err)
	}
}

// TestThreadReceiptSwitchOnRefusesToStartWithoutATokenSource pins the mint refusal at boot. With the literal true
// and a full configuration, a token source that cannot be built refuses to start, naming the switch and never the
// key, and no client is dialled.
func TestThreadReceiptSwitchOnRefusesToStartWithoutATokenSource(t *testing.T) {
	clientBuilds := 0
	reader, err := threadReceiptReader(receiptEnv("true", fullReceiptConfig()),
		func(string, string, string) (clients.TokenSource, error) {
			return nil, errors.New("the fake token source refuses")
		},
		func(string, clients.TokenSource) (*clients.SessionThreadRetentionClient, error) {
			clientBuilds++
			return nil, errors.New("a client must not be built without a token source")
		},
	)
	if err == nil || reader != nil || !strings.Contains(err.Error(), retention.EnvThreadReceiptCheck) {
		t.Fatalf("a token source that cannot be built refuses to start, naming the switch: reader=%v err=%v", reader, err)
	}
	if strings.Contains(err.Error(), receiptTestAPIKey) {
		t.Fatal("the refusal must not carry the service API key")
	}
	if clientBuilds != 0 {
		t.Fatalf("no client is built without a token source, got %d builds", clientBuilds)
	}
}

// TestThreadReceiptMintsAsOrgCoreAgainstTheConfiguredAddress pins what the reader is built from. The token source
// is minted for org-core and no other principal, from the configured Auth Core URL and service API key, and the
// client is dialled at the configured address.
func TestThreadReceiptMintsAsOrgCoreAgainstTheConfiguredAddress(t *testing.T) {
	var gotAuthURL, gotServiceID, gotAPIKey, gotAddr string
	reader, err := threadReceiptReader(receiptEnv("true", fullReceiptConfig()),
		func(authCoreURL, serviceID, credential string) (clients.TokenSource, error) {
			gotAuthURL, gotServiceID, gotAPIKey = authCoreURL, serviceID, credential
			return fakeTokenSource{}, nil
		},
		func(addr string, _ clients.TokenSource) (*clients.SessionThreadRetentionClient, error) {
			gotAddr = addr
			return nil, errors.New("the fake client refuses")
		},
	)
	if err == nil || reader != nil || !strings.Contains(err.Error(), retention.EnvThreadReceiptCheck) {
		t.Fatalf("a client that cannot be built refuses to start, naming the switch: reader=%v err=%v", reader, err)
	}
	if gotServiceID != "org-core" || gotAuthURL != receiptTestAuthURL || gotAPIKey != receiptTestAPIKey || gotAddr != receiptTestAddr {
		t.Fatalf("the reader is built as org-core against the configured address: service=%q url=%q addr=%q", gotServiceID, gotAuthURL, gotAddr)
	}
}
