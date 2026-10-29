//! The persisted message tree (build plan WS17 S8).
//!
//! A turn can have more than one continuation: a regenerated answer, an edited turn, a
//! rewind. The tree keeps every branch. One chain of it is the active path, from the first
//! turn of the thread to its leaf, and only that chain is the conversation: the transcript,
//! the prompts, learning and Dreaming all read it. Migration 0061 adds the columns; this
//! module owns the rules that move the leaf and keep the path flags in step with it.
//!
//! Nothing is deleted. A rewind moves the leaf back, and the turns beneath it stay on the
//! record with `on_active_path = false`. Every history reader filters on that flag whether
//! or not `SESSION_CORE_MESSAGE_VERSIONS_ENABLED` is on, so a turn on an inactive branch
//! never reaches a prompt, learning or Dreaming.
//!
//! The switch gates the writes only: an append that extends a thread's tree, and
//! `SetActiveLeaf`. Off, appends are byte-identical to before and the tree columns are not
//! written. A Space thread keeps no tree (it keeps append-only revision entries, DEC-29),
//! and so does a thread that holds a waived run (DEC-32).

use mp_contracts::model_plane::v1::{self as pb, message_tree_service_server::MessageTreeService};
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Instant;
use tonic::{Request, Response, Status};

use crate::auth::{authorize_operation, identity, OwnerIntent};
use crate::grpc::{authorize_thread_owner, record_metrics};

/// The environment switch for the tree. Default off, read by the strict parser the other
/// switches share, so only the literal `true` turns it on.
pub(crate) const ENABLED_ENV: &str = "SESSION_CORE_MESSAGE_VERSIONS_ENABLED";

/// Read where it is used rather than once at boot: the append path is a free function with no
/// service to hold a value, and one environment read costs little beside the queries around it.
pub(crate) fn message_versions_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// One turn as the tree rules read it: where it continues, its place in the thread, and
/// whether the active path holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Turn {
    id: String,
    parent: Option<String>,
    sequence: i64,
    on_active_path: bool,
}

/// The turns of one thread, in sequence order.
#[derive(Debug, Default)]
struct Tree {
    turns: Vec<Turn>,
}

impl Tree {
    fn new(turns: Vec<Turn>) -> Self {
        Self { turns }
    }

    fn turn(&self, id: &str) -> Option<&Turn> {
        self.turns.iter().find(|turn| turn.id == id)
    }

    /// The newest turn beneath `id`, one child at a time. A turn is always written after the
    /// turn it continues, so following the newest child reaches a leaf. The walk is bounded
    /// by the turn count, so a corrupt cycle cannot hold a request.
    fn descend_newest_child(&self, id: &str) -> String {
        let mut current = id.to_owned();
        for _ in 0..self.turns.len() {
            let newest = self
                .turns
                .iter()
                .filter(|turn| turn.parent.as_deref() == Some(current.as_str()))
                .max_by_key(|turn| turn.sequence);
            match newest {
                Some(child) => current = child.id.clone(),
                None => break,
            }
        }
        current
    }

    /// The chain from the first turn to `leaf`, oldest first. `None` when `leaf` is not a turn
    /// of this tree, or when its parents do not close into a chain.
    fn path_to(&self, leaf: &str) -> Option<Vec<String>> {
        let mut path = Vec::new();
        let mut current = Some(leaf);
        while let Some(id) = current {
            if path.len() == self.turns.len() {
                return None;
            }
            let turn = self.turn(id)?;
            path.push(turn.id.clone());
            current = turn.parent.as_deref();
        }
        path.reverse();
        Some(path)
    }

    /// The leaf a move to `target` lands on. A target on the active path is a rewind, and is
    /// its own leaf. Any other turn is a version switch, and lands on the newest turn beneath
    /// it, so the chosen branch shows its own continuation.
    fn resolve_leaf(&self, target: &str) -> Option<String> {
        let turn = self.turn(target)?;
        Some(if turn.on_active_path {
            target.to_owned()
        } else {
            self.descend_newest_child(target)
        })
    }
}

