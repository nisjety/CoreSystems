#!/usr/bin/env python3
"""
Pre-implementation verification script.

Run this BEFORE implementing the Power BI fix to verify current state.
This helps identify issues and validates assumptions.
"""

import os
import sys
from datetime import date


def check_environment():
    """Check that environment is properly configured."""
    print("=" * 70)
    print("ENVIRONMENT CHECK")
    print("=" * 70)
    
    required_vars = [
        "GCP_PROJECT_ID",
        "GCP_DATASET_ID", 
        "GOOGLE_APPLICATION_CREDENTIALS"
    ]
    
    missing = []
    for var in required_vars:
        value = os.getenv(var)
        if value:
            # Mask credentials path for security
            if "CREDENTIALS" in var:
                display = "***" + value[-20:] if len(value) > 20 else "***"
            else:
                display = value
            print(f"✅ {var} = {display}")
        else:
            print(f"❌ {var} = NOT SET")
            missing.append(var)
    
    if missing:
        print(f"\n⚠️  Missing environment variables: {', '.join(missing)}")
        print("Set them in .env file or export them")
        return False
    
    return True


def check_dependencies():
    """Check that required Python packages are installed."""
    print("\n" + "=" * 70)
    print("DEPENDENCY CHECK")
    print("=" * 70)
    
    required = [
        ("google.cloud.bigquery", "google-cloud-bigquery"),
        ("pytest", "pytest"),
        ("pytest_cov", "pytest-cov"),
    ]
    
    missing = []
    for module, package in required:
        try:
            __import__(module)
            print(f"✅ {package}")
        except ImportError:
            print(f"❌ {package} NOT INSTALLED")
            missing.append(package)
    
    if missing:
        print(f"\n⚠️  Install missing packages:")
        print(f"   pip install {' '.join(missing)}")
        return False
    
    return True


def check_bigquery_connectivity():
    """Check that we can connect to BigQuery."""
    print("\n" + "=" * 70)
    print("BIGQUERY CONNECTIVITY CHECK")
    print("=" * 70)
    
    try:
        from shared.gcp_client import get_gcp_client
        
        gcp = get_gcp_client()
        print(f"✅ Connected to project: {gcp.project_id}")
        print(f"✅ Dataset: {gcp.dataset_id}")
        
        # Try to list tables
        client = gcp.bigquery
        dataset_ref = f"{gcp.project_id}.{gcp.dataset_id}"
        tables = list(client.list_tables(dataset_ref))
        
        print(f"✅ Found {len(tables)} tables in dataset")
        for table in tables:
            print(f"   - {table.table_id} ({table.table_type})")
        
        return True
        
    except Exception as e:
        print(f"❌ BigQuery connection failed: {e}")
        return False


def check_current_schema():
    """Verify current Rate table schema."""
    print("\n" + "=" * 70)
    print("CURRENT SCHEMA CHECK")
    print("=" * 70)
    
    try:
        from shared.gcp_client import get_gcp_client
        
        gcp = get_gcp_client()
        client = gcp.bigquery
        
        # Check Rate table
        table = client.get_table(f"{gcp.project_id}.{gcp.dataset_id}.Rate")
        print(f"✅ Rate table exists")
        print(f"   Schema:")
        for field in table.schema:
            print(f"   - {field.name:20} {field.field_type:15} {field.mode:10}")
        
        # Verify rate column is FLOAT64
        rate_field = next((f for f in table.schema if f.name == "rate"), None)
        if rate_field and rate_field.field_type == "FLOAT64":
            print(f"✅ rate column is FLOAT64 (correct)")
        else:
            print(f"⚠️  rate column is {rate_field.field_type if rate_field else 'MISSING'}")
        
        # Check for existing Power BI views
        print(f"\n   Checking for existing Power BI views:")
        for view_name in ["Rate_PowerBI", "Historyrates_PowerBI"]:
            try:
                view = client.get_table(f"{gcp.project_id}.{gcp.dataset_id}.{view_name}")
                print(f"   ⚠️  {view_name} already exists (will be replaced)")
            except:
                print(f"   ✅ {view_name} does not exist (will be created)")
        
        return True
        
    except Exception as e:
        print(f"❌ Schema check failed: {e}")
        return False


