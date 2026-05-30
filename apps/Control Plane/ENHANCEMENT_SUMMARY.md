# Control Plane Enhancement Summary

## What Was Added

This enhancement ensures the Control Plane is the **single source of truth** for all critical organizational and user data, with clear event-driven architecture for downstream consumers like Convex.

---

## 🗄️ New Database Schema (org-core)

### 1. Quotas Table (`org_quotas`)
Tracks resource limits per organization based on their plan.

**Fields:**
- `org_id` - Organization reference
- `quota_key` - Resource type (api_calls, users, storage_mb)
- `quota_value` - Current usage
- `quota_limit` - Maximum allowed (-1 = unlimited)
- `reset_period` - Frequency (daily, monthly, none)
- `last_reset_at` - Last reset timestamp

**Default Quotas by Plan:**
- Free: 1,000 API calls, 5 users, 1GB storage
- Pro: 10,000 API calls, 50 users, 10GB storage
- Enterprise: Unlimited

### 2. Billing Table (`org_billing`)
Stores billing and subscription information.

**Fields:**
- `billing_email` - Billing contact
- `payment_method_id` - Stripe/payment gateway reference
- `subscription_id` - Current subscription
- `subscription_status` - active, past_due, canceled, trialing
- `trial_ends_at` - Trial period end
- `current_period_start/end` - Billing cycle
- `auto_renew` - Auto-renewal flag
- `billing_address` - JSONB address data
- `tax_id` - Tax identification

### 3. Compliance Table (`org_compliance`)
Manages regulatory and security compliance settings.

**Fields:**
- `data_residency` - us, eu, asia
- `gdpr_compliant` - GDPR compliance flag
- `hipaa_compliant` - HIPAA compliance flag
- `soc2_compliant` - SOC2 compliance flag
- `data_retention_days` - Data retention period
- `require_mfa` - Enforce MFA for all users
- `ip_allowlist` - JSONB IP allowlist
- `audit_log_retention_days` - Audit log retention (default: 90)
- `encryption_at_rest` - Encryption flag
- `encryption_in_transit` - TLS enforcement

### 4. Role Mappings Table (`org_role_mappings`)
Defines roles and permissions per organization.

**Fields:**
- `role_name` - owner, admin, member, viewer, or custom
- `permissions` - JSONB array of permission strings
- `is_custom` - Custom role flag

**Default Roles:**
- **Owner**: org:delete, org:update, members:invite, members:remove, billing:manage, roles:manage
- **Admin**: org:update, members:invite, members:remove, roles:manage
- **Member**: org:read, resources:create, resources:read, resources:update
- **Viewer**: org:read, resources:read

### 5. Plan History Table (`org_plan_history`)
Audit trail for plan changes.

**Fields:**
- `previous_plan` - Old plan
- `new_plan` - New plan
- `changed_by` - User who changed it
- `change_reason` - Reason for change
- `changed_at` - Timestamp
- `metadata` - Additional context

---

## 🔐 GDPR Hard Delete Functions

### Auth-Core Functions

**`gdpr_hard_delete_user(user_id)`**
- Deletes all user data from all tables
- Removes sessions, OAuth accounts, 2FA secrets
- Removes organization memberships
- Deletes API keys, bearer tokens
- Returns JSONB with deleted record counts

**`gdpr_anonymize_user(user_id)`**
- Softer alternative to hard delete
- Anonymizes email: `deleted_{user_id}@anonymized.local`
- Sets name to "Deleted User"
- Marks as banned with GDPR reason
- Removes sensitive data but keeps audit trail

### Org-Core Functions

**`gdpr_hard_delete_organization(org_id)`**
- Deletes organization and all related data
- Removes quotas, billing, compliance settings
- Deletes role mappings and plan history
- Returns JSONB with deleted record counts

**`soft_delete_organization(org_id)`**
- Sets `deleted_at` timestamp
- Sets status to 'deleted'
- Allows for recovery period

**`purge_old_deleted_organizations(days_threshold)`**
- Runs periodically (e.g., nightly cron)
- Hard deletes orgs deleted > threshold days ago
- Default: 30 days retention after soft delete

---

## 📡 New NATS Events

### Organization Events

**`organization.created`**
```json
{
  "type": "organization.created",
  "organization_id": "org_abc123",
  "name": "Acme Corp",
  "slug": "acme-corp",
  "plan": "free",
  "creator_id": "user_xyz789",
  "timestamp": "2026-02-19T10:30:00Z"
}
```

**`organization.updated`**
```json
{
  "type": "organization.updated",
  "organization_id": "org_abc123",
  "changes": {
    "name": "Acme Corporation",
    "slug": "acme-corporation"
  },
  "updated_by": "user_xyz789",
  "timestamp": "2026-02-19T11:00:00Z"
}
```

