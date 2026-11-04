package config

import (
	"encoding/hex"
	"strings"
	"testing"
)

// webPushEnvironment is a valid web push configuration. The VAPID halves are checked for presence here; their
// cryptographic validity is checked when the process loads them.
func webPushEnvironment(t *testing.T) {
	t.Helper()
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_WEB_PUSH_ENABLED", "true")
	t.Setenv("NOTIFICATION_VAPID_PUBLIC_KEY", "BPublicHalfForConfigTests")
	t.Setenv("NOTIFICATION_VAPID_PRIVATE_KEY", "PrivateHalfForConfigTests")
	t.Setenv("NOTIFICATION_VAPID_SUBJECT", "mailto:ops@example.test")
	t.Setenv("NOTIFICATION_PUSH_SUBSCRIPTION_KEY", hex.EncodeToString(make([]byte, 32)))
}

func TestWebPushIsOffUnlessTheLiteralTrueIsSet(t *testing.T) {
	baseEnvironment(t)
	for _, value := range []string{"", "1", "TRUE", "yes"} {
		t.Setenv("NOTIFICATION_WEB_PUSH_ENABLED", value)
		cfg, err := Load()
		if err != nil {
			t.Fatalf("Load() with %q error = %v", value, err)
		}
		if cfg.WebPushEnabled {
			t.Fatalf("WebPushEnabled with %q = true, want the strict literal only", value)
		}
	}
}

func TestWebPushAcceptsAFullConfiguration(t *testing.T) {
	webPushEnvironment(t)
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if !cfg.WebPushEnabled || cfg.VAPIDSubject != "mailto:ops@example.test" {
		t.Fatalf("cfg = %+v, want web push enabled with its subject", cfg)
	}
}

func TestWebPushRefusesAnIncompleteConfigurationWhenEnabled(t *testing.T) {
	cases := map[string]struct {
		key   string
		value string
		want  string
	}{
		"missing public key":       {key: "NOTIFICATION_VAPID_PUBLIC_KEY", value: "", want: "NOTIFICATION_VAPID_PUBLIC_KEY"},
		"missing private key":      {key: "NOTIFICATION_VAPID_PRIVATE_KEY", value: "", want: "NOTIFICATION_VAPID_PRIVATE_KEY"},
		"subject without a scheme": {key: "NOTIFICATION_VAPID_SUBJECT", value: "ops@example.test", want: "NOTIFICATION_VAPID_SUBJECT"},
		"subscription key short":   {key: "NOTIFICATION_PUSH_SUBSCRIPTION_KEY", value: hex.EncodeToString(make([]byte, 31)), want: "NOTIFICATION_PUSH_SUBSCRIPTION_KEY"},
		"subscription key not hex": {key: "NOTIFICATION_PUSH_SUBSCRIPTION_KEY", value: strings.Repeat("z", 64), want: "NOTIFICATION_PUSH_SUBSCRIPTION_KEY"},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			webPushEnvironment(t)
			t.Setenv(tc.key, tc.value)
			_, err := Load()
			if err == nil {
				t.Fatalf("Load() error = nil, want a refused %s", tc.key)
			}
			if !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("Load() error = %v, want it to name %s", err, tc.want)
			}
		})
	}
}

func TestWebPushConfigurationIsIgnoredWhileOff(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_VAPID_SUBJECT", "not-a-contact")
	if _, err := Load(); err != nil {
		t.Fatalf("Load() with web push off and a bad subject error = %v, want it ignored", err)
	}
}