def check_sample_data():
    """Query sample data to verify rate values."""
    print("\n" + "=" * 70)
    print("SAMPLE DATA CHECK")
    print("=" * 70)
    
    try:
        from shared.gcp_client import get_gcp_client
        
        gcp = get_gcp_client()
        
        query = f"""
            SELECT 
                currency_code,
                rate,
                date,
                TYPEOF(rate) as rate_type
            FROM `{gcp.project_id}.{gcp.dataset_id}.Rate`
            ORDER BY date DESC
            LIMIT 3
        """
        
        # Note: TYPEOF is not a standard SQL function, so this may fail
        # Try without it
        query_simple = f"""
            SELECT 
                currency_code,
                rate,
                date
            FROM `{gcp.project_id}.{gcp.dataset_id}.Rate`
            ORDER BY date DESC
            LIMIT 3
        """
        
        results = gcp.query_bigquery(query_simple)
        
        print("✅ Sample data from Rate table:")
        print(f"   {'Currency':<10} {'Rate':<15} {'Date':<12} {'Python Type':<15}")
        print(f"   {'-'*10} {'-'*15} {'-'*12} {'-'*15}")
        
        for row in results:
            python_type = type(row.rate).__name__
            rate_str = f"{row.rate:.4f}" if isinstance(row.rate, (int, float)) else str(row.rate)
            print(f"   {row.currency_code:<10} {rate_str:<15} {str(row.date):<12} {python_type:<15}")
        
        # Check if rate is returned as float
        first_row = list(gcp.query_bigquery(query_simple))[0]
        if isinstance(first_row.rate, float):
            print(f"\n✅ Rate values are returned as Python float (correct)")
        else:
            print(f"\n⚠️  Rate values are returned as {type(first_row.rate).__name__}")
        
        return True
        
    except Exception as e:
        print(f"❌ Sample data check failed: {e}")
        return False


def check_test_files():
    """Verify test files exist."""
    print("\n" + "=" * 70)
    print("TEST FILES CHECK")
    print("=" * 70)
    
    test_file = "test_bigquery_sync.py"
    if os.path.exists(test_file):
        print(f"✅ {test_file} exists")
        
        # Count test functions
        with open(test_file, 'r') as f:
            content = f.read()
            test_count = content.count("def test_")
        
        print(f"   Found {test_count} test functions")
        return True
    else:
        print(f"❌ {test_file} NOT FOUND")
        return False


def main():
    """Run all pre-implementation checks."""
    print("\n" + "=" * 70)
    print("BIGQUERY POWER BI FIX - PRE-IMPLEMENTATION VERIFICATION")
    print("=" * 70)
    print("\nThis script verifies the environment before implementing the fix.")
    print("Run this to identify potential issues early.\n")
    
    checks = [
        ("Environment Variables", check_environment),
        ("Python Dependencies", check_dependencies),
        ("BigQuery Connectivity", check_bigquery_connectivity),
        ("Current Schema", check_current_schema),
        ("Sample Data", check_sample_data),
        ("Test Files", check_test_files),
    ]
    
    results = {}
    for name, check_func in checks:
        try:
            results[name] = check_func()
        except Exception as e:
            print(f"\n❌ {name} check crashed: {e}")
            results[name] = False
    
    # Summary
    print("\n" + "=" * 70)
    print("SUMMARY")
    print("=" * 70)
    
    passed = sum(1 for v in results.values() if v)
    total = len(results)
    
    for name, passed_check in results.items():
        status = "✅ PASS" if passed_check else "❌ FAIL"
        print(f"{status:12} {name}")
    
    print(f"\nOverall: {passed}/{total} checks passed")
    
    if passed == total:
        print("\n🎉 All checks passed! Ready to implement the fix.")
        print("\nNext steps:")
        print("1. Review QUICKSTART_POWERBI_FIX.md")
        print("2. Run: pytest test_bigquery_sync.py -v (expect failures - RED phase)")
        print("3. Implement the view creation functions")
        print("4. Re-run tests to verify (GREEN phase)")
    else:
        print("\n⚠️  Some checks failed. Fix issues before proceeding.")
        print("\nRefer to:")
        print("- BIGQUERY_POWERBI_FIX.md for troubleshooting")
        print("- QUICKSTART_POWERBI_FIX.md for setup instructions")
    
    return 0 if passed == total else 1


if __name__ == "__main__":
    sys.exit(main())
