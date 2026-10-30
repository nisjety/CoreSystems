# SuperOffice CRM GCP Integration

This document details the integration between SuperOffice CRM Service and Google Cloud Platform (BigQuery).

## Overview

The service synchronizes SuperOffice CRM entities (Companies, Contacts, Sales) to BigQuery for analytics and reporting.

## Configuration

The integration is configured via environment variables:

```env
GCP_PROJECT_ID=aquatiq-integrasjonen
GCP_DATASET_ID=dw_1_staging_socrm
GCP_LOCATION=europe-west1
```

## BigQuery Schema

The integration uses the following table structure in the `dw_1_staging_socrm` dataset:

### Table: `Company`
Stores company/customer information.

| Field | Type | Description |
|-------|------|-------------|
| company_id | STRING | Unique identifier |
| name | STRING | Company name |
| organization_number | STRING | Business registration number |
| country | STRING | Country name |
| city | STRING | City |
| category | STRING | Customer category |
| email | STRING | General email |
| phone | STRING | General phone |
| website | STRING | Website URL |
| is_active | BOOLEAN | Active status |
| created_date | TIMESTAMP | Creation timestamp |
| synced_at | TIMESTAMP | Sync timestamp (Partitioning Key) |
| _metadata | JSON | Sync metadata (tenant_id, source, etc.) |

### Table: `Contact`
Stores person/contact information.

| Field | Type | Description |
|-------|------|-------------|
| contact_id | STRING | Unique identifier |
| first_name | STRING | First name |
| last_name | STRING | Last name |
| email | STRING | Email address |
| phone | STRING | Phone number |
| mobile | STRING | Mobile number |
| company_id | STRING | Reference to Company |
| company_name | STRING | Denormalized company name |
| position | STRING | Job title |
| department | STRING | Department |
| is_active | BOOLEAN | Active status |
| created_date | TIMESTAMP | Creation timestamp |
| synced_at | TIMESTAMP | Sync timestamp (Partitioning Key) |
| _metadata | JSON | Sync metadata |

### Table: `Sale`
Stores sales opportunities and orders.

| Field | Type | Description |
|-------|------|-------------|
| sale_id | STRING | Unique identifier |
| title | STRING | Sale title/description |
| amount | FLOAT64 | Amount in original currency |
| currency | STRING | Currency code (e.g., NOK, USD) |
| amount_nok | FLOAT64 | Amount converted to NOK |
| status | STRING | Sale status (Open, Sold, Lost) |
| stage | STRING | Sales stage |
| probability | INTEGER | Probability percentage |
| company_id | STRING | Reference to Company |
| company_name | STRING | Denormalized company name |
| contact_id | STRING | Reference to Contact |
| owner | STRING | Sales rep name |
| sale_date | DATE | Expected/Actual sale date |
| close_date | DATE | Closing date |
| created_date | TIMESTAMP | Creation timestamp |
| synced_at | TIMESTAMP | Sync timestamp (Partitioning Key) |
| _metadata | JSON | Sync metadata |

## Sync Process

1. **Trigger**: Sync is triggered via API endpoint `/api/superoffice/sync/gcp` or scheduled job.
2. **Fetch**: Data is fetched from SuperOffice API.
3. **Transform**: Data is normalized and enriched (e.g., currency conversion).
4. **Load**: Data is inserted into BigQuery tables using `insert_rows_json`.

## Partitioning

All tables are partitioned by `synced_at` (DAY) to optimize query performance and cost.
