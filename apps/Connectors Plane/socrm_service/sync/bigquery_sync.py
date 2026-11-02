"""
BigQuery Sync for SuperOffice CRM Integration

Syncs SuperOffice CRM data to Google BigQuery for data warehousing and analysis.
"""

import logging
from datetime import datetime
from typing import Dict, List, Any, Optional
from google.cloud import bigquery

from shared.gcp_client import get_gcp_client

logger = logging.getLogger(__name__)


# BigQuery table schemas for SuperOffice entities

COMPANIES_SCHEMA = [
    bigquery.SchemaField("company_id", "STRING", mode="REQUIRED", description="SuperOffice Contact ID"),
    bigquery.SchemaField("name", "STRING", mode="REQUIRED", description="Company name"),
    bigquery.SchemaField("organization_number", "STRING", description="Organization number"),
    bigquery.SchemaField("country", "STRING", description="Country"),
    bigquery.SchemaField("city", "STRING", description="City"),
    bigquery.SchemaField("postal_code", "STRING", description="Postal code"),
    bigquery.SchemaField("category", "STRING", description="Company category"),
    bigquery.SchemaField("business", "STRING", description="Business type"),
    bigquery.SchemaField("email", "STRING", description="Primary email"),
    bigquery.SchemaField("phone", "STRING", description="Primary phone"),
    bigquery.SchemaField("website", "STRING", description="Website URL"),
    bigquery.SchemaField("is_active", "BOOLEAN", description="Is company active"),
    bigquery.SchemaField("created_date", "TIMESTAMP", description="Created timestamp"),
    bigquery.SchemaField("updated_date", "TIMESTAMP", description="Last updated timestamp"),
    bigquery.SchemaField("synced_at", "TIMESTAMP", mode="REQUIRED", description="When synced to BigQuery"),
    bigquery.SchemaField("tenant_id", "STRING", mode="REQUIRED", description="Organization tenant ID"),
    bigquery.SchemaField("_metadata", "JSON", description="Additional metadata"),
]

CONTACTS_SCHEMA = [
    bigquery.SchemaField("contact_id", "STRING", mode="REQUIRED", description="SuperOffice Person ID"),
    bigquery.SchemaField("first_name", "STRING", description="First name"),
    bigquery.SchemaField("middle_name", "STRING", description="Middle name"),
    bigquery.SchemaField("last_name", "STRING", description="Last name"),
    bigquery.SchemaField("email", "STRING", description="Primary email"),
    bigquery.SchemaField("phone", "STRING", description="Primary phone"),
    bigquery.SchemaField("mobile", "STRING", description="Mobile phone"),
    bigquery.SchemaField("direct_phone", "STRING", description="Direct phone"),
    bigquery.SchemaField("company_id", "STRING", description="Related company ID"),
    bigquery.SchemaField("company_name", "STRING", description="Related company name"),
    bigquery.SchemaField("position", "STRING", description="Job position/title"),
    bigquery.SchemaField("department", "STRING", description="Department"),
    bigquery.SchemaField("is_active", "BOOLEAN", description="Is contact active"),
    bigquery.SchemaField("created_date", "TIMESTAMP", description="Created timestamp"),
    bigquery.SchemaField("updated_date", "TIMESTAMP", description="Last updated timestamp"),
    bigquery.SchemaField("synced_at", "TIMESTAMP", mode="REQUIRED", description="When synced to BigQuery"),
    bigquery.SchemaField("tenant_id", "STRING", mode="REQUIRED", description="Organization tenant ID"),
    bigquery.SchemaField("_metadata", "JSON", description="Additional metadata"),
]

