package conversation

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// fakeTx implements pgx.Tx for transactional unit tests. Only Exec/Commit/Rollback
// are exercised by ReviewAIAction; the remaining pgx.Tx methods are inherited from
// the embedded (nil) interface and panic if ever called — proving they are not.
type fakeTx struct {
	pgx.Tx
	execResults []pgconn.CommandTag
	execErrors  []error
	execSQL     []string
	execCalls   int
	queryRows   []pgx.Row
	querySQL    []string
	queryCalls  int
	committed   bool
	rolledBack  bool
}

func (t *fakeTx) Exec(_ context.Context, sql string, _ ...any) (pgconn.CommandTag, error) {
	i := t.execCalls
	t.execCalls++
	t.execSQL = append(t.execSQL, sql)
	var tag pgconn.CommandTag
	if i < len(t.execResults) {
		tag = t.execResults[i]
	}
	if i < len(t.execErrors) && t.execErrors[i] != nil {
		return tag, t.execErrors[i]
	}
	return tag, nil
}

func (t *fakeTx) Commit(_ context.Context) error   { t.committed = true; return nil }
func (t *fakeTx) Rollback(_ context.Context) error { t.rolledBack = true; return nil }

func (t *fakeTx) QueryRow(_ context.Context, sql string, _ ...any) pgx.Row {
	i := t.queryCalls
	t.queryCalls++
	t.querySQL = append(t.querySQL, sql)
	if i < len(t.queryRows) {
		return t.queryRows[i]
	}
	return fakeRow{err: pgx.ErrNoRows}
}

type fakeRow struct {
	value  string
	values []any
	err    error
}

func (r fakeRow) Scan(dest ...any) error {
	if r.err != nil {
		return r.err
	}
	values := r.values
	if len(values) == 0 {
		values = []any{r.value}
	}
	if len(values) != len(dest) {
		return fmt.Errorf("fake row has %d values for %d destinations", len(values), len(dest))
	}
	for index := range dest {
		target := reflect.ValueOf(dest[index])
		if target.Kind() != reflect.Pointer || target.IsNil() {
			return fmt.Errorf("destination %d is not a pointer", index)
		}
		source := reflect.ValueOf(values[index])
		if !source.IsValid() || !source.Type().AssignableTo(target.Elem().Type()) {
			return fmt.Errorf("value %d type %T is not assignable to %s", index, values[index], target.Elem().Type())
		}
		target.Elem().Set(source)
	}
	return nil
}

// fakePool implements PgxPool. Only Begin is exercised by ReviewAIAction; the
// other methods panic to prove they are not reached on this path.
type fakePool struct {
	tx         *fakeTx
	beginErr   error
	queryRows  []pgx.Row
	queryCalls int
}

func (p *fakePool) Begin(_ context.Context) (pgx.Tx, error) {
	if p.beginErr != nil {
		return nil, p.beginErr
	}
	return p.tx, nil
}
func (p *fakePool) Query(context.Context, string, ...any) (pgx.Rows, error) { panic("unused") }
func (p *fakePool) QueryRow(context.Context, string, ...any) pgx.Row {
	i := p.queryCalls
	p.queryCalls++
	if i < len(p.queryRows) {
		return p.queryRows[i]
	}
	return fakeRow{err: pgx.ErrNoRows}
}
func (p *fakePool) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	panic("unused")
}

// A review for an action that does not exist for the org (missing or foreign-org
// id) must return ErrNotFound (→404) and must NOT insert a conversation_ai_reviews
// row — the UPDATE matched zero rows, so the transaction is rolled back before the
// INSERT ever runs.
func TestReviewAIActionReturnsNotFoundWithoutPhantomReview(t *testing.T) {
	tx := &fakeTx{execResults: []pgconn.CommandTag{pgconn.NewCommandTag("UPDATE 0")}}
	repo := &PGRepository{pool: &fakePool{tx: tx}}

	err := repo.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:      "org_1",
		AIActionID: "missing",
		ReviewerID: "user_1",
		Decision:   "approved",
		OccurredAt: time.Date(2026, time.June, 19, 8, 0, 0, 0, time.UTC),
	})

	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("error = %v, want ErrNotFound", err)
	}
	if tx.execCalls != 1 {
		t.Fatalf("Exec called %d times, want 1 (UPDATE only; the INSERT must be skipped so no phantom review row is written)", tx.execCalls)
	}
	if tx.committed {
		t.Fatal("transaction committed; want rollback for a non-existent action")
	}
	if !tx.rolledBack {
		t.Fatal("transaction not rolled back for a non-existent action")
	}
}

