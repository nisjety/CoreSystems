#!/usr/bin/env python3
from azuresql_sync import sync_rates_to_azure_sql, verify_azure_sql_sync

print("=== Azure SQL Sync Test ===\n")
success, msg, count = sync_rates_to_azure_sql()
print(f"Success: {success}")
print(f"Message: {msg}")
print(f"Synced: {count} records")

if success:
    print("\n=== Verification ===")
    result = verify_azure_sql_sync()
    print(f"Status: {result['status']}")
    print(f"Total records: {result.get('total_records', 'N/A')}")
    print(f"Min date: {result.get('min_date', 'N/A')}")
    print(f"Max date: {result.get('max_date', 'N/A')}")
