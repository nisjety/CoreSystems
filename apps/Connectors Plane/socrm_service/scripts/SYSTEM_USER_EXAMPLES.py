"""
SuperOffice System User Authentication - Example Usage

This file demonstrates how to use the System User flow for server-to-server
(back-channel) authentication with SuperOffice CRM.

System User authentication enables:
- Non-interactive API access (no user login required)
- Unlimited data access (not restricted by user permissions)
- Long-running background processes
- Scheduled synchronization jobs
- Automated workflows

Documentation: https://docs.superoffice.com/en/api/netserver/authentication/online/auth-application/
"""

import os
import sys
from pathlib import Path

# Add parent directory to path
sys.path.insert(0, str(Path(__file__).parent))

from config.settings import Settings
from auth.system_user_handler import SystemUserHandler, SystemUserError
from clients.socrm_system_user_client import SuperOfficeSystemUserClient


# ============================================================
# EXAMPLE 1: Basic System User Authentication
# ============================================================

def example_basic_authentication():
    """
    Basic example of getting a System User ticket and making API calls.
    """
    print("\n" + "="*60)
    print("EXAMPLE 1: Basic System User Authentication")
    print("="*60)
    
    # Initialize settings
    settings = Settings()
    
    # System User Token - obtain this from the id_token claim when
    # an admin first authorizes your application via OAuth
    # Format: "YourAppName-abc123xyz456..."
    system_user_token = os.getenv("SOCRM_SYSTEM_USER_TOKEN", "YourApp-abc123...")
    
    # Context Identifier - the tenant/customer ID
    # Format: "Cust12345"
    context_identifier = os.getenv("SOCRM_CONTEXT_IDENTIFIER", "Cust12345")
    
    try:
        # Step 1: Initialize System User handler
        handler = SystemUserHandler(settings)
        
        # Step 2: Get System User ticket (cached automatically for 6 hours)
        print("\n📝 Obtaining System User ticket...")
        ticket = handler.get_system_user_ticket(
            system_user_token=system_user_token,
            context_identifier=context_identifier
        )
        print(f"✓ Ticket obtained: {ticket[:30]}...")
        
        # Step 3: Create System User client
        print("\n🔌 Creating System User client...")
        client = SuperOfficeSystemUserClient(ticket, settings)
        
        # Step 4: Make API calls
        print("\n🏥 Testing connection...")
        if client.health_check():
            print("✓ Connection successful!")
            
            # Get current principal info
            principal = client.get_current_principal()
            print(f"✓ Authenticated as: {principal.get('FullName', 'Unknown')}")
        else:
            print("✗ Connection failed")
        
        # Step 5: Fetch some data
        print("\n📊 Fetching companies...")
        companies = client.get_companies(page=0, page_size=5)
        print(f"✓ Retrieved {len(companies)} companies")
        
        for company in companies[:3]:
            print(f"  - {company.get('Name')} (ID: {company.get('ContactId')})")
    
    except SystemUserError as e:
        print(f"✗ System User error: {e}")
    except Exception as e:
        print(f"✗ Error: {e}")


# ============================================================
# EXAMPLE 2: Ticket Caching and Refresh
# ============================================================

def example_ticket_caching():
    """
    Demonstrate ticket caching and refresh mechanism.
    
    The System User ticket is valid for 6 hours with sliding expiration.
    The handler automatically caches tickets and only refreshes when needed.
    """
    print("\n" + "="*60)
    print("EXAMPLE 2: Ticket Caching and Refresh")
    print("="*60)
    
    settings = Settings()
    handler = SystemUserHandler(settings)
    
    system_user_token = os.getenv("SOCRM_SYSTEM_USER_TOKEN", "YourApp-abc123...")
    context_identifier = os.getenv("SOCRM_CONTEXT_IDENTIFIER", "Cust12345")
    
    try:
        # First call - fetches new ticket
        print("\n📝 First call - fetching new ticket...")
        ticket1 = handler.get_system_user_ticket(
            system_user_token=system_user_token,
            context_identifier=context_identifier
        )
        print(f"✓ Ticket 1: {ticket1[:30]}...")
        
        # Check ticket status
        status = handler.get_ticket_status(context_identifier, system_user_token)
        if status:
            print(f"\nTicket Status:")
            print(f"  - Created: {status['created_at']}")
            print(f"  - Last used: {status['last_used']}")
            print(f"  - Expires: {status['expires_at']}")
            print(f"  - Time remaining: {status['time_remaining_minutes']} minutes")
            print(f"  - Valid: {status['is_valid']}")
        
        # Second call - uses cached ticket
        print("\n📝 Second call - using cached ticket...")
        ticket2 = handler.get_system_user_ticket(
            system_user_token=system_user_token,
            context_identifier=context_identifier
        )
        print(f"✓ Ticket 2: {ticket2[:30]}...")
        print(f"✓ Tickets match: {ticket1 == ticket2}")
        
        # Force refresh
        print("\n🔄 Force refresh...")
        ticket3 = handler.get_system_user_ticket(
            system_user_token=system_user_token,
            context_identifier=context_identifier,
            force_refresh=True
        )
        print(f"✓ New ticket: {ticket3[:30]}...")
        print(f"✓ Ticket changed: {ticket1 != ticket3}")
    
    except SystemUserError as e:
        print(f"✗ Error: {e}")


