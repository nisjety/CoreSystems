"""Contact synchronization logic."""

import logging
from datetime import datetime
from typing import Dict, List, Any

from clients.socrm_client import SuperOfficeClient
from cache.cache_manager import CacheManager

logger = logging.getLogger(__name__)


class ContactSync:
    """Handle contact synchronization between SuperOffice and local storage."""
    
    def __init__(self, client: SuperOfficeClient, cache_manager: CacheManager):
        """Initialize contact sync."""
        self.client = client
        self.cache = cache_manager
        self.cache_key = "contacts"
    
    def get_contacts(self, use_cache: bool = True) -> List[Dict[str, Any]]:
        """Get all contacts with optional caching."""
        if use_cache:
            cached = self.cache.get(self.cache_key)
            if cached:
                logger.info("Returning cached contacts")
                return cached
        
        logger.info("Fetching contacts from SuperOffice")
        contacts = self._fetch_all_contacts()
        
        # Cache the results
        self.cache.set(self.cache_key, contacts)
        
        return contacts
    
    def _fetch_all_contacts(self) -> List[Dict[str, Any]]:
        """Fetch all contacts from SuperOffice using pagination."""
        all_contacts = []
        page = 0
        page_size = 100
        
        while True:
            contacts = self.client.get_contacts(page=page, page_size=page_size)
            
            if not contacts:
                break
            
            all_contacts.extend(contacts)
            page += 1
            
            logger.info(f"Fetched {len(contacts)} contacts (total: {len(all_contacts)})")
        
        return all_contacts
    
    def sync_contacts(self) -> Dict[str, Any]:
        """Synchronize contacts from SuperOffice."""
        start_time = datetime.utcnow()
        
        try:
            contacts = self._fetch_all_contacts()
            
            # Store in cache
            self.cache.set(self.cache_key, contacts)
            
            # TODO: Store in database for persistence
            
            end_time = datetime.utcnow()
            duration = (end_time - start_time).total_seconds()
            
            result = {
                "status": "success",
                "contacts_synced": len(contacts),
                "start_time": start_time.isoformat(),
                "end_time": end_time.isoformat(),
                "duration_seconds": duration
            }
            
            logger.info(f"Contact sync completed: {len(contacts)} contacts in {duration:.2f}s")
            
            return result
        
        except Exception as e:
            logger.error(f"Contact sync failed: {e}")
            return {
                "status": "failed",
                "error": str(e),
                "timestamp": datetime.utcnow().isoformat()
            }
    
    def get_contact_by_id(self, contact_id: int) -> Dict[str, Any]:
        """Get single contact by ID."""
        return self.client.get_contact(contact_id)
    
    def create_contact(self, contact_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new contact in SuperOffice."""
        contact = self.client.create_contact(contact_data)
        
        # Invalidate cache
        self.cache.delete(self.cache_key)
        
        logger.info(f"Created contact: {contact.get('ContactId')}")
        
        return contact
    
    def update_contact(self, contact_id: int, contact_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update an existing contact in SuperOffice."""
        contact = self.client.update_contact(contact_id, contact_data)
        
        # Invalidate cache
        self.cache.delete(self.cache_key)
        
        logger.info(f"Updated contact: {contact_id}")
        
        return contact
