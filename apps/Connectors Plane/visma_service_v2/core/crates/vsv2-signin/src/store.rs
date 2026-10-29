//! The sign-in on disk: who signed in, and one record per connected company.
//!
//! One file per company, so refreshing the tokens of one company never rewrites another's and two
//! processes refreshing different companies cannot lose each other's new refresh token. Every write is a
//! temporary file renamed over the target, so a reader never sees half a file. Directories are 0700 and
//! files 0600 (the same protection as an SSH key); the tokens are not encrypted at rest.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Result, SignInError};
use vsv2_common::text::one_line;

const FORMAT: u32 = 1;
const MAX_LABEL_CHARS: usize = 100;
const SESSION_FILE: &str = "user.json";
const CONNECTIONS_DIR: &str = "connections";

/// Who is signed in on this computer. There is one person at a time.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub version: u32,
    /// Visma Connect's id for the person (the `sub` claim).
    pub sub: String,
    /// A name or e-mail address for messages and the audit log, when Visma gave one.
    pub label: Option<String>,
    /// The application the sign-in belongs to: refresh tokens are bound to it.
    pub client_id: String,
    pub signed_in_at: DateTime<Utc>,
}

impl Session {
    /// The label is text from outside (a display name or an e-mail address): it is made one bounded line.
    pub fn new(sub: String, label: Option<String>, client_id: String) -> Self {
        let label = label
            .map(|l| one_line(&l, MAX_LABEL_CHARS))
            .filter(|l| !l.is_empty());
        Self {
            version: FORMAT,
            sub,
            label,
            client_id,
            signed_in_at: Utc::now(),
        }
    }

    /// What to call this person: the label if there is one, else the id.
    pub fn display(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.sub)
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("sub", &self.sub)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

/// One company the person has approved, with the tokens for it.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Connection {
    pub version: u32,
    pub tenant_id: String,
    /// The person these tokens belong to (the session's `sub`): tokens of anyone else are never used.
    pub sub: String,
    #[serde(default)]
    pub name: Option<String>,
    pub scopes: Vec<String>,
    pub refresh_token: String,
    pub access_token: String,
    pub access_expires_at: DateTime<Utc>,
    pub connected_at: DateTime<Utc>,
}

impl Connection {
    pub fn new(
        tenant_id: String,
        sub: String,
        scopes: Vec<String>,
        refresh_token: String,
        access_token: String,
        access_expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            version: FORMAT,
            tenant_id,
            sub,
            name: None,
            scopes,
            refresh_token,
            access_token,
            access_expires_at,
            connected_at: Utc::now(),
        }
    }

    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// Whether the access token is still good for `margin_secs` more seconds.
    pub fn access_is_fresh(&self, margin_secs: i64) -> bool {
        self.access_expires_at > Utc::now() + chrono::Duration::seconds(margin_secs)
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("tenant_id", &self.tenant_id)
            .field("scopes", &self.scopes)
            .field("tokens", &"<hidden>")
            .field("access_expires_at", &self.access_expires_at)
            .finish_non_exhaustive()
    }
}

/// A company id becomes a file name, so it must be a plain identifier (Visma's are UUIDs).
pub fn valid_tenant_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

