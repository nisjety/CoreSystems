"""
Power BI SQL Server Models
Models for syncing currency data to SQL Server for Power BI consumption
"""

from datetime import datetime
from sqlalchemy import create_engine, Column, Integer, String, Float, DateTime, DECIMAL, Index, text
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.orm import sessionmaker
import os
import logging

logger = logging.getLogger(__name__)

PowerBIBase = declarative_base()


class Rate(PowerBIBase):
    """Main rate table for Power BI - matches Norwegian view structure"""
    __tablename__ = 'Rate'
    
    id = Column(Integer, primary_key=True)
    date = Column(DateTime, nullable=False, index=True)
    base = Column(String(3), nullable=False, index=True)  # Base currency (e.g., USD, EUR)
    quote = Column(String(3), nullable=False, index=True)  # Quote currency (always NOK for Power BI)
    value = Column(DECIMAL(18, 8), nullable=False)  # Exchange rate value
    src = Column(String(50), nullable=False)  # Source: norgesbank, fcs, etc.
    created_at = Column(DateTime, default=datetime.utcnow, nullable=False)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow, nullable=False)
    
    __table_args__ = (
        Index('idx_rate_base_quote_date', 'base', 'quote', 'date'),
        Index('idx_rate_date', 'date'),
    )


class PowerBISyncLog(PowerBIBase):
    """Log of Power BI synchronization activities"""
    __tablename__ = 'PowerBISyncLog'
    
    id = Column(Integer, primary_key=True)
    sync_type = Column(String(50), nullable=False)  # 'scheduled', 'manual'
    status = Column(String(20), nullable=False)  # 'success', 'failed', 'partial'
    records_synced = Column(Integer, default=0)
    errors = Column(String(2000), nullable=True)
    started_at = Column(DateTime, default=datetime.utcnow, nullable=False)
    completed_at = Column(DateTime, nullable=True)
    duration_ms = Column(Integer, nullable=True)
    
    __table_args__ = (
        Index('idx_sync_started_at', 'started_at'),
        Index('idx_sync_status', 'status'),
    )
    
    def to_dict(self):
        """Convert to dictionary"""
        return {
            'id': self.id,
            'sync_type': self.sync_type,
            'status': self.status,
            'records_synced': self.records_synced,
            'errors': self.errors,
            'started_at': self.started_at.isoformat() if self.started_at else None,
            'completed_at': self.completed_at.isoformat() if self.completed_at else None,
            'duration_ms': self.duration_ms,
        }


def init_powerbi_db():
    """Initialize Power BI SQL Server database connection"""
    # Get SQL Server connection details from environment
    sql_server = os.getenv('POWERBI_SQL_SERVER', 'localhost')
    sql_database = os.getenv('POWERBI_SQL_DATABASE', 'CurrencyPowerBI')
    sql_username = os.getenv('POWERBI_SQL_USERNAME', 'sa')
    sql_password = os.getenv('POWERBI_SQL_PASSWORD', '')
    sql_driver = os.getenv('POWERBI_SQL_DRIVER', 'ODBC Driver 18 for SQL Server')
    
    # Build connection string for SQL Server
    connection_string = (
        f"mssql+pyodbc://{sql_username}:{sql_password}@{sql_server}/"
        f"{sql_database}?driver={sql_driver.replace(' ', '+')}"
    )
    
    logger.info(f"Connecting to Power BI SQL Server: {sql_server}/{sql_database}")
    
    engine = create_engine(
        connection_string,
        pool_size=5,
        max_overflow=10,
        pool_pre_ping=True,
        echo=False
    )
    
    # Create tables if they don't exist
    PowerBIBase.metadata.create_all(engine)
    logger.info("Power BI database tables created/verified")
    
    # Create views
    create_views(engine)
    
    SessionLocal = sessionmaker(autocommit=False, autoflush=False, bind=engine)
    return SessionLocal, engine