/// The message tree is written for this write when the switch is on, the thread is not a
/// Space thread and the thread holds no waived run. Read under the thread lock.
pub(crate) async fn tree_applies(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
    is_space: bool,
) -> Result<bool, Status> {
    if is_space || !message_versions_enabled() {
        return Ok(false);
    }
    Ok(!thread_is_waived(tx, thread_id).await?)
}

/// The thread's active leaf, after a legacy chain is linked when the leaf is missing or stale.
/// A turn written while the switch was off has no parent and sits after the leaf, so the leaf
/// is stale. Called under the thread lock, before a turn is added or the leaf moves.
pub(crate) async fn current_leaf(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<Option<String>, Status> {
    let leaf = read_leaf(tx, thread_id).await?;
    let stale: bool = match &leaf {
        None => sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM messages WHERE thread_id = $1)")
            .bind(thread_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(internal)?,
        Some(leaf_id) => sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM messages
                 WHERE thread_id = $1 AND parent_message_id IS NULL
                   AND sequence > (SELECT sequence FROM messages WHERE id = $2)
             )",
        )
        .bind(thread_id)
        .bind(leaf_id.as_str())
        .fetch_one(&mut **tx)
        .await
        .map_err(internal)?,
    };
    if !stale {
        return Ok(leaf);
    }
    link_chain(tx, thread_id).await?;
    read_leaf(tx, thread_id).await
}

/// Places a turn that was just appended under the thread's leaf: `parent` is the leaf read
/// before the insert. The turn's sibling ordinal is counted in the same statement, and the turn
/// becomes the leaf. Called inside the append's transaction.
pub(crate) async fn record_append(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
    message_id: &str,
    parent: Option<&str>,
) -> Result<(), Status> {
    sqlx::query(
        "UPDATE messages
         SET parent_message_id = $2::TEXT,
             sibling_ordinal = (
                 SELECT COUNT(*)::INTEGER + 1 FROM messages AS s
                 WHERE s.thread_id = $1 AND s.id <> $3
                   AND s.parent_message_id IS NOT DISTINCT FROM $2::TEXT
             )
         WHERE id = $3",
    )
    .bind(thread_id)
    .bind(parent)
    .bind(message_id)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    sqlx::query("UPDATE threads SET active_leaf_message_id = $2 WHERE id = $1")
        .bind(thread_id)
        .bind(message_id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    Ok(())
}

/// Links a thread's legacy chain once. Every active turn without a parent continues the active
/// turn before it, the sibling ordinals are counted from those links, and the leaf becomes the
/// newest active turn. Idempotent: a thread that is already linked changes nothing.
async fn link_chain(tx: &mut Transaction<'_, Postgres>, thread_id: &str) -> Result<(), Status> {
    sqlx::query(
        "WITH path AS (
             SELECT id, parent_message_id,
                    LAG(id) OVER (ORDER BY sequence) AS prev_id
             FROM messages
             WHERE thread_id = $1 AND on_active_path
         )
         UPDATE messages AS m
         SET parent_message_id = path.prev_id
         FROM path
         WHERE m.id = path.id AND path.parent_message_id IS NULL AND path.prev_id IS NOT NULL",
    )
    .bind(thread_id)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    sqlx::query(
        "WITH ranked AS (
             SELECT id, ROW_NUMBER() OVER (PARTITION BY parent_message_id ORDER BY sequence) AS ordinal
             FROM messages
             WHERE thread_id = $1
         )
         UPDATE messages AS m
         SET sibling_ordinal = ranked.ordinal
         FROM ranked
         WHERE m.id = ranked.id AND m.sibling_ordinal <> ranked.ordinal",
    )
    .bind(thread_id)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    sqlx::query(
        "UPDATE threads
         SET active_leaf_message_id = (
             SELECT id FROM messages
             WHERE thread_id = $1 AND on_active_path
             ORDER BY sequence DESC
             LIMIT 1
         )
         WHERE id = $1",
    )
    .bind(thread_id)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(())
}

