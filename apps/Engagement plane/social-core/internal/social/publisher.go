package social

import (
	"bytes"
	"context"
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"mime/multipart"
	"net/http"
	"net/url"
	"strings"
	"time"
)

const socialPublisherConsumer = "social-publisher"

type PublisherConfig struct {
	LinkedInAPIBaseURL       string
	LinkedInAPIVersion       string
	XAPIBaseURL              string
	InstagramGraphAPIBaseURL string
	FacebookGraphAPIBaseURL  string
	TikTokAPIBaseURL         string
	SnapchatAPIBaseURL       string
	// SnapchatBusinessAPIBaseURL is the Public Profile API host
	// (businessapi.snapchat.com) used for organic Story/Spotlight posting.
	SnapchatBusinessAPIBaseURL string
	// SnapchatLivePublishing gates real organic posting to the Public Profile
	// API. It defaults OFF: the Public Profile API is allowlist-only (Snap must
	// allowlist the OAuth app's client id) and needs vendor credentials, so
	// live posting stays disabled until an operator explicitly enables it —
	// same hard test-mode pattern as the Bring shipping adapter.
	SnapchatLivePublishing bool
}

type HTTPPublisher struct {
	tokenBroker TokenBroker
	httpClient  *http.Client
	cfg         PublisherConfig
	now         func() time.Time
}

func NewHTTPPublisher(tokenBroker TokenBroker, httpClient *http.Client, cfg PublisherConfig) *HTTPPublisher {
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 20 * time.Second}
	}
	return &HTTPPublisher{
		tokenBroker: tokenBroker,
		httpClient:  httpClient,
		cfg: PublisherConfig{
			LinkedInAPIBaseURL:         strings.TrimRight(fallback(cfg.LinkedInAPIBaseURL, "https://api.linkedin.com"), "/"),
			LinkedInAPIVersion:         fallback(cfg.LinkedInAPIVersion, "202606"),
			XAPIBaseURL:                strings.TrimRight(fallback(cfg.XAPIBaseURL, "https://api.x.com"), "/"),
			InstagramGraphAPIBaseURL:   strings.TrimRight(fallback(cfg.InstagramGraphAPIBaseURL, "https://graph.facebook.com/v23.0"), "/"),
			FacebookGraphAPIBaseURL:    strings.TrimRight(fallback(cfg.FacebookGraphAPIBaseURL, "https://graph.facebook.com/v23.0"), "/"),
			TikTokAPIBaseURL:           strings.TrimRight(fallback(cfg.TikTokAPIBaseURL, "https://open.tiktokapis.com"), "/"),
			SnapchatAPIBaseURL:         strings.TrimRight(fallback(cfg.SnapchatAPIBaseURL, "https://adsapi.snapchat.com/v1"), "/"),
			SnapchatBusinessAPIBaseURL: strings.TrimRight(fallback(cfg.SnapchatBusinessAPIBaseURL, "https://businessapi.snapchat.com/v1"), "/"),
			SnapchatLivePublishing:     cfg.SnapchatLivePublishing,
		},
		now: func() time.Time { return time.Now().UTC() },
	}
}

