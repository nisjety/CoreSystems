"""Company synchronization logic."""

import logging
from datetime import datetime
from typing import Dict, List, Any

from clients.socrm_client import SuperOfficeClient
from cache.cache_manager import CacheManager

logger = logging.getLogger(__name__)


class CompanySync:
    """Handle company synchronization between SuperOffice and local storage."""
    
    def __init__(self, client: SuperOfficeClient, cache_manager: CacheManager):
        """Initialize company sync."""
        self.client = client
        self.cache = cache_manager
        self.cache_key = "companies"
    
    def get_companies(self, use_cache: bool = True) -> List[Dict[str, Any]]:
        """Get all companies with optional caching."""
        if use_cache:
            cached = self.cache.get(self.cache_key)
            if cached:
                logger.info("Returning cached companies")
                return cached
        
        logger.info("Fetching companies from SuperOffice")
        companies = self._fetch_all_companies()
        
        # Cache the results
        self.cache.set(self.cache_key, companies)
        
        return companies
    
    def _fetch_all_companies(self) -> List[Dict[str, Any]]:
        """Fetch all companies from SuperOffice using pagination."""
        all_companies = []
        page = 0
        page_size = 100
        
        while True:
            companies = self.client.get_companies(page=page, page_size=page_size)
            
            if not companies:
                break
            
            all_companies.extend(companies)
            page += 1
            
            logger.info(f"Fetched {len(companies)} companies (total: {len(all_companies)})")
        
        return all_companies
    
    def sync_companies(self) -> Dict[str, Any]:
        """Synchronize companies from SuperOffice."""
        start_time = datetime.utcnow()
        
        try:
            companies = self._fetch_all_companies()
            
            # Store in cache
            self.cache.set(self.cache_key, companies)
            
            # TODO: Store in database for persistence
            
            end_time = datetime.utcnow()
            duration = (end_time - start_time).total_seconds()
            
            result = {
                "status": "success",
                "companies_synced": len(companies),
                "start_time": start_time.isoformat(),
                "end_time": end_time.isoformat(),
                "duration_seconds": duration
            }
            
            logger.info(f"Company sync completed: {len(companies)} companies in {duration:.2f}s")
            
            return result
        
        except Exception as e:
            logger.error(f"Company sync failed: {e}")
            return {
                "status": "failed",
                "error": str(e),
                "timestamp": datetime.utcnow().isoformat()
            }
    
    def get_company_by_id(self, company_id: int) -> Dict[str, Any]:
        """Get single company by ID."""
        return self.client.get_company(company_id)
    
    def create_company(self, company_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new company in SuperOffice."""
        company = self.client.create_company(company_data)
        
        # Invalidate cache
        self.cache.delete(self.cache_key)
        
        logger.info(f"Created company: {company.get('CompanyId')}")
        
        return company
    
    def update_company(self, company_id: int, company_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update an existing company in SuperOffice."""
        company = self.client.update_company(company_id, company_data)
        
        # Invalidate cache
        self.cache.delete(self.cache_key)
        
        logger.info(f"Updated company: {company_id}")
        
        return company
