package store

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"
)

func TestMemoryRepositoryStoresAndDeletesConnections(t *testing.T) {
	repo := NewMemoryRepository()
	ctx := context.Background()
	connection := Connection{
		ID:                   "conn-1",
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		Status:               "active",
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	}
	if _, err := repo.UpsertConnection(ctx, connection); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	got, err := repo.FindActiveConnection(ctx, "org-1", "microsoft-graph")
	if err != nil {
		t.Fatalf("FindActiveConnection error: %v", err)
	}
	if got.ID != "conn-1" {
		t.Fatalf("connection ID = %q, want conn-1", got.ID)
	}
	if _, err := repo.MarkConnectionDeleted(ctx, "conn-1"); err != nil {
		t.Fatalf("MarkConnectionDeleted error: %v", err)
	}
	if _, err := repo.FindActiveConnection(ctx, "org-1", "microsoft-graph"); err != ErrNotFound {
		t.Fatalf("FindActiveConnection error = %v, want ErrNotFound", err)
	}
}

func TestMemoryActionReceiptLifecycleAndCompletedReplay(t *testing.T) {
	repo := NewMemoryRepository()
	ctx := context.Background()
	want := actionReceiptFixture()

	claimed, acquired, err := repo.ClaimActionReceipt(ctx, want)
	if err != nil {
		t.Fatalf("ClaimActionReceipt error: %v", err)
	}
	if !acquired {
		t.Fatal("ClaimActionReceipt acquired = false, want true")
	}
	if claimed.Status != "pending" {
		t.Fatalf("claimed status = %q, want pending", claimed.Status)
	}
	if claimed.CreatedAt.IsZero() || claimed.UpdatedAt.IsZero() {
		t.Fatal("claimed receipt timestamps must be populated")
	}

	different := want
	different.RequestSHA256 = "different-fingerprint"
	if _, _, err := repo.ClaimActionReceipt(ctx, different); !errors.Is(err, ErrConflict) {
		t.Fatalf("changed binding ClaimActionReceipt error = %v, want ErrConflict", err)
	}

	replayed, acquired, err := repo.ClaimActionReceipt(ctx, want)
	if err != nil || acquired || replayed.Status != "pending" {
		t.Fatalf("exact pending replay = (%#v, acquired=%v, err=%v), want existing pending", replayed, acquired, err)
	}

	executing, err := repo.BeginActionReceiptExecution(ctx, want.OrganizationID, want.IdempotencyKey)
	if err != nil || executing.Status != "executing" {
		t.Fatalf("BeginActionReceiptExecution = (%#v, %v), want executing", executing, err)
	}

	completed, err := repo.CompleteActionReceipt(ctx, want.OrganizationID, want.IdempotencyKey, " provider-message-1 ")
	if err != nil {
		t.Fatalf("CompleteActionReceipt error: %v", err)
	}
	if completed.Status != "completed" || completed.ProviderMessageID != "provider-message-1" {
		t.Fatalf("completed receipt = %#v, want completed with trimmed provider ID", completed)
	}

	replayed, acquired, err = repo.ClaimActionReceipt(ctx, want)
	if err != nil {
		t.Fatalf("completed replay ClaimActionReceipt error: %v", err)
	}
	if acquired || replayed.Status != "completed" || replayed.ProviderMessageID != "provider-message-1" {
		t.Fatalf("completed replay = (%#v, acquired=%v), want stored completed receipt", replayed, acquired)
	}
	if _, err := repo.CompleteActionReceipt(ctx, want.OrganizationID, want.IdempotencyKey, "provider-message-1"); !errors.Is(err, ErrConflict) {
		t.Fatalf("second CompleteActionReceipt error = %v, want ErrConflict", err)
	}
	if err := repo.MarkActionReceiptUnknown(ctx, want.OrganizationID, want.IdempotencyKey); !errors.Is(err, ErrConflict) {
		t.Fatalf("MarkActionReceiptUnknown(completed) error = %v, want ErrConflict", err)
	}
}