func (p *HTTPPublisher) Publish(ctx context.Context, job PublishJob, post Post, account Account) PublishAttempt {
	attempt := PublishAttempt{
		OrgID:       job.OrgID,
		JobID:       job.ID,
		PostID:      post.ID,
		ProviderKey: normalizePlatform(account.ProviderKey),
		Status:      AttemptStatusBlocked,
		Mode:        "api",
		Warnings:    []string{},
		Response:    map[string]any{},
		AttemptedAt: p.now(),
	}
	if p == nil || p.tokenBroker == nil {
		attempt.Message = "Provider publisher is not configured."
		return attempt
	}
	if strings.TrimSpace(account.ConnectionID) == "" {
		attempt.Message = "Connected account does not include an integration-core connection id."
		return attempt
	}
	if !accountHasCapability(account, "social.post.write") {
		attempt.Message = "Connected account does not grant social.post.write for organic publishing."
		return attempt
	}
	token, err := p.tokenBroker.AccessToken(ctx, TokenRequest{
		OrganizationID: job.OrgID,
		ConnectionID:   account.ConnectionID,
		ConnectorType:  normalizePlatform(account.ProviderKey),
		Consumer:       socialPublisherConsumer,
	})
	if err != nil {
		attempt.Status = AttemptStatusFailed
		attempt.Message = "Could not lease provider token from integration-core."
		attempt.Response = map[string]any{"error": err.Error()}
		return attempt
	}
	if token == nil || strings.TrimSpace(token.AccessToken) == "" {
		attempt.Status = AttemptStatusFailed
		attempt.Message = "Integration-core returned an empty provider token lease."
		return attempt
	}

	switch attempt.ProviderKey {
	case "linkedin":
		return p.publishLinkedIn(ctx, attempt, post, account, token.AccessToken)
	case "x":
		return p.publishX(ctx, attempt, post, token.AccessToken)
	case "instagram":
		return p.publishInstagram(ctx, attempt, post, account, token.AccessToken)
	case "facebook":
		return p.publishFacebook(ctx, attempt, post, account, token.AccessToken)
	case "meta":
		// Unified Meta connection (Facebook Pages + Instagram + WhatsApp + Ads
		// behind one grant): organic posts publish via the Facebook Pages
		// surface. When the account metadata carries an Instagram user id,
		// route to the Instagram publisher instead.
		if metadataString(account.Metadata, "instagram_user_id", "instagramUserId", "ig_user_id") != "" {
			return p.publishInstagram(ctx, attempt, post, account, token.AccessToken)
		}
		return p.publishFacebook(ctx, attempt, post, account, token.AccessToken)
	case "whatsapp":
		attempt.Message = "WhatsApp is a messaging surface — send template/session messages via inbox workflows, not organic publishing."
		return attempt
	case "meta-ads":
		attempt.Message = "Meta Ads connections manage campaigns, not organic posts; use ads workflows."
		return attempt
	case "tiktok":
		return p.publishTikTok(ctx, attempt, post, account, token.AccessToken)
	case "snapchat":
		return p.publishSnapchat(ctx, attempt, post, account, token.AccessToken)
	default:
		attempt.Message = "Unsupported social provider."
		return attempt
	}
}

