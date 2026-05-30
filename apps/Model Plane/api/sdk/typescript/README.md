# @model-plane/sdk@0.1.0

A TypeScript SDK client for the localhost API.

## Usage

First, install the SDK from npm.

```bash
npm install @model-plane/sdk --save
```

Next, try it out.


```ts
import {
  Configuration,
  AiApi,
} from '@model-plane/sdk';
import type { AiChatOperationRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new AiApi(config);

  const body = {
    // AiChatRequest
    aiChatRequest: ...,
  } satisfies AiChatOperationRequest;

  try {
    const data = await api.aiChat(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```


## Documentation

### API Endpoints

All URIs are relative to *http://localhost:8080*

| Class | Method | HTTP request | Description
| ----- | ------ | ------------ | -------------
*AiApi* | [**aiChat**](docs/AiApi.md#aichatoperation) | **POST** /v1/ai/chat | Chat completion via inference-core
*AiApi* | [**aiDocuments**](docs/AiApi.md#aidocumentsoperation) | **POST** /v1/ai/documents | Document processing (queued)
*AiApi* | [**aiImages**](docs/AiApi.md#aiimagesoperation) | **POST** /v1/ai/images | Image generation (queued)
*AiApi* | [**aiRealtime**](docs/AiApi.md#airealtimeoperation) | **POST** /v1/ai/realtime | Realtime session placeholder
*AiApi* | [**aiSpeech**](docs/AiApi.md#aispeechoperation) | **POST** /v1/ai/speech | Text-to-speech synthesis (queued)
*AiApi* | [**aiTranslate**](docs/AiApi.md#aitranslateoperation) | **POST** /v1/ai/translate | Text translation (queued)
*CapabilitiesApi* | [**getCapability**](docs/CapabilitiesApi.md#getcapability) | **GET** /v1/capabilities/{id} | Get a capability by ID
*CapabilitiesApi* | [**listCapabilities**](docs/CapabilitiesApi.md#listcapabilities) | **GET** /v1/capabilities | List registered capabilities
*CronApi* | [**createCron**](docs/CronApi.md#createcron) | **POST** /v1/cron | Create a cron job
*CronApi* | [**deleteCron**](docs/CronApi.md#deletecron) | **DELETE** /v1/cron/{id} | Delete a cron job
*CronApi* | [**getCron**](docs/CronApi.md#getcron) | **GET** /v1/cron/{id} | Get a cron job by ID
*CronApi* | [**listCron**](docs/CronApi.md#listcron) | **GET** /v1/cron | List cron jobs
*CronApi* | [**patchCron**](docs/CronApi.md#patchcron) | **PATCH** /v1/cron/{id} | Update a cron job
*EventsApi* | [**streamRunEvents**](docs/EventsApi.md#streamrunevents) | **GET** /v1/runs/{run_id}/events | Stream orchestration events for a run via SSE
*HealthApi* | [**healthz**](docs/HealthApi.md#healthz) | **GET** /healthz | Liveness probe
*HealthApi* | [**metrics**](docs/HealthApi.md#metrics) | **GET** /metrics | Prometheus metrics
*HealthApi* | [**readyz**](docs/HealthApi.md#readyz) | **GET** /readyz | Readiness probe
*InvokeApi* | [**invoke**](docs/InvokeApi.md#invokeoperation) | **POST** /v1/invoke | Synchronous inference request
*InvokeApi* | [**invokeStream**](docs/InvokeApi.md#invokestream) | **POST** /v1/invoke/stream | Streaming inference via SSE
*MemoryApi* | [**createMemory**](docs/MemoryApi.md#creatememory) | **POST** /v1/memory | Create a memory entry
*MemoryApi* | [**deleteMemory**](docs/MemoryApi.md#deletememory) | **DELETE** /v1/memory/{id} | Delete a memory entry
*MemoryApi* | [**getMemory**](docs/MemoryApi.md#getmemory) | **GET** /v1/memory/{id} | Get a memory entry by ID
*MemoryApi* | [**listMemory**](docs/MemoryApi.md#listmemory) | **GET** /v1/memory | List memory entries
*MemoryApi* | [**patchMemory**](docs/MemoryApi.md#patchmemory) | **PATCH** /v1/memory/{id} | Update a memory entry
*OrchestrationApi* | [**approvePlan**](docs/OrchestrationApi.md#approveplan) | **POST** /v1/orchestration/plans/{plan_id}/approve | Approve a plan
*OrchestrationApi* | [**cancelRun**](docs/OrchestrationApi.md#cancelrun) | **POST** /v1/orchestration/runs/{run_id}/cancel | Request cancellation of a run
*OrchestrationApi* | [**decideApproval**](docs/OrchestrationApi.md#decideapproval) | **POST** /v1/orchestration/approvals/{approval_id}/decide | Approve or reject an approval request
*OrchestrationApi* | [**getApproval**](docs/OrchestrationApi.md#getapproval) | **GET** /v1/orchestration/approvals/{approval_id} | Get an approval by ID
*OrchestrationApi* | [**getPlan**](docs/OrchestrationApi.md#getplan) | **GET** /v1/orchestration/plans/{plan_id} | Get a plan by ID
*OrchestrationApi* | [**getSubagentLineage**](docs/OrchestrationApi.md#getsubagentlineage) | **GET** /v1/orchestration/threads/{thread_id}/lineage | Get subagent lineage for a thread
*OrchestrationApi* | [**getTodo**](docs/OrchestrationApi.md#gettodo) | **GET** /v1/orchestration/todos/{todo_id} | Get a todo by ID
*OrchestrationApi* | [**listApprovals**](docs/OrchestrationApi.md#listapprovals) | **GET** /v1/orchestration/runs/{run_id}/approvals | List approvals for a run
*OrchestrationApi* | [**listPlans**](docs/OrchestrationApi.md#listplans) | **GET** /v1/orchestration/runs/{run_id}/plans | List plans for a run
*OrchestrationApi* | [**listTodos**](docs/OrchestrationApi.md#listtodos) | **GET** /v1/orchestration/threads/{thread_id}/todos | List todos for a thread
*OrchestrationApi* | [**rejectPlan**](docs/OrchestrationApi.md#rejectplan) | **POST** /v1/orchestration/plans/{plan_id}/reject | Reject a plan
*OrchestrationApi* | [**resumeRun**](docs/OrchestrationApi.md#resumerun) | **POST** /v1/orchestration/runs/{run_id}/resume | Request resumption of a cancelled/paused run
*OrchestrationApi* | [**updateTodoStatus**](docs/OrchestrationApi.md#updatetodostatus) | **POST** /v1/orchestration/todos/{todo_id}/status | Transition a todo to a new status
*SkillsApi* | [**createSkill**](docs/SkillsApi.md#createskill) | **POST** /v1/skills | Create a skill
*SkillsApi* | [**deleteSkill**](docs/SkillsApi.md#deleteskill) | **DELETE** /v1/skills/{id} | Delete a skill
*SkillsApi* | [**getSkill**](docs/SkillsApi.md#getskill) | **GET** /v1/skills/{id} | Get a skill by ID
*SkillsApi* | [**listSkills**](docs/SkillsApi.md#listskills) | **GET** /v1/skills | List skills
*SkillsApi* | [**patchSkill**](docs/SkillsApi.md#patchskill) | **PATCH** /v1/skills/{id} | Update a skill
*TasksApi* | [**cancelTask**](docs/TasksApi.md#canceltask) | **POST** /v1/tasks/{id}/cancel | Cancel a task
*TasksApi* | [**createTask**](docs/TasksApi.md#createtask) | **POST** /v1/tasks | Create a task
*TasksApi* | [**getTask**](docs/TasksApi.md#gettask) | **GET** /v1/tasks/{id} | Get a task by ID
*TasksApi* | [**listTasks**](docs/TasksApi.md#listtasks) | **GET** /v1/tasks | List tasks
*TasksApi* | [**patchTask**](docs/TasksApi.md#patchtask) | **PATCH** /v1/tasks/{id} | Update a task


### Models

- [AiChatRequest](docs/AiChatRequest.md)
- [AiChatResponse](docs/AiChatResponse.md)
- [AiChatResponseUsage](docs/AiChatResponseUsage.md)
- [AiDocumentsRequest](docs/AiDocumentsRequest.md)
- [AiImagesRequest](docs/AiImagesRequest.md)
- [AiRealtimeRequest](docs/AiRealtimeRequest.md)
- [AiSpeechRequest](docs/AiSpeechRequest.md)
- [AiTranslateRequest](docs/AiTranslateRequest.md)
- [Approval](docs/Approval.md)
- [CancelRun200Response](docs/CancelRun200Response.md)
- [Capability](docs/Capability.md)
- [ChatMessage](docs/ChatMessage.md)
- [DecideApprovalBody](docs/DecideApprovalBody.md)
- [GetApproval200Response](docs/GetApproval200Response.md)
- [GetPlan200Response](docs/GetPlan200Response.md)
- [GetSubagentLineage200Response](docs/GetSubagentLineage200Response.md)
- [GetTodo200Response](docs/GetTodo200Response.md)
- [InvokeRequest](docs/InvokeRequest.md)
- [InvokeResponse](docs/InvokeResponse.md)
- [ListApprovals200Response](docs/ListApprovals200Response.md)
- [ListCapabilities200Response](docs/ListCapabilities200Response.md)
- [ListPlans200Response](docs/ListPlans200Response.md)
- [ListTodos200Response](docs/ListTodos200Response.md)
- [ModelError](docs/ModelError.md)
- [OrchestrationEvent](docs/OrchestrationEvent.md)
- [Plan](docs/Plan.md)
- [PlanState](docs/PlanState.md)
- [PlanStep](docs/PlanStep.md)
- [QueuedResponse](docs/QueuedResponse.md)
- [ResumeRun200Response](docs/ResumeRun200Response.md)
- [SseChunk](docs/SseChunk.md)
- [SubagentLineage](docs/SubagentLineage.md)
- [SubagentLineageEdgesInner](docs/SubagentLineageEdgesInner.md)
- [TaskInput](docs/TaskInput.md)
- [Todo](docs/Todo.md)
- [TodoState](docs/TodoState.md)
- [TransitionPlanBody](docs/TransitionPlanBody.md)
- [TransitionTodoBody](docs/TransitionTodoBody.md)

### Authorization


Authentication schemes defined for the API:
<a id="bearerAuth"></a>
#### bearerAuth


- **Type**: HTTP Bearer Token authentication (JWT)

## About

This TypeScript SDK client supports the [Fetch API](https://fetch.spec.whatwg.org/)
and is automatically generated by the
[OpenAPI Generator](https://openapi-generator.tech) project:

- API version: `0.1.0`
- Package version: `0.1.0`
- Generator version: `7.22.0`
- Build package: `org.openapitools.codegen.languages.TypeScriptFetchClientCodegen`

The generated npm module supports the following:

- Environments
  * Node.js
  * Webpack
  * Browserify
- Language levels
  * ES5 - you must have a Promises/A+ library installed
  * ES6
- Module systems
  * CommonJS
  * ES6 module system


## Development

### Building

To build the TypeScript source code, you need to have Node.js and npm installed.
After cloning the repository, navigate to the project directory and run:

```bash
npm install
npm run build
```

### Publishing

Once you've built the package, you can publish it to npm:

```bash
npm publish
```

## License

[]()