fn io_err(what: &str, path: &Path, e: std::io::Error) -> SignInError {
    SignInError::Store(format!("{what} {}: {e}", path.display()))
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn session_path(&self) -> PathBuf {
        self.dir.join(SESSION_FILE)
    }

    fn connections_dir(&self) -> PathBuf {
        self.dir.join(CONNECTIONS_DIR)
    }

    fn connection_path(&self, tenant_id: &str) -> PathBuf {
        self.connections_dir().join(format!("{tenant_id}.json"))
    }

    pub fn session(&self) -> Result<Option<Session>> {
        read_json(&self.session_path())
    }

    pub fn save_session(&self, session: &Session) -> Result<()> {
        self.ensure_dirs()?;
        write_json(&self.session_path(), session)
    }

    /// Every connected company, ordered by id. A file that cannot be read is skipped with a warning
    /// rather than hiding the rest.
    pub fn connections(&self) -> Result<Vec<Connection>> {
        let dir = self.connections_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_err("reading", &dir, e)),
        };
        let mut out = Vec::new();
        for entry in entries {
            let path = entry.map_err(|e| io_err("reading", &dir, e))?.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if name.starts_with('.') || path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match read_json::<Connection>(&path) {
                Ok(Some(connection)) => out.push(connection),
                Ok(None) => {}
                Err(e) => tracing::warn!("skipping an unreadable sign-in file: {e}"),
            }
        }
        out.sort_by(|a, b| a.tenant_id.cmp(&b.tenant_id));
        Ok(out)
    }

    pub fn connection(&self, tenant_id: &str) -> Result<Option<Connection>> {
        if !valid_tenant_id(tenant_id) {
            return Ok(None);
        }
        read_json(&self.connection_path(tenant_id))
    }

    pub fn save_connection(&self, connection: &Connection) -> Result<()> {
        if !valid_tenant_id(&connection.tenant_id) {
            return Err(SignInError::BadToken(format!(
                "company id {:?} is not a plain identifier",
                connection.tenant_id
            )));
        }
        self.ensure_dirs()?;
        write_json(&self.connection_path(&connection.tenant_id), connection)
    }

    /// Forget one company. True if it was connected.
    pub fn remove_connection(&self, tenant_id: &str) -> Result<bool> {
        if !valid_tenant_id(tenant_id) {
            return Ok(false);
        }
        let path = self.connection_path(tenant_id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io_err("removing", &path, e)),
        }
    }

    /// Forget everything: the person and every company. Besides the companies that can be read, this removes the
    /// files this version cannot parse and the temporary files a crash left behind, since they can hold tokens too.
    /// Only the store's own file patterns are touched, never other files in the directory.
    pub fn clear(&self) -> Result<usize> {
        let mut removed = 0;
        for connection in self.connections()? {
            removed += usize::from(self.remove_connection(&connection.tenant_id)?);
        }
        remove_matching(&self.connections_dir(), |name| {
            name.ends_with(".json") || is_temp(name)
        })?;
        remove_matching(&self.dir, |name| name == SESSION_FILE || is_temp(name))?;
        // Empty directories are tidied up, not required to go.
        let _ = fs::remove_dir(self.connections_dir());
        let _ = fs::remove_dir(&self.dir);
        Ok(removed)
    }

    fn ensure_dirs(&self) -> Result<()> {
        for dir in [self.dir.clone(), self.connections_dir()] {
            fs::create_dir_all(&dir).map_err(|e| io_err("creating", &dir, e))?;
            restrict(&dir, 0o700)?;
        }
        Ok(())
    }
}

/// A temporary file of [`write_private`]: `.<name>.<uuid>.tmp`.
fn is_temp(name: &str) -> bool {
    name.starts_with('.') && name.ends_with(".tmp")
}

