import os

MG = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Model Plane/rust/services/model-gateway/src"


def edit(path, pairs):
    b = open(path, "rb").read().decode("utf-8")
    crlf = "\r\n" in b
    t = b.replace("\r\n", "\n")
    for old, new in pairs:
        assert t.count(old) == 1, (path, t.count(old), old[:90])
        t = t.replace(old, new)
    if crlf:
        t = t.replace("\n", "\r\n")
    open(path, "wb").write(t.encode("utf-8"))


# 1. the allowlist gained `work` (WS09-S3, commit 7f6f5cdab); the test still described the earlier list
edit(os.path.join(MG, "room_metadata.rs"), [(
    """    #[test]
    fn the_allowlist_is_artifacts_and_the_retrieval_decline_until_a_later_slice_adds_work() {
        assert_eq!(ROOM_VISIBLE_METADATA_KEYS, ["artifacts", "retrievalDeclined"]);
""",
    """    #[test]
    fn the_allowlist_is_artifacts_the_retrieval_decline_and_the_content_free_work_summary() {
        assert_eq!(ROOM_VISIBLE_METADATA_KEYS, ["artifacts", "retrievalDeclined", "work"]);
""",
)])

# 2. the live view refuses a ZDR caller through the posture module, failing closed: a posture that cannot
#    be decided is refused the same way, before any work
edit(os.path.join(MG, "space_routes", "runs.rs"), [(
    """    if claims.zdr {
        count_attach("zdr");
        return (
            StatusCode::FORBIDDEN,
            Json(Refusal::feature_not_supported("agent_run").json_body()),
        )
            .into_response();
    }
""",
    """    // A posture that cannot be decided is refused like a ZDR one (fail closed).
    let zero_retention = crate::zdr_posture::zdr_or_refuse(&claims, &crate::zdr_posture::Route::plain(false))
        .unwrap_or(true);
    if zero_retention {
        count_attach("zdr");
        return (
            StatusCode::FORBIDDEN,
            Json(Refusal::feature_not_supported("agent_run").json_body()),
        )
            .into_response();
    }
""",
)])
print("rust edits applied")