// Once an action has left suggested state, a second human decision must fail
// closed. This prevents a rejected or already executed action from being
// re-approved and published again through a retry or alternate endpoint.
func TestReviewAIActionReturnsConflictForTerminalAction(t *testing.T) {
	tx := &fakeTx{
		execResults: []pgconn.CommandTag{pgconn.NewCommandTag("UPDATE 0")},
		queryRows:   []pgx.Row{fakeRow{value: "executed"}},
	}
	repo := &PGRepository{pool: &fakePool{tx: tx}}

	err := repo.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:      "org_1",
		AIActionID: "aiact_1",
		ReviewerID: "user_1",
		Decision:   "approved",
		OccurredAt: time.Date(2026, time.July, 13, 8, 0, 0, 0, time.UTC),
	})

	if !errors.Is(err, ErrConflict) {
		t.Fatalf("error = %v, want ErrConflict", err)
	}
	if tx.execCalls != 1 {
		t.Fatalf("Exec called %d times, want UPDATE only", tx.execCalls)
	}
	if tx.queryCalls != 1 {
		t.Fatalf("QueryRow called %d times, want terminal-state lookup", tx.queryCalls)
	}
	if !strings.Contains(tx.execSQL[0], "status = 'suggested'") {
		t.Fatalf("UPDATE is not a compare-and-set from suggested state:\n%s", tx.execSQL[0])
	}
	if tx.committed || !tx.rolledBack {
		t.Fatalf("terminal review transaction state = committed:%v rolledBack:%v, want rollback", tx.committed, tx.rolledBack)
	}
}

// A review for an existing action runs the UPDATE then the review-row INSERT and
// commits.
func TestReviewAIActionCommitsWhenActionExists(t *testing.T) {
	tx := &fakeTx{execResults: []pgconn.CommandTag{
		pgconn.NewCommandTag("UPDATE 1"),
		pgconn.NewCommandTag("INSERT 0 1"),
	}}
	repo := &PGRepository{pool: &fakePool{tx: tx}}

	err := repo.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:      "org_1",
		AIActionID: "aiact_1",
		ReviewerID: "user_1",
		Decision:   "approved",
		OccurredAt: time.Date(2026, time.June, 19, 8, 0, 0, 0, time.UTC),
	})

	if err != nil {
		t.Fatalf("error = %v, want nil", err)
	}
	if tx.execCalls != 2 {
		t.Fatalf("Exec called %d times, want 2 (UPDATE action + INSERT review row)", tx.execCalls)
	}
	if !tx.committed {
		t.Fatal("transaction not committed for an existing action")
	}
}

func outboundIntentRow(status, fingerprint, messageID string) pgx.Row {
	now := time.Date(2026, time.July, 13, 12, 0, 0, 0, time.UTC)
	return fakeRow{values: []any{
		"outintent_1", "org_1", "human-reply-0001", "conv_1", "",
		fingerprint, status, "whatsapp", "conn_1", "recipient_1",
		"human_intent", "user_1", "", "outintent_1", "whatsapp.messages.send", strings.Repeat("a", 64),
		"wamid.1", messageID, "", now, now,
	}}
}

func boundOutboundIntentRow(status, fingerprint, messageID string) pgx.Row {
	return outboundIntentRow(status, fingerprint, messageID)
}

func outboundIntentValueRow(intent OutboundIntent) pgx.Row {
	return fakeRow{values: []any{
		intent.ID, intent.OrgID, intent.IdempotencyKey, intent.ConversationID, intent.AIActionID,
		intent.RequestFingerprint, intent.Status, intent.Provider, intent.ConnectionID, intent.ProviderThreadID,
		intent.AuthorizationKind, intent.ActorUserID, intent.ApprovalID, intent.ActionID, intent.Operation, intent.PayloadSHA256,
		intent.ProviderMessageID, intent.MessageID, intent.ErrorCode, intent.CreatedAt, intent.UpdatedAt,
	}}
}