async fn read_leaf(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<Option<String>, Status> {
    let (leaf,): (Option<String>,) =
        sqlx::query_as("SELECT active_leaf_message_id FROM threads WHERE id = $1")
            .bind(thread_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(internal)?;
    Ok(leaf)
}

async fn load_turns(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<Vec<Turn>, Status> {
    let rows: Vec<(String, Option<String>, i64, bool)> = sqlx::query_as(
        "SELECT id, parent_message_id, sequence, on_active_path
         FROM messages
         WHERE thread_id = $1
         ORDER BY sequence",
    )
    .bind(thread_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(rows
        .into_iter()
        .map(|(id, parent, sequence, on_active_path)| Turn {
            id,
            parent,
            sequence,
            on_active_path,
        })
        .collect())
}

async fn thread_is_waived(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<bool, Status> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM runs WHERE thread_id = $1 AND retention_class = 'zdr_waived')",
    )
    .bind(thread_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)
}

async fn has_active_run(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<bool, Status> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM runs WHERE thread_id = $1 AND status IN ('queued', 'running'))",
    )
    .bind(thread_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)
}

fn internal(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

/// `MessageTreeService`: the move of a thread's active leaf. Served beside `SessionCore` behind the
/// verifying interceptor, so that trait and its mocks are unchanged.
pub struct MessageTreeGrpc {
    pub(crate) pool: PgPool,
}

#[tonic::async_trait]
impl MessageTreeService for MessageTreeGrpc {
    async fn set_active_leaf(
        &self,
        request: Request<pb::SetActiveLeafRequest>,
    ) -> Result<Response<pb::SetActiveLeafResponse>, Status> {
        let started = Instant::now();
        let result = self.set_active_leaf_checked(request).await;
        record_metrics("set_active_leaf", started, result.is_ok());
        result
    }
}

impl MessageTreeGrpc {
    async fn set_active_leaf_checked(
        &self,
        request: Request<pb::SetActiveLeafRequest>,
    ) -> Result<Response<pb::SetActiveLeafResponse>, Status> {
        let caller = identity(&request)?;
        authorize_operation(&caller, "session:write")?;
        let req = request.into_inner();
        if req.org_id.is_empty() || req.thread_id.is_empty() || req.target_message_id.is_empty() {
            return Err(Status::invalid_argument(
                "org_id, thread_id and target_message_id are required",
            ));
        }
        caller.authorize_org(&req.org_id)?;
        if !message_versions_enabled() {
            return Err(Status::unavailable("message_versions_disabled"));
        }
        authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Mutate).await?;
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let response = set_active_leaf_in_tx(&mut tx, &req).await?;
        tx.commit().await.map_err(internal)?;
        Ok(Response::new(response))
    }
}

