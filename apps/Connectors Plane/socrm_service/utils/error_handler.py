"""Error handling utilities."""

import logging
from functools import wraps
from typing import Callable, Any

from flask import jsonify

logger = logging.getLogger(__name__)


def handle_errors(func: Callable) -> Callable:
    """Decorator for handling errors in Flask routes."""
    
    @wraps(func)
    def wrapper(*args: Any, **kwargs: Any) -> Any:
        try:
            return func(*args, **kwargs)
        
        except ValueError as e:
            logger.warning(f"Validation error in {func.__name__}: {e}")
            return jsonify({"error": str(e)}), 400
        
        except PermissionError as e:
            logger.warning(f"Permission error in {func.__name__}: {e}")
            return jsonify({"error": "Unauthorized"}), 403
        
        except Exception as e:
            logger.error(f"Unexpected error in {func.__name__}: {e}", exc_info=True)
            return jsonify({"error": "Internal server error"}), 500
    
    return wrapper


class SuperOfficeError(Exception):
    """Base exception for SuperOffice integration errors."""
    pass


class AuthenticationError(SuperOfficeError):
    """Raised when authentication fails."""
    pass


class AuthorizationError(SuperOfficeError):
    """Raised when authorization fails."""
    pass


class APIError(SuperOfficeError):
    """Raised when API request fails."""
    pass


class SyncError(SuperOfficeError):
    """Raised when synchronization fails."""
    pass
