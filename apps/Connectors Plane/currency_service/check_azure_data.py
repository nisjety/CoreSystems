#!/usr/bin/env python3
"""Check Azure SQL Rate table for current data"""

import pyodbc

server = "finansehub-sql.database.windows.net"
database = "finansehub"
username = "fshadmin"
password = "FinHub2025!Secure"
driver = "ODBC Driver 18 for SQL Server"

try:
    connection_string = f"Driver={{{driver}}};Server=tcp:{server},1433;Database={database};Uid={username};Pwd={password};Encrypt=yes;TrustServerCertificate=no;Connection Timeout=30;"
    conn = pyodbc.connect(connection_string)
    cursor = conn.cursor()
    
    print("=== Azure SQL Rate Table ===\n")
    
    # Check record count and date range
    cursor.execute("""
    SELECT 
        COUNT(*) as total_records,
        MIN([date]) as min_date,
        MAX([date]) as max_date
    FROM [dbo].[Rate]
    """)
    
    result = cursor.fetchone()
    print(f"Total records: {result[0]}")
    print(f"Min date: {result[1]}")
    print(f"Max date: {result[2]}")
    
    # Check records for 2026-03-23
    print("\n=== Records for 2026-03-23 ===")
    cursor.execute("""
    SELECT [date], [base], [quote], [value], [src]
    FROM [dbo].[Rate]
    WHERE CAST([date] AS DATE) = '2026-03-23'
    ORDER BY [base]
    """)
    
    rows = cursor.fetchall()
    print(f"Found {len(rows)} records:\n")
    for row in rows:
        print(f"  {row[1]}/{row[2]}: {row[3]:.8f} (source: {row[4]})")
    
    conn.close()
    print("\n✅ Success")
    
except pyodbc.Error as e:
    print(f"❌ Database Error: {e}")
except Exception as e:
    print(f"❌ Error: {e}")
