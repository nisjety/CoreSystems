import os

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


edit(ROOT + "/apps/Frontend Plane/verevonv3/apps/gateway/tests/space_routes_zdr_classes.rs", [
    ('''    let problems = unclassified(&routes, &surface);

    // DEPTH-sc2-2: routes the Space slices registered without adding their rows to the golden.
    // Each stays listed until its owner adds the row; the guard below says when to remove it.
    let known_unregistered = known_unregistered();
    for known in &known_unregistered {
        assert!(
            problems.iter().any(|line| *line == format!("{known} has no row")),
            "{known} now has a row: remove it from known_unregistered (DEPTH-sc2-2)"
        );
    }
    let unexpected: Vec<&String> = problems
        .iter()
        .filter(|line| !known_unregistered.iter().any(|known| **line == format!("{known} has no row")))
        .collect();
    assert!(
        unexpected.is_empty(),
        "Space routes without a classified row in space-surface.v1.json (add the row to \\
         api/contracts, re-vendor and re-pin):\\n{unexpected:#?}"
    );
}

/// The routes of this tree that have no row yet (DEPTH-sc2-2): the WS01-S4 human posts, the WS04
/// room details and member role routes, the WS07-S9 artifacts route and the WS02-S4 event stream.
fn known_unregistered() -> Vec<String> {
    KNOWN_UNREGISTERED.iter().map(|id| (*id).to_owned()).collect()
}

const KNOWN_UNREGISTERED: &[&str] = &[
    "bff:POST:/api/v1/spaces/{space_ref}/threads",
    "bff:POST:/api/v1/spaces/{space_ref}/threads/{thread_id}/messages",
    "bff:PATCH:/api/v1/spaces/{space_ref}",
    "bff:PATCH:/api/v1/spaces/{space_ref}/members/{member_id}",
    "bff:GET:/api/v1/spaces/{space_ref}/artifacts",
    "bff:GET:/api/v1/spaces/{space_ref}/events",
];
''',
     '''    let problems = unclassified(&routes, &surface);
    assert!(
        problems.is_empty(),
        "Space routes without a classified row in space-surface.v1.json (add the row to \\
         api/contracts, re-vendor and re-pin):\\n{problems:#?}"
    );
}
'''),
])

edit(ROOT + "/apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs", [
    ('''        let missing = unclassified(&routes, &surface);
        // DEPTH-sc2-1: WS01-S4 registered the two human-post routes (create_space_post,
        // append_space_post) without adding their rows to the golden. They stay listed here until
        // the owner adds the rows (then the guard below tells the next person to empty the list).
        const KNOWN_UNREGISTERED: &[&str] = &[
            "mg:POST:/v1/threads",
            "mg:POST:/v1/threads/:thread_id/messages",
        ];
        for known in KNOWN_UNREGISTERED {
            assert!(
                missing.iter().any(|line| *line == format!("{known} has no row")),
                "{known} now has a row: remove it from KNOWN_UNREGISTERED (DEPTH-sc2-1)"
            );
        }
        let unexpected: Vec<&String> = missing
            .iter()
            .filter(|line| {
                !KNOWN_UNREGISTERED
                    .iter()
                    .any(|known| **line == format!("{known} has no row"))
            })
            .collect();
        assert!(
            unexpected.is_empty(),
            "non-GET model-gateway routes without a classified row in space-surface.v1.json \\
             (add the row to api/contracts, re-vendor and re-pin):\\n{unexpected:#?}"
        );
''',
     '''        let missing = unclassified(&routes, &surface);
        assert!(
            missing.is_empty(),
            "non-GET model-gateway routes without a classified row in space-surface.v1.json \\
             (add the row to api/contracts, re-vendor and re-pin):\\n{missing:#?}"
        );
'''),
])
print("lints edited")
