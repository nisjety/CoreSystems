package conversation

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"
	"time"
	"unicode/utf8"

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
	execArgs    [][]any
	execCalls   int
	queryRows   []pgx.Row
	querySQL    []string
	queryCalls  int
	committed   bool
	rolledBack  bool
	// multiQueryRows/multiQueryErr back Query (plural), used by statements
	// that RETURNING multiple rows (e.g. ReconcileStaleOutboundIntents' bulk
	// UPDATE); QueryRow (singular, above) is unaffected.
	multiQueryRows  pgx.Rows
	multiQueryErr   error
	multiQuerySQL   []string
	multiQueryArgs  [][]any
	multiQueryCalls int
}

func (t *fakeTx) Exec(_ context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	i := t.execCalls
	t.execCalls++
	t.execSQL = append(t.execSQL, sql)
	t.execArgs = append(t.execArgs, args)
	var tag pgconn.CommandTag
	if i < len(t.execResults) {
		tag = t.execResults[i]
	}
	if i < len(t.execErrors) && t.execErrors[i] != nil {
		return tag, t.execErrors[i]
	}
	return tag, nil
}

func TestReconcileTeamsConversationMetadataUsesCounterpart(t *testing.T) {
	tx := &fakeTx{}
	event := InboundEvent{
		OrgID:     "org_1",
		Provider:  "teams",
		Direction: DirectionOutbound,
		Subject:   "Robert Røsten",
		From:      ParticipantInput{Name: "Ima Fernandes Da Costa", Email: "ima@aquatiq.com"},
		To:        []ParticipantInput{{Name: "Robert Røsten", Email: "robert@example.com"}},
	}

	if err := reconcileTeamsConversationMetadata(t.Context(), tx, event, "conv_1"); err != nil {
		t.Fatalf("reconcileTeamsConversationMetadata: %v", err)
	}
	if len(tx.execSQL) != 2 {
		t.Fatalf("Exec calls = %d, want contact upsert and conversation repair", len(tx.execSQL))
	}
	if !strings.Contains(tx.execSQL[1], "title IN ('', '(chat)', 'Re: (chat)')") {
		t.Fatalf("conversation repair does not replace placeholder titles:\n%s", tx.execSQL[1])
	}
	if got := tx.execArgs[0][2]; got != "Robert Røsten" {
		t.Fatalf("contact name = %v, want Teams counterpart", got)
	}
	if got := tx.execArgs[0][3]; got != "robert@example.com" {
		t.Fatalf("contact email = %v, want Teams counterpart", got)
	}
}

func TestReconcileTeamsMessageMetadataMarksSelfAsAgent(t *testing.T) {
	tx := &fakeTx{}
	event := InboundEvent{
		OrgID:     "org_1",
		Provider:  "teams",
		Direction: DirectionOutbound,
		From:      ParticipantInput{Name: "Ima Fernandes Da Costa", Email: "ima@aquatiq.com"},
	}

	if err := reconcileTeamsMessageMetadata(t.Context(), tx, event, "msg_1"); err != nil {
		t.Fatalf("reconcileTeamsMessageMetadata: %v", err)
	}
	if len(tx.execSQL) != 1 || !strings.Contains(tx.execSQL[0], "sender_type = $4") {
		t.Fatalf("message repair SQL = %q, want sender ownership update", tx.execSQL)
	}
	if got := tx.execArgs[0][2]; got != DirectionOutbound {
		t.Fatalf("direction = %v, want outbound", got)
	}
	if got := tx.execArgs[0][3]; got != "agent" {
		t.Fatalf("sender type = %v, want agent", got)
	}
}

func TestStoredEventAuditTypeUsesMessageSentForOutboundHistory(t *testing.T) {
	if got := storedEventAuditType(InboundEvent{Direction: DirectionOutbound}, false); got != "message.sent" {
		t.Fatalf("audit type = %q, want message.sent", got)
	}
	if got := storedEventAuditType(InboundEvent{Direction: DirectionInbound}, false); got != "message.received" {
		t.Fatalf("audit type = %q, want message.received", got)
	}
	if got := storedEventAuditType(InboundEvent{Direction: DirectionOutbound}, true); got != "conversation.created" {
		t.Fatalf("created audit type = %q, want conversation.created", got)
	}
}

func (t *fakeTx) Commit(_ context.Context) error   { t.committed = true; return nil }
func (t *fakeTx) Rollback(_ context.Context) error { t.rolledBack = true; return nil }