func boundHumanClaimInput() OutboundIntentClaimInput {
	return OutboundIntentClaimInput{
		IntentID: "outintent_1", OrgID: "org_1", IdempotencyKey: "human-reply-0001", ConversationID: "conv_1",
		RequestFingerprint: "fingerprint-1", Provider: "whatsapp", ConnectionID: "conn_1", ProviderThreadID: "recipient_1",
		AuthorizationKind: "human_intent", ActorUserID: "user_1", ActionID: "outintent_1",
		Operation: "whatsapp.messages.send", PayloadSHA256: strings.Repeat("a", 64),
	}
}

func outboundAIIntentRow(status, fingerprint, messageID string) pgx.Row {
	row := outboundIntentRow(status, fingerprint, messageID).(fakeRow)
	row.values[2] = "conversation-ai:act_1"
	row.values[4] = "act_1"
	row.values[10] = "human_approved_ai_action"
	row.values[11] = "reviewer_1"
	row.values[12] = "act_1"
	row.values[13] = "act_1"
	return row
}

func messageRow(messageID string) pgx.Row {
	now := time.Date(2026, time.July, 13, 12, 1, 0, 0, time.UTC)
	return fakeRow{values: []any{
		messageID, "org_1", "conv_1", DirectionOutbound, "agent", "Ada", "",
		"Hello", "", false, "whatsapp", "wamid.1", "", now, now,
	}}
}

func TestClaimOutboundIntentClaimsFirstAndReplaysExactFingerprint(t *testing.T) {
	input := boundHumanClaimInput()

	firstPool := &fakePool{queryRows: []pgx.Row{outboundIntentRow(OutboundIntentSending, "fingerprint-1", "")}}
	first, err := (&PGRepository{pool: firstPool}).ClaimOutboundIntent(t.Context(), input)
	if err != nil || !first.Claimed || first.Intent.Status != OutboundIntentSending {
		t.Fatalf("first claim = %#v/%v", first, err)
	}

	replayPool := &fakePool{queryRows: []pgx.Row{
		fakeRow{err: pgx.ErrNoRows},
		outboundIntentRow(OutboundIntentSubmitted, "fingerprint-1", "msg_1"),
	}}
	replay, err := (&PGRepository{pool: replayPool}).ClaimOutboundIntent(t.Context(), input)
	if err != nil || replay.Claimed || replay.Intent.MessageID != "msg_1" {
		t.Fatalf("replay claim = %#v/%v", replay, err)
	}
}

func TestClaimOutboundIntentRejectsSameKeyDifferentFingerprint(t *testing.T) {
	pool := &fakePool{queryRows: []pgx.Row{
		fakeRow{err: pgx.ErrNoRows},
		outboundIntentRow(OutboundIntentSubmitted, "original-fingerprint", "msg_1"),
	}}
	input := boundHumanClaimInput()
	input.RequestFingerprint = "changed-fingerprint"
	_, err := (&PGRepository{pool: pool}).ClaimOutboundIntent(t.Context(), input)
	if !errors.Is(err, ErrConflict) {
		t.Fatalf("error = %v, want ErrConflict", err)
	}
}

func TestClaimOutboundIntentRejectsAlteredAuthorizationBindingAndLegacyRows(t *testing.T) {
	input := boundHumanClaimInput()
	for name, mutate := range map[string]func(*OutboundIntent){
		"actor":              func(i *OutboundIntent) { i.ActorUserID = "other-user" },
		"operation":          func(i *OutboundIntent) { i.Operation = "other.send" },
		"payload":            func(i *OutboundIntent) { i.PayloadSHA256 = strings.Repeat("b", 64) },
		"authorization kind": func(i *OutboundIntent) { i.AuthorizationKind = "" },
		"legacy empty binding": func(i *OutboundIntent) {
			i.AuthorizationKind, i.ActorUserID, i.ActionID, i.Operation, i.PayloadSHA256 = "", "", "", "", ""
		},
	} {
		t.Run(name, func(t *testing.T) {
			row := boundOutboundIntentRow(OutboundIntentSubmitted, input.RequestFingerprint, "msg_1")
			intent := row.(fakeRow)
			stored, err := scanOutboundIntent(intent)
			if err != nil {
				t.Fatal(err)
			}
			mutate(&stored)
			pool := &fakePool{queryRows: []pgx.Row{fakeRow{err: pgx.ErrNoRows}, outboundIntentValueRow(stored)}}
			_, err = (&PGRepository{pool: pool}).ClaimOutboundIntent(t.Context(), input)
			if !errors.Is(err, ErrConflict) {
				t.Fatalf("error = %v, want ErrConflict", err)
			}
		})
	}
}

