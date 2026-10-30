# @quarry/client@0.1.0

A TypeScript SDK client for the localhost API.

## Usage

First, install the SDK from npm.

```bash
npm install @quarry/client --save
```

Next, try it out.


```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { AgentInteractRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // AgentStepRequest
    agentStepRequest: ...,
  } satisfies AgentInteractRequest;

  try {
    const data = await api.agentInteract(body);
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
*AgentApi* | [**agentInteract**](docs/AgentApi.md#agentinteract) | **POST** /v1/agent/runs/{run_id}/interact | Ergonomic alias for one governed browser action
*AgentApi* | [**agentStep**](docs/AgentApi.md#agentstepoperation) | **POST** /v1/agent/runs/{run_id}/step | Execute one governed browser action
*AgentApi* | [**checkAgentProcedureReplay**](docs/AgentApi.md#checkagentprocedurereplay) | **POST** /v1/agent/procedures/replay-check | Compare actions with a procedure without executing effects
*AgentApi* | [**closeAgentRun**](docs/AgentApi.md#closeagentrun) | **DELETE** /v1/agent/runs/{run_id} | Close a browser-agent run and release its lease
*AgentApi* | [**compileAgentProcedure**](docs/AgentApi.md#compileagentprocedure) | **POST** /v1/agent/runs/{run_id}/procedure | Compile verified receipts into a replay candidate
*AgentApi* | [**impactCheckAgentProcedure**](docs/AgentApi.md#impactcheckagentprocedure) | **POST** /v1/agent/procedures/impact-check | Identify changed sources that require procedure quarantine
*AgentApi* | [**listAgentEgressReceipts**](docs/AgentApi.md#listagentegressreceipts) | **GET** /v1/agent/runs/{run_id}/egress-receipts | Read redacted live browser egress decisions
*AgentApi* | [**listAgentReceipts**](docs/AgentApi.md#listagentreceipts) | **GET** /v1/agent/runs/{run_id}/receipts | Read tenant-scoped immutable action receipts
*AgentApi* | [**qualityCheckAgentProcedure**](docs/AgentApi.md#qualitycheckagentprocedure) | **POST** /v1/agent/procedures/quality-check | Evaluate deterministic procedure promotion evidence
*AgentApi* | [**startAgentRun**](docs/AgentApi.md#startagentrunoperation) | **POST** /v1/agent/runs | Acquire a governed browser-agent run
*BatchApi* | [**batchHandoff**](docs/BatchApi.md#batchhandoff) | **POST** /v1/batch | Start a batch scrape job
*ChangeTrackingApi* | [**changeCheck**](docs/ChangeTrackingApi.md#changecheckoperation) | **POST** /v1/change/check | Re-fetch a URL and compare against its baseline
*ChangeTrackingApi* | [**changeHistory**](docs/ChangeTrackingApi.md#changehistory) | **GET** /v1/change/history | Ordered baseline chain for a tracked URL
*ChangeTrackingApi* | [**changeLatest**](docs/ChangeTrackingApi.md#changelatest) | **GET** /v1/change/latest | Most recent baseline for a tracked URL
*ChangeTrackingApi* | [**promoteTrackedResultToSnapshot**](docs/ChangeTrackingApi.md#promotetrackedresulttosnapshot) | **POST** /v1/change/snapshot | Promote the latest baseline to the public Snapshot shape
*ChangeTrackingApi* | [**scheduleRefreshRun**](docs/ChangeTrackingApi.md#schedulerefreshrunoperation) | **POST** /v1/change/refresh | Enqueue an immediate re-check onto the durable frontier
*CrawlApi* | [**crawlHandoff**](docs/CrawlApi.md#crawlhandoff) | **POST** /v1/crawl | Start a crawl job
*ExtractApi* | [**extract**](docs/ExtractApi.md#extractoperation) | **POST** /v1/extract | Multi-URL structured extraction
*InternalApi* | [**internalRunPage**](docs/InternalApi.md#internalrunpage) | **POST** /v1/internal/run_page | Internal page execution (orchestrator → edge)
*MapApi* | [**map**](docs/MapApi.md#mapoperation) | **POST** /v1/map | Whole-site URL discovery
*OpsApi* | [**health**](docs/OpsApi.md#health) | **GET** /health | Liveness probe
*OpsApi* | [**ready**](docs/OpsApi.md#ready) | **GET** /ready | Readiness probe
*ResourcesApi* | [**createSource**](docs/ResourcesApi.md#createsourceoperation) | **POST** /v1/sources | Register an ingestion source
*ResourcesApi* | [**deleteSource**](docs/ResourcesApi.md#deletesource) | **DELETE** /v1/sources/{source_id} | Soft-delete an ingestion source
*ResourcesApi* | [**getArtifact**](docs/ResourcesApi.md#getartifact) | **GET** /v1/artifacts/{artifact_id} | Fetch artifact bytes or metadata
*ResourcesApi* | [**listArtifacts**](docs/ResourcesApi.md#listartifacts) | **GET** /v1/artifacts | List stored artifacts (served locally by edge)
*ResourcesApi* | [**listBenchmarks**](docs/ResourcesApi.md#listbenchmarks) | **GET** /v1/benchmarks | List benchmark runs (placeholder)
*ResourcesApi* | [**listJobEvents**](docs/ResourcesApi.md#listjobevents) | **GET** /v1/jobs/{job_id}/events | Durable events keyed by handoff job_id
*ResourcesApi* | [**listJobsByKind**](docs/ResourcesApi.md#listjobsbykind) | **GET** /v1/{kind}/jobs | List jobs of one kind
*ResourcesApi* | [**listRequestQueues**](docs/ResourcesApi.md#listrequestqueues) | **GET** /v1/request-queues | List request queues with live stats
*ResourcesApi* | [**listRunEvents**](docs/ResourcesApi.md#listrunevents) | **GET** /v1/runs/{run_id}/events | Durable event history for one run
*ResourcesApi* | [**listSnapshots**](docs/ResourcesApi.md#listsnapshots) | **GET** /v1/snapshots | List captured page snapshots
*ResourcesApi* | [**listSources**](docs/ResourcesApi.md#listsources) | **GET** /v1/sources | List ingestion sources
*SchedulesApi* | [**backfillSchedule**](docs/SchedulesApi.md#backfillschedule) | **POST** /v1/schedules/{schedule_id}/backfill | Backfill missed runs
*SchedulesApi* | [**createSchedule**](docs/SchedulesApi.md#createscheduleoperation) | **POST** /v1/schedules | Create a cron / one-shot / change-monitor schedule
*SchedulesApi* | [**deleteSchedule**](docs/SchedulesApi.md#deleteschedule) | **DELETE** /v1/schedules/{schedule_id} | Delete a schedule
*SchedulesApi* | [**listSchedules**](docs/SchedulesApi.md#listschedules) | **GET** /v1/schedules | List schedules
*SchedulesApi* | [**pauseSchedule**](docs/SchedulesApi.md#pauseschedule) | **POST** /v1/schedules/{schedule_id}/pause | Pause a schedule
*SchedulesApi* | [**triggerSchedule**](docs/SchedulesApi.md#triggerschedule) | **POST** /v1/schedules/{schedule_id}/trigger | Trigger an immediate scheduled run
*SchedulesApi* | [**unpauseSchedule**](docs/SchedulesApi.md#unpauseschedule) | **POST** /v1/schedules/{schedule_id}/unpause | Unpause a schedule
*ScrapeApi* | [**scrape**](docs/ScrapeApi.md#scrapeoperation) | **POST** /v1/scrape | Scrape a single URL
*ScrapeApi* | [**scrapeStream**](docs/ScrapeApi.md#scrapestream) | **POST** /v1/scrape/stream | Scrape with SSE event stream
*SearchApi* | [**search**](docs/SearchApi.md#searchoperation) | **POST** /v1/search | SERP + own-corpus search (hybrid when Data Plane wired)
*TeamApi* | [**teamActivity**](docs/TeamApi.md#teamactivity) | **GET** /v1/team/activity | Recent activity feed
*TeamApi* | [**teamConcurrency**](docs/TeamApi.md#teamconcurrency) | **GET** /v1/team/concurrency | Per-org concurrency snapshot
*TeamApi* | [**teamCreditUsage**](docs/TeamApi.md#teamcreditusage) | **GET** /v1/team/credit-usage | Team credit usage for a window
*TeamApi* | [**teamQueueStatus**](docs/TeamApi.md#teamqueuestatus) | **GET** /v1/team/queue-status | Aggregate queue snapshot for the org
*TeamApi* | [**teamTokenUsage**](docs/TeamApi.md#teamtokenusage) | **GET** /v1/team/token-usage | Team token usage for a window


### Models

- [AccessibilityNode](docs/AccessibilityNode.md)
- [AccessibilityProjection](docs/AccessibilityProjection.md)
- [ActionOutcome](docs/ActionOutcome.md)
- [AgentAction](docs/AgentAction.md)
- [AgentConstraints](docs/AgentConstraints.md)
- [AgentStepRequest](docs/AgentStepRequest.md)
- [ArtifactSummary](docs/ArtifactSummary.md)
- [BackAction](docs/BackAction.md)
- [BaselineSnapshot](docs/BaselineSnapshot.md)
- [BatchRequest](docs/BatchRequest.md)
- [BenchmarkSummary](docs/BenchmarkSummary.md)
- [BrowserDialog](docs/BrowserDialog.md)
- [BrowserDriverCapabilities](docs/BrowserDriverCapabilities.md)
- [BrowserEgressReceipt](docs/BrowserEgressReceipt.md)
- [BrowserFrame](docs/BrowserFrame.md)
- [BrowserObservation](docs/BrowserObservation.md)
- [BrowserProcedure](docs/BrowserProcedure.md)
- [BrowserProcedureStepsInner](docs/BrowserProcedureStepsInner.md)
- [BrowserSnapshot](docs/BrowserSnapshot.md)
- [BrowserTelemetry](docs/BrowserTelemetry.md)
- [CachePolicy](docs/CachePolicy.md)
- [ChangeCheck200Response](docs/ChangeCheck200Response.md)
- [ChangeCheckRequest](docs/ChangeCheckRequest.md)
- [ChangeHistory200Response](docs/ChangeHistory200Response.md)
- [ChangeInfo](docs/ChangeInfo.md)
- [ChangeLatest200Response](docs/ChangeLatest200Response.md)
- [ChangeRecord](docs/ChangeRecord.md)
- [ClickAction](docs/ClickAction.md)
- [ClickPointAction](docs/ClickPointAction.md)
- [ClickRefAction](docs/ClickRefAction.md)
- [ClickSemanticAction](docs/ClickSemanticAction.md)
- [CompileProcedureRequest](docs/CompileProcedureRequest.md)
- [CrawlRequest](docs/CrawlRequest.md)
- [CreateSchedule200Response](docs/CreateSchedule200Response.md)
- [CreateScheduleRequest](docs/CreateScheduleRequest.md)
- [CreateSource200Response](docs/CreateSource200Response.md)
- [CreateSourceRequest](docs/CreateSourceRequest.md)
- [DeleteSchedule200Response](docs/DeleteSchedule200Response.md)
- [DeleteSource200Response](docs/DeleteSource200Response.md)
- [DialogResponseAction](docs/DialogResponseAction.md)
- [DownloadRefAction](docs/DownloadRefAction.md)
- [DriverInfo](docs/DriverInfo.md)
- [DriverSignals](docs/DriverSignals.md)
- [ElementFingerprint](docs/ElementFingerprint.md)
- [EnvelopeBrowserObservation](docs/EnvelopeBrowserObservation.md)
- [EnvelopeBrowserProcedure](docs/EnvelopeBrowserProcedure.md)
- [EnvelopeClosedRun](docs/EnvelopeClosedRun.md)
- [EnvelopeClosedRunData](docs/EnvelopeClosedRunData.md)
- [EnvelopeEgressReceipts](docs/EnvelopeEgressReceipts.md)
- [EnvelopeEgressReceiptsData](docs/EnvelopeEgressReceiptsData.md)
- [EnvelopeError](docs/EnvelopeError.md)
- [EnvelopeErrorError](docs/EnvelopeErrorError.md)
- [EnvelopeHandoffAck](docs/EnvelopeHandoffAck.md)
- [EnvelopeNormalizedOutput](docs/EnvelopeNormalizedOutput.md)
- [EnvelopeProcedureImpact](docs/EnvelopeProcedureImpact.md)
- [EnvelopeProcedureQuality](docs/EnvelopeProcedureQuality.md)
- [EnvelopeReplayDecision](docs/EnvelopeReplayDecision.md)
- [EnvelopeStartAgentRun](docs/EnvelopeStartAgentRun.md)
- [EnvelopeStartAgentRunData](docs/EnvelopeStartAgentRunData.md)
- [EnvelopeStepReceipts](docs/EnvelopeStepReceipts.md)
- [ErrorBody](docs/ErrorBody.md)
- [ErrorDetail](docs/ErrorDetail.md)
- [EvaluateAction](docs/EvaluateAction.md)
- [EvidenceDelta](docs/EvidenceDelta.md)
- [EvidenceDeltaDom](docs/EvidenceDeltaDom.md)
- [EvidenceDeltaNetwork](docs/EvidenceDeltaNetwork.md)
- [ExtractItem](docs/ExtractItem.md)
- [ExtractRef](docs/ExtractRef.md)
- [ExtractRequest](docs/ExtractRequest.md)
- [ExtractResponse](docs/ExtractResponse.md)
- [FacetCount](docs/FacetCount.md)
- [FormatRef](docs/FormatRef.md)
- [ForwardAction](docs/ForwardAction.md)
- [FrameClickRefAction](docs/FrameClickRefAction.md)
- [FrameDownloadRefAction](docs/FrameDownloadRefAction.md)
- [FrameSelectRefAction](docs/FrameSelectRefAction.md)
- [FrameSnapshotRefAction](docs/FrameSnapshotRefAction.md)
- [FrameSnapshotTargetBase](docs/FrameSnapshotTargetBase.md)
- [FrameTypeRefAction](docs/FrameTypeRefAction.md)
- [FrameUploadRefAction](docs/FrameUploadRefAction.md)
- [FrameWaitForRefAction](docs/FrameWaitForRefAction.md)
- [GetArtifact200Response](docs/GetArtifact200Response.md)
- [GetContentAction](docs/GetContentAction.md)
- [HandoffAck](docs/HandoffAck.md)
- [HostConcurrency](docs/HostConcurrency.md)
- [InternalRunPage](docs/InternalRunPage.md)
- [InternalRunPageResult](docs/InternalRunPageResult.md)
- [JobHistoryEvent](docs/JobHistoryEvent.md)
- [JobStage](docs/JobStage.md)
- [JobSummary](docs/JobSummary.md)
- [LabelLocator](docs/LabelLocator.md)
- [LegacyAgentAction](docs/LegacyAgentAction.md)
- [Link](docs/Link.md)
- [ListArtifacts200Response](docs/ListArtifacts200Response.md)
- [ListArtifacts200ResponseData](docs/ListArtifacts200ResponseData.md)
- [ListBenchmarks200Response](docs/ListBenchmarks200Response.md)
- [ListBenchmarks200ResponseData](docs/ListBenchmarks200ResponseData.md)
- [ListJobsByKind200Response](docs/ListJobsByKind200Response.md)
- [ListJobsByKind200ResponseData](docs/ListJobsByKind200ResponseData.md)
- [ListRequestQueues200Response](docs/ListRequestQueues200Response.md)
- [ListRequestQueues200ResponseData](docs/ListRequestQueues200ResponseData.md)
- [ListRunEvents200Response](docs/ListRunEvents200Response.md)
- [ListRunEvents200ResponseData](docs/ListRunEvents200ResponseData.md)
- [ListSchedules200Response](docs/ListSchedules200Response.md)
- [ListSchedules200ResponseData](docs/ListSchedules200ResponseData.md)
- [ListSnapshots200Response](docs/ListSnapshots200Response.md)
- [ListSnapshots200ResponseData](docs/ListSnapshots200ResponseData.md)
- [ListSources200Response](docs/ListSources200Response.md)
- [ListSources200ResponseData](docs/ListSources200ResponseData.md)
- [MapLink](docs/MapLink.md)
- [MapRequest](docs/MapRequest.md)
- [MapResponse](docs/MapResponse.md)
- [MouseWheelAction](docs/MouseWheelAction.md)
- [NavigateAction](docs/NavigateAction.md)
- [NormalizedOutput](docs/NormalizedOutput.md)
- [NthLocator](docs/NthLocator.md)
- [ObservationDelta](docs/ObservationDelta.md)
- [OutputFormats](docs/OutputFormats.md)
- [OverlapPolicy](docs/OverlapPolicy.md)
- [PageMetadata](docs/PageMetadata.md)
- [PdfAction](docs/PdfAction.md)
- [PlaceholderLocator](docs/PlaceholderLocator.md)
- [PressAction](docs/PressAction.md)
- [ProcedureImpactRequest](docs/ProcedureImpactRequest.md)
- [PromoteTrackedResultToSnapshot200Response](docs/PromoteTrackedResultToSnapshot200Response.md)
- [QueueStatusEntry](docs/QueueStatusEntry.md)
- [RateLimitError](docs/RateLimitError.md)
- [ReplayCheckRequest](docs/ReplayCheckRequest.md)
- [RequestQueueSummary](docs/RequestQueueSummary.md)
- [RequestQueueSummaryStats](docs/RequestQueueSummaryStats.md)
- [ResponseMeta](docs/ResponseMeta.md)
- [ResponseMetaPage](docs/ResponseMetaPage.md)
- [RoleLocator](docs/RoleLocator.md)
- [ScheduleRefreshRun200Response](docs/ScheduleRefreshRun200Response.md)
- [ScheduleRefreshRun200ResponseData](docs/ScheduleRefreshRun200ResponseData.md)
- [ScheduleRefreshRunRequest](docs/ScheduleRefreshRunRequest.md)
- [ScheduleSummary](docs/ScheduleSummary.md)
- [ScrapeRequest](docs/ScrapeRequest.md)
- [ScreenshotAction](docs/ScreenshotAction.md)
- [ScrollAction](docs/ScrollAction.md)
- [SearchRequest](docs/SearchRequest.md)
- [SearchResponse](docs/SearchResponse.md)
- [SearchResultItem](docs/SearchResultItem.md)
- [SelectAction](docs/SelectAction.md)
- [SelectRefAction](docs/SelectRefAction.md)
- [SelectSemanticAction](docs/SelectSemanticAction.md)
- [SemanticLocator](docs/SemanticLocator.md)
- [SemanticTargetAction](docs/SemanticTargetAction.md)
- [Snapshot](docs/Snapshot.md)
- [SnapshotRefAction](docs/SnapshotRefAction.md)
- [SnapshotTarget](docs/SnapshotTarget.md)
- [SnapshotTargetBase](docs/SnapshotTargetBase.md)
- [Source](docs/Source.md)
- [StartAgentRunRequest](docs/StartAgentRunRequest.md)
- [StartAgentRunRequestViewport](docs/StartAgentRunRequestViewport.md)
- [StepReceipt](docs/StepReceipt.md)
- [TeamActivity200Response](docs/TeamActivity200Response.md)
- [TeamActivity200ResponseData](docs/TeamActivity200ResponseData.md)
- [TeamActivityEntry](docs/TeamActivityEntry.md)
- [TeamConcurrency](docs/TeamConcurrency.md)
- [TeamConcurrency200Response](docs/TeamConcurrency200Response.md)
- [TeamCreditUsage](docs/TeamCreditUsage.md)
- [TeamCreditUsage200Response](docs/TeamCreditUsage200Response.md)
- [TeamQueueStatus](docs/TeamQueueStatus.md)
- [TeamQueueStatus200Response](docs/TeamQueueStatus200Response.md)
- [TeamTokenUsage](docs/TeamTokenUsage.md)
- [TeamTokenUsage200Response](docs/TeamTokenUsage200Response.md)
- [TestIdLocator](docs/TestIdLocator.md)
- [TextLocator](docs/TextLocator.md)
- [TriggerSchedule202Response](docs/TriggerSchedule202Response.md)
- [TypeAction](docs/TypeAction.md)
- [TypeRefAction](docs/TypeRefAction.md)
- [TypeSemanticAction](docs/TypeSemanticAction.md)
- [UploadRefAction](docs/UploadRefAction.md)
- [UrlTriple](docs/UrlTriple.md)
- [WaitAction](docs/WaitAction.md)
- [WaitForAction](docs/WaitForAction.md)
- [WaitForRefAction](docs/WaitForRefAction.md)
- [WaitForSemanticAction](docs/WaitForSemanticAction.md)

### Authorization

Endpoints do not require authorization.


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

[Apache-2.0]()
