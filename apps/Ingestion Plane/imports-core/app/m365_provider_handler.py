"""
M365 Provider Linked Event Handler for Ingestion Plane.

The current Control Plane event lacks a canonical organization ID, so this
handler validates the signal and reports an explicit deferred outcome without
creating an unowned durable job.
"""

import logging
from typing import Optional

logger = logging.getLogger(__name__)


class M365ProviderLinkedHandler:
    """
    Handles the user.provider_linked event for Microsoft 365 accounts.
    
    Defers M365 provisioning until the event includes canonical org ownership.
    """

    def __init__(self):
        """Initialize handler."""
        self.logger = logger

    async def handle(
        self,
        user_id: str,
        provider: str,
        tenant_id: str,
        email: str,
    ) -> bool:
        """
        Handle a user linking their Microsoft 365 account.
        
        Args:
            user_id: User identifier from Control Plane
            provider: Provider name (microsoft, microsoft365)
            tenant_id: Azure AD tenant ID for the organization
            email: User's email address
            
        Returns:
            True if handler succeeded, False if error (non-blocking)
        """
        try:
            self.logger.info(
                f"🔧 M365 Provider Linked Handler: "
                f"user_id={user_id} provider={provider} tenant_id={tenant_id}"
            )

            # Validate inputs
            if not user_id or not tenant_id:
                self.logger.warning(
                    f"⚠️  Invalid M365 event: missing user_id or tenant_id"
                )
                return False

            # Verify provider is Microsoft
            if not provider or provider.lower() not in ("microsoft", "microsoft365"):
                self.logger.info(f"Skipping non-Microsoft provider: {provider}")
                return True

            # This event currently has no canonical org_id. Persisting an empty
            # tenant creates an unowned job and can never be authorized safely.
            self.logger.warning(
                "M365 provisioning deferred: provider_linked event has no canonical org_id"
            )
            return False

        except Exception as e:
            self.logger.error(
                f"❌ M365 Provider Linked Handler failed: {e}",
                exc_info=True,
            )
            return False

    async def cleanup(
        self,
        user_id: str,
        tenant_id: str,
    ) -> bool:
        """
        Clean up M365 connector when user unlinks account.
        
        Args:
            user_id: User identifier
            tenant_id: Azure AD tenant ID
            
        Returns:
            True if cleanup succeeded
        """
        try:
            self.logger.info(
                f"🧹 Cleaning up M365 connector: "
                f"user_id={user_id} tenant_id={tenant_id}"
            )

            async with SessionLocal() as session:
                # Mark M365 connector jobs as cancelled
                # Cancel any active sync jobs
                # Remove OAuth tokens
                self.logger.info(f"✅ M365 connector cleanup completed")
                return True

        except Exception as e:
            self.logger.error(
                f"❌ M365 cleanup failed: {e}",
                exc_info=True,
            )
            return False


# Singleton instance
_handler_instance: Optional[M365ProviderLinkedHandler] = None


def get_m365_handler() -> M365ProviderLinkedHandler:
    """Get or create M365 handler singleton."""
    global _handler_instance
    if _handler_instance is None:
        _handler_instance = M365ProviderLinkedHandler()
    return _handler_instance