/// Remove the files directly inside `dir` whose names `wanted` accepts. Directories are never entered.
fn remove_matching(dir: &Path, wanted: impl Fn(&str) -> bool) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_err("reading", dir, e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| io_err("reading", dir, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !wanted(&name) || entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err("removing", &entry.path(), e)),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn mode_of(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .ok()
        .map(|m| m.permissions().mode() & 0o777)
}

/// Make `path` owner-only. On a platform without Unix modes this is a no-op.
fn restrict(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if mode_of(path).is_some_and(|current| current & 0o077 != 0) {
            tracing::warn!(
                "tightening the permissions of {} to owner-only",
                path.display()
            );
        }
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|e| io_err("protecting", path, e))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_err("reading", path, e)),
    };
    #[cfg(unix)]
    if mode_of(path).is_some_and(|mode| mode & 0o077 != 0) {
        restrict(path, 0o600)?;
    }
    // Parse through Value first: the error then never echoes file contents (tokens).
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| {
        SignInError::Store(format!(
            "{} is not valid JSON (line {})",
            path.display(),
            e.line()
        ))
    })?;
    serde_json::from_value(value).map(Some).map_err(|_| {
        SignInError::Store(format!(
            "{} does not have the expected fields",
            path.display()
        ))
    })
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| SignInError::Store(format!("serializing: {e}")))?;
    write_private(path, &bytes)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| SignInError::Store(format!("{} has no directory", path.display())))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let temp = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .map_err(|e| io_err("creating", &temp, e))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written {
        let _ = fs::remove_file(&temp);
        return Err(io_err("writing", &temp, e));
    }
    fs::rename(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        io_err("replacing", path, e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("user"));
        (dir, store)
    }

    fn connection(tenant: &str) -> Connection {
        Connection::new(
            tenant.into(),
            "user-1".into(),
            vec!["a:read".into()],
            format!("refresh-{tenant}"),
            format!("access-{tenant}"),
            Utc::now() + chrono::Duration::hours(1),
        )
    }

    #[test]
    fn an_empty_store_has_nobody_and_no_companies() {
        let (_dir, store) = store();
        assert!(store.session().unwrap().is_none());
        assert!(store.connections().unwrap().is_empty());
        assert!(store.connection("t-1").unwrap().is_none());
    }

    #[test]
    fn the_session_and_the_companies_round_trip_in_id_order() {
        let (_dir, store) = store();
        let session = Session::new(
            "user-1".into(),
            Some("ima@example.com".into()),
            "web-app".into(),
        );
        store.save_session(&session).unwrap();
        for id in ["t-b", "t-a", "t-c"] {
            store.save_connection(&connection(id)).unwrap();
        }
        assert_eq!(store.session().unwrap().unwrap(), session);
        let ids: Vec<String> = store
            .connections()
            .unwrap()
            .into_iter()
            .map(|c| c.tenant_id)
            .collect();
        assert_eq!(ids, ["t-a", "t-b", "t-c"]);
        assert_eq!(
            store.connection("t-b").unwrap().unwrap().refresh_token,
            "refresh-t-b"
        );
        assert_eq!(session.display(), "ima@example.com");
    }

    #[test]
    fn saving_one_company_leaves_the_others_untouched() {
        let (_dir, store) = store();
        store.save_connection(&connection("t-a")).unwrap();
        store.save_connection(&connection("t-b")).unwrap();
        let mut renewed = connection("t-a");
        renewed.refresh_token = "rotated".into();
        store.save_connection(&renewed).unwrap();
        assert_eq!(
            store.connection("t-a").unwrap().unwrap().refresh_token,
            "rotated"
        );
        assert_eq!(
            store.connection("t-b").unwrap().unwrap().refresh_token,
            "refresh-t-b"
        );
    }

    #[test]
    fn a_company_id_that_is_not_a_plain_identifier_is_refused_and_never_touches_the_disk() {
        let (_dir, store) = store();
        for bad in ["", "../x", "a/b", "a b", "t.json", &"x".repeat(65)] {
            assert!(!valid_tenant_id(bad), "{bad:?}");
            assert!(store.save_connection(&connection(bad)).is_err(), "{bad:?}");
            assert!(store.connection(bad).unwrap().is_none());
            assert!(!store.remove_connection(bad).unwrap());
        }
        assert!(valid_tenant_id("33f319b9-4d57-11ee-960c-025417856183"));
    }

    #[test]
    fn removing_a_company_and_clearing_everything() {
        let (_dir, store) = store();
        store
            .save_session(&Session::new("u".into(), None, "c".into()))
            .unwrap();
        store.save_connection(&connection("t-a")).unwrap();
        store.save_connection(&connection("t-b")).unwrap();
        assert!(store.remove_connection("t-a").unwrap());
        assert!(!store.remove_connection("t-a").unwrap());
        assert_eq!(store.clear().unwrap(), 1);
        assert!(store.session().unwrap().is_none() && store.connections().unwrap().is_empty());
        assert!(!store.dir().exists(), "the empty directory is tidied up");
    }

    #[test]
    fn a_corrupt_company_file_is_skipped_and_the_rest_still_load() {
        let (_dir, store) = store();
        store.save_connection(&connection("t-a")).unwrap();
        store.save_connection(&connection("t-b")).unwrap();
        fs::write(store.connection_path("t-a"), b"{ not json").unwrap();
        let ids: Vec<String> = store
            .connections()
            .unwrap()
            .into_iter()
            .map(|c| c.tenant_id)
            .collect();
        assert_eq!(ids, ["t-b"]);
        let e = store.connection("t-a").unwrap_err().to_string();
        assert!(
            e.contains("not valid JSON") && !e.contains("refresh"),
            "{e}"
        );
    }

    #[test]
    fn clearing_removes_files_this_version_cannot_read_and_leftover_temporary_files_but_nothing_else(
    ) {
        let (_dir, store) = store();
        store
            .save_session(&Session::new("u".into(), None, "c".into()))
            .unwrap();
        store.save_connection(&connection("t-a")).unwrap();
        // A file in a format this version cannot parse, a crash's temporary files, and an unrelated file.
        fs::write(
            store.connections_dir().join("t-old.json"),
            br#"{"refresh_token":"old-format-secret"}"#,
        )
        .unwrap();
        fs::write(
            store.connections_dir().join(".t-a.json.123.tmp"),
            br#"{"refresh_token":"half-written"}"#,
        )
        .unwrap();
        fs::write(store.dir().join(".user.json.456.tmp"), b"{}").unwrap();
        fs::write(store.dir().join("notes.txt"), b"not ours").unwrap();
        assert_eq!(store.clear().unwrap(), 1, "one company could be read");
        assert!(
            !store.connections_dir().exists(),
            "nothing of ours is left in the connections directory"
        );
        assert!(
            !store.dir().join("user.json").exists()
                && !store.dir().join(".user.json.456.tmp").exists()
        );
        assert_eq!(
            fs::read(store.dir().join("notes.txt")).unwrap(),
            b"not ours",
            "other files are never touched"
        );
    }

    #[test]
    fn a_label_from_outside_is_one_bounded_line() {
        let session = Session::new(
            "u".into(),
            Some("Ima\r\nApproved by: finance\u{202E}".into()),
            "c".into(),
        );
        assert_eq!(session.display(), "Ima Approved by: finance");
        let long = Session::new("u".into(), Some("x".repeat(500)), "c".into());
        assert_eq!(long.display().chars().count(), 100);
        assert_eq!(
            Session::new("sub-1".into(), Some("  \n ".into()), "c".into()).display(),
            "sub-1"
        );
    }

    #[test]
    fn a_leftover_temporary_file_is_not_a_company() {
        let (_dir, store) = store();
        store.save_connection(&connection("t-a")).unwrap();
        fs::write(store.connections_dir().join(".t-b.json.abc.tmp"), b"{}").unwrap();
        assert_eq!(store.connections().unwrap().len(), 1);
    }

    #[test]
    fn debug_output_hides_every_token() {
        let c = connection("t-a");
        let shown = format!("{c:?}");
        assert!(
            !shown.contains("refresh-t-a") && !shown.contains("access-t-a"),
            "{shown}"
        );
    }

    #[test]
    fn access_freshness_uses_the_margin() {
        let mut c = connection("t-a");
        assert!(c.access_is_fresh(60));
        c.access_expires_at = Utc::now() + chrono::Duration::seconds(30);
        assert!(!c.access_is_fresh(60) && c.access_is_fresh(0));
    }

    #[cfg(unix)]
    #[test]
    fn files_and_directories_are_owner_only_and_loosened_ones_are_tightened_on_read() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, store) = store();
        store
            .save_session(&Session::new("u".into(), None, "c".into()))
            .unwrap();
        store.save_connection(&connection("t-a")).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(store.dir()), 0o700);
        assert_eq!(mode(&store.connections_dir()), 0o700);
        assert_eq!(mode(&store.session_path()), 0o600);
        let file = store.connection_path("t-a");
        assert_eq!(mode(&file), 0o600);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.connection("t-a").unwrap().is_some());
        assert_eq!(mode(&file), 0o600);
    }
}
