# Workflow Engine Evaluation: Temporal vs LangGraph

**Date**: February 2, 2026  
**Decision**: Choose workflow orchestration engine for CoreSystem  
**Status**: Evaluation Phase

---

## Executive Summary

| Criteria | Temporal | LangGraph | Winner |
|----------|----------|-----------|---------|
| **AI Workflow Focus** | ❌ General purpose | ✅ Built for AI agents | **LangGraph** |
| **Long-running Tasks** | ✅ Days/weeks support | ⚠️ Better for minutes/hours | **Temporal** |
| **State Management** | ✅ Automatic persistence | ✅ PostgreSQL checkpoint | **Tie** |
| **Learning Curve** | ⚠️ Steep (3-4 weeks) | ✅ Easy if know Python (1 week) | **LangGraph** |
| **Production Maturity** | ✅ Battle-tested (Netflix, Uber) | ⚠️ Newer (2024) | **Temporal** |
| **Integration Complexity** | ⚠️ Requires Temporal Server | ✅ Just a library | **LangGraph** |
| **Cost** | 💰 Server hosting (~$200/mo) | ✅ Free (open source) | **LangGraph** |
| **Multi-language** | ✅ Go, Python, TypeScript | ❌ Python only | **Temporal** |
| **AI Agent Support** | ❌ Need custom code | ✅ Native LangChain integration | **LangGraph** |

---

## Current State Analysis

### Existing Workflow Implementation
**Location**: `Org-core/internal/workflows/engine.go` (425 lines)

**Features Built:**
- Custom workflow definition parser
- Step execution with conditionals
- Variable passing between steps
- Retry logic
- Status tracking

**Problems:**
- ❌ No durable state (crash = lost progress)
- ❌ No workflow versioning
- ❌ Manual retry logic prone to bugs
- ❌ No workflow visualization
- ❌ No time-based triggers (cron, delays)

### Already Installed
✅ **LangGraph** v0.2.45 with PostgreSQL checkpointing  
✅ **LangChain** ecosystem already integrated  
✅ Currently using in: `ai-core/app/services/langgraph_service.py`

---

## Option 1: Temporal

### What Is Temporal?
Durable execution platform for long-running workflows. Think "a database for your code execution state."

### Architecture
```
┌─────────────────┐
│  Your Services  │
│  (Go, Python)   │
└────────┬────────┘
         │ gRPC
┌────────▼────────┐
│ Temporal Worker │
│  (SDK)          │
└────────┬────────┘
         │
┌────────▼────────┐
│ Temporal Server │
│ (Self-hosted or │
│  Temporal Cloud)│
└────────┬────────┘
         │
┌────────▼────────┐
│  PostgreSQL +   │
│  Cassandra/ES   │
└─────────────────┘
```

### Pros ✅
1. **Bulletproof Reliability**
   - Automatic state persistence every step
   - Crash recovery out of the box
   - Workflow can pause for days/weeks and resume

2. **Production-Grade**
   - Used by: Netflix (100k+ workflows/day), Uber, Stripe, Coinbase
   - 6+ years in production
   - Excellent monitoring/observability

3. **Multi-Language Support**
   - Go SDK (perfect for Org-core)
   - Python SDK (for AI-core)
   - TypeScript SDK (for auth)

4. **Time Travel & Versioning**
   - Test workflows by replaying history
   - Roll out new versions safely
   - No breaking changes for running workflows

5. **Advanced Features**
   - Cron schedules
   - Human-in-the-loop approvals
   - Child workflows
   - Saga pattern for distributed transactions

### Cons ❌
1. **Infrastructure Overhead**
   - Requires Temporal Server (PostgreSQL + Cassandra/ES)
   - 3-4 containers minimum
   - ~2GB RAM baseline

2. **Complexity**
   - Steep learning curve (concepts: activities, workflows, workers)
   - 3-4 weeks to be productive
   - Overkill for simple workflows

3. **Cost**
   - Self-hosted: ~$200/mo (infrastructure)
   - Temporal Cloud: ~$200-500/mo (managed)

4. **Not AI-Native**
   - Need custom code for LangChain integration
   - No built-in LLM retry strategies
   - Manual token tracking

### Use Cases ✅ Perfect For:
- Long-running business processes (hours/days/weeks)
- E-commerce order fulfillment
- Payment processing with retries
- Multi-step approval workflows
- Distributed transactions (saga pattern)
- Background jobs that must complete

