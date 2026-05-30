# Audit Platform Recommendations for SOC 2 Compliance

**Date**: February 2, 2026  
**Purpose**: Future SOC 2 audit readiness  
**Status**: Evaluation Phase (Implement when needed)

---

## Executive Summary

SOC 2 compliance requires comprehensive audit logging of:
- User actions (authentication, authorization, data access)
- System changes (configuration, deployments, schema updates)
- Security events (failed logins, privilege escalation, suspicious activity)
- Data lifecycle (creation, modification, deletion, exports)

**Timeline**: Implement 3-6 months before SOC 2 audit  
**Cost**: $50-500/month depending on solution  
**Effort**: 1-2 weeks integration

---

## SOC 2 Audit Requirements

### Trust Service Criteria (TSC)

#### CC6.1: Logical and Physical Access Controls
- Log all login attempts (successful and failed)
- Track password changes and resets
- Record multi-factor authentication events
- Monitor privileged access

#### CC6.2: Prior to Issuing System Credentials and Privileges
- Log user provisioning and de-provisioning
- Track permission changes
- Record role assignments

#### CC6.3: Removes Access When Appropriate
- Log access revocation
- Track session terminations
- Record user deletions

#### CC7.2: System Operations
- Log system configuration changes
- Track deployments and rollbacks
- Record infrastructure changes

#### CC7.3: Monitoring Activities and Logs
- Retain logs for minimum 90 days (1 year recommended)
- Enable log integrity (tamper-proof)
- Provide search and reporting capabilities

---

## Current State Analysis

### Existing Implementation
**Location**: `Org-core/internal/audit/logger.go`

**Current Features:**
```go
// Basic audit logging to stdout/file
type Logger struct {
	logger zerolog.Logger
}

func (l *Logger) Log(ctx context.Context, event AuditEvent) {
	l.logger.Info().
		Str("event_type", event.Type).
		Str("user_id", event.UserID).
		Str("org_id", event.OrgID).
		Interface("metadata", event.Metadata).
		Msg(event.Description)
}
```

**Problems for SOC 2:**
- ❌ Logs not stored separately (mixed with application logs)
- ❌ No tamper-proof storage
- ❌ No retention policy enforcement
- ❌ No searchable audit trail
- ❌ No compliance reporting
- ❌ No real-time alerting
- ❌ Difficult to prove log integrity during audit

---

## Solution Options

| Solution | Type | Cost/Month | Setup Time | SOC 2 Ready | Winner |
|----------|------|------------|------------|-------------|---------|
| **Retraced** | SaaS | $50-200 | 1-2 days | ✅ Purpose-built | ⭐⭐⭐⭐⭐ |
| **Panther** | SaaS | $500-2000 | 1 week | ✅ Enterprise | ⭐⭐⭐⭐ |
| **Datadog Audit** | Add-on | $300-1000 | 3-4 days | ✅ If using DD | ⭐⭐⭐ |
| **AWS CloudTrail** | Cloud | $50-200 | 2-3 days | ✅ If using AWS | ⭐⭐⭐ |
| **Seq** | Self-hosted | $0-100 | 1 week | ⚠️ Need config | ⭐⭐ |
| **Custom + Postgres** | DIY | $0 | 3-4 weeks | ⚠️ Hard to prove | ⭐ |

---

## Option 1: Retraced ⭐ RECOMMENDED

### What Is Retraced?
Purpose-built audit log API for B2B SaaS applications seeking SOC 2 compliance.

### Why Retraced is Perfect for You

#### 1. **Built for SOC 2**
- Tamper-proof log storage
- Built-in retention policies
- Immutable audit trail
- Compliance-ready exports

#### 2. **Developer-Friendly API**
```go
// Simple integration (2-3 hours)
import "github.com/retracedhq/retraced-go"

client := retraced.NewClient(apiKey)

// Log any event
client.LogEvent(&retraced.Event{
	Action:      "user.login",
	CRUD:        "c",
	Description: "User logged in via SSO",
	Actor: &retraced.Actor{
		ID:   "user_123",
		Name: "john@example.com",
	},
	Group: &retraced.Group{
		ID:   "org_456",
		Name: "Acme Corp",
	},
	Target: &retraced.Target{
		ID:   "session_789",
		Type: "session",
	},
	Metadata: map[string]interface{}{
		"ip_address": "203.0.113.1",
		"user_agent": "Chrome 120",
	},
})
```

#### 3. **Embedded Audit UI**
- White-label audit viewer for customers
- Customers can view their own audit logs
- Export to CSV/JSON
- Search and filter

