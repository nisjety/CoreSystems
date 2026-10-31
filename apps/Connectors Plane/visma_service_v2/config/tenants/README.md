# Tenants

One file per Visma tenant. `vsv2 tenants-register` reads them and writes each tenant's DataMart
company to `vsv2.tenant_company_map`, which is what a DataMart sync looks up (`get_company_id`).
The tenants are the ones v1 registers (`scripts/add_tenants.py` in v1).

| Field | |
|---|---|
| `tenant_id` | Visma tenant UUID |
| `tenant_name` | name in Visma |
| `datamart_company_id` | from `vsv2 dm-companies --tenant <id>`; `null` until discovered (the tenant is then listed, not registered) |
| `status` | `active`, or `historical` for a tenant merged into another entity |
| `valid_to` | last day with data, `YYYY-MM-DD`; required when `historical` |
| `bu_label` | business unit the tenant became |
| `notes` | free text |

An unknown field is an error, and two tenants may not share a company id.

```bash
vsv2 tenants-register --dry-run   # what would change
vsv2 tenants-register             # write it
```
