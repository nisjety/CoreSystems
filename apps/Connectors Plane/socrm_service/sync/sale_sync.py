"""Sale synchronization logic."""

import logging
from datetime import datetime
from typing import Dict, List, Any

from clients.socrm_client import SuperOfficeClient
from cache.cache_manager import CacheManager

logger = logging.getLogger(__name__)


class SaleSync:
    """Handle sale synchronization between SuperOffice and local storage."""
    
    def __init__(self, client: SuperOfficeClient, cache_manager: CacheManager):
        """Initialize sale sync."""
        self.client = client
        self.cache = cache_manager
        self.cache_key = "sales"
    
    def get_sales(self, use_cache: bool = True) -> List[Dict[str, Any]]:
        """Get all sales with optional caching."""
        if use_cache:
            cached = self.cache.get(self.cache_key)
            if cached:
                logger.info("Returning cached sales")
                return cached
        
        logger.info("Fetching sales from SuperOffice")
        sales = self._fetch_all_sales()
        
        # Cache the results
        self.cache.set(self.cache_key, sales)
        
        return sales
    
    def _fetch_all_sales(self) -> List[Dict[str, Any]]:
        """Fetch all sales from SuperOffice using pagination."""
        all_sales = []
        page = 0
        page_size = 100
        
        while True:
            sales = self.client.get_sales(page=page, page_size=page_size)
            
            if not sales:
                break
            
            all_sales.extend(sales)
            page += 1
            
            logger.info(f"Fetched {len(sales)} sales (total: {len(all_sales)})")
        
        return all_sales
    
    def sync_sales(self) -> Dict[str, Any]:
        """Synchronize sales from SuperOffice."""
        start_time = datetime.utcnow()
        
        try:
            sales = self._fetch_all_sales()
            
            # Store in cache
            self.cache.set(self.cache_key, sales)
            
            # TODO: Store in database for persistence
            
            end_time = datetime.utcnow()
            duration = (end_time - start_time).total_seconds()
            
            result = {
                "status": "success",
                "sales_synced": len(sales),
                "start_time": start_time.isoformat(),
                "end_time": end_time.isoformat(),
                "duration_seconds": duration
            }
            
            logger.info(f"Sale sync completed: {len(sales)} sales in {duration:.2f}s")
            
            return result
        
        except Exception as e:
            logger.error(f"Sale sync failed: {e}")
            return {
                "status": "failed",
                "error": str(e),
                "timestamp": datetime.utcnow().isoformat()
            }
    
    def get_sale_by_id(self, sale_id: int) -> Dict[str, Any]:
        """Get single sale by ID."""
        return self.client.get_sale(sale_id)
    
    def create_sale(self, sale_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new sale in SuperOffice."""
        sale = self.client.create_sale(sale_data)
        
        # Invalidate cache
        self.cache.delete(self.cache_key)
        
        logger.info(f"Created sale: {sale.get('SaleId')}")
        
        return sale
    
    def update_sale(self, sale_id: int, sale_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update an existing sale in SuperOffice."""
        sale = self.client.update_sale(sale_id, sale_data)
        
        # Invalidate cache
        self.cache.delete(self.cache_key)
        
        logger.info(f"Updated sale: {sale_id}")
        
        return sale