**`organization.plan.changed`** ⭐ NEW
```json
{
  "type": "organization.plan.changed",
  "organization_id": "org_abc123",
  "organization_name": "Acme Corp",
  "previous_plan": "free",
  "new_plan": "pro",
  "changed_by": "user_xyz789",
  "change_reason": "Upgraded to unlock SSO",
  "timestamp": "2026-02-19T12:00:00Z"
}
```

### User Events

**`user.created`** ⭐ NEW
```json
{
  "type": "user.created",
  "user_id": "user_xyz789",
  "email": "john@acme.com",
  "name": "John Doe",
  "role": "user",
  "verified": true,
  "timestamp": "2026-02-19T10:00:00Z"
}
```

**`user.updated`** ⭐ NEW
```json
{
  "type": "user.updated",
  "user_id": "user_xyz789",
  "email": "john@acme.com",
  "changes": {
    "name": "John D. Doe",
    "phone_number": "+1234567890"
  },
  "timestamp": "2026-02-19T14:00:00Z"
}
```

---

## 📂 New Files

### Migrations

1. **`org-core/migrations/002_add_enterprise_fields.up.sql`**
   - Creates quotas, billing, compliance, role_mappings, plan_history tables
   - Inserts default data for existing organizations
   - ~200 lines

2. **`org-core/migrations/002_add_enterprise_fields.down.sql`**
   - Rollback script

3. **`org-core/migrations/003_gdpr_hard_delete.up.sql`**
   - GDPR deletion functions for organizations
   - Soft delete and purge functions

4. **`auth-core/migrations/gdpr_hard_delete.sql`**
   - GDPR deletion functions for users
   - Anonymization function

### Code Files

5. **`org-core/internal/org/types.go`** (UPDATED)
   - Added Go structs: Quota, Billing, Compliance, RoleMapping, PlanHistory
   - Added OrganizationWithDetails struct
   - Added default constants for quotas and permissions

6. **`org-core/internal/org/service_enhanced.go`** (NEW)
   - Enhanced service with quota/billing/compliance support
   - UpdatePlan() method with plan change tracking
   - HardDelete() method with GDPR support
   - Event publishing for all state changes

7. **`org-core/internal/nats/events.go`** (UPDATED)
   - Added event type constants
   - Added event payload structs
   - Documented all event types

8. **`auth-core/src/internal/auth-event.publisher.ts`** (UPDATED)
   - Added UserCreatedEvent, UserUpdatedEvent
   - Added OrganizationUpdatedEvent, OrganizationPlanChangedEvent
   - Dual-publish support (old + new event names)

### Documentation

9. **`CONTROL_PLANE_OWNERSHIP.md`** (NEW)
   - Complete architecture documentation
   - Data ownership boundaries
   - Event catalog
   - GDPR flow diagrams
   - Testing guide

10. **`ENHANCEMENT_SUMMARY.md`** (THIS FILE)

---

## 🚀 Migration Steps

### Step 1: Run Database Migrations

```bash
# org-core migrations
cd org-core
make migrate-up
# or manually:
psql -d org_core -f migrations/002_add_enterprise_fields.up.sql
psql -d org_core -f migrations/003_gdpr_hard_delete.up.sql

# auth-core migrations
cd auth-core
psql -d auth_service -f migrations/gdpr_hard_delete.sql
```

### Step 2: Verify Default Data

```sql
-- Verify quotas created for existing orgs
SELECT org_id, quota_key, quota_limit FROM org_quotas;

-- Verify billing records
SELECT org_id, subscription_status FROM org_billing;

-- Verify compliance settings
SELECT org_id, gdpr_compliant, require_mfa FROM org_compliance;

-- Verify role mappings
SELECT org_id, role_name, permissions FROM org_role_mappings;
```

### Step 3: Update Services (Optional - Code Already Added)

If using the enhanced service layer:

```go
// In org-core main.go or server initialization
publisher := nats.NewPublisher(natsClient)
service := org.NewServiceEnhanced(repo, publisher)

// Now you can use:
service.UpdatePlan(ctx, orgID, "pro", adminUserID, "Upgrade request")
service.HardDelete(ctx, orgID) // GDPR deletion
```

### Step 4: Test Event Publishing

```bash
# Subscribe to all organization events
nats sub "organization.>"

# Create an organization (in another terminal)
curl -X POST http://localhost:3011/api/v2/auth/organization/create \
  -H "Content-Type: application/json" \
  -d '{"name": "Test Org", "slug": "test-org"}'

# You should see: organization.created event
```

### Step 5: Test Plan Change Event

```bash
# Subscribe to plan change events
nats sub "organization.plan.changed"

# Change plan (will need to implement HTTP endpoint or use service directly)
# Expected event output:
{
  "type": "organization.plan.changed",
  "organization_id": "...",
  "previous_plan": "free",
  "new_plan": "pro",
  ...
}
```

### Step 6: Test GDPR Deletion

