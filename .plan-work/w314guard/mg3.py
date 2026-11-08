import sys
sys.path.insert(0, r'C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\w314guard')
from edit import edit

STUB_OLD = """            Err(tonic::Status::unimplemented("get_quarantine_preview is not served by this double"))
        }
"""
STUB_NEW = STUB_OLD + """
        async fn get_approval_preview(
            &self,
            _: tonic::Request<mp_contracts::model_plane::v1::GetApprovalPreviewRequest>,
        ) -> Result<tonic::Response<mp_contracts::model_plane::v1::GetApprovalPreviewResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_approval_preview is not served by this double"))
        }
"""
edit('execution-core/src/browser_events.rs', [(STUB_OLD, STUB_NEW, 1)])
edit('execution-core/src/runtime_loop/agent.rs', [(STUB_OLD, STUB_NEW, 1)])
edit('model-gateway/src/http_routes.rs', [(
"""        "rule_id": empty_to_null(&cause.rule_id),
        "scope_options": offered_scope_options(cause),
""", """        "rule_id": empty_to_null(&cause.rule_id),
        "assist_kind": empty_to_null(&cause.assist_kind),
        "assist_origin": empty_to_null(&cause.assist_origin),
        "host": empty_to_null(&cause.host),
        "scope_options": offered_scope_options(cause),
""", 1)])