# ============================================================
# EXAMPLE 3: Background Data Sync Job
# ============================================================

def example_background_sync():
    """
    Example of a background job that syncs data from SuperOffice.
    
    This is a common use case for System User authentication:
    - Runs without user interaction
    - Scheduled to run periodically
    - Accesses all tenant data
    """
    print("\n" + "="*60)
    print("EXAMPLE 3: Background Data Sync Job")
    print("="*60)
    
    settings = Settings()
    handler = SystemUserHandler(settings)
    
    system_user_token = os.getenv("SOCRM_SYSTEM_USER_TOKEN")
    context_identifier = os.getenv("SOCRM_CONTEXT_IDENTIFIER")
    
    if not system_user_token or not context_identifier:
        print("⚠️  Set SOCRM_SYSTEM_USER_TOKEN and SOCRM_CONTEXT_IDENTIFIER in .env")
        return
    
    try:
        print("\n🚀 Starting sync job...")
        
        # Get ticket
        ticket = handler.get_system_user_ticket(
            system_user_token=system_user_token,
            context_identifier=context_identifier
        )
        
        # Create client
        client = SuperOfficeSystemUserClient(ticket, settings)
        
        # Sync companies
        print("\n📦 Syncing companies...")
        all_companies = []
        page = 0
        page_size = 100
        
        while True:
            companies = client.get_companies(page=page, page_size=page_size)
            if not companies:
                break
            
            all_companies.extend(companies)
            print(f"  ✓ Fetched page {page + 1} ({len(companies)} companies)")
            page += 1
            
            # In production, you would save to database here
            # save_companies_to_database(companies)
        
        print(f"\n✓ Sync complete! Total companies: {len(all_companies)}")
        
        # Sync sales
        print("\n💰 Syncing sales...")
        sales = client.get_sales(page=0, page_size=50)
        print(f"✓ Fetched {len(sales)} sales")
        
        print("\n✅ Background sync job completed successfully")
    
    except SystemUserError as e:
        print(f"✗ Authentication error: {e}")
        print("💡 Ticket may have expired. Will retry on next run.")
    except Exception as e:
        print(f"✗ Sync failed: {e}")


# ============================================================
# EXAMPLE 4: Handling 401 Errors (Expired Tickets)
# ============================================================

def example_handle_expired_ticket():
    """
    Demonstrate handling of expired tickets.
    
    When a ticket expires or becomes invalid, the API returns 401 Unauthorized.
    You should catch this, get a new ticket, and retry.
    """
    print("\n" + "="*60)
    print("EXAMPLE 4: Handling Expired Tickets")
    print("="*60)
    
    settings = Settings()
    handler = SystemUserHandler(settings)
    
    system_user_token = os.getenv("SOCRM_SYSTEM_USER_TOKEN")
    context_identifier = os.getenv("SOCRM_CONTEXT_IDENTIFIER")
    
    if not system_user_token or not context_identifier:
        print("⚠️  Set environment variables first")
        return
    
    def make_api_call_with_retry():
        """Make API call with automatic ticket refresh on 401."""
        max_retries = 2
        
        for attempt in range(max_retries):
            try:
                # Get ticket (cached if still valid)
                ticket = handler.get_system_user_ticket(
                    system_user_token=system_user_token,
                    context_identifier=context_identifier
                )
                
                # Create client
                client = SuperOfficeSystemUserClient(ticket, settings)
                
                # Make API call
                companies = client.get_companies(page=0, page_size=10)
                return companies
            
            except SystemUserError as e:
                if attempt < max_retries - 1:
                    print(f"⚠️  Ticket expired, refreshing... (attempt {attempt + 1})")
                    # Force refresh on next iteration
                    handler.invalidate_ticket(context_identifier)
                else:
                    print(f"✗ Failed after {max_retries} attempts")
                    raise
    
    try:
        print("\n📞 Making API call with automatic retry...")
        companies = make_api_call_with_retry()
        print(f"✓ Success! Fetched {len(companies)} companies")
    
    except Exception as e:
        print(f"✗ Error: {e}")


