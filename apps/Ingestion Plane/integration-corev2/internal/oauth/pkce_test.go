package oauth

import "testing"

func TestCodeChallengeS256(t *testing.T) {
	got := CodeChallengeS256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
	want := "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
	if got != want {
		t.Fatalf("challenge = %q, want %q", got, want)
	}
}

func TestCodeChallengeS256Hex(t *testing.T) {
	got := CodeChallengeS256Hex("abc")
	want := "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
	if got != want {
		t.Fatalf("challenge = %q, want %q", got, want)
	}
}

func TestHashStateIsStable(t *testing.T) {
	a := HashState("state")
	b := HashState("state")
	if a != b || a == "state" {
		t.Fatalf("HashState not stable/hashed: %q %q", a, b)
	}
}