```bash
# Test user hard delete
psql -d auth_service -c "SELECT gdpr_hard_delete_user('user_test_123');"

# Check result
psql -d auth_service -c "SELECT * FROM \"user\" WHERE id = 'user_test_123';"
# Should return 0 rows

# Test organization hard delete
psql -d org_core -c "SELECT gdpr_hard_delete_organization('org_test_123');"

# Check result
psql -d org_core -c "SELECT * FROM organizations WHERE id = 'org_test_123';"
# Should return 0 rows
```

---

## ✅ Verification Checklist

After migration, verify:

- [ ] `org_quotas` table exists with default quotas for each org
- [ ] `org_billing` table exists with default billing records
- [ ] `org_compliance` table exists with default compliance settings
- [ ] `org_role_mappings` table exists with 4 default roles per org
- [ ] `org_plan_history` table exists (empty initially)
- [ ] GDPR functions exist in both databases (`\df gdpr*` in psql)
- [ ] NATS events published when creating organization
- [ ] `user.created` event published on user registration
- [ ] `organization.plan.changed` event published when plan changes
- [ ] Convex still syncs org/user data correctly
- [ ] Convex schema has `externalOrgId` and `externalAuthId` (already exists)

---

## 🎯 Key Benefits

1. **Complete Ownership**: Control Plane owns all critical data
   - No ambiguity about where to create/update orgs or users
   - Single source of truth for quotas, billing, compliance

2. **GDPR Compliance**: Built-in hard delete and anonymization
   - Legal requirement for right to be forgotten
   - Proper audit trail before deletion

3. **Event-Driven Architecture**: Clear boundaries via events
   - Convex subscribes to events (read-only projection)
   - Other services can also subscribe (analytics, billing, etc.)

4. **Flexible Permissions**: Role-based access with granular permissions
   - Custom roles per organization
   - Extensible permission system

5. **Plan Management**: Automated quota adjustments
   - Plan changes trigger quota updates
   - Historical tracking of plan changes

6. **Compliance Ready**: Settings for GDPR, HIPAA, SOC2
   - Data residency configuration
   - IP allowlists, MFA requirements
   - Audit log retention policies

---

## 🔄 Next Steps (Optional)

### Implement HTTP Endpoints

Add REST/gRPC endpoints in org-core:

```go
POST   /organizations/:id/plan           // Update plan
GET    /organizations/:id/quotas         // Get quotas
POST   /organizations/:id/quotas/:key    // Update quota
GET    /organizations/:id/billing        // Get billing info
PUT    /organizations/:id/billing        // Update billing
GET    /organizations/:id/compliance     // Get compliance settings
PUT    /organizations/:id/compliance     // Update compliance
DELETE /organizations/:id/gdpr           // GDPR hard delete
```

### Add Quota Enforcement

```go
func (s *Service) CheckQuota(ctx context.Context, orgID, quotaKey string) (bool, error) {
    quotas, err := s.repo.GetQuotas(ctx, orgID)
    if err != nil {
        return false, err
    }
    
    for _, quota := range quotas {
        if quota.Key == quotaKey {
            if quota.Limit == -1 {
                return true, nil // Unlimited
            }
            return quota.Value < quota.Limit, nil
        }
    }
    
    return false, fmt.Errorf("quota not found: %s", quotaKey)
}
```

### Add Billing Webhook Integration

```go
// Stripe webhook handler
func (s *Service) HandleStripeWebhook(ctx context.Context, event StripeEvent) error {
    switch event.Type {
    case "customer.subscription.updated":
        // Update subscription status in org_billing
    case "invoice.payment_failed":
        // Mark subscription as past_due
    case "customer.subscription.deleted":
        // Cancel subscription, downgrade to free
    }
}
```

---

## 📚 Documentation References

- [CONTROL_PLANE_OWNERSHIP.md](./CONTROL_PLANE_OWNERSHIP.md) - Full architecture
- [org-core/migrations/002_add_enterprise_fields.up.sql](./org-core/migrations/002_add_enterprise_fields.up.sql) - Schema
- [org-core/internal/org/types.go](./org-core/internal/org/types.go) - Go types
- [auth-core/src/internal/auth-event.publisher.ts](./auth-core/src/internal/auth-event.publisher.ts) - Events

---

## ❓ FAQ

**Q: Do I need to run migrations immediately?**  
A: Yes, if you want to use quotas, billing, or GDPR deletion. Otherwise, the basic org functionality still works.

**Q: Will this break existing organizations?**  
A: No, migrations include default data for existing orgs (default quotas, billing status, etc.)

**Q: Does Convex need schema changes?**  
A: No, Convex already has `externalOrgId` and `externalAuthId` fields for syncing.

**Q: How do I test plan changes?**  
A: Use the enhanced service layer or create an HTTP endpoint that calls `service.UpdatePlan()`.

**Q: Are GDPR functions safe to run in production?**  
A: Yes, but they are **irreversible**. Always backup data before running hard deletes. Use soft delete (`deleted_at`) for recovery periods.

**Q: Can I customize default quotas or roles?**  
A: Yes, modify the migration SQL or update the Go constants in `types.go`.