### Code Example
```go
// Temporal Workflow (Go)
func RAGIndexWorkflow(ctx workflow.Context, docs []string) error {
    // This code can pause for days and resume!
    
    // Step 1: Chunk documents (5 min)
    var chunks []Chunk
    err := workflow.ExecuteActivity(ctx, ChunkDocuments, docs).Get(ctx, &chunks)
    if err != nil {
        return err
    }
    
    // Step 2: Generate embeddings (2 hours, may retry)
    var embeddings []Embedding
    err = workflow.ExecuteActivity(ctx, GenerateEmbeddings, chunks).Get(ctx, &embeddings)
    if err != nil {
        return err
    }
    
    // Step 3: Index in Qdrant
    return workflow.ExecuteActivity(ctx, IndexQdrant, embeddings).Get(ctx, nil)
}
```

---

## Option 2: LangGraph

### What Is LangGraph?
Python library for building stateful AI agent workflows. Built on LangChain, designed specifically for LLM applications.

### Architecture
```
┌─────────────────┐
│   Your App      │
│  (FastAPI)      │
└────────┬────────┘
         │
┌────────▼────────┐
│   LangGraph     │
│   (Library)     │
└────────┬────────┘
         │
┌────────▼────────┐
│  PostgreSQL     │
│  (Checkpoints)  │
└─────────────────┘
```

### Pros ✅
1. **Built for AI Workflows**
   - Native LangChain tool integration
   - Built-in retry logic for LLM calls
   - Streaming support for real-time responses
   - Token tracking out of the box

2. **Simple to Use**
   - Just a Python library
   - Define workflows as state graphs
   - 1 week learning curve

3. **Already Integrated**
   - You have LangGraph v0.2.45 installed
   - PostgreSQL checkpointing configured
   - Active usage in `langgraph_service.py`

4. **Low Operational Overhead**
   - No additional infrastructure
   - Uses existing PostgreSQL
   - ~50MB memory per workflow

5. **Perfect for Short-Medium Workflows**
   - Minutes to hours (not weeks)
   - Multi-agent collaboration
   - RAG pipelines with conditional logic

### Cons ❌
1. **Python Only**
   - Can't use directly in Go (Org-core)
   - Need gRPC calls from Org-core to AI-core

