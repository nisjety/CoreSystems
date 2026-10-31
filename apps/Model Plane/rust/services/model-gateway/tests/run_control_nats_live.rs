//! Run control over a real NATS broker (build plan WS04-S1). Opt-in: `#[ignore]`, needs
//! `NATS_URL`, and the credentials the gateway itself uses (`NATS_USER`, `NATS_PASSWORD`) when the
//! broker has the ACL on. The lead runs it against the model plane's NATS after the ACL is
//! deployed:
//!
//! `cd "apps/Model Plane/rust" && cargo test -q -p model-gateway --test run_control_nats_live -- --ignored`
//!
//! What it settles that the loopback tests cannot: whether async-nats 0.49.1 against the deployed
//! server answers a request to a subject nobody listens on with "no responders" (an immediate
//! `NoOwner`) rather than a timeout, because the gateway turns that difference into a 404 versus a
//! retryable 503. It also proves the ACL by effect: a request, a reply and a throwaway
//! subscription all have to work for the boot self-test to pass.

use std::time::{Duration, Instant};

use model_gateway::nats_connection;
use model_gateway::run_control::{
    RunControl, RunControlError, RunControlMsg, RunControlOp, RunControlReply, RunControlStatus,
    RunControlSubject,
};

fn unique(prefix: &str) -> String {
    format!("{prefix}-{:016x}", rand::random::<u64>())
}

/// A gateway-shaped handle on its own connection, so two of them are two replicas.
async fn replica(timeout: Duration) -> RunControl {
    let url = std::env::var("NATS_URL").expect("NATS_URL names the broker under test");
    let client = nats_connection::connect(&url)
        .await
        .expect("connect to the broker with the gateway's credentials");
    RunControl::nats(client, timeout)
}

fn cancel(request_id: &str) -> (RunControlSubject, RunControlMsg) {
    let mut msg = RunControlMsg::new(RunControlOp::Cancel, "org-live", "user-live");
    request_id.clone_into(&mut msg.request_id);
    (
        RunControlSubject::request(request_id, RunControlOp::Cancel).expect("subject"),
        msg,
    )
}

async fn listen_for(
    control: &RunControl,
    request_id: &str,
) -> model_gateway::run_control::RemoteListener {
    control
        .listen(
            &[RunControlSubject::request_any_op(request_id).expect("subject")],
            |msg| {
                if msg.org_id == "org-live" && msg.user_id == "user-live" {
                    RunControlReply::found()
                } else {
                    RunControlReply::not_found()
                }
            },
        )
        .await
        .expect("subscribe to the request subjects")
}

#[tokio::test]
#[ignore = "needs a NATS broker: NATS_URL, and NATS_USER/NATS_PASSWORD when the ACL is on"]
async fn a_request_to_nobody_is_no_owner_and_not_a_timeout() {
    let requester = replica(Duration::from_secs(3)).await;
    let (subject, msg) = cancel(&unique("nobody"));

    let started = Instant::now();
    let answer = requester.request(&subject, &msg).await;

    assert_eq!(
        answer,
        Err(RunControlError::NoOwner),
        "no responders must map to NoOwner"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "no-responders answers at once; waiting out the timeout means the server did not say so ({:?})",
        started.elapsed()
    );
}

#[tokio::test]
#[ignore = "needs a NATS broker: NATS_URL, and NATS_USER/NATS_PASSWORD when the ACL is on"]
async fn a_request_crosses_to_the_other_replica_and_the_answer_comes_back() {
    let (owner_replica, requester) = (
        replica(Duration::from_secs(3)).await,
        replica(Duration::from_secs(3)).await,
    );
    let request_id = unique("stream");
    let _owner = listen_for(&owner_replica, &request_id).await;

    let (subject, msg) = cancel(&request_id);
    let reply = requester
        .request(&subject, &msg)
        .await
        .expect("the owner answers");
    assert!(reply.found);

    let mut foreign = msg.clone();
    foreign.org_id = "another-org".into();
    let reply = requester
        .request(&subject, &foreign)
        .await
        .expect("answered");
    assert!(
        !reply.found,
        "a caller that is not the stream's owner is told what a missing stream says"
    );
}

#[tokio::test]
#[ignore = "needs a NATS broker: NATS_URL, and NATS_USER/NATS_PASSWORD when the ACL is on"]
async fn dropping_the_listener_stops_the_owner_answering() {
    let (owner_replica, requester) = (
        replica(Duration::from_secs(3)).await,
        replica(Duration::from_secs(3)).await,
    );
    let request_id = unique("stream");
    let listener = listen_for(&owner_replica, &request_id).await;
    let (subject, msg) = cancel(&request_id);
    assert!(
        requester
            .request(&subject, &msg)
            .await
            .expect("owner answers")
            .found
    );

    drop(listener);
    // The unsubscribe is sent from an aborted task, so it takes a moment to reach the server.
    let mut answer = requester.request(&subject, &msg).await;
    for _ in 0..20 {
        if answer == Err(RunControlError::NoOwner) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        answer = requester.request(&subject, &msg).await;
    }
    assert_eq!(answer, Err(RunControlError::NoOwner));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs a NATS broker: NATS_URL, and NATS_USER/NATS_PASSWORD when the ACL is on"]
async fn an_owner_that_does_not_answer_in_time_is_a_timeout() {
    let (owner_replica, requester) = (
        replica(Duration::from_secs(3)).await,
        replica(Duration::from_millis(300)).await,
    );
    let request_id = unique("stuck");
    let _owner = owner_replica
        .listen(
            &[RunControlSubject::request_any_op(&request_id).expect("subject")],
            |_| {
                std::thread::sleep(Duration::from_millis(1_500));
                RunControlReply::found()
            },
        )
        .await
        .expect("subscribe");

    let (subject, msg) = cancel(&request_id);
    assert_eq!(
        requester.request(&subject, &msg).await,
        Err(RunControlError::Timeout)
    );
}

#[tokio::test]
#[ignore = "needs a NATS broker: NATS_URL, and NATS_USER/NATS_PASSWORD when the ACL is on"]
async fn the_boot_self_test_passes_against_the_deployed_acl() {
    let checked = replica(Duration::from_secs(3)).await.self_tested().await;
    assert_eq!(
        checked.status(),
        RunControlStatus::Ok,
        "the self-test failed: is the mp.v1.runctl.> grant of nats.conf deployed? \
         look for 'Permissions Violation' in the broker log"
    );
    assert!(checked.enabled());
}