func TestClaimOutboundIntentReclaimsOnlyExactRetryableBinding(t *testing.T) {
	input := boundHumanClaimInput()
	pool := &fakePool{queryRows: []pgx.Row{
		fakeRow{err: pgx.ErrNoRows},
		boundOutboundIntentRow(OutboundIntentRetryable, input.RequestFingerprint, ""),
		boundOutboundIntentRow(OutboundIntentSending, input.RequestFingerprint, ""),
	}}
	claim, err := (&PGRepository{pool: pool}).ClaimOutboundIntent(t.Context(), input)
	if err != nil || !claim.Claimed || claim.Intent.Status != OutboundIntentSending {
		t.Fatalf("retryable claim = %#v, %v", claim, err)
	}
}

func TestValidateOutboundIntentClaimRejectsIncompleteAndCrossKindBindings(t *testing.T) {
	validHuman := boundHumanClaimInput()
	for _, missing := range []string{
		"intent_id", "org_id", "idempotency_key", "conversation_id", "request_fingerprint",
		"provider", "connection_id", "actor_user_id", "action_id", "operation", "payload_sha256",
	} {
		t.Run("missing "+missing, func(t *testing.T) {
			input := validHuman
			switch missing {
			case "intent_id":
				input.IntentID = ""
			case "org_id":
				input.OrgID = ""
			case "idempotency_key":
				input.IdempotencyKey = ""
			case "conversation_id":
				input.ConversationID = ""
			case "request_fingerprint":
				input.RequestFingerprint = ""
			case "provider":
				input.Provider = ""
			case "connection_id":
				input.ConnectionID = ""
			case "actor_user_id":
				input.ActorUserID = ""
			case "action_id":
				input.ActionID = ""
			case "operation":
				input.Operation = ""
			case "payload_sha256":
				input.PayloadSHA256 = ""
			}
			if err := validateOutboundIntentClaim(input); !errors.Is(err, ErrInvalidInput) {
				t.Fatalf("error = %v", err)
			}
		})
	}
	for name, mutate := range map[string]func(*OutboundIntentClaimInput){
		"short digest":           func(i *OutboundIntentClaimInput) { i.PayloadSHA256 = "abc" },
		"manual approval":        func(i *OutboundIntentClaimInput) { i.ApprovalID = "approval" },
		"manual ai id":           func(i *OutboundIntentClaimInput) { i.AIActionID = "action" },
		"manual action mismatch": func(i *OutboundIntentClaimInput) { i.ActionID = "other" },
		"unknown kind":           func(i *OutboundIntentClaimInput) { i.AuthorizationKind = "system" },
	} {
		t.Run(name, func(t *testing.T) {
			input := validHuman
			mutate(&input)
			if err := validateOutboundIntentClaim(input); !errors.Is(err, ErrInvalidInput) {
				t.Fatalf("error = %v", err)
			}
		})
	}
	validAI := validHuman
	validAI.AuthorizationKind = "human_approved_ai_action"
	validAI.ApprovalID, validAI.ActionID, validAI.AIActionID = "action-1", "action-1", "action-1"
	if err := validateOutboundIntentClaim(validAI); err != nil {
		t.Fatalf("valid AI binding = %v", err)
	}
	for name, mutate := range map[string]func(*OutboundIntentClaimInput){
		"missing approval":   func(i *OutboundIntentClaimInput) { i.ApprovalID = "" },
		"approval mismatch":  func(i *OutboundIntentClaimInput) { i.ApprovalID = "other" },
		"AI action mismatch": func(i *OutboundIntentClaimInput) { i.AIActionID = "other" },
	} {
		t.Run("ai "+name, func(t *testing.T) {
			input := validAI
			mutate(&input)
			if err := validateOutboundIntentClaim(input); !errors.Is(err, ErrInvalidInput) {
				t.Fatalf("error = %v", err)
			}
		})
	}
}