func (t *fakeTx) Query(_ context.Context, sql string, args ...any) (pgx.Rows, error) {
	t.multiQueryCalls++
	t.multiQuerySQL = append(t.multiQuerySQL, sql)
	t.multiQueryArgs = append(t.multiQueryArgs, args)
	if t.multiQueryErr != nil {
		return nil, t.multiQueryErr
	}
	if t.multiQueryRows != nil {
		return t.multiQueryRows, nil
	}
	return &fakeRows{}, nil
}

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

// fakeRows implements pgx.Rows over a fixed slice of fakeRow values so a
// multi-row RETURNING statement (e.g. ReconcileStaleOutboundIntents) can be
// unit-tested without a live database. Only Next/Scan/Err/Close are
// exercised; the remaining pgx.Rows methods panic to prove they are not
// reached on this path.
type fakeRows struct {
	values []fakeRow
	index  int
	err    error
}

func (r *fakeRows) Close()                                       {}
func (r *fakeRows) Err() error                                   { return r.err }
func (r *fakeRows) CommandTag() pgconn.CommandTag                { panic("unused") }
func (r *fakeRows) FieldDescriptions() []pgconn.FieldDescription { panic("unused") }
func (r *fakeRows) Next() bool {
	if r.index >= len(r.values) {
		return false
	}
	r.index++
	return true
}
func (r *fakeRows) Scan(dest ...any) error { return r.values[r.index-1].Scan(dest...) }
func (r *fakeRows) Values() ([]any, error) { panic("unused") }
func (r *fakeRows) RawValues() [][]byte    { panic("unused") }
func (r *fakeRows) Conn() *pgx.Conn        { panic("unused") }

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

func TestStoreInboundEventOnlyPromotesChronologicallyNewerMessages(t *testing.T) {
	tx := &fakeTx{
		queryRows: []pgx.Row{
			fakeRow{err: pgx.ErrNoRows},
			fakeRow{value: "conv_existing"},
		},
		execErrors: []error{nil, nil, nil, nil, nil, errors.New("stop after latest-message update")},
	}
	repo := &PGRepository{pool: &fakePool{tx: tx}}
	_, err := repo.StoreInboundEvent(t.Context(), InboundEvent{
		OrgID: "org_1", ConnectionID: "conn_ms", Provider: "microsoft",
		ProviderThreadID: "thread_1", ProviderMessageID: "message_old",
		ProviderEventID: "event_old", IDempotencyKey: "microsoft:event_old",
		Direction: DirectionInbound, Subject: "Old mail", BodyText: "older body",
		From:       ParticipantInput{Name: "Customer", Email: "customer@example.com"},
		OccurredAt: time.Date(2026, time.July, 18, 8, 0, 0, 0, time.UTC),
	})
	if err == nil {
		t.Fatal("StoreInboundEvent error = nil, want sentinel after update")
	}
	if len(tx.execSQL) < 5 {
		t.Fatalf("Exec calls = %d, want latest-message update", len(tx.execSQL))
	}
	latestUpdate := tx.execSQL[4]
	if !strings.Contains(latestUpdate, "last_message_at IS NULL OR last_message_at <= $4") {
		t.Fatalf("latest-message update can regress on older backfill:\n%s", latestUpdate)
	}
	if !strings.Contains(latestUpdate, "updated_at = GREATEST(updated_at, $4)") {
		t.Fatalf("activity timestamp can regress on delayed backfill:\n%s", latestUpdate)
	}
}