SALES_SCHEMA = [
    bigquery.SchemaField("sale_id", "STRING", mode="REQUIRED", description="SuperOffice Sale ID"),
    bigquery.SchemaField("title", "STRING", mode="REQUIRED", description="Sale title/heading"),
    bigquery.SchemaField("amount", "FLOAT64", description="Sale amount in original currency"),
    bigquery.SchemaField("currency", "STRING", description="Currency code"),
    bigquery.SchemaField("amount_nok", "FLOAT64", description="Converted amount in NOK"),
    bigquery.SchemaField("status", "STRING", description="Sale status"),
    bigquery.SchemaField("stage", "STRING", description="Sale stage"),
    bigquery.SchemaField("probability", "INTEGER", description="Win probability percentage"),
    bigquery.SchemaField("company_id", "STRING", description="Related company ID"),
    bigquery.SchemaField("company_name", "STRING", description="Related company name"),
    bigquery.SchemaField("contact_id", "STRING", description="Related contact ID"),
    bigquery.SchemaField("contact_name", "STRING", description="Related contact name"),
    bigquery.SchemaField("owner_id", "STRING", description="Sale owner/associate ID"),
    bigquery.SchemaField("owner", "STRING", description="Sale owner name"),
    bigquery.SchemaField("sale_date", "DATE", description="Sale date"),
    bigquery.SchemaField("close_date", "DATE", description="Expected close date"),
    bigquery.SchemaField("created_date", "TIMESTAMP", description="Created timestamp"),
    bigquery.SchemaField("updated_date", "TIMESTAMP", description="Last updated timestamp"),
    bigquery.SchemaField("synced_at", "TIMESTAMP", mode="REQUIRED", description="When synced to BigQuery"),
    bigquery.SchemaField("tenant_id", "STRING", mode="REQUIRED", description="Organization tenant ID"),
    bigquery.SchemaField("_metadata", "JSON", description="Additional metadata"),
]

PROJECTS_SCHEMA = [
    bigquery.SchemaField("project_id", "STRING", mode="REQUIRED", description="SuperOffice Project ID"),
    bigquery.SchemaField("name", "STRING", mode="REQUIRED", description="Project name"),
    bigquery.SchemaField("description", "STRING", description="Project description"),
    bigquery.SchemaField("type", "STRING", description="Project type"),
    bigquery.SchemaField("status", "STRING", description="Project status"),
    bigquery.SchemaField("company_id", "STRING", description="Related company ID"),
    bigquery.SchemaField("company_name", "STRING", description="Related company name"),
    bigquery.SchemaField("start_date", "DATE", description="Project start date"),
    bigquery.SchemaField("end_date", "DATE", description="Project end date"),
    bigquery.SchemaField("is_active", "BOOLEAN", description="Is project active"),
    bigquery.SchemaField("created_date", "TIMESTAMP", description="Created timestamp"),
    bigquery.SchemaField("updated_date", "TIMESTAMP", description="Last updated timestamp"),
    bigquery.SchemaField("synced_at", "TIMESTAMP", mode="REQUIRED", description="When synced to BigQuery"),
    bigquery.SchemaField("tenant_id", "STRING", mode="REQUIRED", description="Organization tenant ID"),
    bigquery.SchemaField("_metadata", "JSON", description="Additional metadata"),
]


def ensure_tables_exist() -> bool:
    """
    Ensure all SuperOffice tables exist in BigQuery.
    Creates tables if they don't exist.
    
    Returns:
        True if all tables exist or were created successfully
    """
    try:
        gcp = get_gcp_client()
        
        tables = {
            "Company": (COMPANIES_SCHEMA, "synced_at"),
            "Contact": (CONTACTS_SCHEMA, "synced_at"),
            "Sale": (SALES_SCHEMA, "synced_at"),
            "Project": (PROJECTS_SCHEMA, "synced_at"),
        }
        
        for table_id, schema_info in tables.items():
            if isinstance(schema_info, tuple):
                schema, partition_field = schema_info
                gcp.create_bigquery_table(
                    table_id=table_id,
                    schema=schema,
                    partitioning_field=partition_field
                )
            else:
                gcp.create_bigquery_table(
                    table_id=table_id,
                    schema=schema_info
                )
        
        logger.info("All SuperOffice BigQuery tables verified/created")
        return True
        
    except Exception as e:
        logger.error(f"Failed to ensure SuperOffice tables exist: {e}")
        return False


