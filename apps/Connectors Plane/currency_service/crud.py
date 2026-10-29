"""
CRUD operations for Currency Service
"""

from datetime import datetime, timedelta
from typing import List, Optional, Dict
from sqlalchemy.orm import Session
from sqlalchemy import desc, and_

from models import ExchangeRate, SyncLog, ServiceLog


class CurrencyRateService:
    """Service for managing exchange rates"""
    
    @staticmethod
    def create_rate(session: Session, from_currency: str, to_currency: str, 
                   rate: float, source: str) -> ExchangeRate:
        """Create a new exchange rate"""
        exchange_rate = ExchangeRate(
            from_currency=from_currency.upper(),
            to_currency=to_currency.upper(),
            rate=rate,
            source=source,
            timestamp=datetime.utcnow()
        )
        session.add(exchange_rate)
        session.commit()
        session.refresh(exchange_rate)
        return exchange_rate
    
    @staticmethod
    def get_rate(session: Session, rate_id: int) -> Optional[ExchangeRate]:
        """Get exchange rate by ID"""
        return session.query(ExchangeRate).filter(ExchangeRate.id == rate_id).first()
    
    @staticmethod
    def get_rate_by_pair(session: Session, from_currency: str, to_currency: str, 
                        source: Optional[str] = None) -> Optional[ExchangeRate]:
        """Get exchange rate by currency pair"""
        query = session.query(ExchangeRate).filter(
            and_(
                ExchangeRate.from_currency == from_currency.upper(),
                ExchangeRate.to_currency == to_currency.upper()
            )
        )
        
        if source:
            query = query.filter(ExchangeRate.source == source)
        
        return query.order_by(desc(ExchangeRate.timestamp)).first()
    
    @staticmethod
    def get_all_rates(session: Session, from_currency: Optional[str] = None,
                     source: Optional[str] = None) -> List[ExchangeRate]:
        """Get all exchange rates with optional filters"""
        query = session.query(ExchangeRate)
        
        if from_currency:
            query = query.filter(ExchangeRate.from_currency == from_currency.upper())
        
        if source:
            query = query.filter(ExchangeRate.source == source)
        
        return query.order_by(desc(ExchangeRate.timestamp)).all()
    
    @staticmethod
    def get_latest_rates(session: Session) -> List[ExchangeRate]:
        """Get the latest rate for each currency pair"""
        # Subquery to get the latest timestamp for each pair
        from sqlalchemy import func
        subquery = session.query(
            ExchangeRate.from_currency,
            ExchangeRate.to_currency,
            func.max(ExchangeRate.timestamp).label('max_timestamp')
        ).group_by(
            ExchangeRate.from_currency,
            ExchangeRate.to_currency
        ).subquery()
        
        # Join with main query to get full records
        rates = session.query(ExchangeRate).join(
            subquery,
            and_(
                ExchangeRate.from_currency == subquery.c.from_currency,
                ExchangeRate.to_currency == subquery.c.to_currency,
                ExchangeRate.timestamp == subquery.c.max_timestamp
            )
        ).all()
        
        return rates
    
    @staticmethod
    def update_rate(session: Session, rate_id: int, rate: float) -> Optional[ExchangeRate]:
        """Update exchange rate"""
        exchange_rate = session.query(ExchangeRate).filter(ExchangeRate.id == rate_id).first()
        if exchange_rate:
            exchange_rate.rate = rate
            exchange_rate.updated_at = datetime.utcnow()
            exchange_rate.timestamp = datetime.utcnow()
            session.commit()
            session.refresh(exchange_rate)
        return exchange_rate
    
    @staticmethod
    def upsert_rate(session: Session, from_currency: str, to_currency: str,
                   rate: float, source: str) -> ExchangeRate:
        """Create or update exchange rate"""
        existing = CurrencyRateService.get_rate_by_pair(
            session, from_currency, to_currency, source
        )
        
        if existing:
            existing.rate = rate
            existing.timestamp = datetime.utcnow()
            existing.updated_at = datetime.utcnow()
            session.commit()
            session.refresh(existing)
            return existing
        else:
            return CurrencyRateService.create_rate(
                session, from_currency, to_currency, rate, source
            )
    
    @staticmethod
    def delete_rate(session: Session, rate_id: int) -> bool:
        """Delete exchange rate"""
        exchange_rate = session.query(ExchangeRate).filter(ExchangeRate.id == rate_id).first()
        if exchange_rate:
            session.delete(exchange_rate)
            session.commit()
            return True
        return False
    
    @staticmethod
    def bulk_upsert_rates(session: Session, rates: List[Dict]) -> int:
        """Bulk upsert exchange rates"""
        count = 0
        for rate_data in rates:
            CurrencyRateService.upsert_rate(
                session,
                rate_data['from_currency'],
                rate_data['to_currency'],
                rate_data['rate'],
                rate_data['source']
            )
            count += 1
        return count