func TestPreviewTruncatesWithoutSplittingUTF8(t *testing.T) {
	// 239 ASCII bytes followed by a three-byte rune crosses the old byte slice
	// boundary at 240 and used to produce invalid UTF-8 for Postgres.
	input := strings.Repeat("a", 239) + "€" + strings.Repeat("b", 20)
	got := preview(input, "")

	if !strings.HasSuffix(got, "€") {
		t.Fatalf("preview suffix = %q, want complete multi-byte rune", got[len(got)-4:])
	}
	if !utf8.ValidString(got) {
		t.Fatalf("preview is invalid UTF-8: %q", got)
	}
	if runeCount := utf8.RuneCountInString(got); runeCount != 240 {
		t.Fatalf("preview rune count = %d, want 240", runeCount)
	}
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

// Approving with edited fields must merge only the caller-provided keys into
// payload.suggested_fields, atomically with the status UPDATE (same Exec, same
// transaction), so the executor's later fresh GetAIAction read observes the
// reviewer's overrides.
func TestReviewAIActionMergesEditedFieldsIntoSuggestedFieldsOnApprove(t *testing.T) {
	tx := &fakeTx{execResults: []pgconn.CommandTag{
		pgconn.NewCommandTag("UPDATE 1"),
		pgconn.NewCommandTag("INSERT 0 1"),
	}}
	repo := &PGRepository{pool: &fakePool{tx: tx}}

	err := repo.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:        "org_1",
		AIActionID:   "aiact_1",
		ReviewerID:   "user_1",
		Decision:     "approved",
		EditedFields: map[string]string{"category": "sales", "priority": "urgent"},
		OccurredAt:   time.Date(2026, time.July, 20, 8, 0, 0, 0, time.UTC),
	})

	if err != nil {
		t.Fatalf("error = %v, want nil", err)
	}
	if tx.execCalls != 2 {
		t.Fatalf("Exec called %d times, want 2 (UPDATE action + INSERT review row -- the merge rides the SAME UPDATE, not a third call)", tx.execCalls)
	}
	if !strings.Contains(tx.execSQL[0], "jsonb_set(payload, '{suggested_fields}'") {
		t.Fatalf("UPDATE does not merge edited fields into suggested_fields:\n%s", tx.execSQL[0])
	}
	if !strings.Contains(tx.execSQL[0], "status = 'suggested'") {
		t.Fatalf("UPDATE is no longer a compare-and-set from suggested state:\n%s", tx.execSQL[0])
	}
	editedFieldsArg, ok := tx.execArgs[0][5].(string)
	if !ok {
		t.Fatalf("edited-fields arg type = %T, want string", tx.execArgs[0][5])
	}
	if !strings.Contains(editedFieldsArg, `"category":"sales"`) || !strings.Contains(editedFieldsArg, `"priority":"urgent"`) {
		t.Fatalf("edited-fields JSON = %s, want category and priority", editedFieldsArg)
	}
	if !tx.committed {
		t.Fatal("transaction not committed for an approve with edited fields")
	}
}

// Approving with no edited fields must leave payload untouched: the JSON arg
// is the empty object and the SQL's own gate (<> '{}'::jsonb) turns the merge
// into a no-op, so behavior is byte-for-byte identical to before edited-fields
// support existed.
func TestReviewAIActionNoEditedFieldsIsNoOpMerge(t *testing.T) {
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
		OccurredAt: time.Date(2026, time.July, 20, 8, 0, 0, 0, time.UTC),
	})

	if err != nil {
		t.Fatalf("error = %v, want nil", err)
	}
	editedFieldsArg, ok := tx.execArgs[0][5].(string)
	if !ok {
		t.Fatalf("edited-fields arg type = %T, want string", tx.execArgs[0][5])
	}
	if editedFieldsArg != "{}" {
		t.Fatalf("edited-fields JSON = %s, want empty object for a no-edits approve", editedFieldsArg)
	}
	if !strings.Contains(tx.execSQL[0], "<> '{}'::jsonb") {
		t.Fatalf("UPDATE does not gate the merge on non-empty edited fields:\n%s", tx.execSQL[0])
	}
}

