ROOT = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567"


def edit(p, pairs):
    b = open(p, "rb").read().decode("utf-8")
    crlf = "\r\n" in b
    b = b.replace("\r\n", "\n")
    for old, new in pairs:
        assert b.count(old) == 1, (p, b.count(old), old[:80])
        b = b.replace(old, new, 1)
    if crlf:
        b = b.replace("\n", "\r\n")
    open(p, "wb").write(b.encode("utf-8"))


# ---------------------------------------------------------------- Control (Go)
GO = ROOT + "/apps/Control Plane/user-core/internal/spaces/mail_task_thread_decision"
edit(GO + ".go", [
    ('''	Nonce             string    `json:"nonce"`
	IssuedAt          time.Time `json:"issued_at"`
	ExpiresAt         time.Time `json:"expires_at"`
}
''',
     '''	Nonce             string    `json:"nonce"`
	// ZeroDataRetention is always false on a decision that exists: Control refuses to issue one for a ZDR
	// org. It is signed anyway so Session Core can require the statement and refuse a true one (the same
	// explicit claim the scheduled-run decision carries), rather than rely on Control alone.
	ZeroDataRetention bool      `json:"zero_data_retention"`
	IssuedAt          time.Time `json:"issued_at"`
	ExpiresAt         time.Time `json:"expires_at"`
}
'''),
    ('''	if d.SessionKey != MailTaskSessionKey(d.OrgID, d.ProviderMessageID) {''',
     '''	if d.ZeroDataRetention {
		return fmt.Errorf("zero data retention forbids a mail task thread decision")
	}
	if d.SessionKey != MailTaskSessionKey(d.OrgID, d.ProviderMessageID) {'''),
    ('''		ActionID:          mailTaskThreadPrepareAction,
		Nonce:             nonce,
		IssuedAt:          now.UTC(),''',
     '''		ActionID:          mailTaskThreadPrepareAction,
		Nonce:             nonce,
		ZeroDataRetention: false,
		IssuedAt:          now.UTC(),'''),
])

edit(GO + "_test.go", [
    ('''import (
	"strings"
	"testing"
	"time"
)
''',
     '''import (
	"encoding/base64"
	"strings"
	"testing"
	"time"
)
'''),
    ('''func TestIssueMailTaskThreadDecisionRefusesWithoutThreadCreateEntitlement(t *testing.T) {''',
     '''// Session Core requires an explicit zero_data_retention statement and refuses a true one, so the signed
// claim set must carry it, and false, on every decision Control issues.
func TestIssueMailTaskThreadDecisionSignsAnExplicitFalseZeroDataRetentionClaim(t *testing.T) {
	signer, verifier := testDecisionKeys(t)
	now := time.Date(2026, time.September, 28, 12, 0, 0, 0, time.UTC)
	token, err := IssueMailTaskThreadDecision(validMailTaskEffectPolicy("org-1"), signer, "org-1", "user-1", "alias-1", "provider-msg-1", now)
	if err != nil {
		t.Fatalf("IssueMailTaskThreadDecision: %v", err)
	}
	payload, err := base64.RawURLEncoding.DecodeString(strings.Split(token, ".")[2])
	if err != nil {
		t.Fatalf("payload is not base64url: %v", err)
	}
	if !strings.Contains(string(payload), `"zero_data_retention":false`) {
		t.Fatalf("the signed claims must state zero_data_retention:false explicitly, got %s", payload)
	}
	decision, err := VerifyMailTaskThreadDecision(verifier, token, now.Add(time.Second))
	if err != nil || decision.ZeroDataRetention {
		t.Fatalf("a freshly issued decision must verify with the claim false: %v %#v", err, decision)
	}
}

func TestMailTaskThreadDecisionIsNeverSignedWithAZeroDataRetentionClaim(t *testing.T) {
	signer, _ := testDecisionKeys(t)
	now := time.Date(2026, time.September, 28, 12, 0, 0, 0, time.UTC)
	decision := MailTaskThreadDecision{
		DecisionRef: "decision-1", OrgID: "org-1", UserID: "user-1", AliasID: "alias-1",
		ProviderMessageID: "provider-msg-1", SessionKey: MailTaskSessionKey("org-1", "provider-msg-1"),
		Origin: "mail_task", ServiceAudience: mailTaskThreadPrepareAudience, ActionID: mailTaskThreadPrepareAction,
		Nonce: "nonce-1", IssuedAt: now, ExpiresAt: now.Add(time.Minute),
	}
	if _, err := SignMailTaskThreadDecision(signer, decision); err != nil {
		t.Fatalf("a decision without the claim must sign: %v", err)
	}
	decision.ZeroDataRetention = true
	if _, err := SignMailTaskThreadDecision(signer, decision); err == nil {
		t.Fatalf("a decision claiming zero data retention must never be signed")
	}
}

func TestIssueMailTaskThreadDecisionRefusesWithoutThreadCreateEntitlement(t *testing.T) {'''),
])

