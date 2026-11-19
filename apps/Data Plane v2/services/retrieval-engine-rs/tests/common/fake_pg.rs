//! A just-enough PostgreSQL server for tests that must see the parameters a query is sent with.
//!
//! The database tests of this crate need a disposable PostgreSQL (`TEST_DATABASE_URL`) and are `#[ignore]`d without
//! one, so a default `cargo test` cannot tell a read that binds the member's history window from one that binds
//! nothing: the SQL text is the same and sqlx binds are typed at run time. This server speaks the extended query
//! protocol over a real socket, so the real handler runs its real sqlx query against it. It records each statement
//! it runs with the parameters the statement was bound to, and answers from a script: rows for a statement a test
//! names, an empty result for any other.
//!
//! It is not a database. It does not parse SQL, check a parameter, or apply a predicate, so it can show that a
//! handler SENT the floor and that a handler applies the rows it was GIVEN; the predicate itself is the SQL, and
//! the gated database tests run that.
//!
//! This file is compiled into the integration tests (`tests/common`) and into the crate's own unit tests (a `#[path]`
//! include from `src/api/mod.rs`), so it names no item of the crate.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use sqlx::PgPool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// The type ids of the two column types the scripts use.
pub const OID_TEXT: u32 = 25;
pub const OID_TIMESTAMPTZ: u32 = 1184;

/// One statement the server ran, with the parameters it was bound to. A parameter is the raw bytes sqlx sent
/// (binary format), or `None` for SQL NULL. The text is whitespace-squashed so a needle does not depend on layout.
#[derive(Clone, Debug)]
pub struct Executed {
    pub sql: String,
    pub params: Vec<Option<Vec<u8>>>,
}

impl Executed {
    /// Parameter `index` (0-based, so `$6` is 5) as the instant a `timestamptz` was bound to; `None` is NULL.
    pub fn instant(&self, index: usize) -> Option<DateTime<Utc>> {
        self.params
            .get(index)
            .unwrap_or_else(|| panic!("`{}` was bound to {} parameters", self.sql, self.params.len()))
            .as_deref()
            .map(decode_timestamptz)
    }

    /// Parameter `index` as text.
    pub fn text(&self, index: usize) -> Option<String> {
        self.params
            .get(index)
            .unwrap_or_else(|| panic!("`{}` was bound to {} parameters", self.sql, self.params.len()))
            .as_deref()
            .map(|bytes| String::from_utf8(bytes.to_vec()).expect("a text parameter"))
    }
}

/// What a statement answers: its result columns (name and type id) and its rows (each value in binary format).
#[derive(Clone, Debug, Default)]
pub struct Reply {
    pub columns: Vec<(&'static str, u32)>,
    pub rows: Vec<Vec<Option<Vec<u8>>>>,
}

/// A text value of a row.
pub fn text(value: &str) -> Option<Vec<u8>> {
    Some(value.as_bytes().to_vec())
}

/// A `timestamptz` value of a row: microseconds since 2000-01-01T00:00:00Z.
pub fn timestamptz(at: DateTime<Utc>) -> Option<Vec<u8>> {
    Some((at - pg_epoch()).num_microseconds().expect("an instant in range").to_be_bytes().to_vec())
}

fn pg_epoch() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap()
}

/// The instant a binary `timestamptz` parameter holds.
pub fn decode_timestamptz(bytes: &[u8]) -> DateTime<Utc> {
    let micros = i64::from_be_bytes(bytes.try_into().expect("a timestamptz parameter is 8 bytes"));
    pg_epoch() + chrono::Duration::microseconds(micros)
}

/// Collapses every run of whitespace to one space, so SQL laid out over several lines compares as one line.
pub fn squashed(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Default)]
struct Shared {
    executed: Mutex<Vec<Executed>>,
    scripts: Mutex<Vec<(String, Reply)>>,
}

pub struct FakePg {
    port: u16,
    shared: Arc<Shared>,
    accept: JoinHandle<()>,
}

