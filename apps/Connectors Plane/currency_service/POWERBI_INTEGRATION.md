# Power BI Integration for Currency Service

## Overview

The Currency Service now includes a Power BI integration that syncs currency exchange rate data to a SQL Server database. This allows Power BI to directly connect to the SQL Server and create reports and visualizations.

## Architecture

```
PostgreSQL (Main DB) --> Currency Service --> SQL Server (Power BI DB)
                              ↓
                      Power BI Desktop/Service
```

### Components

1. **PostgreSQL Database**: Primary data store for currency rates
2. **SQL Server Database**: Dedicated database for Power BI with Norwegian-named views
3. **Sync Service**: Automated synchronization between PostgreSQL and SQL Server
4. **Cron Job**: Scheduled sync Mon-Fri at 17:00
5. **API Endpoints**: Manual trigger and status monitoring

## Setup Instructions

### 1. SQL Server Configuration

#### Install SQL Server

**Option A: Docker (Recommended for Development)**
```bash
docker run -e "ACCEPT_EULA=Y" -e "SA_PASSWORD=YourStrongPassword123!" \
  -p 1433:1433 --name sql_powerbi \
  -d mcr.microsoft.com/mssql/server:2022-latest
```

**Option B: Azure SQL Database**
- Create an Azure SQL Database
- Note the server name, database name, and credentials

#### Install ODBC Driver

**macOS:**
```bash
brew tap microsoft/mssql-release https://github.com/Microsoft/homebrew-mssql-release
brew update
brew install msodbcsql17 mssql-tools
```

**Ubuntu/Debian:**
```bash
curl https://packages.microsoft.com/keys/microsoft.asc | sudo apt-key add -
curl https://packages.microsoft.com/config/ubuntu/20.04/prod.list | sudo tee /etc/apt/sources.list.d/mssql-release.list
sudo apt-get update
sudo ACCEPT_EULA=Y apt-get install -y msodbcsql17
```