#### 4. **Pricing**
```
Startup Plan: $50/mo
- 10,000 events/month
- 1 year retention
- Unlimited end-users

Growth Plan: $200/mo
- 100,000 events/month
- 2 years retention
- Priority support

Enterprise: Custom
- Unlimited events
- Custom retention
- SLA
```

#### 5. **SOC 2 Benefits**
- Auditor-approved architecture
- Generates compliance reports
- Meets all TSC requirements
- Customer audit logs (bonus!)

### Implementation (1-2 Days)

**Step 1: Install SDK (30 minutes)**
```bash
cd Org-core
go get github.com/retracedhq/retraced-go
```

**Step 2: Create Audit Service (2 hours)**

**File**: `Org-core/internal/audit/retraced_logger.go`

```go
package audit

import (
	"context"
	
	retraced "github.com/retracedhq/retraced-go"
	"github.com/rs/zerolog"
)

type RetracedLogger struct {
	client *retraced.Client
	logger zerolog.Logger
}

func NewRetracedLogger(apiKey, projectID string, logger zerolog.Logger) *RetracedLogger {
	client := retraced.NewClient(&retraced.Config{
		APIKey:    apiKey,
		ProjectID: projectID,
	})
	
	return &RetracedLogger{
		client: client,
		logger: logger,
	}
}

// SOC 2 Required Events

func (l *RetracedLogger) LogAuthentication(ctx context.Context, event AuthEvent) error {
	return l.client.LogEvent(ctx, &retraced.Event{
		Action:      fmt.Sprintf("auth.%s", event.Action), // auth.login, auth.logout
		CRUD:        "c",
		Description: event.Description,
		Actor: &retraced.Actor{
			ID:   event.UserID,
			Name: event.UserEmail,
		},
		Group: &retraced.Group{
			ID:   event.OrgID,
			Name: event.OrgName,
		},
		Metadata: map[string]interface{}{
			"ip_address":  event.IPAddress,
			"user_agent":  event.UserAgent,
			"mfa_enabled": event.MFAEnabled,
			"success":     event.Success,
		},
	})
}

func (l *RetracedLogger) LogDataAccess(ctx context.Context, event DataAccessEvent) error {
	return l.client.LogEvent(ctx, &retraced.Event{
		Action:      fmt.Sprintf("data.%s", event.Operation), // data.read, data.update
		CRUD:        l.mapCRUD(event.Operation),
		Description: event.Description,
		Actor: &retraced.Actor{
			ID:   event.UserID,
			Name: event.UserEmail,
		},
		Group: &retraced.Group{
			ID:   event.OrgID,
			Name: event.OrgName,
		},
		Target: &retraced.Target{
			ID:   event.ResourceID,
			Type: event.ResourceType, // "document", "collection", etc.
			Name: event.ResourceName,
		},
		Metadata: map[string]interface{}{
			"operation":   event.Operation,
			"fields":      event.FieldsAccessed,
			"record_count": event.RecordCount,
		},
	})
}

func (l *RetracedLogger) LogPermissionChange(ctx context.Context, event PermissionEvent) error {
	return l.client.LogEvent(ctx, &retraced.Event{
		Action:      "permission.change",
		CRUD:        "u",
		Description: fmt.Sprintf("User %s permission changed", event.TargetUserID),
		Actor: &retraced.Actor{
			ID:   event.ActorUserID,
			Name: event.ActorUserEmail,
		},
		Group: &retraced.Group{
			ID:   event.OrgID,
			Name: event.OrgName,
		},
		Target: &retraced.Target{
			ID:   event.TargetUserID,
			Type: "user",
			Name: event.TargetUserEmail,
		},
		Metadata: map[string]interface{}{
			"old_role": event.OldRole,
			"new_role": event.NewRole,
			"reason":   event.Reason,
		},
	})
}

func (l *RetracedLogger) LogSystemChange(ctx context.Context, event SystemEvent) error {
	return l.client.LogEvent(ctx, &retraced.Event{
		Action:      fmt.Sprintf("system.%s", event.Action),
		CRUD:        l.mapCRUD(event.Action),
		Description: event.Description,
		Actor: &retraced.Actor{
			ID:   event.ActorUserID,
			Name: event.ActorUserEmail,
		},
		Group: &retraced.Group{
			ID:   event.OrgID,
			Name: event.OrgName,
		},
		Target: &retraced.Target{
			ID:   event.ResourceID,
			Type: event.ResourceType,
			Name: event.ResourceName,
		},
		Metadata: map[string]interface{}{
			"changes": event.Changes,
			"reason":  event.Reason,
		},
	})
}

func (l *RetracedLogger) mapCRUD(operation string) string {
	switch operation {
	case "create", "login", "provision":
		return "c"
	case "read", "view", "access":
		return "r"
	case "update", "modify", "change":
		return "u"
	case "delete", "remove", "revoke":
		return "d"
	default:
		return "r"
	}
}
```

