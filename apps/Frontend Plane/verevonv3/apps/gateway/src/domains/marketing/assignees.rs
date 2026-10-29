//! Assignment identities come from Control Plane's authenticated membership
//! API. Browser-supplied user IDs never become membership assertions.
use super::*;
use axum::body::to_bytes;
use serde_json::Value;

pub(super) async fn members(state:&AppState,user:&AuthenticatedUser,headers:&HeaderMap)->Result<Vec<Value>,()> {
 let org=user.active_org_id.as_deref().ok_or(())?;
 let cookie=headers.get("cookie").and_then(|v|v.to_str().ok()).filter(|v|!v.is_empty()).ok_or(())?;
 let target=format!("{}/api/auth/organization/list-members?organizationId={}&limit=1000&offset=0",state.auth_core_url.trim_end_matches('/'),urlencoding::encode(org));
 let response=crate::upstream::proxy_auth(state,Method::GET,&target,None,Some(cookie),Some(state.verevon_public_origin.as_str())).await;
 if !response.status().is_success(){return Err(())};let body=to_bytes(response.into_body(),256*1024).await.map_err(|_|())?;let value:Value=serde_json::from_slice(&body).map_err(|_|())?;
 let rows=value.get("members").or_else(||value.pointer("/data/members")).and_then(Value::as_array).ok_or(())?;
 if rows.len()>1000{return Err(())};let mut out=vec![];let mut seen=std::collections::HashSet::new();
 for row in rows {
  if row.get("organizationId").and_then(Value::as_str)!=Some(org){return Err(())};
  let id=row.get("userId").and_then(Value::as_str).ok_or(())?;let role=row.get("role").and_then(Value::as_str).ok_or(())?;
  if id.is_empty()||id.len()>200||!id.bytes().all(|c|c.is_ascii_alphanumeric()||b"._:-".contains(&c))||!matches!(role,"owner"|"admin"|"member"|"viewer")||!seen.insert(id.to_owned()){return Err(())};
  let name=row.pointer("/user/name").and_then(Value::as_str).unwrap_or(id);
  if name.len()>200||name.chars().any(char::is_control){return Err(())};
  out.push(json!({"id":id,"name":name}));
 }
 Ok(out)
}
