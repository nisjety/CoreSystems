//! The NATS ACL carries what run control needs, and only to the gateway (build plan WS04-S1).
//!
//! Run control is a core-NATS request/reply between gateway replicas. A broker that does not
//! grant the subjects drops the publish without telling the publisher, so a gateway switched on
//! before the ACL would just see every Stop time out. The gateway's boot self-test catches that at
//! runtime; this test catches it before a release: it reads the `deploy/nats.conf` that ships in
//! the artifact and checks each subject the gateway can publish, subscribe to or reply on against
//! the grants of the `model-gateway-runtime` principal, with the same wildcard rules the broker
//! applies.

use model_gateway::run_control::{subject_matches, RunControlOp, RunControlSubject};

const NATS_CONF: &str = include_str!("../../../../deploy/nats.conf");

/// The gateway's reply inbox prefix (`nats_connection.rs`); a reply is published to it, so it
/// needs a publish grant as well as the subscribe grant the requester's own replies arrive on.
const GATEWAY_INBOX: &str = "_INBOX.MODEL_GATEWAY_RUNTIME";

/// The quoted strings of one `publish:` or `subscribe:` list of the principal `user`, comments
/// removed.
fn grants(user: &str, list: &str) -> Vec<String> {
    let conf: String = NATS_CONF
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let start = conf
        .find(&format!("user: \"{user}\""))
        .unwrap_or_else(|| panic!("nats.conf has no principal {user}"));
    let block = &conf[start..];
    // The principal's block ends where the next one starts.
    let block = &block[..block[1..]
        .find("user: \"")
        .map_or(block.len(), |end| end + 1)];
    let list_start = block
        .find(&format!("{list}: ["))
        .unwrap_or_else(|| panic!("{user} has no {list} list"));
    let list_body = &block[list_start..];
    let list_body = &list_body[..list_body.find(']').expect("a closed list")];
    list_body
        .split('"')
        .enumerate()
        .filter(|(index, _)| index % 2 == 1)
        .map(|(_, grant)| grant.to_owned())
        .collect()
}

fn covered(grants: &[String], subject: &str) -> bool {
    grants.iter().any(|grant| subject_matches(grant, subject))
}

#[test]
fn the_gateway_may_publish_and_subscribe_every_run_control_subject() {
    let publish = grants("model-gateway-runtime", "publish");
    let subscribe = grants("model-gateway-runtime", "subscribe");
    let subjects = [
        RunControlSubject::request("req-1", RunControlOp::Cancel),
        RunControlSubject::request("req-1", RunControlOp::Queue),
        RunControlSubject::request("req-1", RunControlOp::Probe),
        RunControlSubject::request_any_op("req-1"),
        RunControlSubject::run_cancel("run_9"),
    ];
    for subject in subjects {
        let subject = subject.expect("a valid subject");
        assert!(
            covered(&publish, subject.as_str()),
            "model-gateway-runtime may not publish {}: add the mp.v1.runctl.> grant to nats.conf",
            subject.as_str()
        );
        assert!(
            covered(&subscribe, subject.as_str()),
            "model-gateway-runtime may not subscribe {}: add the mp.v1.runctl.> grant to nats.conf",
            subject.as_str()
        );
    }
}

#[test]
fn a_reply_can_be_published_to_and_received_on_the_gateways_inbox() {
    let publish = grants("model-gateway-runtime", "publish");
    let subscribe = grants("model-gateway-runtime", "subscribe");
    let reply_subject = format!("{GATEWAY_INBOX}.abc123");
    assert!(
        covered(&publish, &reply_subject),
        "the owner replies to the requester's inbox; without a publish grant on {GATEWAY_INBOX}.> \
         every request times out"
    );
    assert!(covered(&subscribe, &reply_subject));
}

#[test]
fn the_run_control_grant_is_the_one_wildcard_and_no_wider() {
    // `mp.v1.runctl.>` is the whole surface; anything wider (`mp.v1.>`, `>`) would hand the
    // gateway every other subject too.
    for list in ["publish", "subscribe"] {
        let grants = grants("model-gateway-runtime", list);
        assert!(
            grants.iter().any(|grant| grant == "mp.v1.runctl.>"),
            "{list} lacks mp.v1.runctl.>"
        );
        for grant in &grants {
            assert!(
                !matches!(grant.as_str(), ">" | "mp.>" | "mp.v1.>"),
                "{list} grants {grant}, which is wider than run control needs"
            );
        }
    }
}

#[test]
fn no_other_principal_is_granted_run_control() {
    let conf: String = NATS_CONF
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let mut owners = Vec::new();
    for part in conf.split("user: \"").skip(1) {
        // The split already ends each part where the next principal starts.
        if part.contains("mp.v1.runctl") {
            owners.push(part.split('"').next().unwrap_or_default().to_owned());
        }
    }
    assert_eq!(
        owners,
        ["model-gateway-runtime"],
        "run control is between gateway replicas; no other service may publish or subscribe it"
    );
}

#[test]
fn the_wildcard_grant_does_not_reach_other_subject_families() {
    for other in ["mp.v1.runctl", "mp.v1.runX.a", "mp.v1.run.x.event.extra"] {
        assert!(
            !subject_matches("mp.v1.runctl.>", other),
            "{other} must not match the run-control wildcard"
        );
    }
}
