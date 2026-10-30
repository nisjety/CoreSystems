package social

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// GovernedPublisher sends provider writes only through integration-corev2's
// action endpoint. It deliberately has no TokenBroker: social-core must never
// receive a provider OAuth token merely to publish a post.
//
// Providers without a matching action contract fail closed. That is preferable
// to silently restoring the old raw-token HTTP path while their operation is
// being added to integration-corev2.
type GovernedPublisher struct {
	actionExecutor ActionExecutor
	now            func() time.Time
}

func NewGovernedPublisher(actionExecutor ActionExecutor) *GovernedPublisher {
	return &GovernedPublisher{
		actionExecutor: actionExecutor,
		now:            func() time.Time { return time.Now().UTC() },
	}
}

func (p *GovernedPublisher) Publish(ctx context.Context, job PublishJob, post Post, account Account) PublishAttempt {
	attempt := PublishAttempt{
		OrgID:       job.OrgID,
		JobID:       job.ID,
		PostID:      post.ID,
		ProviderKey: normalizePlatform(account.ProviderKey),
		Status:      AttemptStatusBlocked,
		Mode:        "api",
		Warnings:    []string{},
		Response:    map[string]any{},
		AttemptedAt: time.Now().UTC(),
	}
	if p != nil && p.now != nil {
		attempt.AttemptedAt = p.now()
	}
	if p == nil || p.actionExecutor == nil {
		attempt.Message = "Governed integration action execution is not configured."
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

	switch attempt.ProviderKey {
	case "linkedin":
		return p.publishLinkedIn(ctx, attempt, post, account)
	case "facebook":
		return p.publishFacebook(ctx, attempt, post, account)
	case "instagram":
		return p.publishInstagram(ctx, attempt, post, account)
	case "meta":
		if metadataString(account.Metadata, "instagram_user_id", "instagramUserId", "ig_user_id") != "" {
			return p.publishInstagram(ctx, attempt, post, account)
		}
		return p.publishFacebook(ctx, attempt, post, account)
	case "whatsapp":
		attempt.Message = "WhatsApp is a messaging surface — send template/session messages via Inbox workflows, not organic publishing."
		return attempt
	case "meta-ads":
		attempt.Message = "Meta Ads connections manage campaigns, not organic posts; use ads workflows."
		return attempt
	default:
		attempt.Message = "This provider has no governed integration action contract for organic publishing."
		return attempt
	}
}

func (p *GovernedPublisher) publishLinkedIn(ctx context.Context, attempt PublishAttempt, post Post, account Account) PublishAttempt {
	authorURN := metadataString(account.Metadata, "author_urn", "authorUrn", "linkedin_author_urn")
	if authorURN == "" && strings.HasPrefix(account.Handle, "urn:li:") {
		authorURN = account.Handle
	}
	if authorURN == "" {
		authorURN = metadataString(account.Metadata, "provider_account_urn", "providerAccountUrn")
	}
	if authorURN == "" {
		attempt.Message = "LinkedIn publishing requires an author URN in integration provider context."
		return attempt
	}
	return p.execute(ctx, attempt, ActionRequest{
		ConnectionID: account.ConnectionID,
		Operation:    "linkedin.posts.create",
		Body: map[string]any{
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
		},
	}, "integration-core LinkedIn publish")
}

func (p *GovernedPublisher) publishFacebook(ctx context.Context, attempt PublishAttempt, post Post, account Account) PublishAttempt {
	pageID := firstNonEmpty(metadataString(account.Metadata, "page_id", "pageId", "facebook_page_id"), account.Handle)
	if pageID == "" {
		attempt.Message = "Facebook publishing requires a Page id in integration provider context."
		return attempt
	}
	if mediaURL := publicMediaURL(post); mediaURL != "" && mediaKind(post) != "video" {
		return p.execute(ctx, attempt, ActionRequest{
			ConnectionID: account.ConnectionID,
			Operation:    "facebook.page.photo",
			Params:       map[string]any{"pageId": pageID},
			Body:         map[string]any{"url": mediaURL, "caption": previewText(post, "facebook")},
		}, "integration-core Facebook photo publish")
	}
	return p.execute(ctx, attempt, ActionRequest{
		ConnectionID: account.ConnectionID,
		Operation:    "facebook.page.post",
		Params:       map[string]any{"pageId": pageID},
		Body:         map[string]any{"message": previewText(post, "facebook")},
	}, "integration-core Facebook publish")
}

func (p *GovernedPublisher) publishInstagram(ctx context.Context, attempt PublishAttempt, post Post, account Account) PublishAttempt {
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
	body := map[string]any{"caption": previewText(post, "instagram")}
	if mediaKind(post) == "video" {
		body["media_type"] = "REELS"
		body["video_url"] = mediaURL
	} else {
		body["image_url"] = mediaURL
	}
	created, err := p.actionExecutor.ExecuteAction(ctx, ActionRequest{
		ConnectionID: account.ConnectionID,
		Operation:    "instagram.media.create",
		Params:       map[string]any{"igUserId": igUserID},
		Body:         body,
	})
	if err != nil {
		return governedFailure(attempt, "Instagram media container was not accepted by the governed action service.")
	}
	creation, ok := governedResultObject(created)
	if !ok || strings.TrimSpace(externalID(creation)) == "" {
		return governedFailure(attempt, "Instagram media container response did not include a creation id.")
	}
	creationID := externalID(creation)
	if mediaKind(post) == "video" {
		if err := p.waitForInstagramMediaReady(ctx, account.ConnectionID, creationID); err != nil {
			return governedFailure(attempt, "Instagram media container did not finish processing.")
		}
	}
	return p.execute(ctx, attempt, ActionRequest{
		ConnectionID: account.ConnectionID,
		Operation:    "instagram.media.publish",
		Params:       map[string]any{"igUserId": igUserID},
		Body:         map[string]any{"creation_id": creationID},
	}, "integration-core Instagram publish")
}

func (p *GovernedPublisher) waitForInstagramMediaReady(ctx context.Context, connectionID, creationID string) error {
	const (
		pollInterval = 3 * time.Second
		maxPolls     = 40
	)
	for range maxPolls {
		result, err := p.actionExecutor.ExecuteAction(ctx, ActionRequest{
			ConnectionID: connectionID,
			Operation:    "instagram.media.status",
			Params:       map[string]any{"creationId": creationID},
		})
		if err != nil {
			return err
		}
		body, ok := governedResultObject(result)
		if !ok {
			return fmt.Errorf("invalid governed Instagram status result")
		}
		switch strings.ToUpper(metadataString(body, "status_code")) {
		case "FINISHED":
			return nil
		case "ERROR", "EXPIRED":
			return fmt.Errorf("Instagram media processing failed")
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(pollInterval):
		}
	}
	return fmt.Errorf("Instagram media container did not finish processing")
}

func (p *GovernedPublisher) execute(ctx context.Context, attempt PublishAttempt, request ActionRequest, successMessage string) PublishAttempt {
	attempt.Endpoint = "integration-core actions/execute (" + request.Operation + ")"
	result, err := p.actionExecutor.ExecuteAction(ctx, request)
	if err != nil {
		return governedFailure(attempt, "The governed provider action was not accepted.")
	}
	body, ok := governedResultObject(result)
	if !ok {
		return governedFailure(attempt, "The governed provider action returned an invalid result.")
	}
	attempt.Status = AttemptStatusSucceeded
	attempt.Message = successMessage
	attempt.ExternalID = externalID(body)
	attempt.Response = redactProviderBody(body)
	return attempt
}

func governedFailure(attempt PublishAttempt, message string) PublishAttempt {
	attempt.Status = AttemptStatusFailed
	attempt.Message = message
	attempt.Response = map[string]any{"error": "governed_action_failed"}
	return attempt
}

func governedResultObject(result *ActionResult) (map[string]any, bool) {
	if result == nil || len(result.Result) == 0 {
		return nil, false
	}
	var body map[string]any
	if err := json.Unmarshal(result.Result, &body); err != nil || body == nil {
		return nil, false
	}
	return body, true
}