func (p *HTTPPublisher) publishLinkedIn(ctx context.Context, attempt PublishAttempt, post Post, account Account, accessToken string) PublishAttempt {
	attempt.Endpoint = "POST /rest/posts"
	authorURN := metadataString(account.Metadata, "author_urn", "authorUrn", "linkedin_author_urn")
	if authorURN == "" && account.Handle != "" && strings.HasPrefix(account.Handle, "urn:li:") {
		authorURN = account.Handle
	}
	if authorURN == "" {
		authorURN = metadataString(account.Metadata, "provider_account_urn", "providerAccountUrn")
	}
	if authorURN == "" {
		attempt.Message = "LinkedIn publishing requires an author URN in integration provider context."
		return attempt
	}
	payload := map[string]any{
		"author":         authorURN,
		"commentary":     previewText(post, "linkedin"),
		"visibility":     "PUBLIC",
		"lifecycleState": "PUBLISHED",
		"distribution": map[string]any{
			"feedDistribution":               "MAIN_FEED",
			"targetEntities":                 []any{},
			"thirdPartyDistributionChannels": []any{},
		},
		"isReshareDisabledByAuthor": false,
	}
	req, err := jsonRequest(ctx, http.MethodPost, p.cfg.LinkedInAPIBaseURL+"/rest/posts", payload)
	if err != nil {
		return failedAttempt(attempt, "Could not build LinkedIn publish request.", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("LinkedIn-Version", p.cfg.LinkedInAPIVersion)
	req.Header.Set("X-Restli-Protocol-Version", "2.0.0")
	return p.doJSON(attempt, req, http.StatusCreated, "LinkedIn post published.")
}

func (p *HTTPPublisher) publishX(ctx context.Context, attempt PublishAttempt, post Post, accessToken string) PublishAttempt {
	attempt.Endpoint = "POST /2/tweets"
	payload := map[string]any{"text": previewText(post, "x")}
	req, err := jsonRequest(ctx, http.MethodPost, p.cfg.XAPIBaseURL+"/2/tweets", payload)
	if err != nil {
		return failedAttempt(attempt, "Could not build X publish request.", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	return p.doJSON(attempt, req, http.StatusCreated, "X post published.")
}

func (p *HTTPPublisher) publishInstagram(ctx context.Context, attempt PublishAttempt, post Post, account Account, accessToken string) PublishAttempt {
	attempt.Endpoint = "POST /{ig-user-id}/media + POST /{ig-user-id}/media_publish"
	mediaURL := publicMediaURL(post)
	if mediaURL == "" {
		attempt.Message = "Instagram publishing requires a public image or video URL."
		return attempt
	}
	igUserID := firstNonEmpty(metadataString(account.Metadata, "ig_user_id", "igUserId", "instagram_user_id"), account.Handle)
	if igUserID == "" {
		attempt.Message = "Instagram publishing requires an Instagram Business user id."
		return attempt
	}
	form := url.Values{}
	form.Set("caption", previewText(post, "instagram"))
	if mediaKind(post) == "video" {
		form.Set("media_type", "REELS")
		form.Set("video_url", mediaURL)
	} else {
		form.Set("image_url", mediaURL)
	}
	createURL := p.cfg.InstagramGraphAPIBaseURL + "/" + url.PathEscape(igUserID) + "/media"
	req, err := formRequest(ctx, http.MethodPost, createURL, form)
	if err != nil {
		return failedAttempt(attempt, "Could not build Instagram media request.", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	createResp, err := p.doRaw(req)
	if err != nil {
		return failedAttempt(attempt, "Instagram media container request failed.", err)
	}
	if createResp.status < 200 || createResp.status >= 300 {
		return providerErrorAttempt(attempt, "Instagram media container was rejected.", createResp)
	}
	creationID, _ := createResp.body["id"].(string)
	if strings.TrimSpace(creationID) == "" {
		attempt.Status = AttemptStatusFailed
		attempt.Message = "Instagram media container response did not include a creation id."
		attempt.Response = redactProviderBody(createResp.body)
		return attempt
	}

	// Video/REELS containers process asynchronously; calling media_publish
	// while status_code is still IN_PROGRESS returns an HTTP 400. Image
	// containers process near-instantly and Meta does not document this
	// wait as required for them, matching the video-only check below.
	if mediaKind(post) == "video" {
		if err := p.waitForInstagramMediaReady(ctx, accessToken, creationID); err != nil {
			attempt.Status = AttemptStatusFailed
			attempt.Message = "Instagram media container did not finish processing."
			attempt.Response = map[string]any{"error": err.Error()}
			return attempt
		}
	}

	publishForm := url.Values{}
	publishForm.Set("creation_id", creationID)
	publishURL := p.cfg.InstagramGraphAPIBaseURL + "/" + url.PathEscape(igUserID) + "/media_publish"
	publishReq, err := formRequest(ctx, http.MethodPost, publishURL, publishForm)
	if err != nil {
		return failedAttempt(attempt, "Could not build Instagram publish request.", err)
	}
	publishReq.Header.Set("Authorization", "Bearer "+accessToken)
	publishResp, err := p.doRaw(publishReq)
	if err != nil {
		return failedAttempt(attempt, "Instagram publish request failed.", err)
	}
	if publishResp.status < 200 || publishResp.status >= 300 {
		return providerErrorAttempt(attempt, "Instagram publish was rejected.", publishResp)
	}
	attempt.Status = AttemptStatusSucceeded
	attempt.Message = "Instagram post published."
	attempt.ExternalID, _ = publishResp.body["id"].(string)
	attempt.Response = redactProviderBody(publishResp.body)
	return attempt
}

const (
	instagramContainerPollInterval = 3 * time.Second
	// ~2 minutes total, inside Meta's documented "30s to a few minutes"
	// typical processing window for video/Reels containers.
	instagramContainerPollAttempts = 40
)

// waitForInstagramMediaReady polls a media container's processing status
// until it reports FINISHED, per Meta's Content Publishing docs.
func (p *HTTPPublisher) waitForInstagramMediaReady(ctx context.Context, accessToken, creationID string) error {
	statusURL := p.cfg.InstagramGraphAPIBaseURL + "/" + url.PathEscape(creationID) + "?fields=status_code"
	for range instagramContainerPollAttempts {
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, statusURL, nil)
		if err != nil {
			return fmt.Errorf("build media status request: %w", err)
		}
		req.Header.Set("Authorization", "Bearer "+accessToken)
		req.Header.Set("Accept", "application/json")
		resp, err := p.doRaw(req)
		if err != nil {
			return fmt.Errorf("call media status: %w", err)
		}
		if resp.status < 200 || resp.status >= 300 {
			return fmt.Errorf("media status lookup returned status %d", resp.status)
		}
		switch status, _ := resp.body["status_code"].(string); status {
		case "FINISHED":
			return nil
		case "ERROR", "EXPIRED":
			return fmt.Errorf("media container processing failed with status %s", status)
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(instagramContainerPollInterval):
		}
	}
	return fmt.Errorf("media container did not finish processing within %s", instagramContainerPollInterval*instagramContainerPollAttempts)
}

func (p *HTTPPublisher) publishFacebook(ctx context.Context, attempt PublishAttempt, post Post, account Account, accessToken string) PublishAttempt {
	pageID := firstNonEmpty(metadataString(account.Metadata, "page_id", "pageId", "facebook_page_id"), account.Handle)
	if pageID == "" {
		attempt.Message = "Facebook publishing requires a Page id in integration provider context."
		return attempt
	}
	// Page Feed/Photos are Page-scoped edges: Meta's Graph API reference for
	// them ("A Page access token, pages_manage_posts, pages_read_engagement,
	// pages_show_list" under Requirements) rejects the connecting user's own
	// User access token even when that user administers the Page. Exchange
	// for the Page token first — this was a "connects clean, every post
	// fails" bug before this fix.
	pageToken, err := p.facebookPageAccessToken(ctx, accessToken, pageID)
	if err != nil {
		return failedAttempt(attempt, "Could not obtain a Facebook Page access token.", err)
	}
	if mediaURL := publicMediaURL(post); mediaURL != "" && mediaKind(post) != "video" {
		attempt.Endpoint = "POST /{page-id}/photos"
		form := url.Values{}
		form.Set("url", mediaURL)
		form.Set("caption", previewText(post, "facebook"))
		req, err := formRequest(ctx, http.MethodPost, p.cfg.FacebookGraphAPIBaseURL+"/"+url.PathEscape(pageID)+"/photos", form)
		if err != nil {
			return failedAttempt(attempt, "Could not build Facebook photo request.", err)
		}
		req.Header.Set("Authorization", "Bearer "+pageToken)
		return p.doJSON(attempt, req, http.StatusOK, "Facebook Page photo published.")
	}
	attempt.Endpoint = "POST /{page-id}/feed"
	form := url.Values{}
	form.Set("message", previewText(post, "facebook"))
	req, err := formRequest(ctx, http.MethodPost, p.cfg.FacebookGraphAPIBaseURL+"/"+url.PathEscape(pageID)+"/feed", form)
	if err != nil {
		return failedAttempt(attempt, "Could not build Facebook Page post request.", err)
	}
	req.Header.Set("Authorization", "Bearer "+pageToken)
	return p.doJSON(attempt, req, http.StatusOK, "Facebook Page post published.")
}

// facebookPageAccessToken exchanges the connecting user's OAuth access token
// for a Page-scoped token, per Meta's documented Page publishing
// requirements. Instagram Content Publishing deliberately does NOT use this
// — Meta's own Graph API accepts the linked user token directly for
// /{ig-user-id}/media and /media_publish (confirmed against real production
// integrations); do not apply this exchange to publishInstagram.
func (p *HTTPPublisher) facebookPageAccessToken(ctx context.Context, userToken, pageID string) (string, error) {
	endpoint := p.cfg.FacebookGraphAPIBaseURL + "/" + url.PathEscape(pageID) + "?fields=access_token"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return "", fmt.Errorf("build Page access token request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+userToken)
	req.Header.Set("Accept", "application/json")
	resp, err := p.doRaw(req)
	if err != nil {
		return "", fmt.Errorf("call Page access token lookup: %w", err)
	}
	if resp.status < 200 || resp.status >= 300 {
		return "", fmt.Errorf("Page access token lookup for %s returned status %d", pageID, resp.status)
	}
	pageToken, _ := resp.body["access_token"].(string)
	if strings.TrimSpace(pageToken) == "" {
		return "", fmt.Errorf("Page %s did not return an access_token — the connected user may not administer this Page", pageID)
	}
	return pageToken, nil
}

func (p *HTTPPublisher) publishTikTok(ctx context.Context, attempt PublishAttempt, post Post, account Account, accessToken string) PublishAttempt {
	attempt.Endpoint = "POST /v2/post/publish/content/init/"
	mediaURL := publicMediaURL(post)
	if mediaURL == "" {
		attempt.Message = "TikTok publishing requires a public video or photo URL."
		return attempt
	}
	if metadataString(account.Metadata, "creator_ready", "creatorReady") != "true" {
		attempt.Message = "TikTok Direct Post requires creator_info readiness in provider context."
		return attempt
	}
	payload := map[string]any{
		"post_info": map[string]any{
			"title":                previewText(post, "tiktok"),
			"privacy_level":        fallback(metadataString(account.Metadata, "privacy_level", "privacyLevel"), "SELF_ONLY"),
			"disable_duet":         true,
			"disable_comment":      false,
			"disable_stitch":       true,
			"auto_add_music":       false,
			"brand_content_toggle": false,
			"brand_organic_toggle": false,
		},
		"source_info": map[string]any{
			"source":    "PULL_FROM_URL",
			"media_url": mediaURL,
		},
		"post_mode":  "DIRECT_POST",
		"media_type": strings.ToUpper(mediaKind(post)),
	}
	req, err := jsonRequest(ctx, http.MethodPost, p.cfg.TikTokAPIBaseURL+"/v2/post/publish/content/init/", payload)
	if err != nil {
		return failedAttempt(attempt, "Could not build TikTok publish request.", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	return p.doJSON(attempt, req, http.StatusOK, "TikTok publish initialized.")
}

const (
	// Snapchat Public Profile multipart upload: chunks up to 32MB, max 35 parts
	// (~1GB). See Profile Asset Management docs.
	snapchatChunkSize = 32 * 1024 * 1024
	snapchatMaxParts  = 35
	// Bound the media we download+encrypt+push. Organic Snapchat content is
	// short-form (Story/Spotlight ≤60s); this caps memory well under Snapchat's
	// 1GB hard ceiling.
	snapchatMaxMediaBytes = 200 * 1024 * 1024
)

// publishSnapchat posts organic content to a Snapchat Public Profile via the
// Public Profile API (businessapi.snapchat.com). Unlike Meta/TikTok, Snapchat
// has no pull-from-URL: the file bytes must be AES-256-CBC encrypted
// (openssl-compatible) and PUSHed via a create-container → multipart
// ADD/FINALIZE → post-story/spotlight sequence, which this method owns.
//
// It is hard-gated behind SnapchatLivePublishing (default off): the Public
// Profile API is allowlist-only and needs vendor credentials, so with the gate
// off — the honest default until an operator wires both — every attempt returns
// a clear blocked result instead of pretending to post. The approval gate is
// enforced upstream in service.go (ensurePublishApproved) for every provider,
// so this path inherits it identically.
func (p *HTTPPublisher) publishSnapchat(ctx context.Context, attempt PublishAttempt, post Post, account Account, accessToken string) PublishAttempt {
	attempt.Endpoint = "POST /v1/public_profiles/{profile_id}/media + /stories|/spotlights"
	if !p.cfg.SnapchatLivePublishing {
		attempt.Message = "Snapchat organic publishing is config-gated (SNAPCHAT_LIVE_PUBLISHING is off). The Public Profile API is allowlist-only and requires vendor credentials plus Snap allowlisting of the OAuth app before live posting can be enabled."
		attempt.Warnings = append(attempt.Warnings, "snapchat_live_publishing_disabled")
		return attempt
	}
	profileID := firstNonEmpty(
		metadataString(account.Metadata, "public_profile_id", "publicProfileId", "profile_id", "snapchat_profile_id"),
		account.Handle,
	)
	if profileID == "" {
		attempt.Message = "Snapchat organic publishing requires a Public Profile id (public_profile_id) in the connected account's provider context."
		return attempt
	}
	mediaURL := publicMediaURL(post)
	if mediaURL == "" {
		attempt.Message = "Snapchat organic publishing requires a public image or video URL."
		return attempt
	}

	mediaBytes, err := p.downloadMedia(ctx, mediaURL)
	if err != nil {
		return failedAttempt(attempt, "Could not download post media for Snapchat upload.", err)
	}
	key := make([]byte, 32)
	iv := make([]byte, 16)
	if _, err := rand.Read(key); err != nil {
		return failedAttempt(attempt, "Could not generate Snapchat media key.", err)
	}
	if _, err := rand.Read(iv); err != nil {
		return failedAttempt(attempt, "Could not generate Snapchat media iv.", err)
	}
	encrypted, err := encryptSnapchatMedia(mediaBytes, key, iv)
	if err != nil {
		return failedAttempt(attempt, "Could not encrypt Snapchat media.", err)
	}

	mediaType := "IMAGE"
	if mediaKind(post) == "video" {
		mediaType = "VIDEO"
	}
	base := p.cfg.SnapchatBusinessAPIBaseURL

	// 1. Create the media container (carries the base64 key/iv Snapchat needs to decrypt).
	createBody := map[string]any{
		"type": mediaType,
		"name": fallback(post.Title, "verevon-media"),
		"key":  base64.StdEncoding.EncodeToString(key),
		"iv":   base64.StdEncoding.EncodeToString(iv),
	}
	createReq, err := jsonRequest(ctx, http.MethodPost, base+"/public_profiles/"+url.PathEscape(profileID)+"/media", createBody)
	if err != nil {
		return failedAttempt(attempt, "Could not build Snapchat media container request.", err)
	}
	createReq.Header.Set("Authorization", "Bearer "+accessToken)
	createResp, err := p.doRaw(createReq)
	if err != nil {
		return failedAttempt(attempt, "Snapchat media container request failed.", err)
	}
	if !snapchatOK(createResp) {
		return providerErrorAttempt(attempt, "Snapchat rejected the media container.", createResp)
	}
	mediaID, _ := createResp.body["media_id"].(string)
	addPath, _ := createResp.body["add_path"].(string)
	finalizePath, _ := createResp.body["finalize_path"].(string)
	if mediaID == "" || addPath == "" || finalizePath == "" {
		attempt.Status = AttemptStatusFailed
		attempt.Message = "Snapchat media container response was missing media_id/add_path/finalize_path."
		attempt.Response = redactProviderBody(createResp.body)
		return attempt
	}

	// 2. Multipart-upload the encrypted bytes in ≤32MB chunks, then finalize.
	origin, err := snapchatOrigin(base)
	if err != nil {
		return failedAttempt(attempt, "Snapchat business API base URL is invalid.", err)
	}
	chunks := chunkBytes(encrypted, snapchatChunkSize)
	if len(chunks) > snapchatMaxParts {
		attempt.Status = AttemptStatusFailed
		attempt.Message = fmt.Sprintf("Snapchat media is too large: %d chunks exceeds the %d-part limit.", len(chunks), snapchatMaxParts)
		return attempt
	}
	for i, chunk := range chunks {
		addResp, err := p.snapchatMultipart(ctx, accessToken, origin+addPath, map[string]string{
			"action":      "ADD",
			"part_number": fmt.Sprintf("%d", i+1),
		}, "file", "media.enc", chunk)
		if err != nil {
			return failedAttempt(attempt, "Snapchat media chunk upload failed.", err)
		}
		if !snapchatOK(addResp) {
			return providerErrorAttempt(attempt, "Snapchat rejected a media chunk.", addResp)
		}
	}
	finalizeResp, err := p.snapchatMultipart(ctx, accessToken, origin+finalizePath, map[string]string{"action": "FINALIZE"}, "", "", nil)
	if err != nil {
		return failedAttempt(attempt, "Snapchat media finalize failed.", err)
	}
	if !snapchatOK(finalizeResp) {
		return providerErrorAttempt(attempt, "Snapchat rejected the media finalize.", finalizeResp)
	}

	// 3. Post the uploaded media as a Story (default) or Spotlight (video only).
	if strings.EqualFold(metadataString(account.Metadata, "snapchat_post_type", "snapchatPostType"), "spotlight") && mediaType == "VIDEO" {
		return p.snapchatPostSpotlight(ctx, attempt, post, account, accessToken, base, profileID, mediaID)
	}
	return p.snapchatPostStory(ctx, attempt, accessToken, base, profileID, mediaID)
}

func (p *HTTPPublisher) snapchatPostStory(ctx context.Context, attempt PublishAttempt, accessToken, base, profileID, mediaID string) PublishAttempt {
	req, err := jsonRequest(ctx, http.MethodPost, base+"/public_profiles/"+url.PathEscape(profileID)+"/stories", map[string]any{"media_id": mediaID})
	if err != nil {
		return failedAttempt(attempt, "Could not build Snapchat story request.", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	resp, err := p.doRaw(req)
	if err != nil {
		return failedAttempt(attempt, "Snapchat story post failed.", err)
	}
	if !snapchatOK(resp) {
		return providerErrorAttempt(attempt, "Snapchat rejected the story post.", resp)
	}
	attempt.Status = AttemptStatusSucceeded
	attempt.Message = "Snapchat Story published to the Public Profile."
	attempt.ExternalID = mediaID
	attempt.Response = redactProviderBody(resp.body)
	return attempt
}

func (p *HTTPPublisher) snapchatPostSpotlight(ctx context.Context, attempt PublishAttempt, post Post, account Account, accessToken, base, profileID, mediaID string) PublishAttempt {
	body := map[string]any{
		"media_id": mediaID,
		"locale":   fallback(metadataString(account.Metadata, "snapchat_locale", "snapchatLocale"), "en_US"),
	}
	if desc := previewText(post, "snapchat"); desc != "" {
		body["description"] = desc
	}
	req, err := jsonRequest(ctx, http.MethodPost, base+"/public_profiles/"+url.PathEscape(profileID)+"/spotlights", body)
	if err != nil {
		return failedAttempt(attempt, "Could not build Snapchat spotlight request.", err)
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	resp, err := p.doRaw(req)
	if err != nil {
		return failedAttempt(attempt, "Snapchat spotlight post failed.", err)
	}
	if !snapchatOK(resp) {
		return providerErrorAttempt(attempt, "Snapchat rejected the spotlight post.", resp)
	}
	attempt.Status = AttemptStatusSucceeded
	attempt.Message = "Snapchat Spotlight submitted (Spotlights are reviewed by Snapchat before going LIVE)."
	attempt.ExternalID = mediaID
	attempt.Response = redactProviderBody(resp.body)
	attempt.Warnings = append(attempt.Warnings, "spotlight_pending_snapchat_review")
	return attempt
}

func (p *HTTPPublisher) downloadMedia(ctx context.Context, mediaURL string) ([]byte, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, mediaURL, nil)
	if err != nil {
		return nil, err
	}
	resp, err := p.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("media download returned status %d", resp.StatusCode)
	}
	data, err := io.ReadAll(io.LimitReader(resp.Body, snapchatMaxMediaBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > snapchatMaxMediaBytes {
		return nil, fmt.Errorf("media exceeds the %d byte upload cap", snapchatMaxMediaBytes)
	}
	if len(data) == 0 {
		return nil, fmt.Errorf("media download returned no content")
	}
	return data, nil
}

func (p *HTTPPublisher) snapchatMultipart(ctx context.Context, accessToken, endpoint string, fields map[string]string, fileField, fileName string, fileBytes []byte) (providerResponse, error) {
	var buf bytes.Buffer
	writer := multipart.NewWriter(&buf)
	for key, value := range fields {
		if err := writer.WriteField(key, value); err != nil {
			return providerResponse{}, err
		}
	}
	if fileField != "" {
		part, err := writer.CreateFormFile(fileField, fileName)
		if err != nil {
			return providerResponse{}, err
		}
		if _, err := part.Write(fileBytes); err != nil {
			return providerResponse{}, err
		}
	}
	if err := writer.Close(); err != nil {
		return providerResponse{}, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, &buf)
	if err != nil {
		return providerResponse{}, err
	}
	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("Content-Type", writer.FormDataContentType())
	req.Header.Set("Accept", "application/json")
	return p.doRaw(req)
}

// snapchatOK treats a Public Profile API response as successful only when the
// HTTP status is 2xx AND the in-band request_status (SUCCESS/ERROR/PARTIAL) is
// SUCCESS — Snapchat returns 200 with request_status:ERROR on validation
// failures, so the HTTP status alone is not enough.
func snapchatOK(resp providerResponse) bool {
	if resp.status < 200 || resp.status >= 300 {
		return false
	}
	if status, ok := resp.body["request_status"].(string); ok {
		return strings.EqualFold(status, "SUCCESS")
	}
	return true
}

func snapchatOrigin(base string) (string, error) {
	parsed, err := url.Parse(base)
	if err != nil || parsed.Scheme == "" || parsed.Host == "" {
		return "", fmt.Errorf("invalid Snapchat business API base url %q", base)
	}
	return parsed.Scheme + "://" + parsed.Host, nil
}

// encryptSnapchatMedia AES-256-CBC encrypts media with openssl-compatible
// PKCS#7 padding, matching Snapchat's documented `openssl enc -aes-256-cbc
// -nosalt` upload contract. key must be 32 bytes, iv 16 bytes.
func encryptSnapchatMedia(plaintext, key, iv []byte) ([]byte, error) {
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	if len(iv) != block.BlockSize() {
		return nil, fmt.Errorf("iv must be %d bytes, got %d", block.BlockSize(), len(iv))
	}
	padded := pkcs7Pad(plaintext, block.BlockSize())
	out := make([]byte, len(padded))
	cipher.NewCBCEncrypter(block, iv).CryptBlocks(out, padded)
	return out, nil
}

func pkcs7Pad(data []byte, blockSize int) []byte {
	pad := blockSize - (len(data) % blockSize)
	return append(data, bytes.Repeat([]byte{byte(pad)}, pad)...)
}

func chunkBytes(data []byte, size int) [][]byte {
	if size <= 0 || len(data) <= size {
		return [][]byte{data}
	}
	chunks := make([][]byte, 0, (len(data)+size-1)/size)
	for i := 0; i < len(data); i += size {
		end := i + size
		if end > len(data) {
			end = len(data)
		}
		chunks = append(chunks, data[i:end])
	}
	return chunks
}

type providerResponse struct {
	status int
	body   map[string]any
}

func (p *HTTPPublisher) doJSON(attempt PublishAttempt, req *http.Request, successStatus int, successMessage string) PublishAttempt {
	resp, err := p.doRaw(req)
	if err != nil {
		return failedAttempt(attempt, "Provider publish request failed.", err)
	}
	if resp.status != successStatus && (resp.status < 200 || resp.status >= 300) {
		return providerErrorAttempt(attempt, "Provider publish request was rejected.", resp)
	}
	attempt.Status = AttemptStatusSucceeded
	attempt.Message = successMessage
	attempt.ExternalID = externalID(resp.body)
	attempt.Response = redactProviderBody(resp.body)
	return attempt
}

func (p *HTTPPublisher) doRaw(req *http.Request) (providerResponse, error) {
	resp, err := p.httpClient.Do(req)
	if err != nil {
		return providerResponse{}, err
	}
	defer resp.Body.Close()
	limited := io.LimitReader(resp.Body, 1<<20)
	var body map[string]any
	if err := json.NewDecoder(limited).Decode(&body); err != nil && err != io.EOF {
		body = map[string]any{"decode_error": err.Error()}
	}
	if body == nil {
		body = map[string]any{}
	}
	return providerResponse{status: resp.StatusCode, body: body}, nil
}

func jsonRequest(ctx context.Context, method, endpoint string, payload any) (*http.Request, error) {
	body, err := json.Marshal(payload)
	if err != nil {
		return nil, err
	}
	req, err := http.NewRequestWithContext(ctx, method, endpoint, bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	return req, nil
}

func formRequest(ctx context.Context, method, endpoint string, values url.Values) (*http.Request, error) {
	req, err := http.NewRequestWithContext(ctx, method, endpoint, strings.NewReader(values.Encode()))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Accept", "application/json")
	return req, nil
}

func previewText(post Post, providerKey string) string {
	for _, preview := range post.Previews {
		if normalizePlatform(preview.Platform) == normalizePlatform(providerKey) && strings.TrimSpace(preview.Content) != "" {
			return strings.TrimSpace(preview.Content)
		}
	}
	return strings.TrimSpace(post.Body)
}

func publicMediaURL(post Post) string {
	for _, media := range post.Media {
		if strings.HasPrefix(media.URL, "https://") {
			return strings.TrimSpace(media.URL)
		}
	}
	return ""
}

func mediaKind(post Post) string {
	for _, media := range post.Media {
		switch strings.ToLower(strings.TrimSpace(media.Type)) {
		case "video":
			return "video"
		case "image", "photo":
			return "photo"
		}
	}
	return "photo"
}

func metadataString(metadata map[string]any, keys ...string) string {
	for _, key := range keys {
		if value, ok := metadata[key]; ok {
			if text, ok := value.(string); ok {
				return strings.TrimSpace(text)
			}
			return strings.TrimSpace(fmt.Sprint(value))
		}
	}
	return ""
}

func failedAttempt(attempt PublishAttempt, message string, err error) PublishAttempt {
	attempt.Status = AttemptStatusFailed
	attempt.Message = message
	if err != nil {
		attempt.Response = map[string]any{"error": err.Error()}
	}
	return attempt
}

func providerErrorAttempt(attempt PublishAttempt, message string, resp providerResponse) PublishAttempt {
	attempt.Status = AttemptStatusFailed
	attempt.Message = message
	attempt.Response = redactProviderBody(resp.body)
	attempt.Response["status"] = resp.status
	return attempt
}

func redactProviderBody(body map[string]any) map[string]any {
	out := map[string]any{}
	for key, value := range body {
		lower := strings.ToLower(key)
		if strings.Contains(lower, "token") || strings.Contains(lower, "secret") {
			continue
		}
		out[key] = value
	}
	return out
}

func externalID(body map[string]any) string {
	if data, ok := body["data"].(map[string]any); ok {
		if id, ok := data["id"].(string); ok {
			return id
		}
	}
	if id, ok := body["id"].(string); ok {
		return id
	}
	return ""
}

func accountHasCapability(account Account, capability string) bool {
	for _, item := range account.Capabilities {
		if strings.TrimSpace(item) == capability {
			return true
		}
	}
	return false
}
