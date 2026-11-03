package activities

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/stretchr/testify/require"
)

// W3-E3.5-4: run_page's text fingerprint rides on the record body only when the workflow
// asked for it (change-text-fp-v1); the raw one is always sent.
func TestCheckChangeForwardsTheTextFingerprintWhenAsked(t *testing.T) {
	srv, bodies := pageAndRecord(t, `{"status":200,"fingerprint":"blake3:raw","links":[],"text_fp":"blake3:text"}`)
	_, err := newChangeActivities(srv).CheckChange(context.Background(), CheckChangeInput{
		RunID: "run_1", OrgID: "org_a", URL: "https://example.com/p", TextFingerprint: true,
	})
	require.NoError(t, err)
	got := bodies()
	require.Len(t, got, 1)
	require.JSONEq(t, `{"org_id":"org_a","url":"https://example.com/p","fresh_fingerprint":"blake3:raw","run_id":"run_1","fresh_text_fingerprint":"blake3:text"}`, string(got[0]))
}

// A history from before the marker (TextFingerprint false) sends the body it always sent.
func TestCheckChangeOmitsTheTextFingerprintUnlessAsked(t *testing.T) {
	srv, bodies := pageAndRecord(t, `{"status":200,"fingerprint":"blake3:raw","links":[],"text_fp":"blake3:text"}`)
	_, err := newChangeActivities(srv).CheckChange(context.Background(), CheckChangeInput{
		RunID: "run_1", OrgID: "org_a", URL: "https://example.com/p",
	})
	require.NoError(t, err)
	got := bodies()
	require.Len(t, got, 1)
	var m map[string]any
	require.NoError(t, json.Unmarshal(got[0], &m))
	require.NotContains(t, m, "fresh_text_fingerprint")
}

// An edge that predates text_fp returns none: asked or not, the key is omitted.
func TestCheckChangeOmitsTheTextFingerprintWhenRunPageHasNone(t *testing.T) {
	srv, bodies := pageAndRecord(t, `{"status":200,"fingerprint":"blake3:raw","links":[]}`)
	_, err := newChangeActivities(srv).CheckChange(context.Background(), CheckChangeInput{
		RunID: "run_1", OrgID: "org_a", URL: "https://example.com/p", TextFingerprint: true,
	})
	require.NoError(t, err)
	var m map[string]any
	require.NoError(t, json.Unmarshal(bodies()[0], &m))
	require.NotContains(t, m, "fresh_text_fingerprint")
}

// The edge's silent re-baseline decodes onto the result the workflow reads.
func TestCheckChangeResultDecodesTheRebaseline(t *testing.T) {
	var res CheckChangeResult
	require.NoError(t, json.Unmarshal([]byte(`{"status":"unchanged","changed":false,"fingerprint":"text:blake3:t","raw_fingerprint":"blake3:r","baseline_id":"bln_2","rebaselined":true}`), &res))
	require.True(t, res.Rebaselined)
	require.False(t, res.Changed)
	require.Equal(t, "blake3:r", res.RawFingerprint)
	require.Equal(t, "text:blake3:t", res.Fingerprint)
}