func TestClaimOutboundIntentHandlesInsertAndReplayLookupFailures(t *testing.T) {
	input := boundHumanClaimInput()
	for name, rows := range map[string][]pgx.Row{
		"insert failure":    {fakeRow{err: errors.New("insert unavailable")}},
		"vanished conflict": {fakeRow{err: pgx.ErrNoRows}, fakeRow{err: pgx.ErrNoRows}},
		"lookup failure":    {fakeRow{err: pgx.ErrNoRows}, fakeRow{err: errors.New("lookup unavailable")}},
	} {
		t.Run(name, func(t *testing.T) {
			_, err := (&PGRepository{pool: &fakePool{queryRows: rows}}).ClaimOutboundIntent(t.Context(), input)
			if err == nil {
				t.Fatal("error = nil")
			}
		})
	}
}

func TestFinalizeOutboundIntentCommitsMessageAuditAndSubmittedStateTogether(t *testing.T) {
	tx := &fakeTx{
		queryRows: []pgx.Row{
			outboundIntentRow(OutboundIntentSending, "fingerprint-1", ""),
			messageRow("msg_1"),
		},
		execResults: []pgconn.CommandTag{
			pgconn.NewCommandTag("UPDATE 1"),
			pgconn.NewCommandTag("INSERT 0 1"),
			pgconn.NewCommandTag("UPDATE 1"),
		},
	}
	repo := &PGRepository{pool: &fakePool{tx: tx}}
	message, err := repo.FinalizeOutboundIntent(t.Context(), OutboundIntentFinalizeInput{
		OrgID: "org_1", IdempotencyKey: "human-reply-0001", RequestFingerprint: "fingerprint-1",
		ProviderMessageID: "wamid.1",
		Message: AddMessageInput{
			OrgID: "org_1", ConversationID: "conv_1", ActorUserID: "user_1", ActorName: "Ada",
			BodyText: "Hello", Direction: DirectionOutbound,
			OccurredAt: time.Date(2026, time.July, 13, 12, 1, 0, 0, time.UTC),
		},
	})
	if err != nil || message.ID != "msg_1" {
		t.Fatalf("FinalizeOutboundIntent() = %#v/%v", message, err)
	}
	if !tx.committed || tx.rolledBack {
		t.Fatalf("transaction committed=%v rolledBack=%v", tx.committed, tx.rolledBack)
	}
	if len(tx.execSQL) != 3 || !strings.Contains(tx.execSQL[1], "conversation_audit_events") || !strings.Contains(tx.execSQL[2], "status = 'submitted'") {
		t.Fatalf("finalization SQL = %#v", tx.execSQL)
	}
	if len(tx.querySQL) < 1 || !strings.Contains(tx.querySQL[0], "FOR UPDATE") {
		t.Fatalf("intent was not locked for finalization: %#v", tx.querySQL)
	}
}

func TestFinalizeOutboundIntentRollsBackAllStateWhenAuditFails(t *testing.T) {
	tx := &fakeTx{
		queryRows: []pgx.Row{
			outboundIntentRow(OutboundIntentSending, "fingerprint-1", ""),
			messageRow("msg_1"),
		},
		execResults: []pgconn.CommandTag{pgconn.NewCommandTag("UPDATE 1")},
		execErrors:  []error{nil, errors.New("audit unavailable")},
	}
	repo := &PGRepository{pool: &fakePool{tx: tx}}
	_, err := repo.FinalizeOutboundIntent(t.Context(), OutboundIntentFinalizeInput{
		OrgID: "org_1", IdempotencyKey: "human-reply-0001", RequestFingerprint: "fingerprint-1",
		Message: AddMessageInput{
			OrgID: "org_1", ConversationID: "conv_1", BodyText: "Hello", Direction: DirectionOutbound,
			OccurredAt: time.Date(2026, time.July, 13, 12, 1, 0, 0, time.UTC),
		},
	})
	if err == nil || tx.committed || !tx.rolledBack {
		t.Fatalf("error/transaction = %v committed=%v rolledBack=%v", err, tx.committed, tx.rolledBack)
	}
}

