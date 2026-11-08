p='browser_session.work.rs'
s=open(p,encoding='utf-8',newline='').read()
def r(a,b):
    global s
    assert s.count(a)==1,a
    s=s.replace(a,b)
r("use std::collections::{HashMap, HashSet};\nuse std::sync::{Mutex, OnceLock, PoisonError};\nuse std::time::Duration;\n",
  "use std::collections::{HashMap, HashSet};\nuse std::sync::atomic::{AtomicU64, Ordering};\nuse std::sync::{Mutex, OnceLock, PoisonError};\nuse std::time::{Duration, SystemTime, UNIX_EPOCH};\n")
r("""/// Remove `run_id`'s hold and say whether it had one.
fn take_hold(run_id: &str) -> bool {
    held_runs()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(run_id)
}""","""/// Remove `run_id`'s hold and say whether the person held the page. A wait that ran out closed the
/// page already (EP-1 S5), so it is no hand-back.
fn take_hold(run_id: &str) -> bool {
    matches!(
        held_runs()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(run_id),
        Some(Hold::Person(_))
    )
}""")
open(p,'w',encoding='utf-8',newline='').write(s)
