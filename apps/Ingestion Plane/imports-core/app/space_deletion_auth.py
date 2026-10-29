"""Narrow authorization for the Ingestion Space-deletion adapter."""

from app.auth_middleware import AuthContext


def is_space_deletion_principal(auth: AuthContext) -> bool:
    """Only the dedicated Control deletion workload may cancel Space jobs."""
    return (
        auth.principal_type == "service"
        and auth.service_name == "control-space-deletion"
        and auth.has_scope("imports:space-delete")
    )