func TestFinalizeOutboundIntentAtomicallyExecutesApprovedAIAction(t *testing.T) {
	tx := &fakeTx{
		queryRows: []pgx.Row{
			outboundAIIntentRow(OutboundIntentSending, "fingerprint-ai", ""),
			messageRow("msg_ai_1"),
		},
		execResults: []pgconn.CommandTag{
			pgconn.NewCommandTag("UPDATE 1"),
			pgconn.NewCommandTag("INSERT 0 1"),
			pgconn.NewCommandTag("UPDATE 1"),
			pgconn.NewCommandTag("UPDATE 1"),
		},
	}
	_, err := (&PGRepository{pool: &fakePool{tx: tx}}).FinalizeOutboundIntent(t.Context(), OutboundIntentFinalizeInput{
		OrgID: "org_1", IdempotencyKey: "conversation-ai:act_1", RequestFingerprint: "fingerprint-ai", AIActionID: "act_1",
		Message: AddMessageInput{
			OrgID: "org_1", ConversationID: "conv_1", BodyText: "Hello", Direction: DirectionOutbound,
			OccurredAt: time.Date(2026, time.July, 13, 12, 1, 0, 0, time.UTC),
		},
	})
	if err != nil || !tx.committed {
		t.Fatalf("FinalizeOutboundIntent() = %v committed=%v", err, tx.committed)
	}
	if len(tx.execSQL) != 4 || !strings.Contains(tx.execSQL[2], "status = 'executed'") || !strings.Contains(tx.execSQL[3], "status = 'submitted'") {
		t.Fatalf("AI finalization SQL = %#v", tx.execSQL)
	}
}