def create_views(engine):
    """Create Norwegian views for Power BI"""
    views_sql = """
    -- Drop views if they exist
    IF OBJECT_ID('dbo.DataSammendrag', 'V') IS NOT NULL DROP VIEW dbo.DataSammendrag;
    IF OBJECT_ID('dbo.Valutakurser', 'V') IS NOT NULL DROP VIEW dbo.Valutakurser;
    IF OBJECT_ID('dbo.SisteKurser', 'V') IS NOT NULL DROP VIEW dbo.SisteKurser;
    IF OBJECT_ID('dbo.HistoricalRate', 'V') IS NOT NULL DROP VIEW dbo.HistoricalRate;
    
    -- DataSammendrag (Data Summary)
    EXEC('CREATE VIEW dbo.DataSammendrag AS
    SELECT 
        COUNT(*) as antall_kurser,
        COUNT(DISTINCT base) as antall_valutaer,
        MIN(date) as eldste_dato,
        MAX(date) as nyeste_dato
    FROM dbo.Rate');
    
    -- Valutakurser (Currency Rates with Norwegian names)
    EXEC('CREATE VIEW dbo.Valutakurser AS
    SELECT 
        id,
        date as kurs_dato,
        base as valuta_kode,
        quote as mot_valuta,
        CAST(value AS DECIMAL(18,4)) as kurs_verdi,
        src as kilde,
        CASE base
            WHEN ''USD'' THEN ''Amerikanske dollar''
            WHEN ''EUR'' THEN ''Euro''
            WHEN ''GBP'' THEN ''Britiske pund''
            WHEN ''SEK'' THEN ''Svenske kroner''
            WHEN ''DKK'' THEN ''Danske kroner''
            WHEN ''CAD'' THEN ''Kanadiske dollar''
            WHEN ''AUD'' THEN ''Australske dollar''
            WHEN ''CHF'' THEN ''Sveitsiske franc''
            WHEN ''JPY'' THEN ''Japanske yen''
            WHEN ''ISK'' THEN ''Islandske kroner''
            ELSE base
        END as valuta_navn
    FROM dbo.Rate
    WHERE quote = ''NOK''');
    
    -- SisteKurser (Latest Rates)
    EXEC('CREATE VIEW dbo.SisteKurser AS
    WITH RankedRates AS (
        SELECT 
            base as valuta_kode,
            quote as mot_valuta,
            date as kurs_dato,
            CAST(value AS DECIMAL(18,4)) as kurs_verdi,
            src as kilde,
            CASE base
                WHEN ''USD'' THEN ''Amerikanske dollar''
                WHEN ''EUR'' THEN ''Euro''
                WHEN ''GBP'' THEN ''Britiske pund''
                WHEN ''SEK'' THEN ''Svenske kroner''
                WHEN ''DKK'' THEN ''Danske kroner''
                WHEN ''CAD'' THEN ''Kanadiske dollar''
                WHEN ''AUD'' THEN ''Australske dollar''
                WHEN ''CHF'' THEN ''Sveitsiske franc''
                WHEN ''JPY'' THEN ''Japanske yen''
                WHEN ''ISK'' THEN ''Islandske kroner''
                ELSE base
            END as valuta_navn,
            ROW_NUMBER() OVER (PARTITION BY base ORDER BY date DESC) as rn
        FROM dbo.Rate
        WHERE quote = ''NOK''
    )
    SELECT 
        valuta_kode,
        mot_valuta,
        kurs_dato,
        kurs_verdi,
        kilde,
        valuta_navn
    FROM RankedRates
    WHERE rn = 1');
    
    -- HistoricalRate (Historical rates with Norwegian column names)
    EXEC('CREATE VIEW dbo.HistoricalRate AS
    SELECT 
        id,
        date as dato,
        base as fra_valuta,
        quote as til_valuta,
        CAST(value AS DECIMAL(18,8)) as verdi,
        src as kilde
    FROM dbo.Rate');
    """
    
    try:
        with engine.begin() as conn:
            # Execute each statement separately
            for statement in views_sql.split(';'):
                if statement.strip():
                    conn.execute(text(statement))
        logger.info("Power BI views created successfully")
    except Exception as e:
        logger.error(f"Error creating views: {e}")
        # Views might already exist or there might be permissions issues
        # This is not critical, so we continue


def get_powerbi_session():
    """Get a Power BI database session"""
    SessionLocal, _ = init_powerbi_db()
    return SessionLocal()


def get_powerbi_connection_string():
    """Get the connection string for Power BI"""
    sql_server = os.getenv('POWERBI_SQL_SERVER', 'localhost')
    sql_database = os.getenv('POWERBI_SQL_DATABASE', 'CurrencyPowerBI')
    sql_username = os.getenv('POWERBI_SQL_USERNAME', 'sa')
    
    # Return a read-only connection string (without password for security)
    return {
        'server': sql_server,
        'database': sql_database,
        'username': sql_username,
        'connection_string': f"Server={sql_server};Database={sql_database};User Id={sql_username};",
        'note': 'Use SQL Server authentication in Power BI with the provided username and password'
    }