func TestMemoryActionReceiptUnknownBlocksBlindRetry(t *testing.T) {
	repo := NewMemoryRepository()
	ctx := context.Background()
	want := actionReceiptFixture()
	if _, _, err := repo.ClaimActionReceipt(ctx, want); err != nil {
		t.Fatalf("ClaimActionReceipt error: %v", err)
	}
	if _, err := repo.BeginActionReceiptExecution(ctx, want.OrganizationID, want.IdempotencyKey); err != nil {
		t.Fatalf("BeginActionReceiptExecution error: %v", err)
	}
	if err := repo.MarkActionReceiptUnknown(ctx, want.OrganizationID, want.IdempotencyKey); err != nil {
		t.Fatalf("MarkActionReceiptUnknown error: %v", err)
	}

	replayed, acquired, err := repo.ClaimActionReceipt(ctx, want)
	if err != nil {
		t.Fatalf("unknown replay ClaimActionReceipt error: %v", err)
	}
	if acquired || replayed.Status != "unknown" {
		t.Fatalf("unknown replay = (%#v, acquired=%v), want unknown and not acquired", replayed, acquired)
	}
	if _, err := repo.CompleteActionReceipt(ctx, want.OrganizationID, want.IdempotencyKey, "provider-message-1"); !errors.Is(err, ErrConflict) {
		t.Fatalf("CompleteActionReceipt(unknown) error = %v, want ErrConflict", err)
	}
	if err := repo.MarkActionReceiptUnknown(ctx, want.OrganizationID, want.IdempotencyKey); !errors.Is(err, ErrConflict) {
		t.Fatalf("second MarkActionReceiptUnknown error = %v, want ErrConflict", err)
	}
}

func TestMemoryActionReceiptMissingTransitionsReturnNotFound(t *testing.T) {
	repo := NewMemoryRepository()
	ctx := context.Background()
	if _, err := repo.CompleteActionReceipt(ctx, "org-1", "missing-key", "provider-message-1"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("CompleteActionReceipt(missing) error = %v, want ErrNotFound", err)
	}
	if err := repo.MarkActionReceiptUnknown(ctx, "org-1", "missing-key"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("MarkActionReceiptUnknown(missing) error = %v, want ErrNotFound", err)
	}
}

func TestMemoryActionReceiptClaimIsAtomicAndTenantScoped(t *testing.T) {
	repo := NewMemoryRepository()
	ctx := context.Background()
	want := actionReceiptFixture()
	type claimResult struct {
		receipt  ActionReceipt
		acquired bool
		err      error
	}
	results := make(chan claimResult, 32)
	var workers sync.WaitGroup
	for range 32 {
		workers.Add(1)
		go func() {
			defer workers.Done()
			receipt, acquired, err := repo.ClaimActionReceipt(ctx, want)
			results <- claimResult{receipt: receipt, acquired: acquired, err: err}
		}()
	}
	workers.Wait()
	close(results)

	acquiredCount := 0
	for result := range results {
		if result.err != nil {
			t.Fatalf("ClaimActionReceipt error: %v", result.err)
		}
		if result.receipt.Status != "pending" || result.receipt.RequestSHA256 != want.RequestSHA256 {
			t.Fatalf("ClaimActionReceipt receipt = %#v, want original pending receipt", result.receipt)
		}
		if result.acquired {
			acquiredCount++
		}
	}
	if acquiredCount != 1 {
		t.Fatalf("acquired claims = %d, want exactly 1", acquiredCount)
	}

	otherTenant := want
	otherTenant.OrganizationID = "org-2"
	_, acquired, err := repo.ClaimActionReceipt(ctx, otherTenant)
	if err != nil {
		t.Fatalf("other-tenant ClaimActionReceipt error: %v", err)
	}
	if !acquired {
		t.Fatal("same idempotency key in another tenant was not independently acquired")
	}
}

