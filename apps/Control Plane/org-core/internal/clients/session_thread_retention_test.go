package clients

import (
	"context"
	"errors"
	"testing"
)

// fakeReceipts answers from one fixed receipt or error and records each call it receives.
type fakeReceipts struct {
	receipt ThreadReceipt
	err     error
	asked   [][2]string
}

func (f *fakeReceipts) GetThreadRetention(_ context.Context, orgID, threadID string) (ThreadReceipt, error) {
	f.asked = append(f.asked, [2]string{orgID, threadID})
	return f.receipt, f.err
}

func TestNothingRetainedFollowsTheReceipt(t *testing.T) {
	cases := []struct {
		name    string
		receipt ThreadReceipt
		want    bool
	}{
		{name: "no receipt is not proven empty", receipt: ThreadReceipt{Found: false, NothingRetained: true}, want: false},
		{name: "a zero-retention receipt", receipt: ThreadReceipt{Found: true, NothingRetained: true}, want: true},
		{name: "a receipt with retained content", receipt: ThreadReceipt{Found: true, NothingRetained: false}, want: false},
	}
	for _, tc := range cases {
		source := &fakeReceipts{receipt: tc.receipt}
		client := newSessionThreadRetentionClient(source, nil)
		got, err := client.NothingRetained(context.Background(), " org-1 ", " thr-1 ")
		if err != nil || got != tc.want {
			t.Fatalf("%s: got %v err=%v want %v", tc.name, got, err, tc.want)
		}
		if len(source.asked) != 1 || source.asked[0] != [2]string{"org-1", "thr-1"} {
			t.Fatalf("%s: the ids are sent trimmed and once per call: %v", tc.name, source.asked)
		}
	}
}

func TestNothingRetainedNeverGuesses(t *testing.T) {
	denied := errors.New("session-core permission denied")
	source := &fakeReceipts{err: denied, receipt: ThreadReceipt{Found: true, NothingRetained: true}}
	client := newSessionThreadRetentionClient(source, nil)

	got, err := client.NothingRetained(context.Background(), "org-1", "thr-1")
	if !errors.Is(err, denied) || got {
		t.Fatalf("a reader error is returned, never read as nothing retained: got=%v err=%v", got, err)
	}
	if _, err := (*SessionThreadRetentionClient)(nil).NothingRetained(context.Background(), "org-1", "thr-1"); err == nil {
		t.Fatal("an unconfigured client refuses, it does not answer")
	}
	if _, err := client.NothingRetained(context.Background(), " ", "thr-1"); err == nil {
		t.Fatal("a call without an organisation is refused before it reaches session-core")
	}
	if len(source.asked) != 1 {
		t.Fatalf("only the one valid call reaches session-core: %v", source.asked)
	}
}

// TestPlaintextIsPrivateNetworkOnly pins the transport posture. session-core serves plaintext on its private
// listener, so the hop is plaintext, and only a target on the private network may carry it. A public address,
// a zoned public IPv6 literal, an IPv4-mapped public address, a missing port, or a port that is not a number from
// 1 to 65535 is refused.
func TestPlaintextIsPrivateNetworkOnly(t *testing.T) {
	for _, target := range []string{
		"model-session-core:9091",
		"127.0.0.1:9091",
		"[::1]:9091",
		"10.0.0.5:9091",
		"172.20.0.7:9091",
		"192.168.1.9:9091",
		"[fd00::5]:9091",
		"[::ffff:10.0.0.5]:9091",
		"[fe80::1%eth0]:9091",
	} {
		if !isPrivateNetworkTarget(target) {
			t.Fatalf("a private-network target may carry the plaintext hop (%s)", target)
		}
	}
	for _, target := range []string{
		"8.8.8.8:9091",
		"[2001:4860:4860::8888]:9091",
		"[2001:4860:4860::8888%eth0]:9091",
		"[::ffff:8.8.8.8]:9091",
		"model-session-core",
		"model-session-core:",
		"model-session-core:0",
		"model-session-core:70000",
		":9091",
		"",
	} {
		if isPrivateNetworkTarget(target) {
			t.Fatalf("%q must not carry the plaintext hop", target)
		}
	}
}

// TestConstructorRefusesAnUnsetAddressOrTokenSource pins the constructor: an unset address, a missing token
// source, or an address off the private network builds no client. A complete configuration builds one that
// closes cleanly, without dialing.
func TestConstructorRefusesAnUnsetAddressOrTokenSource(t *testing.T) {
	tokens := &fakeTokens{}
	cases := map[string]struct {
		addr   string
		tokens TokenSource
	}{
		"no address":       {"", tokens},
		"blank address":    {"   ", tokens},
		"no token source":  {"model-session-core:9091", nil},
		"a public address": {"8.8.8.8:9091", tokens},
	}
	for name, tc := range cases {
		client, err := NewSessionThreadRetentionClient(tc.addr, tc.tokens)
		if client != nil || err == nil {
			t.Fatalf("%s: no client is built: client=%v err=%v", name, client, err)
		}
	}
	client, err := NewSessionThreadRetentionClient("model-session-core:9091", tokens)
	if err != nil || client == nil {
		t.Fatalf("a complete configuration builds the client without dialing: client=%v err=%v", client, err)
	}
	if err := client.Close(); err != nil {
		t.Fatalf("closing the owned connection: %v", err)
	}
}