2. **Newer Technology**
   - Released 2024 (vs Temporal's 2019)
   - Less battle-tested at scale
   - Smaller community

3. **Limited to Shorter Workflows**
   - Best for <1 hour workflows
   - Not designed for days/weeks duration
   - PostgreSQL checkpoint size can grow

4. **Less Robust Retry Logic**
   - Simple exponential backoff
   - No advanced saga patterns
   - Manual compensation logic

5. **No Multi-Service Coordination**
   - Works best within one Python app
   - Can't coordinate Go + Python + TypeScript workflows easily

### Use Cases ✅ Perfect For:
- AI agent workflows (RAG, multi-agent, ReAct)
- LLM pipelines with conditional branching
- Real-time chat with tool calling
- Document processing with AI analysis
- Agentic RAG (query → retrieve → analyze → respond)

### Code Example
```python
# LangGraph Workflow (Python)
from langgraph.graph import StateGraph

def agentic_rag_workflow():
    # Define state
    class State(TypedDict):
        query: str
        documents: List[str]
        analysis: str
        response: str
    
    # Define nodes
    def retrieve(state: State) -> State:
        # Query Qdrant
        state["documents"] = qdrant.search(state["query"])
        return state
    
    def analyze(state: State) -> State:
        # LLM analyzes relevance
        prompt = f"Analyze: {state['documents']}"
        state["analysis"] = llm.invoke(prompt)
        return state
    
    def respond(state: State) -> State:
        # Generate final response
        state["response"] = llm.invoke(state["analysis"])
        return state
    
    # Build graph
    graph = StateGraph(State)
    graph.add_node("retrieve", retrieve)
    graph.add_node("analyze", analyze)
    graph.add_node("respond", respond)
    graph.add_edge("retrieve", "analyze")
    graph.add_edge("analyze", "respond")
    
    return graph.compile()
```

---

## Detailed Comparison

### 1. State Management

**Temporal:**
- ✅ Automatic state persistence after every activity
- ✅ Event sourcing (full history replay)
- ✅ State survives process crashes, restarts, redeploys

**LangGraph:**
- ✅ PostgreSQL checkpointing at node boundaries
- ✅ Can resume from any checkpoint
- ⚠️ Checkpoints must be explicitly configured

**Winner**: **Temporal** (more automatic, more robust)

---

### 2. Error Handling & Retries

**Temporal:**
```go
// Sophisticated retry policy
activityOptions := workflow.ActivityOptions{
    StartToCloseTimeout: 10 * time.Minute,
    RetryPolicy: &temporal.RetryPolicy{
        InitialInterval:    time.Second,
        BackoffCoefficient: 2.0,
        MaximumInterval:    time.Minute,
        MaximumAttempts:    5,
    },
}
```
- ✅ Per-activity retry policies
- ✅ Exponential backoff with jitter
- ✅ Circuit breaker patterns
- ✅ Compensation workflows (saga)

**LangGraph:**
```python
# Simple retry with exponential backoff
@retry(stop=stop_after_attempt(3), wait=wait_exponential())
def call_llm(prompt: str) -> str:
    return llm.invoke(prompt)
```
- ✅ Basic retry decorators
- ⚠️ Manual implementation for complex logic
- ❌ No built-in saga pattern

**Winner**: **Temporal** (enterprise-grade error handling)

---

### 3. Observability

**Temporal:**
- ✅ Built-in Web UI (workflow history, stack traces)
- ✅ Prometheus metrics
- ✅ OpenTelemetry integration
- ✅ Time-travel debugging

**LangGraph:**
- ✅ LangSmith tracing (paid)
- ✅ Custom logging to structlog
- ⚠️ No built-in UI (need custom dashboards)

**Winner**: **Temporal** (best-in-class observability)

---

### 4. Development Experience

**Temporal:**
- ⚠️ Steep learning curve (workflows vs activities concept)
- ✅ Type-safe (Go, TypeScript SDKs)
- ⚠️ Requires local Temporal Server for development

**LangGraph:**
- ✅ Simple Python code (just define a graph)
- ✅ Easy testing (run graphs locally)
- ✅ Great documentation & examples

**Winner**: **LangGraph** (much easier to learn)

---

### 5. Cost Analysis

**Temporal Self-Hosted:**
```
Monthly Cost:
- 2x EC2 t3.medium (Temporal Server): $120
- 1x RDS PostgreSQL: $50
- 1x ElasticSearch: $80
- Total: ~$250/mo
```

**Temporal Cloud:**
```
Monthly Cost:
- Startup plan: $200/mo
- Growth plan: $500-2000/mo
- No infrastructure management
```

**LangGraph:**
```
Monthly Cost:
- $0 (open source library)
- Uses existing PostgreSQL
- Optional: LangSmith tracing ($39-99/mo)
```

**Winner**: **LangGraph** (free vs $200-500/mo)

---

## Decision Framework

### Choose **Temporal** if:
✅ Workflows run for **hours to weeks**  
✅ Need **multi-language support** (Go + Python + TypeScript)  
✅ Require **enterprise-grade reliability** (financial transactions, payments)  
✅ Need **workflow versioning** (rolling updates)  
✅ Budget allows **$200-500/mo** for infrastructure  
✅ Have **DevOps resources** to manage infrastructure  

**Example workflows:**
- Order fulfillment (customer order → payment → shipping → delivery notification)
- Document processing pipeline (upload → OCR → classification → indexing → notification)
- Multi-day approval workflows (submit → review → approve → execute)

---

### Choose **LangGraph** if:
✅ Building **AI agent workflows** (RAG, multi-agent, ReAct)  
✅ Workflows complete in **minutes to hours** (not days)  
✅ Team primarily uses **Python**  
✅ Want **minimal infrastructure** (just PostgreSQL)  
✅ Need **fast iteration** (1 week vs 1 month learning)  
✅ **Budget-conscious** (free vs $500/mo)  

**Example workflows:**
- Agentic RAG (query → retrieve → rerank → analyze → respond)
- Multi-agent collaboration (researcher + writer + editor)
- Document Q&A pipeline (upload → chunk → embed → index → query)
- Content moderation (analyze → classify → ticket if needed)

---

## Recommendation for CoreSystem

### **Hybrid Approach**: Use Both! 🎯

```
┌─────────────────────────────────────────────────────┐
│                  CoreSystem                          │
├─────────────────────────────────────────────────────┤
│                                                      │
│  LangGraph (AI-Core)                                │
│  ├─ Agentic RAG workflows                           │
│  ├─ Multi-agent collaboration                       │
│  ├─ Real-time chat with tools                       │
│  └─ Content moderation analysis                     │
│                                                      │
│  Temporal (Org-Core)                                │
│  ├─ Long-running document indexing (hours)          │
│  ├─ Batch export jobs (days)                        │
│  ├─ Web crawling (continuous)                       │
│  └─ Scheduled cleanup jobs (cron)                   │
│                                                      │
└─────────────────────────────────────────────────────┘
```

### Implementation Plan

#### Phase 1: LangGraph First (Week 1-2)
**Why**: Already installed, low-risk, immediate value

1. **Move AI workflows to LangGraph:**
   - Agentic RAG pipeline (replace custom Go workflow)
   - Multi-agent document processing
   - Real-time chat workflows

2. **Expose via gRPC from AI-core to Org-core**
   ```proto
   service WorkflowService {
     rpc ExecuteRAGWorkflow(RAGWorkflowRequest) returns (stream RAGWorkflowResponse);
     rpc GetWorkflowStatus(WorkflowStatusRequest) returns (WorkflowStatus);
   }
   ```

3. **Benefits:**
   - ✅ Use existing LangGraph installation
   - ✅ AI-native workflow patterns
   - ✅ Fast iteration for AI features
   - ✅ Zero infrastructure cost

#### Phase 2: Temporal Later (Month 2-3)
**Why**: Only if you need long-running workflows

1. **Evaluate actual needs:**
   - Do you have workflows running >1 hour?
   - Need multi-day processes?
   - Require Go-native workflows?

2. **If yes, add Temporal:**
   - Start with Temporal Cloud ($200/mo, no infrastructure)
   - Migrate batch jobs, crawling, scheduled tasks
   - Keep LangGraph for AI workflows

3. **If no:**
   - Stay with LangGraph
   - Use Asynq (already have) for background jobs
   - Save $200-500/mo

---

## Quick Start: LangGraph Implementation

### 1. Enhance Existing LangGraph Service

**File**: `ai-core/app/services/langgraph_service.py`

```python
from langgraph.graph import StateGraph, END
from langgraph.checkpoint.postgres import PostgresSaver
from typing import TypedDict, List
import structlog

logger = structlog.get_logger(__name__)

# Define workflow state
class RAGWorkflowState(TypedDict):
    query: str
    strategy: str  # "hybrid", "rerank", "expansion"
    documents: List[dict]
    reranked_docs: List[dict]
    context: str
    response: str
    metadata: dict

class EnhancedLangGraphService:
    def __init__(self, qdrant_client, llm_service, cohere_client):
        self.qdrant = qdrant_client
        self.llm = llm_service
        self.cohere = cohere_client
        
        # PostgreSQL checkpointing (already configured)
        self.checkpointer = PostgresSaver(settings.DATABASE_URL)
        
        # Build workflow graph
        self.workflow = self._build_rag_workflow()
    
    def _build_rag_workflow(self) -> StateGraph:
        """Build the RAG workflow graph"""
        graph = StateGraph(RAGWorkflowState)
        
        # Add nodes
        graph.add_node("query_expansion", self._expand_query)
        graph.add_node("retrieve", self._retrieve_docs)
        graph.add_node("rerank", self._rerank_with_cohere)
        graph.add_node("generate", self._generate_response)
        
        # Define edges
        graph.set_entry_point("query_expansion")
        graph.add_edge("query_expansion", "retrieve")
        graph.add_edge("retrieve", "rerank")
        graph.add_edge("rerank", "generate")
        graph.add_edge("generate", END)
        
        # Compile with checkpointing
        return graph.compile(checkpointer=self.checkpointer)
    
    async def _expand_query(self, state: RAGWorkflowState) -> RAGWorkflowState:
        """Use LLM to expand query (replaces custom Go query_expander)"""
        prompt = f"Expand this search query with synonyms: {state['query']}"
        expanded = await self.llm.generate(prompt)
        state["metadata"]["expanded_query"] = expanded
        return state
    
    async def _retrieve_docs(self, state: RAGWorkflowState) -> RAGWorkflowState:
        """Retrieve from Qdrant (hybrid search)"""
        results = await self.qdrant.search(
            query=state["query"],
            limit=20
        )
        state["documents"] = results
        return state
    
    async def _rerank_with_cohere(self, state: RAGWorkflowState) -> RAGWorkflowState:
        """Use Cohere Rerank v4 (replaces custom Go reranker)"""
        docs_text = [doc["content"] for doc in state["documents"]]
        
        reranked = self.cohere.rerank(
            model="rerank-v4",
            query=state["query"],
            documents=docs_text,
            top_n=5
        )
        
        state["reranked_docs"] = [
            state["documents"][result.index] 
            for result in reranked.results
        ]
        return state
    
    async def _generate_response(self, state: RAGWorkflowState) -> RAGWorkflowState:
        """Generate final response"""
        context = "\n\n".join(doc["content"] for doc in state["reranked_docs"])
        state["context"] = context
        
        prompt = f"Context: {context}\n\nQuestion: {state['query']}\n\nAnswer:"
        state["response"] = await self.llm.generate(prompt)
        return state
    
    async def execute(self, query: str, strategy: str = "hybrid") -> dict:
        """Execute RAG workflow with streaming"""
        config = {"configurable": {"thread_id": str(uuid.uuid4())}}
        
        initial_state = RAGWorkflowState(
            query=query,
            strategy=strategy,
            documents=[],
            reranked_docs=[],
            context="",
            response="",
            metadata={}
        )
        
        # Execute workflow (with automatic checkpointing)
        final_state = await self.workflow.ainvoke(initial_state, config)
        
        return {
            "response": final_state["response"],
            "context": final_state["context"],
            "metadata": final_state["metadata"]
        }
```

### 2. Expose via gRPC

**File**: `ai-core/proto/workflow.proto`

```protobuf
syntax = "proto3";

package workflow;

message RAGWorkflowRequest {
  string query = 1;
  string strategy = 2; // "hybrid", "rerank", "expansion"
  string org_id = 3;
}

message RAGWorkflowResponse {
  string response = 1;
  string context = 2;
  map<string, string> metadata = 3;
}

service WorkflowService {
  rpc ExecuteRAGWorkflow(RAGWorkflowRequest) returns (RAGWorkflowResponse);
}
```

### 3. Call from Org-core

**File**: `Org-core/internal/grpc/workflow_client.go`

```go
// Replace custom workflow engine with LangGraph call
func (c *Client) ExecuteRAGWorkflow(ctx context.Context, query string) (*WorkflowResponse, error) {
    // Call AI-core's LangGraph service via gRPC
    resp, err := c.workflowClient.ExecuteRAGWorkflow(ctx, &pb.RAGWorkflowRequest{
        Query:    query,
        Strategy: "hybrid",
        OrgId:    c.orgID,
    })
    if err != nil {
        return nil, err
    }
    
    return &WorkflowResponse{
        Response: resp.Response,
        Context:  resp.Context,
        Metadata: resp.Metadata,
    }, nil
}
```

---

## Migration Path

### Week 1: LangGraph Setup
- [x] Already have LangGraph v0.2.45 installed
- [ ] Create enhanced workflow in `langgraph_service.py`
- [ ] Add gRPC endpoint in AI-core
- [ ] Test with sample queries

### Week 2: Replace Custom Go Workflows
- [ ] Replace `Org-core/internal/workflows/engine.go` with gRPC calls
- [ ] Remove custom workflow definitions
- [ ] Update handlers to use new workflow client
- [ ] Run integration tests

### Week 3: Production Validation
- [ ] Monitor workflow execution times
- [ ] Check PostgreSQL checkpoint growth
- [ ] Validate retry behavior
- [ ] Load test with concurrent workflows

### Decision Point (End of Month 1)
**Question**: Do we need Temporal?

**Evaluate**:
- Are any workflows taking >1 hour?
- Do we need Go-native workflows?
- Is budget available ($200-500/mo)?

**If YES → Add Temporal (Month 2)**  
**If NO → Stay with LangGraph (save $500/mo)**

---

## Conclusion

### **Immediate Recommendation: Start with LangGraph**

**Reasons:**
1. ✅ Already installed and configured
2. ✅ Perfect fit for AI workflows (RAG, agents)
3. ✅ Zero additional infrastructure
4. ✅ Fast to implement (1-2 weeks)
5. ✅ Low risk (can add Temporal later if needed)

**Next Steps:**
1. Enhance `langgraph_service.py` with RAG workflow
2. Add gRPC endpoint for Org-core to call
3. Replace custom `workflows/engine.go` with gRPC client
4. Monitor for 1 month
5. Decide if Temporal needed based on actual usage

**Cost Savings**: $0 now, potential $500/mo saved by avoiding Temporal

---

**Questions for Discussion:**
1. What are your longest-running workflows currently?
2. Do you expect workflows that take >1 hour?
3. Is there budget for Temporal if needed later?