impl Drop for FakePg {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

impl FakePg {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind a local port");
        let port = listener.local_addr().expect("local address").port();
        let shared = Arc::new(Shared::default());
        let served = shared.clone();
        let accept = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let shared = served.clone();
                tokio::spawn(async move {
                    // A client that goes away mid-message is not a failure of the test.
                    let _ = serve(socket, shared).await;
                });
            }
        });
        Self { port, shared, accept }
    }

    /// Answers every statement whose (squashed) text contains `needle` with `reply`. The first script that
    /// matches wins; a statement no script matches answers an empty result.
    pub fn answer(&self, needle: &str, reply: Reply) {
        self.shared.scripts.lock().unwrap().push((squashed(needle), reply));
    }

    /// A pool whose connections all go to this server. Connections are opened when first needed.
    pub fn pool(&self) -> PgPool {
        PgPoolOptions::new()
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(10))
            .connect_lazy_with(
                PgConnectOptions::new()
                    .host("127.0.0.1")
                    .port(self.port)
                    .username("fake")
                    .database("fake")
                    .ssl_mode(PgSslMode::Disable),
            )
    }

    /// Every statement run so far, in order.
    pub fn executed(&self) -> Vec<Executed> {
        self.shared.executed.lock().unwrap().clone()
    }

    /// The runs of the statement whose text is `sql` (compared squashed).
    pub fn runs_of(&self, sql: &str) -> Vec<Executed> {
        let wanted = squashed(sql);
        self.executed().into_iter().filter(|run| run.sql == wanted).collect()
    }

    /// The runs of any statement whose text contains `needle` (compared squashed).
    pub fn runs_containing(&self, needle: &str) -> Vec<Executed> {
        let wanted = squashed(needle);
        self.executed().into_iter().filter(|run| run.sql.contains(&wanted)).collect()
    }
}

// ─────────────────────────────── the wire protocol ───────────────────────────────

struct Statement {
    sql: String,
    param_oids: Vec<u32>,
    reply: Option<Reply>,
}

struct Portal {
    sql: String,
    params: Vec<Option<Vec<u8>>>,
    reply: Option<Reply>,
}

/// The bytes of one client message, read front to back.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let (head, tail) = self.bytes.split_at(n);
        self.bytes = tail;
        head
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn i16(&mut self) -> i16 {
        i16::from_be_bytes(self.take(2).try_into().unwrap())
    }
    fn i32(&mut self) -> i32 {
        i32::from_be_bytes(self.take(4).try_into().unwrap())
    }
    fn cstr(&mut self) -> String {
        let end = self.bytes.iter().position(|b| *b == 0).expect("a terminated string");
        let text = String::from_utf8(self.take(end).to_vec()).expect("a UTF-8 string");
        self.take(1);
        text
    }
}

fn put_cstr(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(value.as_bytes());
    out.push(0);
}

/// One backend message: its type byte, its length (which counts itself) and its body.
fn message(out: &mut Vec<u8>, kind: u8, body: &[u8]) {
    out.push(kind);
    out.extend_from_slice(&(body.len() as i32 + 4).to_be_bytes());
    out.extend_from_slice(body);
}

fn ready_for_query(out: &mut Vec<u8>, in_transaction: bool) {
    message(out, b'Z', &[if in_transaction { b'T' } else { b'I' }]);
}

