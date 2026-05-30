# Review Ticketing Evaluation: Linear vs Jira

**Date**: February 2, 2026  
**Decision**: Choose review ticketing system for content moderation  
**Status**: Evaluation Phase

---

## Executive Summary

| Criteria | Linear | Jira | Custom (Current) | Winner |
|----------|--------|------|------------------|---------|
| **Setup Time** | ✅ 10 minutes | ⚠️ 1-2 days | ❌ Already 479 lines | **Linear** |
| **API Quality** | ✅ GraphQL, modern | ⚠️ REST, complex | ✅ Full control | **Linear** |
| **Cost (small team)** | ✅ Free (10 users) | 💰 $7.75/user/mo | ✅ $0 (DIY) | **Linear** |
| **Cost (large team)** | 💰 $8/user/mo | 💰 $7.75-15/user/mo | ✅ $0 (DIY) | **Custom** |
| **User Experience** | ✅ Modern, fast | ⚠️ Complex, slow | ⚠️ Custom UI needed | **Linear** |
| **Customization** | ⚠️ Limited fields | ✅ Highly customizable | ✅ Full control | **Jira** |
| **Integration Effort** | ✅ 1-2 hours | ⚠️ 1-2 days | ❌ Ongoing maintenance | **Linear** |
| **Automation** | ✅ Built-in workflows | ✅ Advanced automation | ⚠️ Manual code | **Jira** |
| **Reporting** | ✅ Modern dashboards | ✅ Advanced reports | ⚠️ Build yourself | **Jira** |
| **Mobile App** | ✅ Excellent | ✅ Good | ❌ None | **Linear** |

**Quick Decision:**
- **Linear** for startups/small teams (simple, fast, beautiful)
- **Jira** for enterprises (complex workflows, advanced reporting)
- **Keep Custom** only if unique requirements

---

## Current State Analysis

### Existing Implementation
**Location**: `ai-core/app/services/review_ticketing_service.py` (479 lines)

**Architecture:**
```
┌──────────────┐
│  AI-core     │
│  (Python)    │
└──────┬───────┘
       │
       │ Creates ticket
       ▼
┌──────────────┐      ┌──────────────┐
│ In-Memory    │────→ │   Webhook    │
│  Storage     │      │  (External   │
│ (Dict)       │      │   Dashboard) │
└──────────────┘      └──────────────┘
```

**Features Built:**
- ✅ Ticket creation with content hash
- ✅ Status tracking (pending, in_review, approved, rejected, escalated)
- ✅ Webhook notifications
- ✅ Auto-expire after 30 days
- ✅ Reviewer callback handling
- ⚠️ **In-memory storage** (loses data on restart!)

**Current Ticket Structure:**
```python
@dataclass
class ReviewTicket:
    ticket_id: str
    org_id: str
    user_id: Optional[str]
    created_at: str
    status: ReviewStatus
    
    # Content (redacted)
    content_hash: str
    content_preview: str  # First 200 chars
    content_type: str  # "text", "image", "audio"
    
    # Safety analysis
    safety_score: float
    severity: str
    categories: Dict[str, float]
    
    # Review outcome
    reviewer_id: Optional[str]
    decision: Optional[ReviewDecision]
    reviewer_notes: Optional[str]
```

**Problems:**
- ❌ 479 lines of custom code to maintain
- ❌ In-memory storage (data loss risk)
- ❌ No built-in UI (need external dashboard)
- ❌ No assignee management
- ❌ No SLA tracking
- ❌ No built-in notifications (only webhooks)
- ❌ No mobile access for reviewers
- ❌ No audit trail beyond basic logging

---

## Option 1: Linear

### What Is Linear?
Modern issue tracking tool built for speed. Think "Jira, but fast and beautiful."

### Architecture
```
┌──────────────┐      ┌──────────────┐
│  AI-core     │─────→│   Linear     │
│  (Python)    │ API  │   (Cloud)    │
└──────────────┘      └──────┬───────┘
                             │
                      ┌──────▼───────┐
                      │   Reviewers  │
                      │  (Web/Mobile)│
                      └──────────────┘
```

### Pros ✅

#### 1. **Blazing Fast**
- Keyboard-first interface (Gmail-style shortcuts)
- Sub-100ms response times
- Instant search across all tickets
- No loading spinners