def sync_companies_to_bigquery(companies: List[Dict[str, Any]], tenant_id: str) -> Dict[str, Any]:
    """
    Sync companies data to BigQuery.
    
    Args:
        companies: List of company records from SuperOffice
        tenant_id: Organization identifier
        
    Returns:
        Sync result summary
    """
    table_name = "Company"
    
    if not companies:
        return {"success": True, "table": table_name, "rows_synced": 0, "message": "No companies to sync"}
    
    try:
        gcp = get_gcp_client()
        synced_at = datetime.utcnow()
        rows = []
        
        for company in companies:
            # Extract nested values from SuperOffice format
            category_val = _extract_value(company.get("Category"))
            country_val = _extract_value(company.get("Country"))
            business_val = _extract_value(company.get("Business"))
            
            # Extract email/phone from arrays if present
            email_val = _extract_first_email(company)
            phone_val = _extract_first_phone(company)
            
            # Handle address fields
            city_val = company.get("City", company.get("city"))
            postal_code_val = company.get("PostalCode", company.get("postal_code", company.get("postalCode")))
            
            # Handle website
            website_val = company.get("WebPage", company.get("website", company.get("URL")))

            row = {
                "company_id": str(company.get("ContactId", company.get("id", company.get("company_id", "")))),
                "name": company.get("Name", company.get("name", "")),
                "organization_number": company.get("OrgNr", company.get("organizationNumber", company.get("organization_number"))),
                "country": country_val,
                "city": city_val,
                "postal_code": postal_code_val,
                "category": category_val,
                "business": business_val,
                "email": email_val,
                "phone": phone_val,
                "website": website_val,
                "is_active": _parse_active_status(company),
                "created_date": _parse_timestamp(company.get("CreatedDate", company.get("created_date", company.get("createdDate")))),
                "updated_date": _parse_timestamp(company.get("UpdatedDate", company.get("updated_date", company.get("updatedDate")))),
                "synced_at": synced_at.isoformat(),
                "tenant_id": tenant_id,
                "_metadata": {
                    "source": "superoffice_crm",
                    "sync_timestamp": synced_at.isoformat(),
                    "raw_id": company.get("ContactId", company.get("id"))
                }
            }
            rows.append(row)
        
        # Insert into BigQuery
        gcp.insert_rows_to_bigquery(table_name, rows)
        
        logger.info(f"Successfully synced {len(rows)} companies to BigQuery")
        return {
            "success": True,
            "table": table_name,
            "rows_synced": len(rows),
            "synced_at": synced_at.isoformat()
        }
        
    except Exception as e:
        logger.error(f"Error syncing companies: {e}", exc_info=True)
        return {"success": False, "error": str(e), "table": table_name}


def sync_contacts_to_bigquery(contacts: List[Dict[str, Any]], tenant_id: str) -> Dict[str, Any]:
    """
    Sync contacts (persons) data to BigQuery.
    
    Args:
        contacts: List of contact records from SuperOffice
        tenant_id: Organization identifier
        
    Returns:
        Sync result summary
    """
    table_name = "Contact"
    
    if not contacts:
        return {"success": True, "table": table_name, "rows_synced": 0, "message": "No contacts to sync"}
    
    try:
        gcp = get_gcp_client()
        synced_at = datetime.utcnow()
        rows = []
        
        for contact in contacts:
            # Extract company relationship
            company_id_val, company_name_val = _extract_company_relation(contact)
            
            # Extract nested values
            position_val = _extract_value(contact.get("Position"))
            
            # Extract email/phone
            email_val = _extract_first_email(contact)
            phone_val = _extract_first_phone(contact)
            mobile_val = _extract_first_mobile(contact)
            direct_phone_val = _extract_first_direct_phone(contact)

            row = {
                "contact_id": str(contact.get("PersonId", contact.get("id", contact.get("contact_id", "")))),
                "first_name": contact.get("Firstname", contact.get("firstName", contact.get("first_name"))),
                "middle_name": contact.get("MiddleName", contact.get("middleName", contact.get("middle_name"))),
                "last_name": contact.get("Lastname", contact.get("lastName", contact.get("last_name"))),
                "email": email_val,
                "phone": phone_val,
                "mobile": mobile_val,
                "direct_phone": direct_phone_val,
                "company_id": company_id_val,
                "company_name": company_name_val,
                "position": position_val,
                "department": contact.get("Department", contact.get("department")),
                "is_active": not contact.get("Retired", False) if "Retired" in contact else _parse_active_status(contact),
                "created_date": _parse_timestamp(contact.get("CreatedDate", contact.get("created_date", contact.get("createdDate")))),
                "updated_date": _parse_timestamp(contact.get("UpdatedDate", contact.get("updated_date", contact.get("updatedDate")))),
                "synced_at": synced_at.isoformat(),
                "tenant_id": tenant_id,
                "_metadata": {
                    "source": "superoffice_crm",
                    "sync_timestamp": synced_at.isoformat(),
                    "raw_id": contact.get("PersonId", contact.get("id"))
                }
            }
            rows.append(row)
        
        gcp.insert_rows_to_bigquery(table_name, rows)
        
        logger.info(f"Successfully synced {len(rows)} contacts to BigQuery")
        return {
            "success": True,
            "table": table_name,
            "rows_synced": len(rows),
            "synced_at": synced_at.isoformat()
        }
        
    except Exception as e:
        logger.error(f"Error syncing contacts: {e}", exc_info=True)
        return {"success": False, "error": str(e), "table": table_name}


