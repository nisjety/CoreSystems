# Power BI Integration - Quick Start

## What's New

The Currency Service now syncs data to SQL Server for Power BI reporting.

### Features

✅ **Automated Sync**: Mon-Fri at 17:00  
✅ **Norwegian Views**: Pre-configured views with Norwegian column names  
✅ **Manual Trigger**: Sync on-demand via API or UI  
✅ **Monitoring**: Real-time status in the frontend  
✅ **90 Days History**: Automatic sync of the last 90 days  

## Quick Setup

### 1. Run Setup Script

```bash
cd apps/currency_service
./setup_powerbi.sh
```

The script will:
- Set up SQL Server (Docker or existing)
- Create the database and tables
- Configure environment variables
- Initialize views

### 2. Start the Service

```bash
python app.py
```

### 3. Trigger Initial Sync

**Via API:**
```bash
curl -X POST http://localhost:5004/api/currency/powerbi/sync \
  -H "Content-Type: application/json" \
  -d '{"days_back": 90}'
```

**Via Frontend:**
1. Go to `Currency Status` page
2. Click "Sync to Power BI" button

### 4. Connect Power BI

1. Open Power BI Desktop
2. Get Data → SQL Server
3. Server: `localhost` (or your SQL Server)
4. Database: `CurrencyPowerBI`
5. Authentication: SQL Server (use your credentials)
6. Select views:
   - `dbo.Valutakurser` (Recommended for most reports)
   - `dbo.SisteKurser` (Latest rates only)
   - `dbo.DataSammendrag` (Summary stats)
   - `dbo.HistoricalRate` (Complete history)

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/currency/powerbi/status` | GET | Get sync status and database info |
| `/api/currency/powerbi/sync` | POST | Trigger manual sync |
| `/api/currency/powerbi/history` | GET | View sync history |
| `/api/currency/powerbi/connection` | GET | Get connection details for Power BI |

## Available Views

### Valutakurser (Recommended)
Currency rates with Norwegian names - perfect for dashboards.

```sql
SELECT * FROM dbo.Valutakurser
WHERE valuta_kode = 'USD'
AND kurs_dato >= DATEADD(day, -30, GETDATE())
```

### SisteKurser
Latest rate for each currency.

```sql
SELECT * FROM dbo.SisteKurser
ORDER BY kurs_verdi DESC
```

### DataSammendrag
Summary statistics.

```sql
SELECT * FROM dbo.DataSammendrag
```

### HistoricalRate
Complete historical data.

```sql
SELECT * FROM dbo.HistoricalRate
WHERE fra_valuta = 'EUR'
AND til_valuta = 'NOK'
ORDER BY dato DESC
```

## Frontend Pages

### Currency Status Page
- Monitor Power BI sync status
- View last sync details
- Manually trigger sync
- Check database statistics

### Currency Data Page
- View synchronized data
- Filter by currency and date range
- Compare PostgreSQL and SQL Server data

## Scheduled Sync

The sync runs automatically:

- **When**: Monday to Friday at 17:00
- **What**: Last 90 days of currency rates
- **After**: Main PostgreSQL sync (16:10)

## Troubleshooting

### SQL Server Connection Error

```bash
# Check if SQL Server is running
docker ps | grep sql_powerbi

# Check logs
docker logs sql_powerbi
```

### No Data in Power BI

```bash
# Check sync status
curl http://localhost:5004/api/currency/powerbi/status

# Trigger manual sync
curl -X POST http://localhost:5004/api/currency/powerbi/sync
```

### Python Module Error

```bash
# Install dependencies
pip install pyodbc sqlalchemy
```

## Files Added

- `powerbi_models.py` - SQL Server models and views
- `powerbi_sync.py` - Sync service between PostgreSQL and SQL Server
- `POWERBI_INTEGRATION.md` - Complete documentation
- `setup_powerbi.sh` - Automated setup script

## Files Modified

- `scheduler.py` - Added Power BI sync job (Mon-Fri 17:00)
- `app.py` - Added Power BI API endpoints
- `requirements.txt` - Added pyodbc dependency
- `.env.example` - Added SQL Server configuration

## Environment Variables

```env
POWERBI_SQL_SERVER=localhost
POWERBI_SQL_DATABASE=CurrencyPowerBI
POWERBI_SQL_USERNAME=sa
POWERBI_SQL_PASSWORD=YourStrongPassword123!
POWERBI_SQL_DRIVER=ODBC Driver 17 for SQL Server
```

## Support

For detailed documentation, see [POWERBI_INTEGRATION.md](./POWERBI_INTEGRATION.md)

For issues:
1. Check the Currency Status page
2. Review service logs
3. Contact the development team
