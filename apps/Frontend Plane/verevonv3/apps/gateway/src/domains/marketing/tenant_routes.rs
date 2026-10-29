//! Explicit deployment routing for organization-isolated Visibility owners.
//! Never derive a destination from an org ID or accept browser scoping headers.
use super::*;
use serde::Deserialize;
use std::{collections::HashSet,path::Path};
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Tenant {pub organization_id:String,pub url:String,pub service_key:String}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Routes {version:u8,tenants:Vec<Tenant>}
fn valid(t:&Tenant)->bool {
 let Ok(u)=url::Url::parse(&t.url) else{return false};
 !t.organization_id.is_empty()&&t.organization_id.len()<=200&&t.organization_id.bytes().all(|c|c.is_ascii_alphanumeric()||b"._:-".contains(&c))
 &&t.service_key.len()>=32&&t.service_key.len()<=512&&!t.service_key.chars().any(char::is_whitespace)
 &&u.username().is_empty()&&u.password().is_none()&&u.query().is_none()&&u.fragment().is_none()&&u.path()=="/"
 &&(u.scheme()=="https"||u.scheme()=="http"&&u.host_str().is_some_and(|h|matches!(h,"localhost"|"127.0.0.1"|"marketing-core"|"visibility-core")||h.starts_with("visibility-tenant-")))
}
fn read(path:&Path)->Result<Vec<Tenant>,()> {
 if !path.is_absolute(){return Err(())};let meta=std::fs::symlink_metadata(path).map_err(|_|())?;
 if !meta.file_type().is_file()||meta.len()>65536{return Err(())};
 #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;if meta.permissions().mode()&0o077!=0{return Err(())}}
 let raw=std::fs::read(path).map_err(|_|())?;if raw.len()>65536{return Err(())};let v:Routes=serde_json::from_slice(&raw).map_err(|_|())?;if v.version!=1||v.tenants.len()>64{return Err(())};let mut seen=HashSet::new();if v.tenants.iter().any(|t|!valid(t)||!seen.insert(t.organization_id.clone())){return Err(())};Ok(v.tenants)
}
pub(super) fn resolve(state:&AppState,org:Option<&str>)->Result<Tenant,()> {
 let org=org.ok_or(())?;
 if let Some(path)=std::env::var_os("VISIBILITY_TENANT_ROUTES_FILE") {let routes=read(Path::new(&path))?;if let Some(t)=routes.into_iter().find(|t|t.organization_id==org){return Ok(t)}}
 if state.marketing_core_organization_id.is_empty()||state.marketing_core_service_key.is_empty(){return Err(())};
 Ok(Tenant{organization_id:state.marketing_core_organization_id.clone(),url:state.marketing_core_url.clone(),service_key:state.marketing_core_service_key.clone()})
}
#[cfg(test)]mod tests {
 use super::*;
 #[test]fn tenant_routes_are_explicit_and_never_allow_browser_destinations(){let t=Tenant{organization_id:"org-one".into(),url:"http://visibility-tenant-one:3165/".into(),service_key:"k".repeat(32)};assert!(valid(&t));for url in ["http://169.254.169.254/","http://example.com/","https://x.example/path","https://user:password@x.example/","https://x.example/?org=other"]{let mut bad=t.clone();bad.url=url.into();assert!(!valid(&bad))}}
 #[test]fn routing_files_require_private_permissions_and_unique_scopes(){let root=std::env::temp_dir().join(format!("visibility-routing-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));std::fs::create_dir(&root).unwrap();let file=root.join("routes.json");let row=json!({"organization_id":"org-one","url":"http://visibility-tenant-one:3165/","service_key":"k".repeat(32)});std::fs::write(&file,serde_json::to_vec(&json!({"version":1,"tenants":[row.clone()]})).unwrap()).unwrap();#[cfg(unix)]{use std::os::unix::fs::PermissionsExt;std::fs::set_permissions(&file,std::fs::Permissions::from_mode(0o600)).unwrap()};assert_eq!(read(&file).unwrap().len(),1);std::fs::write(&file,serde_json::to_vec(&json!({"version":1,"tenants":[row.clone(),row]})).unwrap()).unwrap();assert!(read(&file).is_err());std::fs::remove_file(&file).unwrap();std::fs::remove_dir(&root).unwrap()}
}