def sync_sales_to_bigquery(sales: List[Dict[str, Any]], tenant_id: str) -> Dict[str, Any]:
    """
    Sync sales data to BigQuery.
    
    Args:
        sales: List of sale records from SuperOffice
        tenant_id: Organization identifier
        
    Returns:
        Sync result summary
    """
    table_name = "Sale"
    
    if not sales:
        return {"success": True, "table": table_name, "rows_synced": 0, "message": "No sales to sync"}
    
    try:
        gcp = get_gcp_client()
        synced_at = datetime.utcnow()
        rows = []
        
        for sale in sales:
            # Extract company relationship
            company_id_val, company_name_val = _extract_company_relation(sale, key="Contact")
            
            # Extract contact/person relationship
            contact_id_val, contact_name_val = _extract_person_relation(sale)
            
            # Extract nested values
            currency_val = _extract_value(sale.get("Currency"), default="NOK")
            stage_val = _extract_value(sale.get("SaleStage"))
            
            # Extract owner/associate info
            owner_id_val, owner_name_val = _extract_associate(sale)
            
            # Parse amount
            amount_val = float(sale.get("Amount", sale.get("amount", 0)))
            amount_nok_val = float(sale.get("amount_nok", amount_val))  # TODO: Currency conversion

            row = {
                "sale_id": str(sale.get("SaleId", sale.get("id", sale.get("sale_id", "")))),
                "title": sale.get("Heading", sale.get("title", "")),
                "amount": amount_val,
                "currency": currency_val,
                "amount_nok": amount_nok_val,
                "status": sale.get("Status", sale.get("status")),
                "stage": stage_val,
                "probability": int(sale.get("Probability", sale.get("probability", 0))),
                "company_id": company_id_val,
                "company_name": company_name_val,
                "contact_id": contact_id_val,
                "contact_name": contact_name_val,
                "owner_id": owner_id_val,
                "owner": owner_name_val,
                "sale_date": _parse_date(sale.get("Saledate", sale.get("sale_date", sale.get("saleDate")))),
                "close_date": _parse_date(sale.get("Saledate", sale.get("close_date", sale.get("closeDate")))),
                "created_date": _parse_timestamp(sale.get("CreatedDate", sale.get("created_date", sale.get("createdDate")))),
                "updated_date": _parse_timestamp(sale.get("UpdatedDate", sale.get("updated_date", sale.get("updatedDate")))),
                "synced_at": synced_at.isoformat(),
                "tenant_id": tenant_id,
                "_metadata": {
                    "source": "superoffice_crm",
                    "sync_timestamp": synced_at.isoformat(),
                    "raw_id": sale.get("SaleId", sale.get("id"))
                }
            }
            rows.append(row)
        
        gcp.insert_rows_to_bigquery(table_name, rows)
        
        logger.info(f"Successfully synced {len(rows)} sales to BigQuery")
        return {
            "success": True,
            "table": table_name,
            "rows_synced": len(rows),
            "synced_at": synced_at.isoformat()
        }
        
    except Exception as e:
        logger.error(f"Error syncing sales: {e}", exc_info=True)
        return {"success": False, "error": str(e), "table": table_name}