fn row_description(out: &mut Vec<u8>, columns: &[(&'static str, u32)], format: i16) {
    let mut body = (columns.len() as i16).to_be_bytes().to_vec();
    for (name, oid) in columns {
        put_cstr(&mut body, name);
        body.extend_from_slice(&0_i32.to_be_bytes()); // table
        body.extend_from_slice(&0_i16.to_be_bytes()); // column number
        body.extend_from_slice(&oid.to_be_bytes());
        body.extend_from_slice(&(-1_i16).to_be_bytes()); // size
        body.extend_from_slice(&(-1_i32).to_be_bytes()); // modifier
        body.extend_from_slice(&format.to_be_bytes());
    }
    message(out, b'T', &body);
}

async fn read_message(socket: &mut TcpStream) -> std::io::Result<(u8, Vec<u8>)> {
    let kind = socket.read_u8().await?;
    let len = socket.read_i32().await?;
    let mut body = vec![0_u8; (len - 4) as usize];
    socket.read_exact(&mut body).await?;
    Ok((kind, body))
}

async fn serve(mut socket: TcpStream, shared: Arc<Shared>) -> std::io::Result<()> {
    // The startup packet has no type byte. A client that offers TLS is told there is none.
    loop {
        let len = socket.read_i32().await?;
        let mut body = vec![0_u8; (len - 4) as usize];
        socket.read_exact(&mut body).await?;
        if i32::from_be_bytes(body[..4].try_into().unwrap()) == 80_877_103 {
            socket.write_all(b"N").await?;
            continue;
        }
        break;
    }
    let mut out = Vec::new();
    message(&mut out, b'R', &0_i32.to_be_bytes()); // AuthenticationOk
    for (name, value) in [
        ("server_version", "16.4"),
        ("client_encoding", "UTF8"),
        ("integer_datetimes", "on"),
        ("standard_conforming_strings", "on"),
    ] {
        let mut body = Vec::new();
        put_cstr(&mut body, name);
        put_cstr(&mut body, value);
        message(&mut out, b'S', &body);
    }
    let mut key = 1_i32.to_be_bytes().to_vec();
    key.extend_from_slice(&2_i32.to_be_bytes());
    message(&mut out, b'K', &key);
    ready_for_query(&mut out, false);
    socket.write_all(&out).await?;

    let mut statements: HashMap<String, Statement> = HashMap::new();
    let mut portals: HashMap<String, Portal> = HashMap::new();
    let mut in_transaction = false;
    loop {
        let (kind, body) = match read_message(&mut socket).await {
            Ok(message) => message,
            Err(_) => return Ok(()),
        };
        let mut reader = Reader { bytes: &body };
        let mut out = Vec::new();
        match kind {
            // A simple query: the transaction statements, and the comment sqlx pings an idle connection with.
            b'Q' => {
                let sql = reader.cstr();
                let sql = sql.trim();
                if sql.is_empty() || sql.starts_with("/*") || sql.starts_with("--") {
                    message(&mut out, b'I', &[]); // EmptyQueryResponse
                } else {
                    let word = sql.split_whitespace().next().unwrap_or_default().to_ascii_uppercase();
                    match word.as_str() {
                        "BEGIN" | "START" => in_transaction = true,
                        "COMMIT" | "ROLLBACK" | "END" => in_transaction = false,
                        _ => {}
                    }
                    let mut tag = Vec::new();
                    put_cstr(&mut tag, &word);
                    message(&mut out, b'C', &tag);
                }
                ready_for_query(&mut out, in_transaction);
            }
            b'P' => {
                let name = reader.cstr();
                let sql = squashed(&reader.cstr());
                let count = reader.i16();
                let param_oids = (0..count).map(|_| reader.i32() as u32).collect();
                let reply = shared
                    .scripts
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(needle, _)| sql.contains(needle.as_str()))
                    .map(|(_, reply)| reply.clone());
                statements.insert(name, Statement { sql, param_oids, reply });
                message(&mut out, b'1', &[]);
            }
            b'D' => {
                let target = reader.u8();
                let name = reader.cstr();
                let (param_oids, reply, format) = if target == b'S' {
                    let statement = statements.get(&name).expect("a prepared statement");
                    (Some(statement.param_oids.clone()), statement.reply.clone(), 0)
                } else {
                    (None, portals.get(&name).expect("a portal").reply.clone(), 1)
                };
                if let Some(oids) = param_oids {
                    let mut body = (oids.len() as i16).to_be_bytes().to_vec();
                    for oid in oids {
                        body.extend_from_slice(&oid.to_be_bytes());
                    }
                    message(&mut out, b't', &body);
                }
                match reply {
                    Some(reply) if !reply.columns.is_empty() => row_description(&mut out, &reply.columns, format),
                    _ => message(&mut out, b'n', &[]),
                }
            }
            b'B' => {
                let portal = reader.cstr();
                let statement = reader.cstr();
                let formats = reader.i16();
                for _ in 0..formats {
                    reader.i16();
                }
                let count = reader.i16();
                let params = (0..count)
                    .map(|_| match reader.i32() {
                        -1 => None,
                        len => Some(reader.take(len as usize).to_vec()),
                    })
                    .collect();
                let statement = statements.get(&statement).expect("a prepared statement");
                portals.insert(
                    portal,
                    Portal { sql: statement.sql.clone(), params, reply: statement.reply.clone() },
                );
                message(&mut out, b'2', &[]);
            }
            b'E' => {
                let name = reader.cstr();
                let portal = portals.get(&name).expect("a portal");
                shared
                    .executed
                    .lock()
                    .unwrap()
                    .push(Executed { sql: portal.sql.clone(), params: portal.params.clone() });
                let rows = portal.reply.as_ref().map(|reply| reply.rows.clone()).unwrap_or_default();
                for row in &rows {
                    let mut body = (row.len() as i16).to_be_bytes().to_vec();
                    for value in row {
                        match value {
                            Some(bytes) => {
                                body.extend_from_slice(&(bytes.len() as i32).to_be_bytes());
                                body.extend_from_slice(bytes);
                            }
                            None => body.extend_from_slice(&(-1_i32).to_be_bytes()),
                        }
                    }
                    message(&mut out, b'D', &body);
                }
                let mut tag = Vec::new();
                put_cstr(&mut tag, &format!("SELECT {}", rows.len()));
                message(&mut out, b'C', &tag);
            }
            b'C' => {
                let target = reader.u8();
                let name = reader.cstr();
                if target == b'S' {
                    statements.remove(&name);
                } else {
                    portals.remove(&name);
                }
                message(&mut out, b'3', &[]);
            }
            b'S' => ready_for_query(&mut out, in_transaction),
            b'X' => return Ok(()),
            // Flush, and anything a test does not need.
            _ => {}
        }
        if !out.is_empty() {
            socket.write_all(&out).await?;
        }
    }
}
