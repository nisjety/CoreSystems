use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;
pub const PREFIX: &str = "verevon-attach-v1.";
pub const ACTION_ID: &str = "verevon-attach-compat-v1";
const MAX_AGE: u64 = 300;
const MAX_FUTURE_SKEW: u64 = 30;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt { pub run_id: String, pub plan_id: String, pub session_run_id: String, pub attach_id: String, pub tab_id: String, pub issued_at: u64 }
fn ids_valid(r: &Receipt) -> bool { [&r.run_id,&r.plan_id,&r.session_run_id,&r.attach_id,&r.tab_id].iter().all(|s| !s.is_empty() && s.len() <= 256 && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_:.".contains(&c))) }
pub fn sign(key: &[u8], r: &Receipt) -> Option<String> {
    if key.len() < 32 || !ids_valid(r) { return None; }
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(r).ok()?);
    let mut mac = HmacSha256::new_from_slice(key).ok()?;
    mac.update(PREFIX.as_bytes()); mac.update(payload.as_bytes());
    Some(format!("{PREFIX}{payload}.{}", URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())))
}
pub fn verify(key: &[u8], envelope: &str, outer_run: &str, outer_plan: &str, now: u64) -> Option<Receipt> {
    if key.len() < 32 || envelope.len() > 4096 { return None; }
    let rest = envelope.strip_prefix(PREFIX)?;
    let (payload, signature) = rest.split_once('.')?;
    if signature.contains('.') { return None; }
    let mut mac = HmacSha256::new_from_slice(key).ok()?;
    mac.update(PREFIX.as_bytes()); mac.update(payload.as_bytes());
    mac.verify_slice(&URL_SAFE_NO_PAD.decode(signature).ok()?).ok()?;
    let receipt: Receipt = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    if !ids_valid(&receipt) || receipt.run_id != outer_run || receipt.plan_id != outer_plan || receipt.issued_at > now.saturating_add(MAX_FUTURE_SKEW) || now.saturating_sub(receipt.issued_at) > MAX_AGE { return None; }
    Some(receipt)
}
pub fn verify_configured(envelope: &str, run: &str, plan: &str) -> Option<Receipt> {
    let key = std::env::var("BROWSER_ATTACH_COMPAT_HMAC_KEY").ok()?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    verify(key.as_bytes(), envelope, run, plan, now)
}
#[cfg(test)] mod tests {
 use super::*;
 fn receipt() -> Receipt { Receipt { run_id:"run-1".into(),plan_id:"plan-1".into(),session_run_id:"session-1".into(),attach_id:"attach-1".into(),tab_id:"tab-1".into(),issued_at:1000 } }
 #[test] fn accepts_valid_and_replay_within_lifetime(){let r=receipt();let s=sign(&[42;32],&r).unwrap();assert_eq!(verify(&[42;32],&s,"run-1","plan-1",1002),Some(r.clone()));assert_eq!(verify(&[42;32],&s,"run-1","plan-1",1002),Some(r));}
 #[test] fn rejects_wrong_key_scope_and_expired(){let s=sign(&[42;32],&receipt()).unwrap();for (key,run,plan,now) in [(&[43;32][..],"run-1","plan-1",1001),(&[42;32][..],"run-2","plan-1",1001),(&[42;32][..],"run-1","plan-2",1001),(&[42;32][..],"run-1","plan-1",1301),(&[42;32][..],"run-1","plan-1",900)] {assert!(verify(key,&s,run,plan,now).is_none());}}
 #[test] fn rejects_tampered_each_id(){let r=receipt(); let original=sign(&[42;32],&r).unwrap(); let (_,sig)=original[PREFIX.len()..].split_once('.').unwrap();for i in 0..5 {let mut forged=r.clone();match i {0=>forged.run_id.push('x'),1=>forged.plan_id.push('x'),2=>forged.session_run_id.push('x'),3=>forged.attach_id.push('x'),_=>forged.tab_id.push('x')};let p=URL_SAFE_NO_PAD.encode(serde_json::to_vec(&forged).unwrap());assert!(verify(&[42;32],&format!("{PREFIX}{p}.{sig}"),&forged.run_id,&forged.plan_id,1001).is_none());}}
 #[test] fn rejects_short_key_and_bad_input(){assert!(sign(&[1;16],&receipt()).is_none());assert!(verify(&[42;32],"verevon-attach-v1.evil", "run-1","plan-1",1000).is_none());let mut r=receipt();r.tab_id="x\nsecret".into();assert!(sign(&[42;32],&r).is_none());}
}