func TestMemoryActionReceiptAuthorizationAllowsFreshSignerMetadataButNotSecondEffectBinding(t *testing.T) {
	repo := NewMemoryRepository()
	want := actionReceiptFixture()
	if _, _, err := repo.ClaimActionReceipt(t.Context(), want); err != nil {
		t.Fatalf("ClaimActionReceipt error: %v", err)
	}
	secondKey := want
	secondKey.IdempotencyKey = "conversation:org-1:reply-2"
	if _, _, err := repo.ClaimActionReceipt(t.Context(), secondKey); !errors.Is(err, ErrConflict) {
		t.Fatalf("same authorization with second key error = %v, want ErrConflict", err)
	}
	rotatedAttestation := want
	rotatedAttestation.AttestationKeyID = "conversation-write-rotated"
	rotatedAttestation.AttestationJTI = "fresh-attestation-jti"
	replayed, acquired, err := repo.ClaimActionReceipt(t.Context(), rotatedAttestation)
	if err != nil || acquired || replayed.AttestationKeyID != want.AttestationKeyID || replayed.AttestationJTI != want.AttestationJTI {
		t.Fatalf("fresh attestation replay = (%#v, acquired=%v, err=%v), want first-seen audit metadata", replayed, acquired, err)
	}

	changes := []struct {
		name   string
		update func(*ActionReceipt)
	}{
		{name: "authorization kind", update: func(r *ActionReceipt) { r.AuthorizationKind = "human_approved_ai_action" }},
		{name: "authorization id", update: func(r *ActionReceipt) { r.AuthorizationID = "different-authorization" }},
		{name: "action id", update: func(r *ActionReceipt) { r.ActionID = "different-action" }},
		{name: "actor id", update: func(r *ActionReceipt) { r.ActorID = "different-actor" }},
		{name: "payload", update: func(r *ActionReceipt) { r.PayloadSHA256 = "different-payload" }},
	}
	for _, change := range changes {
		t.Run(change.name, func(t *testing.T) {
			changed := want
			change.update(&changed)
			if _, _, err := repo.ClaimActionReceipt(t.Context(), changed); !errors.Is(err, ErrConflict) {
				t.Fatalf("changed binding error = %v, want ErrConflict", err)
			}
		})
	}
}

func TestMemoryPendingReceiptCanRetryBeforeProviderButExecutingCannot(t *testing.T) {
	repo := NewMemoryRepository()
	want := actionReceiptFixture()
	if _, _, err := repo.ClaimActionReceipt(t.Context(), want); err != nil {
		t.Fatalf("ClaimActionReceipt error: %v", err)
	}
	if retry, acquired, err := repo.ClaimActionReceipt(t.Context(), want); err != nil || acquired || retry.Status != "pending" {
		t.Fatalf("pending retry = (%#v, %v, %v), want existing pending", retry, acquired, err)
	}
	if _, err := repo.BeginActionReceiptExecution(t.Context(), want.OrganizationID, want.IdempotencyKey); err != nil {
		t.Fatalf("BeginActionReceiptExecution error: %v", err)
	}
	if _, err := repo.BeginActionReceiptExecution(t.Context(), want.OrganizationID, want.IdempotencyKey); !errors.Is(err, ErrConflict) {
		t.Fatalf("second BeginActionReceiptExecution error = %v, want ErrConflict", err)
	}
	if replay, acquired, err := repo.ClaimActionReceipt(t.Context(), want); err != nil || acquired || replay.Status != "executing" {
		t.Fatalf("executing retry = (%#v, %v, %v), want blocked existing execution", replay, acquired, err)
	}
}

func actionReceiptFixture() ActionReceipt {
	return ActionReceipt{
		OrganizationID:    "org-1",
		IdempotencyKey:    "conversation:org-1:reply-1",
		RequestSHA256:     "f00dbabe",
		ConnectionID:      "conn-1",
		ProviderKey:       "microsoft",
		Operation:         "mail.send",
		AttestationIssuer: "conversation-core",
		AttestationKeyID:  "conversation-write-2026-07",
		AuthorizationKind: "human_intent",
		AuthorizationID:   "human-intent-1",
		ActionID:          "human-intent-1",
		ActorID:           "user-1",
		AttestationJTI:    "attestation-1",
		PayloadSHA256:     "f00dbabe",
	}
}