def sync_projects_to_bigquery(projects: List[Dict[str, Any]], tenant_id: str) -> Dict[str, Any]:
    """
    Sync projects data to BigQuery.
    
    Args:
        projects: List of project records from SuperOffice
        tenant_id: Organization identifier
        
    Returns:
        Sync result summary
    """
    table_name = "Project"
    
    if not projects:
        return {"success": True, "table": table_name, "rows_synced": 0, "message": "No projects to sync"}
    
    try:
        gcp = get_gcp_client()
        synced_at = datetime.utcnow()
        rows = []
        
        for project in projects:
            # Extract company relationship
            company_id_val, company_name_val = _extract_company_relation(project, key="Contact")
            
            # Extract nested values
            type_val = _extract_value(project.get("Type"))
            status_val = _extract_value(project.get("Status"))

            row = {
                "project_id": str(project.get("ProjectId", project.get("id", project.get("project_id", "")))),
                "name": project.get("Name", project.get("name", "")),
                "description": project.get("Description", project.get("description")),
                "type": type_val,
                "status": status_val,
                "company_id": company_id_val,
                "company_name": company_name_val,
                "start_date": _parse_date(project.get("StartDate", project.get("start_date", project.get("startDate")))),
                "end_date": _parse_date(project.get("EndDate", project.get("end_date", project.get("endDate")))),
                "is_active": _parse_active_status(project),
                "created_date": _parse_timestamp(project.get("CreatedDate", project.get("created_date", project.get("createdDate")))),
                "updated_date": _parse_timestamp(project.get("UpdatedDate", project.get("updated_date", project.get("updatedDate")))),
                "synced_at": synced_at.isoformat(),
                "tenant_id": tenant_id,
                "_metadata": {
                    "source": "superoffice_crm",
                    "sync_timestamp": synced_at.isoformat(),
                    "raw_id": project.get("ProjectId", project.get("id"))
                }
            }
            rows.append(row)
        
        gcp.insert_rows_to_bigquery(table_name, rows)
        
        logger.info(f"Successfully synced {len(rows)} projects to BigQuery")
        return {
            "success": True,
            "table": table_name,
            "rows_synced": len(rows),
            "synced_at": synced_at.isoformat()
        }
        
    except Exception as e:
        logger.error(f"Error syncing projects: {e}", exc_info=True)
        return {"success": False, "error": str(e), "table": table_name}


def sync_all_to_bigquery(data: Dict[str, List[Dict]], tenant_id: str) -> Dict[str, Any]:
    """
    Sync all SuperOffice entities to BigQuery.
    
    Args:
        data: Dictionary with entity types as keys ('companies', 'contacts', 'sales', 'projects')
        tenant_id: Organization identifier
        
    Returns:
        Combined sync results
    """
    # Ensure tables exist before syncing
    if not ensure_tables_exist():
        return {
            "success": False,
            "error": "Failed to ensure BigQuery tables exist",
            "timestamp": datetime.utcnow().isoformat()
        }
    
    results = {}
    
    # Sync companies
    if "companies" in data and data["companies"]:
        results["companies"] = sync_companies_to_bigquery(data["companies"], tenant_id)
    
    # Sync contacts
    if "contacts" in data and data["contacts"]:
        results["contacts"] = sync_contacts_to_bigquery(data["contacts"], tenant_id)
    
    # Sync sales
    if "sales" in data and data["sales"]:
        results["sales"] = sync_sales_to_bigquery(data["sales"], tenant_id)
    
    # Sync projects
    if "projects" in data and data["projects"]:
        results["projects"] = sync_projects_to_bigquery(data["projects"], tenant_id)
    
    # Calculate overall success
    all_success = all(r.get("success", False) for r in results.values()) if results else False
    total_rows = sum(r.get("rows_synced", 0) for r in results.values())
    
    return {
        "success": all_success,
        "tables_synced": len(results),
        "total_rows": total_rows,
        "results": results,
        "timestamp": datetime.utcnow().isoformat()
    }


# Helper functions for data extraction

def _extract_value(obj: Any, default: Optional[str] = None) -> Optional[str]:
    """Extract value from SuperOffice nested object format {"Value": "..."}"""
    if obj is None:
        return default
    if isinstance(obj, dict):
        return obj.get("Value", default)
    return str(obj) if obj else default


def _extract_first_email(entity: Dict[str, Any]) -> Optional[str]:
    """Extract first email from entity (handles both simple string and Email array)"""
    # Direct email field
    if entity.get("Email"):
        return entity["Email"]
    if entity.get("email"):
        return entity["email"]
    
    # Email array from SuperOffice
    emails = entity.get("Emails") or entity.get("EmailAddresses")
    if emails and isinstance(emails, list) and len(emails) > 0:
        return _extract_value(emails[0])
    
    return None


def _extract_first_phone(entity: Dict[str, Any]) -> Optional[str]:
    """Extract first phone number"""
    if entity.get("Phone"):
        return entity["Phone"]
    if entity.get("phone"):
        return entity["phone"]
    
    phones = entity.get("Phones")
    if phones and isinstance(phones, list) and len(phones) > 0:
        return _extract_value(phones[0])
    
    return None


