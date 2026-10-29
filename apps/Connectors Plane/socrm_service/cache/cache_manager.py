"""Cache manager for SuperOffice data."""

import json
import logging
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any, Dict, Optional

logger = logging.getLogger(__name__)


class CacheManager:
    """Manage file-based caching for SuperOffice data."""
    
    def __init__(self, cache_dir: Path, ttl: int = 3600):
        """Initialize cache manager.
        
        Args:
            cache_dir: Directory for cache files
            ttl: Time to live in seconds (default: 1 hour)
        """
        self.cache_dir = Path(cache_dir)
        self.ttl = ttl
        self.cache_dir.mkdir(parents=True, exist_ok=True)
    
    def _get_cache_path(self, key: str) -> Path:
        """Get cache file path for key."""
        safe_key = key.replace("/", "_").replace(":", "_")
        return self.cache_dir / f"{safe_key}.json"
    
    def get(self, key: str) -> Optional[Any]:
        """Get value from cache."""
        cache_path = self._get_cache_path(key)
        
        if not cache_path.exists():
            return None
        
        try:
            with cache_path.open("r", encoding="utf-8") as f:
                cache_data = json.load(f)
            
            # Check if cache is expired
            cached_at = datetime.fromisoformat(cache_data["cached_at"])
            if datetime.utcnow() > cached_at + timedelta(seconds=self.ttl):
                logger.debug(f"Cache expired for key: {key}")
                cache_path.unlink()
                return None
            
            logger.debug(f"Cache hit for key: {key}")
            return cache_data["data"]
        
        except (json.JSONDecodeError, KeyError, ValueError) as e:
            logger.warning(f"Failed to read cache for key {key}: {e}")
            return None
    
    def set(self, key: str, value: Any) -> bool:
        """Set value in cache."""
        cache_path = self._get_cache_path(key)
        
        cache_data = {
            "data": value,
            "cached_at": datetime.utcnow().isoformat()
        }
        
        try:
            with cache_path.open("w", encoding="utf-8") as f:
                json.dump(cache_data, f, indent=2)
            
            logger.debug(f"Cached data for key: {key}")
            return True
        
        except (IOError, TypeError) as e:
            logger.error(f"Failed to write cache for key {key}: {e}")
            return False
    
    def delete(self, key: str) -> bool:
        """Delete value from cache."""
        cache_path = self._get_cache_path(key)
        
        if cache_path.exists():
            try:
                cache_path.unlink()
                logger.debug(f"Deleted cache for key: {key}")
                return True
            except OSError as e:
                logger.error(f"Failed to delete cache for key {key}: {e}")
                return False
        
        return False
    
    def clear(self) -> int:
        """Clear all cached data."""
        count = 0
        
        for cache_file in self.cache_dir.glob("*.json"):
            try:
                cache_file.unlink()
                count += 1
            except OSError as e:
                logger.error(f"Failed to delete cache file {cache_file}: {e}")
        
        logger.info(f"Cleared {count} cache files")
        return count
    
    def cleanup_expired(self) -> int:
        """Clean up expired cache entries."""
        count = 0
        
        for cache_file in self.cache_dir.glob("*.json"):
            try:
                with cache_file.open("r", encoding="utf-8") as f:
                    cache_data = json.load(f)
                
                cached_at = datetime.fromisoformat(cache_data["cached_at"])
                if datetime.utcnow() > cached_at + timedelta(seconds=self.ttl):
                    cache_file.unlink()
                    count += 1
            
            except (json.JSONDecodeError, KeyError, ValueError, OSError) as e:
                logger.warning(f"Failed to process cache file {cache_file}: {e}")
        
        if count > 0:
            logger.info(f"Cleaned up {count} expired cache entries")
        
        return count
