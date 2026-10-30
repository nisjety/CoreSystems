"""
Database models for Currency Service
"""

from datetime import datetime
from sqlalchemy import create_engine, Column, Integer, String, Float, DateTime, Index, UniqueConstraint
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.orm import sessionmaker
import os

Base = declarative_base()


class ExchangeRate(Base):
    """Exchange rate storage model"""
    __tablename__ = 'exchange_rates'
    
    id = Column(Integer, primary_key=True)
    from_currency = Column(String(3), nullable=False, index=True)
    to_currency = Column(String(3), nullable=False, index=True)
    rate = Column(Float, nullable=False)
    source = Column(String(50), nullable=False)  # 'norgesbank', 'abstract', 'manual'
    timestamp = Column(DateTime, default=datetime.utcnow, nullable=False)
    created_at = Column(DateTime, default=datetime.utcnow, nullable=False)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow, nullable=False)
    
    # Unique constraint on currency pair and source
    __table_args__ = (
        UniqueConstraint('from_currency', 'to_currency', 'source', name='uix_currency_pair_source'),
        Index('idx_currency_pair', 'from_currency', 'to_currency'),
        Index('idx_timestamp', 'timestamp'),
    )
    
    def to_dict(self):
        """Convert to dictionary"""
        return {
            'id': self.id,
            'from_currency': self.from_currency,
            'to_currency': self.to_currency,
            'rate': self.rate,
            'source': self.source,
            'timestamp': self.timestamp.isoformat() if self.timestamp else None,
            'created_at': self.created_at.isoformat() if self.created_at else None,
            'updated_at': self.updated_at.isoformat() if self.updated_at else None,
        }


class SyncLog(Base):
    """Synchronization log model"""
    __tablename__ = 'sync_logs'
    
    id = Column(Integer, primary_key=True)
    sync_type = Column(String(50), nullable=False)  # 'manual', 'scheduled', 'api'
    status = Column(String(20), nullable=False)  # 'success', 'partial', 'failed'
    rates_updated = Column(Integer, default=0)
    errors = Column(String(1000), nullable=True)
    started_at = Column(DateTime, default=datetime.utcnow, nullable=False)
    completed_at = Column(DateTime, nullable=True)
    duration_ms = Column(Integer, nullable=True)
    
    __table_args__ = (
        Index('idx_started_at', 'started_at'),
        Index('idx_status', 'status'),
    )
    
    def to_dict(self):
        """Convert to dictionary"""
        return {
            'id': self.id,
            'sync_type': self.sync_type,
            'status': self.status,
            'rates_updated': self.rates_updated,
            'errors': self.errors,
            'started_at': self.started_at.isoformat() if self.started_at else None,
            'completed_at': self.completed_at.isoformat() if self.completed_at else None,
            'duration_ms': self.duration_ms,
        }


class ServiceLog(Base):
    """Service activity log model"""
    __tablename__ = 'service_logs'
    
    id = Column(Integer, primary_key=True)
    level = Column(String(10), nullable=False)  # 'INFO', 'WARNING', 'ERROR', 'DEBUG'
    message = Column(String(500), nullable=False)
    details = Column(String(2000), nullable=True)
    timestamp = Column(DateTime, default=datetime.utcnow, nullable=False, index=True)
    
    def to_dict(self):
        """Convert to dictionary"""
        return {
            'id': self.id,
            'level': self.level,
            'message': self.message,
            'details': self.details,
            'timestamp': self.timestamp.isoformat() if self.timestamp else None,
        }


# Database setup
def get_database_url():
    """Get database URL from environment"""
    return os.getenv(
        'DATABASE_URL',
        'postgresql://postgres:postgres@postgres:5432/aquatiq_db'
    )


def init_db():
    """Initialize database connection and create tables"""
    engine = create_engine(get_database_url(), pool_pre_ping=True)
    Base.metadata.create_all(engine)
    Session = sessionmaker(bind=engine)
    return Session


def get_session():
    """Get database session"""
    Session = sessionmaker(bind=create_engine(get_database_url(), pool_pre_ping=True))
    return Session()