**Windows:**
- Download and install [ODBC Driver 17 for SQL Server](https://learn.microsoft.com/en-us/sql/connect/odbc/download-odbc-driver-for-sql-server)

### 2. Environment Configuration

Add the following to your `.env` file in the currency_service directory:

```env
# Power BI SQL Server Configuration
POWERBI_SQL_SERVER=localhost
# For Azure SQL: your-server.database.windows.net
POWERBI_SQL_DATABASE=CurrencyPowerBI
POWERBI_SQL_USERNAME=sa
POWERBI_SQL_PASSWORD=YourStrongPassword123!
POWERBI_SQL_DRIVER=ODBC Driver 17 for SQL Server
```

### 3. Create the Database

Connect to SQL Server and create the database:

```sql
CREATE DATABASE CurrencyPowerBI;
GO

USE CurrencyPowerBI;
GO
```

The tables and views will be created automatically when the service starts.

### 4. Install Python Dependencies

```bash
cd apps/currency_service
pip install -r requirements.txt
```

### 5. Start the Service

```bash
python app.py
```

The service will:
- Connect to SQL Server
- Create necessary tables and views
- Schedule automatic sync for Mon-Fri at 17:00

## Available Views

The following views are available for Power BI reporting:

### 1. DataSammendrag (Data Summary)
Overview of the entire dataset.

**Columns:**
- `antall_kurser` - Total number of exchange rates
- `antall_valutaer` - Number of distinct currencies
- `eldste_dato` - Earliest date in dataset
- `nyeste_dato` - Latest date in dataset

### 2. Valutakurser (Currency Rates)
Detailed exchange rates with Norwegian names.

**Columns:**
- `id` - Unique identifier
- `kurs_dato` - Exchange rate date
- `valuta_kode` - Currency code (USD, EUR, etc.)
- `mot_valuta` - Quote currency (always NOK)
- `kurs_verdi` - Exchange rate value
- `kilde` - Data source (norgesbank, fcs)
- `valuta_navn` - Currency name in Norwegian

### 3. SisteKurser (Latest Rates)
Most recent exchange rate for each currency.

**Columns:**
- `valuta_kode` - Currency code
- `mot_valuta` - Quote currency (NOK)
- `kurs_dato` - Date of the rate
- `kurs_verdi` - Exchange rate value
- `kilde` - Data source
- `valuta_navn` - Currency name in Norwegian

### 4. HistoricalRate
Complete historical data with Norwegian column names.

**Columns:**
- `id` - Unique identifier
- `dato` - Date
- `fra_valuta` - From currency
- `til_valuta` - To currency
- `verdi` - Exchange rate value
- `kilde` - Data source

## Connecting Power BI

### Step 1: Get Connection Information

Use the API endpoint to get connection details:

```bash
curl http://localhost:5004/api/currency/powerbi/connection
```

Response:
```json
{
  "success": true,
  "connection": {
    "server": "localhost",
    "database": "CurrencyPowerBI",
    "username": "sa",
    "connection_string": "Server=localhost;Database=CurrencyPowerBI;User Id=sa;"
  },
  "available_views": [
    "DataSammendrag",
    "Valutakurser",
    "SisteKurser",
    "HistoricalRate"
  ],
  "instructions": { ... }
}
```

### Step 2: Connect Power BI Desktop

1. **Open Power BI Desktop**

2. **Click "Get Data"**
   - Select "SQL Server"

3. **Enter Connection Details**
   - **Server**: `localhost` (or your Azure SQL server)
   - **Database**: `CurrencyPowerBI`
   - Click "OK"

4. **Authentication**
   - Select "Database" (SQL Server authentication)
   - **User name**: `sa` (or your SQL username)
   - **Password**: Your SQL password
   - Click "Connect"

5. **Select Data**
   - Choose one or more views:
     - `dbo.DataSammendrag`
     - `dbo.Valutakurser`
     - `dbo.SisteKurser`
     - `dbo.HistoricalRate`
   - Click "Load"

6. **Create Visualizations**
   - Use the loaded data to create reports and dashboards

### Step 3: Publish to Power BI Service (Optional)

1. Click "Publish" in Power BI Desktop
2. Sign in to Power BI Service
3. Select your workspace
4. Configure scheduled refresh if needed

## API Endpoints

### Get Power BI Status
```http
GET /api/currency/powerbi/status
```

Returns:
- Total records in SQL Server
- Last sync information
- Date range of available data
- Number of currencies

### Manual Sync
```http
POST /api/currency/powerbi/sync
Content-Type: application/json

{
  "days_back": 90
}
```

Manually trigger a sync of the last N days of data.

### Get Sync History
```http
GET /api/currency/powerbi/history?limit=10
```

Returns recent sync operations.

### Get Connection Info
```http
GET /api/currency/powerbi/connection
```

Returns SQL Server connection details for Power BI.

## Scheduled Sync

The Power BI sync runs automatically:

- **Schedule**: Monday to Friday at 17:00
- **Timezone**: Server timezone
- **Data Range**: Last 90 days
- **Dependencies**: Runs after the main PostgreSQL sync (16:10)

### Monitoring

View sync status in the frontend:
1. Navigate to `Currency Status` page
2. View the "Power BI SQL Server" card
3. Monitor:
   - Total records
   - Currency count
   - Last sync status
   - Date range

### Manual Trigger

Trigger a manual sync:
1. Go to `Currency Status` page
2. Click "Sync to Power BI" button
3. Wait for completion
4. Check updated status

## Example Power BI Reports

### 1. Currency Trend Dashboard

**Data Source**: `Valutakurser` view

**Visualizations**:
- Line chart: `kurs_verdi` over `kurs_dato` by `valuta_kode`
- Card: Latest `kurs_verdi` for selected currency
- Table: All currencies with latest rates

### 2. Currency Comparison

**Data Source**: `SisteKurser` view

**Visualizations**:
- Bar chart: Compare latest rates across currencies
- Card: Number of tracked currencies
- Matrix: Currency names and values

### 3. Historical Analysis

**Data Source**: `HistoricalRate` view

**Visualizations**:
- Area chart: Historical trends over time
- Slicer: Date range selector
- KPI: Rate change percentage

## Troubleshooting

### Connection Errors

**Error**: "Cannot open database"
```bash
# Check if database exists
sqlcmd -S localhost -U sa -P 'YourPassword' -Q "SELECT name FROM sys.databases"

# Create database if missing
sqlcmd -S localhost -U sa -P 'YourPassword' -Q "CREATE DATABASE CurrencyPowerBI"
```

**Error**: "Login failed for user"
- Verify SQL Server credentials
- Check that SQL Server authentication is enabled
- Ensure the user has permissions to the database

### Sync Errors

**Error**: "pyodbc module not found"
```bash
pip install pyodbc
```

**Error**: "Driver not found"
- Install ODBC Driver 17 for SQL Server (see setup instructions)

### Data Not Appearing

1. Check Power BI status:
   ```bash
   curl http://localhost:5004/api/currency/powerbi/status
   ```

2. Trigger manual sync:
   ```bash
   curl -X POST http://localhost:5004/api/currency/powerbi/sync \
     -H "Content-Type: application/json" \
     -d '{"days_back": 90}'
   ```

3. Verify data in SQL Server:
   ```sql
   SELECT COUNT(*) FROM Rate;
   SELECT * FROM SisteKurser;
   ```

## Security Considerations

1. **Credentials**: Store SQL Server credentials in environment variables, never in code
2. **Network**: Use SSL/TLS for production SQL Server connections
3. **Firewall**: Configure SQL Server firewall rules appropriately
4. **Authentication**: Use separate read-only credentials for Power BI in production
5. **Encryption**: Enable transparent data encryption (TDE) for Azure SQL

## Performance Tips

1. **Indexing**: The views use indexed columns for fast queries
2. **Data Range**: Limit sync to necessary date range (default: 90 days)
3. **Refresh Schedule**: Schedule Power BI refresh after the 17:00 sync
4. **Incremental Refresh**: Configure incremental refresh in Power BI for large datasets

## Maintenance

### Backup

**SQL Server:**
```sql
BACKUP DATABASE CurrencyPowerBI
TO DISK = '/var/opt/mssql/backup/CurrencyPowerBI.bak'
WITH FORMAT, COMPRESSION;
```

**Azure SQL:**
- Automatic backups are enabled
- Configure geo-redundant backups if needed

### Monitoring

Check logs:
```bash
# View sync history
curl http://localhost:5004/api/currency/powerbi/history

# Check scheduler logs
tail -f /var/log/currency_service/app.log
```

## Support

For issues or questions:
- Check logs in the Currency Service
- Review the frontend status page
- Contact the development team

## License

Copyright © 2025 Aquatiq