#### 2. **Beautiful UX**
- Modern, clean interface
- Dark mode support
- Real-time updates (WebSocket)
- Mobile app (iOS/Android)

#### 3. **Excellent API**
- GraphQL API (strongly typed)
- Webhooks for events
- 10-minute integration time
- SDKs: TypeScript, Python (unofficial)

#### 4. **Free for Small Teams**
```
Pricing:
- Free: Up to 10 users (perfect for reviewers)
- Standard: $8/user/mo (unlimited guests)
- Plus: $14/user/mo (advanced features)
```

#### 5. **Built-in Automation**
```yaml
# Example automation
When: Issue created with label "high-severity"
Then:
  - Assign to: on-call reviewer
  - Set priority: Urgent
  - Send notification to: Slack #content-safety
  - Set SLA: 2 hours
```

#### 6. **Integrations**
- Slack notifications
- GitHub (link tickets to code)
- Figma (attach designs)
- Webhooks (custom integrations)

### Cons ❌

#### 1. **Limited Customization**
- Fixed fields (can't add arbitrary custom fields like Jira)
- Workflows are predefined (backlog → started → done)
- Can't create complex automation rules

#### 2. **Relatively New**
- Founded 2019 (vs. Jira's 2002)
- Smaller ecosystem
- Less enterprise features (no SAML SSO on free tier)

#### 3. **Not Built for Content Moderation**
- General-purpose issue tracker
- No built-in content preview
- No moderation-specific workflows

#### 4. **Less Reporting**
- Basic dashboards
- No advanced custom reports like Jira
- Limited data export

### Use Cases ✅ Perfect For:
- Startups & small teams (5-50 people)
- Need fast, modern UI
- Developers using GitHub
- Want zero setup time
- Budget-conscious

### Code Example

**Create Ticket:**
```python
import requests

LINEAR_API_KEY = "lin_api_xxx"
LINEAR_API_URL = "https://api.linear.app/graphql"

def create_review_ticket(content_hash: str, severity: str, categories: dict) -> str:
    """Create Linear issue for content review"""
    
    mutation = """
    mutation CreateIssue($input: IssueCreateInput!) {
      issueCreate(input: $input) {
        success
        issue {
          id
          identifier
          url
        }
      }
    }
    """
    
    # Labels for severity
    priority_map = {
        "critical": 1,  # Urgent
        "high": 2,      # High
        "medium": 3,    # Normal
        "low": 4        # Low
    }
    
    variables = {
        "input": {
            "teamId": "content-safety-team-id",
            "title": f"Review Content: {content_hash[:8]}",
            "description": f"""
## Content Review Required

**Content Hash**: `{content_hash}`
**Severity**: {severity}

### Categories Flagged
{format_categories(categories)}

### Actions
- [ ] Review content
- [ ] Make decision (Allow/Block/Sanitize)
- [ ] Add notes
            """,
            "priority": priority_map.get(severity, 3),
            "labelIds": ["content-review-label-id"],
        }
    }
    
    response = requests.post(
        LINEAR_API_URL,
        json={"query": mutation, "variables": variables},
        headers={"Authorization": LINEAR_API_KEY}
    )
    
    data = response.json()
    issue = data["data"]["issueCreate"]["issue"]
    
    return issue["url"]  # https://linear.app/team/issue/CS-123

def format_categories(categories: dict) -> str:
    """Format categories as markdown list"""
    return "\n".join([f"- **{cat}**: {score:.2%}" for cat, score in categories.items()])
```

**Update Ticket Status:**
```python
def update_ticket_status(issue_id: str, status: str, notes: str):
    """Update ticket with reviewer decision"""
    
    mutation = """
    mutation UpdateIssue($id: String!, $input: IssueUpdateInput!) {
      issueUpdate(id: $id, input: $input) {
        success
        issue {
          id
          state {
            name
          }
        }
      }
    }
    """
    
    # Map your statuses to Linear states
    state_map = {
        "approved": "done",
        "rejected": "canceled",
        "escalated": "started",
    }
    
    variables = {
        "id": issue_id,
        "input": {
            "stateId": state_map.get(status, "started"),
            "description": f"Original description\n\n---\n\n## Reviewer Decision\n{notes}"
        }
    }
    
    response = requests.post(
        LINEAR_API_URL,
        json={"query": mutation, "variables": variables},
        headers={"Authorization": LINEAR_API_KEY}
    )
```

**Webhook Handler:**
```python
from fastapi import APIRouter, Request

router = APIRouter()

@router.post("/webhooks/linear")
async def linear_webhook(request: Request):
    """Handle Linear webhook events"""
    payload = await request.json()
    
    if payload["type"] == "Issue":
        action = payload["action"]
        issue = payload["data"]
        
        if action == "update":
            # Ticket status changed
            if issue["state"]["name"] == "done":
                # Reviewer approved
                await handle_approval(issue["id"])
            elif issue["state"]["name"] == "canceled":
                # Reviewer rejected
                await handle_rejection(issue["id"])
    
    return {"status": "ok"}
```

---

## Option 2: Jira

### What Is Jira?
Enterprise-grade project management and issue tracking. Industry standard for 20+ years.

### Architecture
```
┌──────────────┐      ┌──────────────┐
│  AI-core     │─────→│     Jira     │
│  (Python)    │ API  │  (Cloud/     │
└──────────────┘      │   Server)    │
                      └──────┬───────┘
                             │
                      ┌──────▼───────┐
                      │   Reviewers  │
                      │  (Web/Mobile)│
                      └──────────────┘
```

### Pros ✅

#### 1. **Highly Customizable**
- Custom fields (unlimited)
- Custom workflows (drag-and-drop editor)
- Custom screens and forms
- Custom dashboards

**Example Custom Fields:**
```
Content Moderation Issue Type:
- Content Hash (text)
- Content Type (dropdown: text/image/audio)
- Safety Score (number)
- Severity (dropdown: low/medium/high/critical)
- Category Scores (JSON field)
- Reviewer Decision (dropdown: allow/block/sanitize/escalate)
- Review Notes (long text)
- SLA Timer (calculated field)
```

#### 2. **Advanced Automation**
```yaml
# Jira Automation Rules
Rule 1: Auto-assign based on severity
  When: Issue created
  If: Severity = Critical
  Then:
    - Assign to: On-call reviewer
    - Send notification to: Slack
    - Set SLA: 1 hour
    - Add label: urgent

Rule 2: Escalate if not reviewed
  When: SLA breached
  If: Status = "Waiting for Review"
  Then:
    - Assign to: Senior Reviewer
    - Set Priority: Highest
    - Comment: "SLA breached - escalating"
    - Send email to: Manager

Rule 3: Close old tickets
  When: Scheduled (daily at 2 AM)
  If: Status = "Done" AND Updated > 30 days ago
  Then:
    - Transition to: Closed
    - Archive issue
```

#### 3. **Enterprise Features**
- SAML SSO integration
- Advanced permissions (role-based)
- Audit logs
- Data residency options (EU, US, Australia)
- 99.9% uptime SLA

#### 4. **Advanced Reporting**
- Custom JQL queries
- Gadgets and dashboards
- Time tracking
- Burndown/burnup charts
- Export to CSV/Excel

**Example Reports:**
```
Average Time to Review by Severity:
- Critical: 45 minutes
- High: 2 hours
- Medium: 4 hours
- Low: 8 hours

Reviewer Performance:
- Alice: 50 tickets/week, 95% accuracy
- Bob: 30 tickets/week, 98% accuracy
- Charlie: 40 tickets/week, 92% accuracy
```

#### 5. **Massive Ecosystem**
- 3,000+ apps in Atlassian Marketplace
- Integrations with everything
- Mature REST API
- Extensive documentation

### Cons ❌

#### 1. **Slow & Complex**
- Heavy interface (lots of loading)
- Steep learning curve (2-3 days training)
- Overwhelming for simple use cases

#### 2. **Expensive**
```
Pricing (Cloud):
- Free: Up to 10 users
- Standard: $7.75/user/mo (up to 35k users)
- Premium: $15.25/user/mo
- Enterprise: Custom pricing

Example:
- 10 reviewers: $77.50/mo
- 50 reviewers: $387.50/mo
```

#### 3. **Overkill for Content Review**
- Built for software development
- Too many features you won't use
- Complex setup (1-2 days)

#### 4. **Poor Mobile Experience**
- Mobile app is functional but clunky
- Not optimized for quick actions

### Use Cases ✅ Perfect For:
- Large enterprises (100+ people)
- Complex workflows (10+ states)
- Need advanced reporting
- Heavy customization required
- Already using Atlassian suite (Confluence, Bitbucket)

### Code Example

**Create Ticket:**
```python
import requests
from requests.auth import HTTPBasicAuth

JIRA_URL = "https://your-domain.atlassian.net"
JIRA_EMAIL = "your-email@example.com"
JIRA_API_TOKEN = "your-api-token"

def create_review_ticket(content_hash: str, severity: str, categories: dict) -> str:
    """Create Jira issue for content review"""
    
    auth = HTTPBasicAuth(JIRA_EMAIL, JIRA_API_TOKEN)
    
    issue_data = {
        "fields": {
            "project": {"key": "CS"},  # Content Safety project
            "summary": f"Review Content: {content_hash[:8]}",
            "description": f"""
h2. Content Review Required

*Content Hash*: {{code}}{content_hash}{{code}}
*Severity*: {severity}

h3. Categories Flagged
{format_categories_jira(categories)}

h3. Actions
* Review content
* Make decision (Allow/Block/Sanitize)
* Add notes
            """,
            "issuetype": {"name": "Content Review"},
            "priority": {"name": severity_to_priority(severity)},
            "labels": ["content-review"],
            
            # Custom fields
            "customfield_10001": content_hash,  # Content Hash
            "customfield_10002": {"value": "text"},  # Content Type
            "customfield_10003": 0.85,  # Safety Score
            "customfield_10004": categories,  # Category Scores (JSON)
        }
    }
    
    response = requests.post(
        f"{JIRA_URL}/rest/api/3/issue",
        json=issue_data,
        auth=auth
    )
    
    issue = response.json()
    return f"{JIRA_URL}/browse/{issue['key']}"  # https://domain.atlassian.net/browse/CS-123

def severity_to_priority(severity: str) -> str:
    """Map severity to Jira priority"""
    return {
        "critical": "Highest",
        "high": "High",
        "medium": "Medium",
        "low": "Low"
    }.get(severity, "Medium")
```

**Update Ticket:**
```python
def update_ticket_decision(issue_key: str, decision: str, notes: str):
    """Update ticket with reviewer decision"""
    
    auth = HTTPBasicAuth(JIRA_EMAIL, JIRA_API_TOKEN)
    
    # Transition to appropriate status
    transition_map = {
        "allow": "31",  # Transition ID for "Approved"
        "block": "41",  # Transition ID for "Rejected"
        "sanitize": "51",  # Transition ID for "Sanitize Required"
        "escalate": "61",  # Transition ID for "Escalated"
    }
    
    # Add comment
    requests.post(
        f"{JIRA_URL}/rest/api/3/issue/{issue_key}/comment",
        json={"body": f"Reviewer Decision: {decision}\n\nNotes: {notes}"},
        auth=auth
    )
    
    # Update custom field
    requests.put(
        f"{JIRA_URL}/rest/api/3/issue/{issue_key}",
        json={
            "fields": {
                "customfield_10005": {"value": decision}  # Reviewer Decision field
            }
        },
        auth=auth
    )
    
    # Transition issue
    requests.post(
        f"{JIRA_URL}/rest/api/3/issue/{issue_key}/transitions",
        json={"transition": {"id": transition_map[decision]}},
        auth=auth
    )
```

**Webhook Handler:**
```python
@router.post("/webhooks/jira")
async def jira_webhook(request: Request):
    """Handle Jira webhook events"""
    payload = await request.json()
    
    if payload["webhookEvent"] == "jira:issue_updated":
        issue = payload["issue"]
        
        # Check if status changed
        if "status" in payload["changelog"]["items"]:
            new_status = issue["fields"]["status"]["name"]
            
            if new_status == "Approved":
                await handle_approval(issue["key"])
            elif new_status == "Rejected":
                await handle_rejection(issue["key"])
    
    return {"status": "ok"}
```

---

## Option 3: Keep Custom Implementation

### Current Implementation
**Location**: `ai-core/app/services/review_ticketing_service.py`

### Improvements Needed (If Keeping)

**1. Add Database Storage:**
```python
# Replace in-memory dict with PostgreSQL
class ReviewTicketingService:
    def __init__(self, db: Database):
        self.db = db  # PostgreSQL connection
    
    async def create_ticket(self, ticket: ReviewTicket) -> str:
        """Store ticket in database"""
        await self.db.execute("""
            INSERT INTO review_tickets (
                ticket_id, org_id, user_id, content_hash,
                severity, categories, status, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        """, ticket.ticket_id, ticket.org_id, ...)
        
        return ticket.ticket_id
```

**2. Add Web UI:**
```python
# Build admin dashboard with FastAPI + React
# Or use existing admin panel framework:
# - Django Admin
# - Flask-Admin
# - Retool (low-code)
```

**3. Add Assignee Management:**
```python
@dataclass
class ReviewTicket:
    # ... existing fields
    assigned_to: Optional[str] = None
    assigned_at: Optional[datetime] = None
```

**4. Add SLA Tracking:**
```python
async def check_sla_breaches(self):
    """Check for tickets exceeding SLA"""
    tickets = await self.db.fetch("""
        SELECT * FROM review_tickets
        WHERE status = 'pending'
        AND created_at < NOW() - INTERVAL '2 hours'
        AND severity = 'critical'
    """)
    
    for ticket in tickets:
        await self.escalate_ticket(ticket.id)
```

### Cost of Maintaining Custom
```
Development Time:
- Database migration: 4 hours
- Web UI: 40 hours
- Assignee system: 8 hours
- SLA tracking: 8 hours
- Notifications: 8 hours
- Mobile access: 80 hours (if needed)
Total: 148 hours = ~$15,000 @ $100/hr

Ongoing Maintenance:
- Bug fixes: 5 hours/month
- Feature requests: 10 hours/month
- Security updates: 2 hours/month
Total: 17 hours/month = ~$20,400/year
```

---

## Detailed Comparison

### 1. Integration Effort

**Linear:**
```python
# 1 hour integration
pip install gql
# Create tickets via GraphQL
# Done!
```

**Jira:**
```python
# 1 day integration
pip install jira
# Set up custom fields in Jira UI
# Map fields in code
# Configure workflows
# Done!
```

**Custom:**
```python
# 150+ hours
# Build everything yourself
```

**Winner**: **Linear** (1 hour) > **Jira** (1 day) > **Custom** (150+ hours)

---

### 2. Cost Over 2 Years

**Scenario: 5 Reviewers**
```
Linear:
- Setup: 1 hour × $100/hr = $100
- Subscription: $0 (free tier)
Total: $100

Jira:
- Setup: 1 day × $800 = $800
- Subscription: $387.50/mo × 24 = $9,300
Total: $10,100

Custom:
- Build: 148 hours × $100/hr = $14,800
- Maintenance: 17 hrs/mo × 24 mo × $100/hr = $40,800
Total: $55,600
```

**Winner**: **Linear** ($100) > **Jira** ($10k) > **Custom** ($55k)

---

### 3. Features

| Feature | Linear | Jira | Custom |
|---------|--------|------|--------|
| **Ticket Creation** | ✅ GraphQL API | ✅ REST API | ✅ Built |
| **Status Tracking** | ✅ | ✅ | ✅ |
| **Assignees** | ✅ Auto-assignment | ✅ Round-robin | ⚠️ Need to build |
| **SLA Tracking** | ✅ | ✅ Advanced | ⚠️ Need to build |
| **Notifications** | ✅ Slack, email | ✅ Slack, email, SMS | ✅ Webhooks only |
| **Mobile App** | ✅ Excellent | ✅ Good | ❌ None |
| **Reporting** | ✅ Basic | ✅ Advanced | ⚠️ Need to build |
| **Custom Fields** | ⚠️ Limited | ✅ Unlimited | ✅ Full control |
| **Audit Logs** | ✅ | ✅ | ⚠️ Need to build |

---

### 4. User Experience

**Linear:**
- ✅ Fast (sub-100ms)
- ✅ Beautiful UI
- ✅ Keyboard shortcuts
- ✅ Real-time updates

**Jira:**
- ⚠️ Slow (3-5s page loads)
- ⚠️ Complex interface
- ✅ Powerful search (JQL)
- ⚠️ Requires training

**Custom:**
- ❓ Depends on your build
- ⚠️ No mobile app (unless you build it)
- ⚠️ Need to maintain UI

**Winner**: **Linear** > **Jira** > **Custom**

---

## Decision Framework

### Choose **Linear** if:
✅ Team is **small** (5-20 reviewers)  
✅ Want **fast setup** (10 minutes vs. 1 day)  
✅ Need **modern UI** (reviewers will love it)  
✅ Budget is **tight** (free for <10 users)  
✅ Want **zero maintenance**  
✅ Simple workflow (pending → reviewed → done)  

**Saves**: $55k over 2 years vs. custom

---

### Choose **Jira** if:
✅ **Large team** (50+ reviewers)  
✅ Need **complex workflows** (10+ states)  
✅ Need **advanced reporting** (custom dashboards)  
✅ Require **heavy customization**  
✅ Already use **Atlassian suite**  
✅ Enterprise features (SAML SSO, audit logs)  

**Saves**: $45k over 2 years vs. custom

---

### Choose **Custom (Keep Current)** if:
✅ **Unique requirements** not met by Linear/Jira  
✅ Need **full data control** (on-premise)  
✅ Have **dedicated engineering team**  
✅ Want **no vendor lock-in**  
❌ **Not recommended** unless absolute necessity

**Costs**: $55k over 2 years (build + maintenance)

---

## Recommendation for CoreSystem

### **Immediate: Migrate to Linear**

#### Reasons:
1. **Cost**: $0/mo (free tier) vs. $55k/2yr (custom)
2. **Time**: 1-2 hours integration vs. 150+ hours building
3. **Quality**: Modern UI, mobile app, notifications
4. **Maintenance**: Zero vs. 17 hours/month

#### Migration Plan

**Phase 1: Setup (30 minutes)**
```bash
# 1. Create Linear team
# 2. Add reviewers (free up to 10 users)
# 3. Create custom labels:
#    - content-review
#    - high-severity
#    - escalated
# 4. Set up workflow states:
#    - Backlog (pending)
#    - In Progress (in_review)
#    - Done (approved)
#    - Canceled (rejected)
```

**Phase 2: Integration (1-2 hours)**

**File**: `ai-core/app/services/linear_ticketing_service.py`

```python
import httpx
from typing import Optional
import structlog

logger = structlog.get_logger(__name__)

class LinearTicketingService:
    """Linear integration for content moderation"""
    
    def __init__(self, api_key: str, team_id: str):
        self.api_key = api_key
        self.team_id = team_id
        self.api_url = "https://api.linear.app/graphql"
        self.client = httpx.AsyncClient(headers={
            "Authorization": api_key,
            "Content-Type": "application/json"
        })
    
    async def create_ticket(
        self,
        content_hash: str,
        severity: str,
        categories: dict,
        org_id: str
    ) -> str:
        """Create Linear issue for content review"""
        
        mutation = """
        mutation CreateIssue($input: IssueCreateInput!) {
          issueCreate(input: $input) {
            success
            issue {
              id
              identifier
              url
            }
          }
        }
        """
        
        description = f"""
## Content Review Required

**Content Hash**: `{content_hash}`  
**Organization**: {org_id}  
**Severity**: {severity}

### Categories Flagged
{self._format_categories(categories)}

### Review Checklist
- [ ] Review content against policies
- [ ] Check for false positives
- [ ] Make decision (Allow/Block/Sanitize/Escalate)
- [ ] Document reasoning
        """
        
        variables = {
            "input": {
                "teamId": self.team_id,
                "title": f"Review: {content_hash[:12]}",
                "description": description,
                "priority": self._severity_to_priority(severity),
                "labelIds": [self._get_label_id("content-review")],
            }
        }
        
        response = await self.client.post(
            self.api_url,
            json={"query": mutation, "variables": variables}
        )
        
        data = response.json()
        if data.get("errors"):
            logger.error("linear_create_failed", errors=data["errors"])
            raise Exception(f"Failed to create Linear issue: {data['errors']}")
        
        issue = data["data"]["issueCreate"]["issue"]
        logger.info("linear_ticket_created", 
                   ticket_id=issue["identifier"],
                   url=issue["url"])
        
        return issue["url"]
    
    def _severity_to_priority(self, severity: str) -> int:
        """Map severity to Linear priority"""
        return {
            "critical": 1,  # Urgent
            "high": 2,      # High
            "medium": 3,    # Normal
            "low": 4        # Low
        }.get(severity, 3)
    
    def _format_categories(self, categories: dict) -> str:
        """Format categories as markdown"""
        lines = []
        for category, score in sorted(categories.items(), 
                                     key=lambda x: x[1], 
                                     reverse=True):
            lines.append(f"- **{category}**: {score:.1%}")
        return "\n".join(lines)
    
    async def close(self):
        """Close HTTP client"""
        await self.client.aclose()


# Global singleton
_linear_service: Optional[LinearTicketingService] = None

def get_linear_service() -> LinearTicketingService:
    """Get or create Linear service"""
    global _linear_service
    if _linear_service is None:
        _linear_service = LinearTicketingService(
            api_key=settings.LINEAR_API_KEY,
            team_id=settings.LINEAR_TEAM_ID
        )
    return _linear_service
```

**Phase 3: Update Safety Service (15 minutes)**

**File**: `ai-core/app/services/media_moderation_service.py`

```python
# Replace old ticketing service
from app.services.linear_ticketing_service import get_linear_service

async def moderate_content(...):
    # ... existing moderation logic
    
    # If escalation needed
    if result.requires_review:
        linear = get_linear_service()
        ticket_url = await linear.create_ticket(
            content_hash=content_hash,
            severity=result.severity,
            categories=result.categories,
            org_id=org_id
        )
        
        logger.info("content_escalated_to_linear", 
                   ticket_url=ticket_url)
```

**Phase 4: Configure Webhooks (15 minutes)**

```python
# Handle Linear webhook callbacks
@router.post("/webhooks/linear")
async def linear_webhook(request: Request):
    """Handle Linear status updates"""
    payload = await request.json()
    
    if payload["type"] == "Issue" and payload["action"] == "update":
        issue = payload["data"]
        state = issue["state"]["name"]
        
        # Extract content hash from title
        content_hash = extract_hash_from_title(issue["title"])
        
        # Update moderation decision
        if state == "Done":
            await record_approval(content_hash, issue["id"])
        elif state == "Canceled":
            await record_rejection(content_hash, issue["id"])
    
    return {"status": "ok"}
```

**Phase 5: Deprecate Custom Service (5 minutes)**

```python
# Mark as deprecated
# File: ai-core/app/services/review_ticketing_service.py

import warnings

warnings.warn(
    "review_ticketing_service is deprecated. Use linear_ticketing_service instead.",
    DeprecationWarning
)

# Keep for 1 month, then delete
```

---

### Environment Variables

```bash
# .env
LINEAR_API_KEY=lin_api_xxx_your_key_here
LINEAR_TEAM_ID=content-safety-team-id
LINEAR_WEBHOOK_SECRET=whsec_xxx

# Get these from:
# https://linear.app/settings/api
```

---

### Benefits After Migration

**Before (Custom):**
- ❌ 479 lines of code to maintain
- ❌ In-memory storage (data loss risk)
- ❌ No UI (need external dashboard)
- ❌ No mobile access
- ❌ Manual notifications

**After (Linear):**
- ✅ 100 lines of integration code
- ✅ Reliable cloud storage
- ✅ Beautiful UI (reviewers love it)
- ✅ Mobile app (iOS/Android)
- ✅ Slack notifications built-in
- ✅ Real-time updates
- ✅ SLA tracking

**Cost Savings**: $55,000 over 2 years  
**Time Savings**: 150+ hours initial + 17 hours/month ongoing

---

## Conclusion

### **Recommended Path: Migrate to Linear Now**

```
Timeline:
├─ Day 1: Set up Linear team (30 min)
├─ Day 1: Integrate API (1-2 hours)
├─ Day 2: Configure webhooks (15 min)
├─ Day 2: Test with sample tickets (30 min)
├─ Day 3: Deploy to production
└─ Week 2: Deprecate custom service
```

**Total Migration Time**: 3-4 hours  
**ROI**: $55k saved over 2 years  
**Break-even**: Immediate (Linear free tier)

---

**Questions for Discussion:**
1. How many reviewers do you have? (<10 = Linear free, >10 = $8/user/mo)
2. Any unique workflow requirements not covered by Linear?
3. Need complex reporting? (If yes, consider Jira)