func TestFinalizeOutboundIntentRejectsMissingMismatchedAndRacedState(t *testing.T) {
	baseInput := OutboundIntentFinalizeInput{
		OrgID: "org_1", IdempotencyKey: "human-reply-0001", RequestFingerprint: "fingerprint-1",
		Message: AddMessageInput{
			OrgID: "org_1", ConversationID: "conv_1", BodyText: "Hello", Direction: DirectionOutbound,
			OccurredAt: time.Date(2026, time.July, 13, 12, 1, 0, 0, time.UTC),
		},
	}
	t.Run("missing intent", func(t *testing.T) {
		tx := &fakeTx{queryRows: []pgx.Row{fakeRow{err: pgx.ErrNoRows}}}
		_, err := (&PGRepository{pool: &fakePool{tx: tx}}).FinalizeOutboundIntent(t.Context(), baseInput)
		if !errors.Is(err, ErrNotFound) || !tx.rolledBack {
			t.Fatalf("error/rollback = %v/%v", err, tx.rolledBack)
		}
	})
	t.Run("begin failure", func(t *testing.T) {
		_, err := (&PGRepository{pool: &fakePool{beginErr: errors.New("begin unavailable")}}).FinalizeOutboundIntent(t.Context(), baseInput)
		if err == nil {
			t.Fatal("error = nil")
		}
	})
	t.Run("intent lookup failure", func(t *testing.T) {
		tx := &fakeTx{queryRows: []pgx.Row{fakeRow{err: errors.New("lookup unavailable")}}}
		_, err := (&PGRepository{pool: &fakePool{tx: tx}}).FinalizeOutboundIntent(t.Context(), baseInput)
		if err == nil || !tx.rolledBack {
			t.Fatalf("error/rollback = %v/%v", err, tx.rolledBack)
		}
	})
	t.Run("already submitted", func(t *testing.T) {
		tx := &fakeTx{queryRows: []pgx.Row{outboundIntentRow(OutboundIntentSubmitted, "fingerprint-1", "msg_1")}}
		_, err := (&PGRepository{pool: &fakePool{tx: tx}}).FinalizeOutboundIntent(t.Context(), baseInput)
		if !errors.Is(err, ErrConflict) {
			t.Fatalf("error = %v", err)
		}
	})
	t.Run("mismatched message tenant", func(t *testing.T) {
		tx := &fakeTx{queryRows: []pgx.Row{outboundIntentRow(OutboundIntentSending, "fingerprint-1", "")}}
		input := baseInput
		input.Message.OrgID = "org_other"
		_, err := (&PGRepository{pool: &fakePool{tx: tx}}).FinalizeOutboundIntent(t.Context(), input)
		if !errors.Is(err, ErrConflict) || tx.execCalls != 0 {
			t.Fatalf("error/execs = %v/%d", err, tx.execCalls)
		}
	})
	t.Run("conversation disappeared", func(t *testing.T) {
		tx := &fakeTx{
			queryRows:   []pgx.Row{outboundIntentRow(OutboundIntentSending, "fingerprint-1", ""), messageRow("msg_1")},
			execResults: []pgconn.CommandTag{pgconn.NewCommandTag("UPDATE 0")},
		}
		_, err := (&PGRepository{pool: &fakePool{tx: tx}}).FinalizeOutboundIntent(t.Context(), baseInput)
		if !errors.Is(err, ErrNotFound) || !tx.rolledBack {
			t.Fatalf("error/rollback = %v/%v", err, tx.rolledBack)
		}
	})
	t.Run("ledger transition lost race", func(t *testing.T) {
		tx := &fakeTx{
			queryRows: []pgx.Row{outboundIntentRow(OutboundIntentSending, "fingerprint-1", ""), messageRow("msg_1")},
			execResults: []pgconn.CommandTag{
				pgconn.NewCommandTag("UPDATE 1"), pgconn.NewCommandTag("INSERT 0 1"), pgconn.NewCommandTag("UPDATE 0"),
			},
		}
		_, err := (&PGRepository{pool: &fakePool{tx: tx}}).FinalizeOutboundIntent(t.Context(), baseInput)
		if !errors.Is(err, ErrConflict) || !tx.rolledBack {
			t.Fatalf("error/rollback = %v/%v", err, tx.rolledBack)
		}
	})
	t.Run("AI action transition lost race", func(t *testing.T) {
		tx := &fakeTx{
			queryRows: []pgx.Row{outboundAIIntentRow(OutboundIntentSending, "fingerprint-ai", ""), messageRow("msg_1")},
			execResults: []pgconn.CommandTag{
				pgconn.NewCommandTag("UPDATE 1"), pgconn.NewCommandTag("INSERT 0 1"), pgconn.NewCommandTag("UPDATE 0"),
			},
		}
		input := baseInput
		input.IdempotencyKey = "conversation-ai:act_1"
		input.RequestFingerprint = "fingerprint-ai"
		input.AIActionID = "act_1"
		_, err := (&PGRepository{pool: &fakePool{tx: tx}}).FinalizeOutboundIntent(t.Context(), input)
		if !errors.Is(err, ErrConflict) || !tx.rolledBack {
			t.Fatalf("error/rollback = %v/%v", err, tx.rolledBack)
		}
	})
}

func TestMarkOutboundIntentOutcomeAtomicallyMarksAIActionUnknown(t *testing.T) {
	tx := &fakeTx{execResults: []pgconn.CommandTag{
		pgconn.NewCommandTag("UPDATE 1"),
		pgconn.NewCommandTag("UPDATE 1"),
	}}
	err := (&PGRepository{pool: &fakePool{tx: tx}}).MarkOutboundIntentOutcome(t.Context(), OutboundIntentOutcomeInput{
		OrgID: "org_1", IdempotencyKey: "conversation-ai:act_1", AIActionID: "act_1",
		Status: OutboundIntentUnknown, ErrorCode: "transport",
	})
	if err != nil || !tx.committed {
		t.Fatalf("MarkOutboundIntentOutcome() = %v committed=%v", err, tx.committed)
	}
	if len(tx.execSQL) != 2 || !strings.Contains(tx.execSQL[1], "conversation_ai_actions") {
		t.Fatalf("outcome SQL = %#v", tx.execSQL)
	}
}