// Defense-in-depth: even if a reject somehow carried edited fields (the
// service layer must never let that happen), the repository's merge is gated
// on decision = 'approved' in the SQL itself, so a reject can never mutate
// payload.suggested_fields.
func TestReviewAIActionRejectNeverMergesEditedFieldsEvenIfPresent(t *testing.T) {
	tx := &fakeTx{execResults: []pgconn.CommandTag{
		pgconn.NewCommandTag("UPDATE 1"),
		pgconn.NewCommandTag("INSERT 0 1"),
	}}
	repo := &PGRepository{pool: &fakePool{tx: tx}}

	err := repo.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:        "org_1",
		AIActionID:   "aiact_1",
		ReviewerID:   "user_1",
		Decision:     "rejected",
		EditedFields: map[string]string{"category": "sales"},
		OccurredAt:   time.Date(2026, time.July, 20, 8, 0, 0, 0, time.UTC),
	})

	if err != nil {
		t.Fatalf("error = %v, want nil", err)
	}
	if !strings.Contains(tx.execSQL[0], "WHEN $3 = 'approved'") {
		t.Fatalf("UPDATE does not gate the merge on decision = 'approved':\n%s", tx.execSQL[0])
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

// staleOutboundIntentRow builds a fake RETURNING row shaped like a
// conversation_outbound_intents record already flipped to `unknown` by
// ReconcileStaleOutboundIntents's UPDATE — id/conversation/ai_action_id vary
// per test case, everything else mirrors outboundIntentRow's fixture values.
func staleOutboundIntentRow(id, conversationID, aiActionID string) fakeRow {
	row := outboundIntentRow(OutboundIntentUnknown, "fp-"+id, "").(fakeRow)
	row.values[0] = id
	row.values[3] = conversationID
	row.values[4] = aiActionID
	row.values[18] = OutboundIntentErrorStaleSendingTimeout
	return row
}

// The sweep must (1) run the bulk UPDATE...RETURNING against the reconciliation
// index columns, (2) flip the linked AI action out of `approved` for every
// reconciled row that carries one, (3) leave a plain human-reply row (no
// ai_action_id) alone, and (4) commit.
func TestReconcileStaleOutboundIntentsFlipsSendingRowsAndLinkedApprovedActions(t *testing.T) {
	rows := &fakeRows{values: []fakeRow{
		staleOutboundIntentRow("oi_ai", "conv_ai", "act_1"),
		staleOutboundIntentRow("oi_human", "conv_human", ""),
	}}
	tx := &fakeTx{
		multiQueryRows: rows,
		execResults:    []pgconn.CommandTag{pgconn.NewCommandTag("UPDATE 1")},
	}
	reconciled, err := (&PGRepository{pool: &fakePool{tx: tx}}).ReconcileStaleOutboundIntents(t.Context(), 15*time.Minute)
	if err != nil || !tx.committed {
		t.Fatalf("ReconcileStaleOutboundIntents() = %v committed=%v", err, tx.committed)
	}
	if len(reconciled) != 2 {
		t.Fatalf("reconciled = %#v, want 2", reconciled)
	}
	if reconciled[0].Status != OutboundIntentUnknown || reconciled[0].ErrorCode != OutboundIntentErrorStaleSendingTimeout {
		t.Fatalf("reconciled[0] = %#v, want status/error_code unknown/%s", reconciled[0], OutboundIntentErrorStaleSendingTimeout)
	}
	if len(tx.multiQuerySQL) != 1 ||
		!strings.Contains(tx.multiQuerySQL[0], "status = 'sending'") ||
		!strings.Contains(tx.multiQuerySQL[0], "make_interval") {
		t.Fatalf("sweep SQL = %#v, want a status='sending'/make_interval RETURNING sweep", tx.multiQuerySQL)
	}
	if tx.execCalls != 1 {
		t.Fatalf("Exec called %d times, want exactly 1 (only the row with an ai_action_id)", tx.execCalls)
	}
	if !strings.Contains(tx.execSQL[0], "conversation_ai_actions") || !strings.Contains(tx.execSQL[0], "status = 'approved'") {
		t.Fatalf("linked action update SQL = %q", tx.execSQL[0])
	}
	if len(tx.execArgs[0]) != 2 || tx.execArgs[0][1] != "act_1" {
		t.Fatalf("linked action update args = %v, want [org_1 act_1]", tx.execArgs[0])
	}
}

// staleAfter must be positive: zero or negative would match every `sending`
// row unconditionally (updated_at < NOW() is always true), reconciling
// send attempts still legitimately in flight. This must fail closed before
// ever touching the pool.
func TestReconcileStaleOutboundIntentsRejectsNonPositiveStaleAfter(t *testing.T) {
	for _, staleAfter := range []time.Duration{0, -1 * time.Minute} {
		_, err := (&PGRepository{pool: &fakePool{}}).ReconcileStaleOutboundIntents(t.Context(), staleAfter)
		if !errors.Is(err, ErrInvalidInput) {
			t.Fatalf("staleAfter=%v error = %v, want ErrInvalidInput", staleAfter, err)
		}
	}
}

func TestReconcileStaleOutboundIntentsRollsBackOnQueryFailure(t *testing.T) {
	tx := &fakeTx{multiQueryErr: errors.New("update unavailable")}
	_, err := (&PGRepository{pool: &fakePool{tx: tx}}).ReconcileStaleOutboundIntents(t.Context(), 15*time.Minute)
	if err == nil || !tx.rolledBack || tx.committed {
		t.Fatalf("error/rolledBack/committed = %v/%v/%v", err, tx.rolledBack, tx.committed)
	}
}

func TestReconcileStaleOutboundIntentsRollsBackWhenLinkedActionUpdateFails(t *testing.T) {
	rows := &fakeRows{values: []fakeRow{staleOutboundIntentRow("oi_ai", "conv_ai", "act_1")}}
	tx := &fakeTx{multiQueryRows: rows, execErrors: []error{errors.New("update unavailable")}}
	_, err := (&PGRepository{pool: &fakePool{tx: tx}}).ReconcileStaleOutboundIntents(t.Context(), 15*time.Minute)
	if err == nil || !tx.rolledBack || tx.committed {
		t.Fatalf("error/rolledBack/committed = %v/%v/%v", err, tx.rolledBack, tx.committed)
	}
}

func TestGetMessageIsOrganizationScoped(t *testing.T) {
	message, err := (&PGRepository{pool: &fakePool{queryRows: []pgx.Row{messageRow("msg_1")}}}).GetMessage(t.Context(), "org_1", "msg_1")
	if err != nil || message.ID != "msg_1" || message.OrgID != "org_1" {
		t.Fatalf("GetMessage() = %#v/%v", message, err)
	}
}

// HardPurgeByOrg must bind every one of its DELETE statements to exactly the
// target org's id and nothing else — that is the only thing standing between
// a purge and touching another org's rows, so it is asserted for every
// statement, not just a sample.
func TestHardPurgeByOrgDeletesEveryOrgScopedTableBoundToOnlyThatOrg(t *testing.T) {
	tx := &fakeTx{}
	repo := &PGRepository{pool: &fakePool{tx: tx}}

	if err := repo.HardPurgeByOrg(context.Background(), "org_A"); err != nil {
		t.Fatalf("HardPurgeByOrg() = %v, want nil", err)
	}
	if !tx.committed || tx.rolledBack {
		t.Fatalf("committed=%v rolledBack=%v, want committed only", tx.committed, tx.rolledBack)
	}
	if len(tx.execSQL) != len(hardPurgeOrgQueries) {
		t.Fatalf("Exec called %d times, want %d (one DELETE per org-scoped table)", len(tx.execSQL), len(hardPurgeOrgQueries))
	}
	for i, sql := range tx.execSQL {
		if !strings.Contains(sql, "WHERE org_id = $1") {
			t.Fatalf("statement %d is not org-scoped: %s", i, sql)
		}
		if len(tx.execArgs[i]) != 1 || tx.execArgs[i][0] != "org_A" {
			t.Fatalf("statement %d args = %v, want exactly [org_A] -- a purge must never bind another org's id", i, tx.execArgs[i])
		}
	}
}

// NATS is at-least-once delivery: a redelivered erasure event re-runs the
// purge for the same org. Every statement is a plain DELETE with no
// compensating insert, so a second run must succeed identically to the
// first (deleting zero rows the second time in a real database).
func TestHardPurgeByOrgIsSafeToRunTwice(t *testing.T) {
	for i := 0; i < 2; i++ {
		tx := &fakeTx{}
		repo := &PGRepository{pool: &fakePool{tx: tx}}
		if err := repo.HardPurgeByOrg(context.Background(), "org_A"); err != nil {
			t.Fatalf("run %d: HardPurgeByOrg() = %v, want nil", i, err)
		}
		if !tx.committed || tx.rolledBack {
			t.Fatalf("run %d: committed=%v rolledBack=%v, want committed only", i, tx.committed, tx.rolledBack)
		}
	}
}

func TestHardPurgeByOrgRejectsBlankOrgID(t *testing.T) {
	repo := &PGRepository{pool: &fakePool{}}
	if err := repo.HardPurgeByOrg(context.Background(), "   "); !errors.Is(err, ErrInvalidInput) {
		t.Fatalf("error = %v, want ErrInvalidInput", err)
	}
}

// A failure partway through the purge must roll back everything already
// deleted in that transaction -- an org purge is all-or-nothing, never left
// half-applied.
func TestHardPurgeByOrgRollsBackAllStatementsOnFailure(t *testing.T) {
	execErrors := make([]error, len(hardPurgeOrgQueries))
	execErrors[len(hardPurgeOrgQueries)-1] = errors.New("delete unavailable")
	tx := &fakeTx{execErrors: execErrors}
	repo := &PGRepository{pool: &fakePool{tx: tx}}

	if err := repo.HardPurgeByOrg(context.Background(), "org_A"); err == nil {
		t.Fatal("error = nil, want failure from the final statement")
	}
	if tx.committed || !tx.rolledBack {
		t.Fatalf("committed=%v rolledBack=%v, want rollback on failure", tx.committed, tx.rolledBack)
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