# ---------------------------------------------------------------- Session Core (Rust)
RS = ROOT + "/apps/Model Plane/rust/services/session-core/src/grpc.rs"
edit(RS, [
    ('''    session_key: String,
    origin: String,
    service_audience: String,
    action_id: String,
    nonce: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

fn scheduled_run_payload_digest(''',
     '''    session_key: String,
    origin: String,
    service_audience: String,
    action_id: String,
    nonce: String,
    /// Required, like the scheduled-run decision's: Control states it on every decision it signs (always
    /// false, since it refuses a ZDR org), and a decision that omits it or claims true is refused here.
    zero_data_retention: bool,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

fn scheduled_run_payload_digest('''),
    ('''        && !claims.decision_ref.trim().is_empty()
        && !claims.alias_id.trim().is_empty()
        && !claims.provider_message_id.trim().is_empty();
    if !matches''',
     '''        && !claims.decision_ref.trim().is_empty()
        && !claims.alias_id.trim().is_empty()
        && !claims.provider_message_id.trim().is_empty()
        && !claims.zero_data_retention;
    if !matches'''),
    ('''        action_id: String,
        nonce: String,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    }

    impl MailTaskDecisionBuilder {''',
     '''        action_id: String,
        nonce: String,
        /// `None` leaves the claim out of the signed payload altogether.
        zero_data_retention: Option<bool>,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    }

    impl MailTaskDecisionBuilder {'''),
    ('''                nonce: "nonce-1".to_owned(),
                issued_at: now - chrono::Duration::seconds(1),
                expires_at: now + chrono::Duration::minutes(5),
            }
        }

        fn keys(&self) -> BTreeMap<String, VerifyingKey> {
            BTreeMap::from([(self.key_id.clone(), self.signing_key.verifying_key())])''',
     '''                nonce: "nonce-1".to_owned(),
                zero_data_retention: Some(false),
                issued_at: now - chrono::Duration::seconds(1),
                expires_at: now + chrono::Duration::minutes(5),
            }
        }

        fn keys(&self) -> BTreeMap<String, VerifyingKey> {
            BTreeMap::from([(self.key_id.clone(), self.signing_key.verifying_key())])'''),
    ('''        fn sign(&self) -> String {
            let claims = serde_json::json!({
                "decision_ref": self.decision_ref,
                "org_id": self.org_id,
                "user_id": self.user_id,
                "alias_id": self.alias_id,
                "provider_message_id": self.provider_message_id,
                "session_key": self.session_key,
                "origin": self.origin,
                "service_audience": self.service_audience,
                "action_id": self.action_id,
                "nonce": self.nonce,
                "issued_at": self.issued_at.to_rfc3339(),
                "expires_at": self.expires_at.to_rfc3339(),
            });
            let payload''',
     '''        fn sign(&self) -> String {
            let mut claims = serde_json::json!({
                "decision_ref": self.decision_ref,
                "org_id": self.org_id,
                "user_id": self.user_id,
                "alias_id": self.alias_id,
                "provider_message_id": self.provider_message_id,
                "session_key": self.session_key,
                "origin": self.origin,
                "service_audience": self.service_audience,
                "action_id": self.action_id,
                "nonce": self.nonce,
                "issued_at": self.issued_at.to_rfc3339(),
                "expires_at": self.expires_at.to_rfc3339(),
            });
            if let Some(zero_data_retention) = self.zero_data_retention {
                claims["zero_data_retention"] = serde_json::json!(zero_data_retention);
            }
            let payload'''),
    ('''    /// prepare_user_owned_thread_rejects_a_decision_minted_for_user_A_when_the_request_names_user_B''',
     '''    /// A mail-task decision claiming zero data retention is refused, and so is one that does not state
    /// the claim at all: Control always states it (false), so a silent decision is not one it signed.
    #[test]
    fn mail_task_thread_decision_is_refused_for_a_zero_data_retention_claim_or_a_missing_one() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-28T12:00:00Z")
            .expect("test time")
            .with_timezone(&Utc);
        for (label, claim) in [("claims true", Some(true)), ("states nothing", None)] {
            let mut decision = MailTaskDecisionBuilder::new(now);
            decision.zero_data_retention = claim;
            let error = super::verify_mail_task_thread_decision_with_keys(
                &decision.request(),
                &decision.keys(),
                now,
            )
            .expect_err("a zero-retention or silent mail-task decision must not authorize a thread");
            assert_eq!(error.code(), tonic::Code::PermissionDenied, "{label}");
        }
        // Control's own shape (the claim stated, false) still passes.
        let decision = MailTaskDecisionBuilder::new(now);
        super::verify_mail_task_thread_decision_with_keys(&decision.request(), &decision.keys(), now)
            .expect("an explicit false claim authorizes");
    }

    /// prepare_user_owned_thread_rejects_a_decision_minted_for_user_A_when_the_request_names_user_B'''),
])
print("edited")