**Step 3: Integrate (3-4 hours)**

**File**: `Org-core/internal/http/middleware/audit.go`

```go
package middleware

import (
	"github.com/gin-gonic/gin"
	"github.com/triodelab/coresystem/org-core/internal/audit"
)

// AuditMiddleware logs all requests
func AuditMiddleware(auditLogger *audit.RetracedLogger) gin.HandlerFunc {
	return func(c *gin.Context) {
		// Before request
		start := time.Now()
		
		// Process request
		c.Next()
		
		// After request - log audit event
		if shouldAudit(c) {
			go auditLogger.LogDataAccess(c.Request.Context(), audit.DataAccessEvent{
				UserID:       getUserID(c),
				UserEmail:    getUserEmail(c),
				OrgID:        getOrgID(c),
				OrgName:      getOrgName(c),
				Operation:    mapOperation(c.Request.Method),
				ResourceType: getResourceType(c.FullPath()),
				ResourceID:   getResourceID(c),
				Description:  fmt.Sprintf("%s %s", c.Request.Method, c.FullPath()),
				IPAddress:    c.ClientIP(),
			})
		}
	}
}

func shouldAudit(c *gin.Context) bool {
	// Only audit authenticated requests
	if getUserID(c) == "" {
		return false
	}
	
	// Don't audit health checks
	if c.FullPath() == "/health" {
		return false
	}
	
	// Audit all data operations
	return true
}
```

**Step 4: Enable Customer Audit Viewer (1 hour)**

```go
// Generate viewer token for customer
func (h *AuditHandler) GetAuditToken(c *gin.Context) {
	orgID := c.GetString("org_id")
	
	// Generate Retraced viewer token
	token, err := h.auditLogger.GetViewerToken(c.Request.Context(), audit.ViewerTokenRequest{
		ActorID:  orgID,
		GroupID:  orgID,
		IsAdmin:  false,
	})
	
	c.JSON(http.StatusOK, gin.H{
		"token":     token,
		"viewer_url": fmt.Sprintf("https://viewer.retraced.io?token=%s", token),
	})
}

// Embed in frontend
// <iframe src="https://viewer.retraced.io?token={token}" />
```

---

## Option 2: Panther (Enterprise)

### What Is Panther?
Cloud-native SIEM + audit logging for security and compliance.

### Pros ✅
- Complete security operations platform
- Real-time threat detection
- Advanced log analysis
- SOC 2 + HIPAA + PCI compliant

### Cons ❌
- **Expensive**: $500-2000/month
- **Complex**: 1 week setup
- **Overkill**: If you only need audit logs

### When to Use
- Large enterprise (1000+ employees)
- Need SIEM + audit logs
- Have security team
- High security requirements

---

## Option 3: Datadog Audit Trail

### What Is It?
Add-on to Datadog monitoring for audit logging.

### Pros ✅
- If already using Datadog (monitoring/logs)
- Integrated with existing dashboards
- Good search and filtering

### Cons ❌
- **Expensive**: $300-1000/month (on top of Datadog)
- Not purpose-built for SOC 2
- More complex configuration

### When to Use
- Already using Datadog for monitoring
- Want single vendor
- Budget allows

---

## Option 4: AWS CloudTrail (If on AWS)

### What Is It?
AWS service that logs all AWS API calls and account activity.

### Pros ✅
- Built-in if using AWS
- Logs all AWS infrastructure changes
- S3 storage with lifecycle policies
- Athena for querying

### Cons ❌
- Only logs AWS API calls (not application events)
- Need to build application audit logging separately
- Complex to query

### When to Use
- Infrastructure on AWS
- Need AWS resource audit trail
- Combine with Retraced for application events

---

## Option 5: Seq (Self-Hosted)

### What Is It?
Structured log server with powerful search and dashboards.

### Pros ✅
- Self-hosted (full control)
- Powerful search
- Good UI
- Free for single server

### Cons ❌
- Need to configure retention, immutability, compliance
- Harder to prove tamper-proof to auditors
- More maintenance

### When to Use
- Want self-hosted solution
- Have DevOps team
- Budget-constrained

---

## Recommendation

### **Phase 1: Use Existing Logger (Now)**
Continue with basic `audit/logger.go` for development.

### **Phase 2: Add Retraced (3-6 months before SOC 2)**
Migrate to Retraced when:
- Planning SOC 2 audit
- Need customer audit logs
- Want compliance-ready solution

### **Timeline**
```
Now                  Month 3-6                Month 12
├─────────────────────┴────────────────────────┴──────→
│                     │                        │
Keep basic logger     Add Retraced             SOC 2 Audit
$0/mo                 $50-200/mo               ✅ Pass
```

