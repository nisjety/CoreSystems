from app.auth_middleware import AuthContext
from app.space_deletion_auth import is_space_deletion_principal


def test_space_deletion_requires_exact_service_identity_and_scope() -> None:
    allowed = AuthContext(
        org_id="org-1",
        service_name="control-space-deletion",
        principal_type="service",
        scopes=("imports:write", "imports:space-delete"),
    )
    assert is_space_deletion_principal(allowed)
    assert not is_space_deletion_principal(
        AuthContext(
            org_id="org-1",
            service_name="control-space-deletion",
            principal_type="service",
            scopes=("imports:write",),
        )
    )
    assert not is_space_deletion_principal(
        AuthContext(org_id="org-1", user_id="user-1", principal_type="user", scopes=("imports:space-delete",))
    )