# ============================================================
# EXAMPLE 5: Initial Setup - Extract System User Token
# ============================================================

def example_extract_system_user_token():
    """
    How to extract the System User Token from the initial OAuth flow.
    
    When an admin first authorizes your application:
    1. They go through OAuth authorization flow
    2. Your redirect URL receives an id_token (JWT)
    3. The id_token contains a claim with the System User Token
    4. Save this token - you'll need it for all future System User authentication
    """
    print("\n" + "="*60)
    print("EXAMPLE 5: Extract System User Token (Initial Setup)")
    print("="*60)
    
    print("""
    During initial application authorization:
    
    1. Admin visits your OAuth authorization URL:
       https://sod.superoffice.com/login/common/oauth/authorize
         ?client_id=YOUR_CLIENT_ID
         &redirect_uri=YOUR_REDIRECT_URI
         &response_type=id_token token
         &scope=openid
         &state=xyz123
    
    2. Admin logs in and grants consent
    
    3. SuperOffice redirects to your redirect_uri with tokens:
       https://your-app.com/callback#
         access_token=xxx...
         &id_token=yyy...
    
    4. Decode the id_token JWT to extract claims:
    """)
    
    # Example code to decode id_token
    example_code = '''
    import jwt
    
    # Decode without verification (for demonstration)
    # In production, validate with SuperOffice public certificate
    decoded = jwt.decode(id_token, options={"verify_signature": False})
    
    # Extract System User Token claim
    system_user_token = decoded.get("http://schemes.superoffice.net/identity/system_user_token")
    
    # Also extract context identifier
    context_identifier = decoded.get("http://schemes.superoffice.net/identity/ctx")
    
    print(f"System User Token: {system_user_token}")
    print(f"Context Identifier: {context_identifier}")
    
    # Save these values! You'll need them for System User authentication
    # Store securely in environment variables or encrypted database
    '''
    
    print(example_code)
    
    print("""
    5. Save the System User Token and Context Identifier:
       - Add to .env file:
         SOCRM_SYSTEM_USER_TOKEN="YourApp-abc123xyz456..."
         SOCRM_CONTEXT_IDENTIFIER="Cust12345"
       
       - Or store in secure database with encryption
    
    6. Now you can use System User authentication for server-to-server calls!
    
    ⚠️  IMPORTANT:
    - System User Token is constant for each tenant/app combination
    - Keep it secret and secure
    - Only obtain during initial setup or re-authorization
    - Use it to get tickets for actual API authentication
    """)


# ============================================================
# MAIN - Run Examples
# ============================================================

def main():
    """Run all examples."""
    print("\n" + "="*60)
    print("SuperOffice System User Authentication Examples")
    print("="*60)
    
    print("""
    Choose an example to run:
    
    1. Basic Authentication
    2. Ticket Caching and Refresh
    3. Background Data Sync Job
    4. Handle Expired Tickets
    5. Extract System User Token (Setup Guide)
    
    Press Ctrl+C to exit
    """)
    
    try:
        choice = input("Enter choice (1-5): ").strip()
        
        if choice == "1":
            example_basic_authentication()
        elif choice == "2":
            example_ticket_caching()
        elif choice == "3":
            example_background_sync()
        elif choice == "4":
            example_handle_expired_ticket()
        elif choice == "5":
            example_extract_system_user_token()
        else:
            print("Invalid choice")
    
    except KeyboardInterrupt:
        print("\n\nExiting...")
    except Exception as e:
        print(f"\nError: {e}")


if __name__ == "__main__":
    main()