---

## SOC 2 Audit Checklist

### Required Audit Events

#### Authentication (CC6.1)
- [ ] User login (success/failure)
- [ ] User logout
- [ ] Password change
- [ ] Password reset
- [ ] MFA enable/disable
- [ ] Session timeout

#### Authorization (CC6.2)
- [ ] User provisioned
- [ ] User de-provisioned
- [ ] Role assigned
- [ ] Permission granted
- [ ] Permission revoked

#### Data Access (CC7.3)
- [ ] Document created
- [ ] Document read
- [ ] Document updated
- [ ] Document deleted
- [ ] Data exported
- [ ] Bulk operations

#### System Changes (CC7.2)
- [ ] Configuration changed
- [ ] Integration added/removed
- [ ] API key created/revoked
- [ ] Deployment executed
- [ ] Database schema changed

#### Security Events
- [ ] Failed login attempts
- [ ] Account lockouts
- [ ] Privilege escalation
- [ ] Unusual access patterns

---

## Implementation Costs

### Retraced (Recommended)
```
Setup:
- Development: 1-2 days × $800/day = $1,600
- Testing: 0.5 day × $800/day = $400
Total: ~$2,000

Monthly:
- Startup plan: $50/mo
- Growth plan (if needed): $200/mo

Annual Cost (Year 1):
- Setup: $2,000
- Subscription: $600 (startup) or $2,400 (growth)
Total: $2,600 - $4,400
```

### DIY (Not Recommended)
```
Setup:
- Design: 1 week × $4,000 = $4,000
- Implementation: 2 weeks × $8,000 = $16,000
- Compliance review: 1 week × $4,000 = $4,000
Total: ~$24,000

Monthly:
- Maintenance: 20 hours/year × $100/hr = $2,000/year
- Storage: $50/mo = $600/year

Annual Cost (Year 1):
- Setup: $24,000
- Ongoing: $2,600
Total: $26,600
```

**Savings**: $22,000 by using Retraced vs. DIY

---

## Audit Requirements by Event Type

### 1. User Authentication
```go
// Login
auditLogger.LogAuthentication(ctx, audit.AuthEvent{
	Action:      "login",
	UserID:      user.ID,
	UserEmail:   user.Email,
	OrgID:       user.OrgID,
	IPAddress:   req.IPAddress,
	UserAgent:   req.UserAgent,
	MFAEnabled:  user.MFAEnabled,
	Success:     true,
	Description: "User logged in via SSO",
})

// Failed login
auditLogger.LogAuthentication(ctx, audit.AuthEvent{
	Action:      "login_failed",
	UserEmail:   req.Email, // No user ID if failed
	IPAddress:   req.IPAddress,
	Success:     false,
	Description: "Failed login attempt - invalid credentials",
})
```

### 2. Data Operations
```go
// Document access
auditLogger.LogDataAccess(ctx, audit.DataAccessEvent{
	Operation:    "read",
	ResourceType: "document",
	ResourceID:   doc.ID,
	ResourceName: doc.Title,
	UserID:       user.ID,
	OrgID:        user.OrgID,
	Description:  "User viewed sensitive document",
})

// Bulk export
auditLogger.LogDataAccess(ctx, audit.DataAccessEvent{
	Operation:    "export",
	ResourceType: "collection",
	ResourceID:   collection.ID,
	RecordCount:  1000,
	Description:  "User exported 1000 records to CSV",
})
```

### 3. Permission Changes
```go
auditLogger.LogPermissionChange(ctx, audit.PermissionEvent{
	ActorUserID:      admin.ID,
	ActorUserEmail:   admin.Email,
	TargetUserID:     user.ID,
	TargetUserEmail:  user.Email,
	OrgID:            org.ID,
	OldRole:          "viewer",
	NewRole:          "editor",
	Reason:           "Promotion",
	Description:      "User role upgraded to editor",
})
```

---

## Conclusion

### **Immediate Action: None Required**
Continue with basic audit logging during development.

### **Timeline: 3-6 Months Before SOC 2**
Integrate Retraced for:
- ✅ Compliance-ready audit logs
- ✅ Tamper-proof storage
- ✅ Customer audit viewer
- ✅ SOC 2 reports

### **ROI**
- **Saves**: $22,000 vs. building custom
- **Time**: 2 days integration vs. 4 weeks custom
- **Risk**: Auditor-approved vs. unproven

### **Cost**: $50-200/month (paid only when needed)

---

**Questions for Future Discussion:**
1. Target date for SOC 2 audit?
2. Need customer-facing audit logs?
3. Budget approval process for $200/mo tool?