/// The move itself, under the thread lock: every refusal is decided before any row changes.
async fn set_active_leaf_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    req: &pb::SetActiveLeafRequest,
) -> Result<pb::SetActiveLeafResponse, Status> {
    let (org_id, space_id): (String, Option<String>) =
        sqlx::query_as("SELECT org_id, space_id FROM threads WHERE id = $1 FOR UPDATE")
            .bind(&req.thread_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(internal)?
            .ok_or_else(|| Status::not_found("thread not found"))?;
    if org_id != req.org_id {
        return Err(Status::not_found("thread not found"));
    }
    if space_id.is_some() {
        return Err(Status::failed_precondition(
            "a Space thread keeps no message versions",
        ));
    }
    if thread_is_waived(tx, &req.thread_id).await? {
        return Err(Status::failed_precondition(
            "a thread with a waived run keeps no message versions",
        ));
    }
    if has_active_run(tx, &req.thread_id).await? {
        return Err(Status::failed_precondition("active_run_blocks_switch"));
    }
    let leaf = current_leaf(tx, &req.thread_id).await?;
    if leaf.as_deref().unwrap_or_default() != req.expected_leaf_message_id.as_str() {
        return Err(Status::aborted("active_leaf_stale"));
    }
    let tree = Tree::new(load_turns(tx, &req.thread_id).await?);
    let new_leaf = tree.resolve_leaf(&req.target_message_id).ok_or_else(|| {
        Status::invalid_argument("target_message_id does not name a message in this thread")
    })?;
    let path = tree
        .path_to(&new_leaf)
        .ok_or_else(|| Status::internal("the message tree is not a chain"))?;
    sqlx::query(
        "UPDATE messages
         SET on_active_path = (id = ANY($2::TEXT[]))
         WHERE thread_id = $1 AND on_active_path IS DISTINCT FROM (id = ANY($2::TEXT[]))",
    )
    .bind(&req.thread_id)
    .bind(&path)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    sqlx::query("UPDATE threads SET active_leaf_message_id = $2 WHERE id = $1")
        .bind(&req.thread_id)
        .bind(&new_leaf)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    Ok(pb::SetActiveLeafResponse {
        active_leaf_message_id: new_leaf,
        active_path: path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::VerifiedIdentity;

    const ORG: &str = "org-1";
    const USER: &str = "user-1";

    fn turn(id: &str, parent: Option<&str>, sequence: i64, on_active_path: bool) -> Turn {
        Turn {
            id: id.to_owned(),
            parent: parent.map(str::to_owned),
            sequence,
            on_active_path,
        }
    }

    /// A is the first turn and the active path runs A, C. B answers A, with two continuations
    /// D and E, and F continues E. The branch beneath B is inactive.
    fn branching() -> Tree {
        Tree::new(vec![
            turn("A", None, 1, true),
            turn("B", Some("A"), 2, false),
            turn("C", Some("A"), 3, true),
            turn("D", Some("B"), 4, false),
            turn("E", Some("B"), 5, false),
            turn("F", Some("E"), 6, false),
        ])
    }

    /// The invariants every move keeps: each turn continues an earlier turn that exists, and the
    /// active path is exactly the chain from the first turn to the leaf.
    fn assert_chain(tree: &Tree, leaf: Option<&str>) {
        for turn in &tree.turns {
            if let Some(parent) = &turn.parent {
                let parent_turn = tree.turn(parent).expect("a parent is a turn of the tree");
                assert!(
                    parent_turn.sequence < turn.sequence,
                    "{} continues a later turn",
                    turn.id
                );
            }
        }
        let mut active: Vec<String> = tree
            .turns
            .iter()
            .filter(|turn| turn.on_active_path)
            .map(|turn| turn.id.clone())
            .collect();
        active.sort();
        match leaf {
            None => assert!(active.is_empty(), "no leaf, yet {active:?} is active"),
            Some(leaf) => {
                let mut path = tree.path_to(leaf).expect("the leaf closes into a chain");
                path.sort();
                assert_eq!(active, path, "the active path is the chain to the leaf");
            }
        }
    }

    #[test]
    fn switching_to_an_inactive_turn_lands_on_its_newest_continuation() {
        // B is inactive, and its newest child E continues into F.
        assert_eq!(branching().resolve_leaf("B").as_deref(), Some("F"));
    }

    #[test]
    fn a_rewind_to_an_active_turn_is_its_own_leaf() {
        assert_eq!(branching().resolve_leaf("A").as_deref(), Some("A"));
    }

    #[test]
    fn a_turn_without_a_continuation_is_its_own_leaf() {
        assert_eq!(branching().resolve_leaf("D").as_deref(), Some("D"));
    }

    #[test]
    fn an_unknown_target_resolves_to_nothing() {
        assert_eq!(branching().resolve_leaf("Z"), None);
    }

    #[test]
    fn the_path_is_the_chain_from_the_first_turn_to_the_leaf() {
        assert_eq!(
            branching().path_to("F"),
            Some(vec![
                "A".to_owned(),
                "B".to_owned(),
                "E".to_owned(),
                "F".to_owned()
            ])
        );
    }

    #[test]
    fn a_loop_in_the_parents_is_refused_and_never_walked_forever() {
        let looped = Tree::new(vec![
            turn("X", Some("Y"), 1, true),
            turn("Y", Some("X"), 2, false),
        ]);
        assert_eq!(looped.path_to("X"), None);
        // The walk ends after the turn count and lands on one of the looped turns.
        assert!(matches!(
            looped.descend_newest_child("X").as_str(),
            "X" | "Y"
        ));
    }

    #[test]
    fn random_moves_keep_the_active_path_the_chain_to_the_leaf() {
        // A fixed-seed xorshift, so the walk is the same on every run: appends under the leaf,
        // switches to any turn and rewinds to a turn on the active path.
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut draw = |bound: usize| -> usize {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % bound as u64) as usize
        };
        let mut tree = Tree::default();
        let mut leaf: Option<String> = None;
        for step in 0..200_i64 {
            let action = if leaf.is_none() { 0 } else { draw(3) };
            if action == 0 {
                let id = format!("turn-{step}");
                // An append continues the leaf, which is on the active path, so only the new
                // turn joins it.
                tree.turns.push(turn(&id, leaf.as_deref(), step, true));
                leaf = Some(id);
            } else {
                let candidates: Vec<String> = match (action, leaf.as_deref()) {
                    (1, _) => tree.turns.iter().map(|turn| turn.id.clone()).collect(),
                    (_, Some(current)) => tree.path_to(current).expect("the leaf is a chain"),
                    (_, None) => unreachable!("an empty thread only appends"),
                };
                let target = candidates[draw(candidates.len())].clone();
                let new_leaf = tree.resolve_leaf(&target).expect("a turn of this tree");
                let path = tree
                    .path_to(&new_leaf)
                    .expect("the new leaf closes into a chain");
                for turn in &mut tree.turns {
                    turn.on_active_path = path.contains(&turn.id);
                }
                leaf = Some(new_leaf);
            }
            assert_chain(&tree, leaf.as_deref());
        }
        assert!(tree.turns.len() > 50, "the walk built a real tree");
    }

    #[test]
    fn the_tree_columns_are_added_additively_and_reversed() {
        let up = include_str!("../migrations/0061_message_versions.sql");
        assert!(
            up.contains("ADD COLUMN IF NOT EXISTS on_active_path BOOLEAN NOT NULL DEFAULT TRUE")
        );
        // The header comment talks about dropping an invalid index; only statements count.
        let statements: String = up
            .lines()
            .filter(|line| !line.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !statements.to_uppercase().contains("DROP "),
            "the up migration only adds"
        );
        let down = include_str!("../migrations/revert/0061_message_versions.down.sql");
        assert!(down.contains("DROP COLUMN IF EXISTS on_active_path"));
    }

    /// The source with its `#[cfg(test)]` items removed, so the scan sees only request-path code.
    fn production_text(source: &str) -> String {
        let mut out = String::with_capacity(source.len());
        let mut lines = source.lines();
        while let Some(line) = lines.next() {
            if line.trim() == "#[cfg(test)]" {
                // The attribute gates the item that follows: skip it, brace for brace.
                let mut depth = 0_i32;
                let mut opened = false;
                for item_line in lines.by_ref() {
                    for character in item_line.chars() {
                        match character {
                            '{' => {
                                depth += 1;
                                opened = true;
                            }
                            '}' => depth -= 1,
                            _ => {}
                        }
                    }
                    if opened && depth == 0 {
                        break;
                    }
                }
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    #[test]
    fn every_messages_reader_filters_the_active_path() {
        // A read of `messages` on the request path filters on the active path, or it is one of
        // the writer-side lookups below, which must see every turn: a resend answers with the row
        // it first wrote, a receipt replay finds its own answer, the existence check counts any
        // turn, a cursor resolves to a position and never to content, and a sibling count is how
        // many versions a turn has, inactive ones included. Erasing a thread removes every turn.
        const WRITER_SIDE: [&str; 6] = [
            "SELECT id, sequence, role, content FROM messages",
            "SELECT id FROM messages",
            "SELECT EXISTS(SELECT 1 FROM messages WHERE thread_id = $1)",
            "WHERE m.id = $1 AND m.thread_id = $2",
            "SELECT COUNT(*) FROM messages AS s",
            "DELETE FROM messages",
        ];
        let sources = [
            ("grpc.rs", include_str!("grpc.rs")),
            ("dreaming.rs", include_str!("dreaming.rs")),
            ("message_search.rs", include_str!("message_search.rs")),
        ];
        for (name, source) in sources {
            let text = production_text(source);
            let reads = text
                .match_indices("FROM messages")
                .chain(text.match_indices("JOIN messages"));
            for (at, _) in reads {
                let start = text[..at].rfind('"').map_or(0, |quote| quote + 1);
                let end = text[at..].find('"').map_or(text.len(), |quote| at + quote);
                let literal = &text[start..end];
                let filtered = literal.contains("on_active_path");
                let writer_side = WRITER_SIDE.iter().any(|known| literal.contains(known));
                assert!(
                    filtered || writer_side,
                    "{name}: a read of messages without the active-path filter: {}",
                    literal.trim().lines().next().unwrap_or_default()
                );
            }
        }
    }

    async fn connect() -> PgPool {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed_thread(pool: &PgPool, thread: &str) {
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(thread)
        .bind(ORG)
        .bind(USER)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn append(pool: &PgPool, thread: &str, role: &str, content: &str) -> String {
        let mut tx = pool.begin().await.unwrap();
        let request = pb::AppendMessageRequest {
            thread_id: thread.to_owned(),
            role: role.to_owned(),
            content: content.to_owned(),
            ..Default::default()
        };
        let appended = crate::grpc::append_message_in_tx(&mut tx, &request, USER)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        appended.message_id
    }

    async fn move_leaf(
        pool: &PgPool,
        thread: &str,
        target: &str,
        expected: &str,
    ) -> Result<pb::SetActiveLeafResponse, Status> {
        let mut request = Request::new(pb::SetActiveLeafRequest {
            org_id: ORG.to_owned(),
            thread_id: thread.to_owned(),
            target_message_id: target.to_owned(),
            expected_leaf_message_id: expected.to_owned(),
        });
        request
            .extensions_mut()
            .insert(VerifiedIdentity::user_for_test(ORG, USER));
        MessageTreeGrpc { pool: pool.clone() }
            .set_active_leaf(request)
            .await
            .map(Response::into_inner)
    }

    /// (id, parent, on_active_path) of every turn of the thread, oldest first.
    async fn shape(pool: &PgPool, thread: &str) -> Vec<(String, Option<String>, bool)> {
        sqlx::query_as(
            "SELECT id, parent_message_id, on_active_path FROM messages
             WHERE thread_id = $1 ORDER BY sequence",
        )
        .bind(thread)
        .fetch_all(pool)
        .await
        .unwrap()
    }

    async fn leaf(pool: &PgPool, thread: &str) -> Option<String> {
        sqlx::query_scalar("SELECT active_leaf_message_id FROM threads WHERE id = $1")
            .bind(thread)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn legacy_thread_materializes_chain_once() {
        let pool = connect().await;
        let thread = mp_ids::new_ulid();
        seed_thread(&pool, &thread).await;
        std::env::remove_var(ENABLED_ENV);
        let first = append(&pool, &thread, "user", "one").await;
        let second = append(&pool, &thread, "assistant", "two").await;
        std::env::set_var(ENABLED_ENV, "true");
        let third = append(&pool, &thread, "user", "three").await;
        assert_eq!(leaf(&pool, &thread).await.as_deref(), Some(third.as_str()));
        let linked = shape(&pool, &thread).await;
        assert_eq!(linked[0], (first.clone(), None, true));
        assert_eq!(linked[1], (second.clone(), Some(first.clone()), true));
        assert_eq!(linked[2], (third.clone(), Some(second.clone()), true));
        // A second pass over a linked thread changes nothing.
        let mut tx = pool.begin().await.unwrap();
        assert_eq!(
            current_leaf(&mut tx, &thread).await.unwrap().as_deref(),
            Some(third.as_str())
        );
        tx.commit().await.unwrap();
        assert_eq!(shape(&pool, &thread).await, linked);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn switch_sibling_moves_flags_and_leaf() {
        let pool = connect().await;
        std::env::set_var(ENABLED_ENV, "true");
        let thread = mp_ids::new_ulid();
        seed_thread(&pool, &thread).await;
        let first = append(&pool, &thread, "user", "question").await;
        let answer = append(&pool, &thread, "assistant", "first answer").await;
        let rewound = move_leaf(&pool, &thread, &first, &answer).await.unwrap();
        assert_eq!(rewound.active_leaf_message_id, first);
        let regenerated = append(&pool, &thread, "assistant", "second answer").await;
        // The regenerated answer is a sibling of the first, and the first is now inactive.
        let before = shape(&pool, &thread).await;
        assert_eq!(before[1], (answer.clone(), Some(first.clone()), false));
        assert_eq!(before[2], (regenerated.clone(), Some(first.clone()), true));
        let switched = move_leaf(&pool, &thread, &answer, &regenerated)
            .await
            .unwrap();
        assert_eq!(switched.active_leaf_message_id, answer);
        assert_eq!(switched.active_path, vec![first.clone(), answer.clone()]);
        let after = shape(&pool, &thread).await;
        assert_eq!(after[1], (answer.clone(), Some(first.clone()), true));
        assert_eq!(after[2], (regenerated.clone(), Some(first.clone()), false));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn nested_edit_of_non_final_message_keeps_both_branches() {
        let pool = connect().await;
        std::env::set_var(ENABLED_ENV, "true");
        let thread = mp_ids::new_ulid();
        seed_thread(&pool, &thread).await;
        append(&pool, &thread, "user", "a").await;
        let b = append(&pool, &thread, "assistant", "b").await;
        let c = append(&pool, &thread, "user", "c").await;
        let d = append(&pool, &thread, "assistant", "d").await;
        move_leaf(&pool, &thread, &b, &d).await.unwrap();
        let e = append(&pool, &thread, "user", "edited c").await;
        let shaped = shape(&pool, &thread).await;
        assert!(
            shaped.contains(&(c.clone(), Some(b.clone()), false)),
            "the original branch is kept"
        );
        assert!(
            shaped.contains(&(d.clone(), Some(c.clone()), false)),
            "its continuation is kept"
        );
        assert!(
            shaped.contains(&(e.clone(), Some(b.clone()), true)),
            "the edit is the active branch"
        );
        assert_eq!(leaf(&pool, &thread).await.as_deref(), Some(e.as_str()));
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn rewind_to_ancestor_is_reversible() {
        let pool = connect().await;
        std::env::set_var(ENABLED_ENV, "true");
        let thread = mp_ids::new_ulid();
        seed_thread(&pool, &thread).await;
        let a = append(&pool, &thread, "user", "a").await;
        let b = append(&pool, &thread, "assistant", "b").await;
        let c = append(&pool, &thread, "user", "c").await;
        let rewound = move_leaf(&pool, &thread, &a, &c).await.unwrap();
        assert_eq!(rewound.active_leaf_message_id, a);
        assert_eq!(leaf(&pool, &thread).await.as_deref(), Some(a.as_str()));
        let restored = move_leaf(&pool, &thread, &c, &a).await.unwrap();
        assert_eq!(restored.active_leaf_message_id, c);
        assert_eq!(restored.active_path, vec![a.clone(), b.clone(), c.clone()]);
        assert_eq!(
            shape(&pool, &thread).await.len(),
            3,
            "a rewind deletes nothing"
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn stale_expected_leaf_conflicts() {
        let pool = connect().await;
        std::env::set_var(ENABLED_ENV, "true");
        let thread = mp_ids::new_ulid();
        seed_thread(&pool, &thread).await;
        let a = append(&pool, &thread, "user", "a").await;
        let _b = append(&pool, &thread, "assistant", "b").await;
        let error = move_leaf(&pool, &thread, &a, "not-the-leaf")
            .await
            .unwrap_err();
        assert_eq!(error.code(), tonic::Code::Aborted);
        assert_eq!(error.message(), "active_leaf_stale");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn active_run_blocks_switch() {
        let pool = connect().await;
        std::env::set_var(ENABLED_ENV, "true");
        let thread = mp_ids::new_ulid();
        seed_thread(&pool, &thread).await;
        let a = append(&pool, &thread, "user", "a").await;
        let b = append(&pool, &thread, "assistant", "b").await;
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status)
             VALUES ($1, $2, 'g', $3, $4, 'running')",
        )
        .bind(mp_ids::new_ulid())
        .bind(&thread)
        .bind(ORG)
        .bind(USER)
        .execute(&pool)
        .await
        .unwrap();
        let error = move_leaf(&pool, &thread, &a, &b).await.unwrap_err();
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert_eq!(error.message(), "active_run_blocks_switch");
    }
}
