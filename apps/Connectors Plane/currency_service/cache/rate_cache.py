"""
Rate cache manager for currency service.

Implements file-based caching with TTL for currency exchange rates.
"""

import json
import logging
from datetime import datetime, timedelta
from pathlib import Path
from typing import Optional

from config import config

logger = logging.getLogger(__name__)


class RateCache:
    """
    File-based cache for currency rates with TTL support.
    """
    
    def __init__(self, cache_dir: Optional[str] = None, ttl_seconds: Optional[int] = None):
        """
        Initialize rate cache.
        
        Args:
            cache_dir: Cache directory path (default: from config)
            ttl_seconds: Cache TTL in seconds (default: from config)
        """
        self.cache_dir = Path(cache_dir or config.cache_dir)
        self.ttl_seconds = ttl_seconds or config.cache_ttl
        
        # Create cache directory if it doesn't exist
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        
        logger.info(f"Rate cache initialized at {self.cache_dir} (TTL: {self.ttl_seconds}s)")
    
    def get(self, key: str) -> Optional[dict]:
        """
        Get cached rate data.
        
        Args:
            key: Cache key (e.g., "latest_NOK", "USD_NOK_2024-01-15")
            
        Returns:
            Cached data or None if expired/missing
        """
        cache_file = self.cache_dir / f"{key}.json"
        
        if not cache_file.exists():
            logger.debug(f"Cache miss: {key}")
            return None
        
        try:
            with cache_file.open('r') as f:
                cached = json.load(f)
            
            # Check expiry
            cached_at = datetime.fromisoformat(cached["cached_at"])
            age_seconds = (datetime.utcnow() - cached_at).total_seconds()
            
            if age_seconds > self.ttl_seconds:
                logger.debug(f"Cache expired: {key} (age: {age_seconds:.0f}s)")
                cache_file.unlink()
                return None
            
            logger.debug(f"Cache hit: {key} (age: {age_seconds:.0f}s)")
            return cached["data"]
            
        except (json.JSONDecodeError, KeyError, ValueError) as e:
            logger.warning(f"Invalid cache file {key}: {e}")
            cache_file.unlink()
            return None
    
    def set(self, key: str, data: dict) -> None:
        """
        Store data in cache.
        
        Args:
            key: Cache key
            data: Data to cache
        """
        cache_file = self.cache_dir / f"{key}.json"
        
        cached = {
            "cached_at": datetime.utcnow().isoformat(),
            "data": data
        }
        
        try:
            with cache_file.open('w') as f:
                json.dump(cached, f, indent=2)
            logger.debug(f"Cached: {key}")
        except IOError as e:
            logger.error(f"Failed to write cache {key}: {e}")
    
    def delete(self, key: str) -> None:
        """
        Delete cached data.
        
        Args:
            key: Cache key
        """
        cache_file = self.cache_dir / f"{key}.json"
        if cache_file.exists():
            cache_file.unlink()
            logger.debug(f"Deleted cache: {key}")
    
    def clear(self) -> int:
        """
        Clear all cached data.
        
        Returns:
            Number of files deleted
        """
        count = 0
        for cache_file in self.cache_dir.glob("*.json"):
            cache_file.unlink()
            count += 1
        
        logger.info(f"Cleared {count} cache files")
        return count
    
    def clear_expired(self) -> int:
        """
        Remove expired cache entries.
        
        Returns:
            Number of expired entries removed
        """
        count = 0
        now = datetime.utcnow()
        
        for cache_file in self.cache_dir.glob("*.json"):
            try:
                with cache_file.open('r') as f:
                    cached = json.load(f)
                
                cached_at = datetime.fromisoformat(cached["cached_at"])
                age_seconds = (now - cached_at).total_seconds()
                
                if age_seconds > self.ttl_seconds:
                    cache_file.unlink()
                    count += 1
                    
            except (json.JSONDecodeError, KeyError, ValueError, IOError):
                # Invalid cache file, remove it
                cache_file.unlink()
                count += 1
        
        if count > 0:
            logger.info(f"Cleared {count} expired cache entries")
        
        return count
