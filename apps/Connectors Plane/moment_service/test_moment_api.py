#!/usr/bin/env python3
"""
Test script for Moment API integration.

Usage:
    python test_moment_api.py
"""

import os
import sys
from datetime import datetime, timedelta

# Add parent directory to path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

# Windows consoles default to a legacy codepage (e.g. cp1252) that can't encode
# the unicode symbols (checkmarks, emoji) printed below; force UTF-8 output.
if sys.stdout.encoding and sys.stdout.encoding.lower() != "utf-8":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

from clients import MomentAPIClient
from config import config


def print_section(title: str):
    """Print a section header."""
    print("\n" + "=" * 60)
    print(f"  {title}")
    print("=" * 60)


def test_connection():
    """Test basic connection to Moment API."""
    print_section("Testing Connection")
    
    try:
        client = MomentAPIClient()
        print(f"✅ Client initialized")
        print(f"   Company: {client.company_code}")
        print(f"   Base URL: {client.base_url}")
        return client
    except Exception as e:
        print(f"❌ Failed to initialize client: {e}")
        sys.exit(1)


def test_projects(client: MomentAPIClient):
    """Test fetching projects."""
    print_section("Testing Projects")
    
    try:
        projects = client.get_projects(includes="base")
        
        if isinstance(projects, dict) and "content" in projects:
            projects_list = projects["content"]
        else:
            projects_list = projects if isinstance(projects, list) else []
        
        print(f"✅ Fetched {len(projects_list)} projects")
        
        if projects_list:
            project = projects_list[0]
            print(f"\n   Sample project:")
            print(f"   - ID: {project.get('id')}")
            print(f"   - Name: {project.get('name')}")
            
            # Test getting single project
            project_id = project.get('id')
            if project_id:
                single_project = client.get_project(project_id, includes="base")
                print(f"\n✅ Fetched single project: {single_project.get('name')}")
        
    except Exception as e:
        print(f"❌ Error fetching projects: {e}")


def test_customers(client: MomentAPIClient):
    """Test fetching customers."""
    print_section("Testing Customers")
    
    try:
        customers = client.get_customers(includes="base")
        
        if isinstance(customers, dict) and "content" in customers:
            customers_list = customers["content"]
        else:
            customers_list = customers if isinstance(customers, list) else []
        
        print(f"✅ Fetched {len(customers_list)} customers")
        
        if customers_list:
            customer = customers_list[0]
            print(f"\n   Sample customer:")
            print(f"   - ID: {customer.get('id')}")
            print(f"   - Name: {customer.get('name')}")
        
    except Exception as e:
        print(f"❌ Error fetching customers: {e}")


def test_user_accounts(client: MomentAPIClient):
    """Test fetching user accounts."""
    print_section("Testing User Accounts")
    
    try:
        users = client.get_user_accounts(includes="base")
        
        if isinstance(users, dict) and "content" in users:
            users_list = users["content"]
        else:
            users_list = users if isinstance(users, list) else []
        
        print(f"✅ Fetched {len(users_list)} user accounts")
        
        if users_list:
            user = users_list[0]
            print(f"\n   Sample user:")
            print(f"   - ID: {user.get('id')}")
            print(f"   - Name: {user.get('name')}")
        
    except Exception as e:
        print(f"❌ Error fetching user accounts: {e}")


def test_time_records(client: MomentAPIClient):
    """Test fetching time records."""
    print_section("Testing Time Records")
    
    try:
        # Get recent time records
        records = client.get_time_records(includes="base")
        
        if isinstance(records, dict) and "content" in records:
            records_list = records["content"]
        else:
            records_list = records if isinstance(records, list) else []
        
        print(f"✅ Fetched {len(records_list)} time records")
        
        if records_list:
            record = records_list[0]
            print(f"\n   Sample time record:")
            print(f"   - ID: {record.get('id')}")
            print(f"   - Date: {record.get('date')}")
            print(f"   - Hours: {record.get('hours')}")
        
    except Exception as e:
        print(f"❌ Error fetching time records: {e}")


def test_reports(client: MomentAPIClient):
    """Test fetching reports."""
    print_section("Testing Reports")
    
    try:
        # Get last 30 days
        to_date = datetime.now()
        from_date = to_date - timedelta(days=30)
        
        print(f"   Date range: {from_date.date()} to {to_date.date()}")
        
        report = client.get_project_registered_hours_report(
            from_date=from_date.strftime("%Y-%m-%d"),
            to_date=to_date.strftime("%Y-%m-%d"),
            billableRateOption="registered",
        )
        
        print(f"✅ Fetched registered hours report")
        
        if isinstance(report, dict):
            if "content" in report:
                print(f"   Projects in report: {len(report['content'])}")
            if "metadata" in report:
                metadata = report["metadata"]
                if "totalSumInCompanyCurrency" in metadata:
                    print(f"   Total sum: {metadata['totalSumInCompanyCurrency']}")
        
    except Exception as e:
        print(f"❌ Error fetching report: {e}")


def main():
    """Run all tests."""
    print("\n" + "=" * 60)
    print("  MOMENT API INTEGRATION TEST")
    print("=" * 60)
    print(f"\n  Configuration:")
    print(f"  - Company: {config.company_code}")
    print(f"  - Environment: {config.environment}")
    print(f"  - Base URL: {config.base_url}")
    print(f"  - API Key: {'✓ Set' if config.api_key else '✗ Missing'}")
    
    # Initialize client
    client = test_connection()
    
    # Run tests
    test_projects(client)
    test_customers(client)
    test_user_accounts(client)
    test_time_records(client)
    test_reports(client)
    
    # Summary
    print("\n" + "=" * 60)
    print("  TEST SUMMARY")
    print("=" * 60)
    print("\n✅ All tests completed!")
    print("\n  Next steps:")
    print("  - Start the service: python app.py")
    print("  - Or use Docker: docker-compose up -d")
    print("  - Check health: curl http://localhost:5006/health")
    print("  - View API docs: See README.md")
    print("\n")


if __name__ == "__main__":
    main()
