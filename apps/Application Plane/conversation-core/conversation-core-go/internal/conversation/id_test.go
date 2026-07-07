package conversation

import "testing"

func TestChannelForProviderMapsEmailFamilyToEmail(t *testing.T) {
	for _, p := range []string{"", "email", "Email", "microsoft", "google"} {
		if got := channelForProvider(p); got != "email" {
			t.Errorf("channelForProvider(%q) = %q, want email", p, got)
		}
	}
}

func TestChannelForProviderKeepsOtherProvidersDistinct(t *testing.T) {
	cases := map[string]string{
		"whatsapp":  "whatsapp",
		"WhatsApp":  "whatsapp",
		"messenger": "messenger",
		"discord":   "discord",
	}
	for provider, want := range cases {
		if got := channelForProvider(provider); got != want {
			t.Errorf("channelForProvider(%q) = %q, want %q", provider, got, want)
		}
	}
}

func TestChannelLabelKnownAndFallback(t *testing.T) {
	if channelLabel("email") != "Email" {
		t.Error("email label mismatch")
	}
	if channelLabel("whatsapp") != "WhatsApp" {
		t.Error("whatsapp label mismatch")
	}
	if channelLabel("messenger") != "Messenger" {
		t.Error("messenger label mismatch")
	}
	if got := channelLabel("discord"); got != "Discord" {
		t.Errorf("channelLabel(discord) = %q, want title-cased fallback", got)
	}
}

func TestContactIdentityKeyPrefersEmailThenPhoneThenRef(t *testing.T) {
	email := InboundEvent{From: ParticipantInput{Email: "a@b.no", Phone: "+4712345678"}}
	if key, kind := contactIdentityKey(email); key != "a@b.no" || kind != "email" {
		t.Errorf("got (%q,%q), want (a@b.no,email)", key, kind)
	}

	phoneOnly := InboundEvent{From: ParticipantInput{Phone: "+4712345678"}}
	if key, kind := contactIdentityKey(phoneOnly); key != "+4712345678" || kind != "phone" {
		t.Errorf("got (%q,%q), want (+4712345678,phone)", key, kind)
	}

	neither := InboundEvent{Provider: "messenger", ProviderThreadID: "psid-123"}
	key, kind := contactIdentityKey(neither)
	if kind != "ref" || key != "messenger:psid-123" {
		t.Errorf("got (%q,%q), want (messenger:psid-123,ref)", key, kind)
	}
}

func TestContactIdentityKeyDistinguishesDifferentSendersWithoutEmail(t *testing.T) {
	a := InboundEvent{Provider: "whatsapp", From: ParticipantInput{Phone: "+4711111111"}}
	b := InboundEvent{Provider: "whatsapp", From: ParticipantInput{Phone: "+4722222222"}}
	keyA, kindA := contactIdentityKey(a)
	keyB, kindB := contactIdentityKey(b)
	idA := stableContactID("org-1", kindA+":"+keyA)
	idB := stableContactID("org-1", kindB+":"+keyB)
	if idA == idB {
		t.Fatal("two different WhatsApp senders resolved to the same contact id")
	}
}

func TestContactIdentityKeyEmailAndPhoneNeverCollide(t *testing.T) {
	// A contrived case where the same literal text is used as both an email
	// local-part-less string and a phone number must not resolve to the same
	// contact id — the "kind" namespace prefix must prevent that collision.
	sameText := "0000000"
	emailID := stableContactID("org-1", "email:"+sameText)
	phoneID := stableContactID("org-1", "phone:"+sameText)
	if emailID == phoneID {
		t.Fatal("email and phone identity keys collided despite the kind namespace")
	}
}