func TestMarkOutboundIntentOutcomeValidatesAndResolvesTransitionRaces(t *testing.T) {
	if err := (&PGRepository{}).MarkOutboundIntentOutcome(t.Context(), OutboundIntentOutcomeInput{Status: OutboundIntentSubmitted}); !errors.Is(err, ErrInvalidInput) {
		t.Fatalf("invalid status error = %v", err)
	}

	tests := map[string]struct {
		row  pgx.Row
		want error
	}{
		"missing":    {row: fakeRow{err: pgx.ErrNoRows}, want: ErrNotFound},
		"conflict":   {row: fakeRow{value: OutboundIntentSubmitted}, want: ErrConflict},
		"idempotent": {row: fakeRow{value: OutboundIntentUnknown}},
	}
	for name, test := range tests {
		t.Run(name, func(t *testing.T) {
			tx := &fakeTx{execResults: []pgconn.CommandTag{pgconn.NewCommandTag("UPDATE 0")}, queryRows: []pgx.Row{test.row}}
			err := (&PGRepository{pool: &fakePool{tx: tx}}).MarkOutboundIntentOutcome(t.Context(), OutboundIntentOutcomeInput{
				OrgID: "org_1", IdempotencyKey: "human-reply-0001", Status: OutboundIntentUnknown, ErrorCode: "transport",
			})
			if test.want == nil && err != nil {
				t.Fatalf("error = %v", err)
			}
			if test.want != nil && !errors.Is(err, test.want) {
				t.Fatalf("error = %v, want %v", err, test.want)
			}
		})
	}

	t.Run("begin failure", func(t *testing.T) {
		err := (&PGRepository{pool: &fakePool{beginErr: errors.New("begin unavailable")}}).MarkOutboundIntentOutcome(t.Context(), OutboundIntentOutcomeInput{
			OrgID: "org_1", IdempotencyKey: "key", Status: OutboundIntentUnknown,
		})
		if err == nil {
			t.Fatal("error = nil")
		}
	})
	t.Run("update failure", func(t *testing.T) {
		tx := &fakeTx{execErrors: []error{errors.New("update unavailable")}}
		err := (&PGRepository{pool: &fakePool{tx: tx}}).MarkOutboundIntentOutcome(t.Context(), OutboundIntentOutcomeInput{
			OrgID: "org_1", IdempotencyKey: "key", Status: OutboundIntentUnknown,
		})
		if err == nil || !tx.rolledBack {
			t.Fatalf("error/rollback = %v/%v", err, tx.rolledBack)
		}
	})
	t.Run("status lookup failure", func(t *testing.T) {
		tx := &fakeTx{execResults: []pgconn.CommandTag{pgconn.NewCommandTag("UPDATE 0")}, queryRows: []pgx.Row{fakeRow{err: errors.New("lookup unavailable")}}}
		err := (&PGRepository{pool: &fakePool{tx: tx}}).MarkOutboundIntentOutcome(t.Context(), OutboundIntentOutcomeInput{
			OrgID: "org_1", IdempotencyKey: "key", Status: OutboundIntentUnknown,
		})
		if err == nil || !tx.rolledBack {
			t.Fatalf("error/rollback = %v/%v", err, tx.rolledBack)
		}
	})
}

func TestGetMessageIsOrganizationScoped(t *testing.T) {
	message, err := (&PGRepository{pool: &fakePool{queryRows: []pgx.Row{messageRow("msg_1")}}}).GetMessage(t.Context(), "org_1", "msg_1")
	if err != nil || message.ID != "msg_1" || message.OrgID != "org_1" {
		t.Fatalf("GetMessage() = %#v/%v", message, err)
	}
}

func TestGetMessageMapsMissingAndDatabaseFailure(t *testing.T) {
	tests := map[string]struct {
		row  pgx.Row
		want error
	}{
		"missing":          {row: fakeRow{err: pgx.ErrNoRows}, want: ErrNotFound},
		"database failure": {row: fakeRow{err: errors.New("database unavailable")}},
	}
	for name, test := range tests {
		t.Run(name, func(t *testing.T) {
			_, err := (&PGRepository{pool: &fakePool{queryRows: []pgx.Row{test.row}}}).GetMessage(t.Context(), "org_1", "msg_1")
			if err == nil {
				t.Fatal("error = nil")
			}
			if test.want != nil && !errors.Is(err, test.want) {
				t.Fatalf("error = %v, want %v", err, test.want)
			}
		})
	}
}
