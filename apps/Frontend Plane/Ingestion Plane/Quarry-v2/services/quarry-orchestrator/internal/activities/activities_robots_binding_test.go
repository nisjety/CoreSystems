package activities

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/stretchr/testify/require"
)

// WS14.S7a golden, pinned on both sides (the edge test crawl_robots_golden_matches_the_go_signer
// in quarry-edge internal_auth.rs holds the same constants). First computed with an independent
// Python implementation: sha256 of the canonical "ignore", the HMAC of
// "org_a\n<url>\nrobots=<hash>", and the HMAC with the policy snapshot golden's line before it.
const (
	goldenRobotsSHA256       = "c571063a8b56f0fcf1cb9d2627406677c4130152ee49752bf5b833c767283751"
	goldenRobotsSig          = "2a239e083a07ed957d8d5621fe30049b567edbdd417baeb5fba1bc8af3927c99"
	goldenPolicyAndRobotsSig = "8ca2074067ac80a7015fa295f2bae3c2557346b6c55fe320e01bda94e65fc166"
)

func TestCrawlRobotsBindingGolden(t *testing.T) {
	sum, err := crawlRobotsSHA256(RobotsIgnore)
	require.NoError(t, err)
	require.Equal(t, goldenRobotsSHA256, sum)

	sig, err := runBindingSigPage(goldenSecret, "org_a", goldenURL, nil, &CrawlContext{V: 1, Kind: "crawl", Robots: RobotsIgnore})
	require.NoError(t, err)
	require.Equal(t, goldenRobotsSig, sig)

	both, err := runBindingSigPage(goldenSecret, "org_a", goldenURL, json.RawMessage(goldenPolicySnapshot), &CrawlContext{Robots: RobotsIgnore})
	require.NoError(t, err)
	require.Equal(t, goldenPolicyAndRobotsSig, both)
}

func TestRunBindingOfARespectingPageIsUnchanged(t *testing.T) {
	plain := runBindingSig(goldenSecret, "org_a", goldenURL)
	for _, crawl := range []*CrawlContext{nil, {Robots: RobotsRespect}} {
		sig, err := runBindingSigPage(goldenSecret, "org_a", goldenURL, nil, crawl)
		require.NoError(t, err)
		require.Equal(t, plain, sig)
		withSnapshot, err := runBindingSigPage(goldenSecret, "org_a", goldenURL, json.RawMessage(goldenPolicySnapshot), crawl)
		require.NoError(t, err)
		require.Equal(t, goldenSig, withSnapshot, "the 6b golden holds for a respecting page")
	}
	// runBindingSigExtras (no crawl context) is the same string as before.
	extras, err := runBindingSigExtras(goldenSecret, "org_a", goldenURL, nil)
	require.NoError(t, err)
	require.Equal(t, plain, extras)
}

func TestRobotsIgnoreCannotReuseARespectingSignature(t *testing.T) {
	respect, err := runBindingSigPage(goldenSecret, "org_a", goldenURL, nil, &CrawlContext{Robots: RobotsRespect})
	require.NoError(t, err)
	ignore, err := runBindingSigPage(goldenSecret, "org_a", goldenURL, nil, &CrawlContext{Robots: RobotsIgnore})
	require.NoError(t, err)
	require.NotEqual(t, respect, ignore)
	// An unknown or empty value is hashed too, never treated as respect (the edge does the same).
	empty, err := runBindingSigPage(goldenSecret, "org_a", goldenURL, nil, &CrawlContext{})
	require.NoError(t, err)
	require.NotEqual(t, respect, empty)
	require.NotEqual(t, ignore, empty)
}

func TestRunPageSignsTheCrawlRobotsOfAnIgnoringPage(t *testing.T) {
	rec := &runPageRecorder{}
	srv := rec.serve(t)
	a := New(Config{RuntimeBaseURL: srv.URL, RuntimeAuthToken: "test-token", EdgeRunSecret: goldenSecret})

	for _, robots := range []string{RobotsIgnore, RobotsRespect} {
		_, err := a.RunPage(context.Background(), RunPageInput{
			RunID: "run_test", URL: goldenURL, OrgID: "org_a", Kind: "crawl",
			Crawl: &CrawlContext{V: CrawlContractVersion, Kind: CrawlKindCrawl, Robots: robots},
		})
		require.NoError(t, err)
	}

	rec.mu.Lock()
	defer rec.mu.Unlock()
	require.Len(t, rec.bodies, 2)
	crawl, ok := rec.bodies[0]["crawl"].(map[string]any)
	require.True(t, ok)
	require.Equal(t, RobotsIgnore, crawl["robots"])
	require.Equal(t, goldenRobotsSig, rec.sigs[0])
	require.Equal(t, runBindingSig(goldenSecret, "org_a", goldenURL), rec.sigs[1])
}

func TestRobotsModeFromParamReadsTheStringAndTheOverrideObject(t *testing.T) {
	require.Equal(t, RobotsRespect, RobotsModeFromParam("respect"))
	require.Equal(t, RobotsIgnore, RobotsModeFromParam(map[string]any{
		"mode": "ignore", "authority": "web.robots_override", "rev": "0123456789ab",
	}))
	require.Equal(t, "", RobotsModeFromParam(nil))
	require.Equal(t, "", RobotsModeFromParam(map[string]any{"mode": 1}))
	require.Equal(t, "", RobotsModeFromParam(true))
}