def _extract_first_mobile(entity: Dict[str, Any]) -> Optional[str]:
    """Extract first mobile number"""
    if entity.get("Mobile"):
        return entity["Mobile"]
    if entity.get("mobile"):
        return entity["mobile"]
    
    mobiles = entity.get("MobilePhones")
    if mobiles and isinstance(mobiles, list) and len(mobiles) > 0:
        return _extract_value(mobiles[0])
    
    return None


def _extract_first_direct_phone(entity: Dict[str, Any]) -> Optional[str]:
    """Extract direct phone number"""
    if entity.get("DirectPhone"):
        return entity["DirectPhone"]
    if entity.get("direct_phone"):
        return entity["direct_phone"]
    
    direct_phones = entity.get("DirectPhones")
    if direct_phones and isinstance(direct_phones, list) and len(direct_phones) > 0:
        return _extract_value(direct_phones[0])
    
    return None


def _extract_company_relation(entity: Dict[str, Any], key: str = "Contact") -> tuple[Optional[str], Optional[str]]:
    """Extract company ID and name from entity relation"""
    company_id_val = None
    company_name_val = None
    
    # Check nested object
    if entity.get(key):
        company_obj = entity.get(key)
        if isinstance(company_obj, dict):
            company_id_val = str(company_obj.get("ContactId", "")) if company_obj.get("ContactId") else None
            company_name_val = company_obj.get("Name")
    
    # Fallback to direct fields
    if not company_id_val:
        company_id_val = entity.get("companyId") or entity.get("company_id")
        if company_id_val:
            company_id_val = str(company_id_val)
    
    if not company_name_val:
        company_name_val = entity.get("company") or entity.get("company_name")
    
    return company_id_val, company_name_val


def _extract_person_relation(entity: Dict[str, Any]) -> tuple[Optional[str], Optional[str]]:
    """Extract person/contact ID and name from entity relation"""
    contact_id_val = None
    contact_name_val = None
    
    # Check nested Person object
    if entity.get("Person"):
        person_obj = entity.get("Person")
        if isinstance(person_obj, dict):
            contact_id_val = str(person_obj.get("PersonId", "")) if person_obj.get("PersonId") else None
            first_name = person_obj.get("Firstname", "")
            last_name = person_obj.get("Lastname", "")
            if first_name or last_name:
                contact_name_val = f"{first_name} {last_name}".strip()
    
    # Fallback to direct fields
    if not contact_id_val:
        contact_id_val = entity.get("contactId") or entity.get("contact_id")
        if contact_id_val:
            contact_id_val = str(contact_id_val)
    
    if not contact_name_val:
        contact_name_val = entity.get("contact") or entity.get("contact_name")
    
    return contact_id_val, contact_name_val


def _extract_associate(entity: Dict[str, Any]) -> tuple[Optional[str], Optional[str]]:
    """Extract associate/owner ID and name"""
    owner_id_val = None
    owner_name_val = None
    
    if entity.get("Associate"):
        associate_obj = entity.get("Associate")
        if isinstance(associate_obj, dict):
            owner_id_val = str(associate_obj.get("AssociateId", "")) if associate_obj.get("AssociateId") else None
            owner_name_val = associate_obj.get("Name")
    
    if not owner_name_val:
        owner_name_val = entity.get("owner")
    
    return owner_id_val, owner_name_val


def _parse_active_status(entity: Dict[str, Any]) -> bool:
    """Parse active status from various field formats"""
    # SuperOffice uses "Active" field (can be int or bool)
    if "Active" in entity:
        active = entity["Active"]
        if isinstance(active, bool):
            return active
        if isinstance(active, int):
            return active != 0
    
    # Check other common field names
    if "is_active" in entity:
        return bool(entity["is_active"])
    if "isActive" in entity:
        return bool(entity["isActive"])
    
    # Default to True
    return True


def _parse_timestamp(value: Any) -> Optional[str]:
    """Parse timestamp to ISO format"""
    if not value:
        return None
    
    if isinstance(value, datetime):
        return value.isoformat()
    
    if isinstance(value, str):
        # Already a string, return as-is (assume it's ISO format)
        return value
    
    return None


def _parse_date(value: Any) -> Optional[str]:
    """Parse date to YYYY-MM-DD format"""
    if not value:
        return None
    
    if isinstance(value, datetime):
        return value.date().isoformat()
    
    if isinstance(value, str):
        # Try to parse and extract date part
        try:
            if "T" in value:  # ISO format with time
                return value.split("T")[0]
            return value  # Assume already in date format
        except:
            return None
    
    return None