class SyncLogService:
    """Service for managing sync logs"""
    
    @staticmethod
    def create_log(session: Session, sync_type: str) -> SyncLog:
        """Create a new sync log"""
        sync_log = SyncLog(
            sync_type=sync_type,
            status='running',
            started_at=datetime.utcnow()
        )
        session.add(sync_log)
        session.commit()
        session.refresh(sync_log)
        return sync_log
    
    @staticmethod
    def complete_log(session: Session, log_id: int, status: str,
                    rates_updated: int, errors: Optional[str] = None):
        """Complete a sync log"""
        sync_log = session.query(SyncLog).filter(SyncLog.id == log_id).first()
        if sync_log:
            sync_log.status = status
            sync_log.rates_updated = rates_updated
            sync_log.errors = errors
            sync_log.completed_at = datetime.utcnow()
            if sync_log.started_at:
                duration = (sync_log.completed_at - sync_log.started_at).total_seconds() * 1000
                sync_log.duration_ms = int(duration)
            session.commit()
    
    @staticmethod
    def get_recent_logs(session: Session, limit: int = 50) -> List[SyncLog]:
        """Get recent sync logs"""
        return session.query(SyncLog).order_by(desc(SyncLog.started_at)).limit(limit).all()
    
    @staticmethod
    def get_last_successful_sync(session: Session) -> Optional[SyncLog]:
        """Get the last successful sync"""
        return session.query(SyncLog).filter(
            SyncLog.status == 'success'
        ).order_by(desc(SyncLog.completed_at)).first()


class ServiceLogService:
    """Service for managing service logs"""
    
    @staticmethod
    def add_log(session: Session, level: str, message: str, details: Optional[str] = None):
        """Add a service log entry"""
        log = ServiceLog(
            level=level,
            message=message,
            details=details,
            timestamp=datetime.utcnow()
        )
        session.add(log)
        session.commit()
        
        # Clean up old logs (keep last 1000)
        ServiceLogService.cleanup_old_logs(session, keep=1000)
    
    @staticmethod
    def get_recent_logs(session: Session, limit: int = 100,
                       level: Optional[str] = None) -> List[ServiceLog]:
        """Get recent service logs"""
        query = session.query(ServiceLog)
        
        if level:
            query = query.filter(ServiceLog.level == level)
        
        return query.order_by(desc(ServiceLog.timestamp)).limit(limit).all()
    
    @staticmethod
    def cleanup_old_logs(session: Session, keep: int = 1000):
        """Remove old log entries, keeping only the most recent ones"""
        total = session.query(ServiceLog).count()
        if total > keep:
            # Get the timestamp of the Nth newest log
            cutoff_log = session.query(ServiceLog).order_by(
                desc(ServiceLog.timestamp)
            ).offset(keep).first()
            
            if cutoff_log:
                session.query(ServiceLog).filter(
                    ServiceLog.timestamp < cutoff_log.timestamp
                ).delete()
                session.commit()
