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
import type { AgentHitTestRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // number | The point\'s horizontal position, in CSS pixels from the viewport\'s left edge.
    x: 8.14,
    // number | The point\'s vertical position, in CSS pixels from the viewport\'s top edge.
    y: 8.14,
  } satisfies AgentHitTestRequest;

  try {
    const data = await api.agentHitTest(body);
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
*AgentApi* | [**agentHitTest**](docs/AgentApi.md#agenthittest) | **GET** /v1/agent/runs/{run_id}/hit-test | Map a viewport point to the observed target under it (dark)
*AgentApi* | [**agentInteract**](docs/AgentApi.md#agentinteract) | **POST** /v1/agent/runs/{run_id}/interact | Ergonomic alias for one governed browser action
*AgentApi* | [**agentStep**](docs/AgentApi.md#agentstepoperation) | **POST** /v1/agent/runs/{run_id}/step | Execute one governed browser action
*AgentApi* | [**applyAgentOriginExtension**](docs/AgentApi.md#applyagentoriginextensionoperation) | **POST** /v1/agent/runs/{run_id}/origin-extension | Apply a person\&#39;s per-origin consent to a live run (dark)
*AgentApi* | [**attachAgentCurrentTab**](docs/AgentApi.md#attachagentcurrenttaboperation) | **POST** /v1/agent/runs/{run_id}/attach-current-tab | Attach an agent to a tab the person already has open (dark; service principal only)
*AgentApi* | [**attachAgentRun**](docs/AgentApi.md#attachagentrunoperation) | **POST** /v1/agent/runs/{run_id}/attach | Attach an agent to a person\&#39;s live run (dark; service principal only)
*AgentApi* | [**browserRedirectHop**](docs/AgentApi.md#browserredirecthopoperation) | **POST** /v1/browser/redirect-hop | One credentialless redirect hop for assistant navigation
*AgentApi* | [**checkAgentProcedureReplay**](docs/AgentApi.md#checkagentprocedurereplay) | **POST** /v1/agent/procedures/replay-check | Compare actions with a procedure without executing effects
*AgentApi* | [**closeAgentRun**](docs/AgentApi.md#closeagentrun) | **DELETE** /v1/agent/runs/{run_id} | Close a browser-agent run and release its lease
*AgentApi* | [**closeAgentTab**](docs/AgentApi.md#closeagenttab) | **DELETE** /v1/agent/runs/{run_id}/tabs/{tab_id} | Close a tab
*AgentApi* | [**compileAgentProcedure**](docs/AgentApi.md#compileagentprocedure) | **POST** /v1/agent/runs/{run_id}/procedure | Compile verified receipts into a replay candidate
*AgentApi* | [**connectAgentLiveFrameSocket**](docs/AgentApi.md#connectagentliveframesocket) | **GET** /v1/agent/runs/{run_id}/frames/ws | Live frames and browser actions over a WebSocket (the GET answers 101)
*AgentApi* | [**getAgentBrowserSession**](docs/AgentApi.md#getagentbrowsersession) | **GET** /v1/agent/runs/{run_id}/browser-session | The owner browser-session projection of a run
*AgentApi* | [**getAgentLiveFrame**](docs/AgentApi.md#getagentliveframe) | **GET** /v1/agent/runs/{run_id}/frame | One transient live frame of the run browser
*AgentApi* | [**getAgentLiveView**](docs/AgentApi.md#getagentliveview) | **GET** /v1/agent/runs/{run_id}/live-view | The run live-view reference, read once; the App Shell asks again when it expires
*AgentApi* | [**handOffAgentRunDownload**](docs/AgentApi.md#handoffagentrundownload) | **POST** /v1/agent/runs/{run_id}/downloads/{artifact_id}/knowledge | Hand one admitted download of a run to Data Plane as a document
*AgentApi* | [**impactCheckAgentProcedure**](docs/AgentApi.md#impactcheckagentprocedure) | **POST** /v1/agent/procedures/impact-check | Identify changed sources that require procedure quarantine
*AgentApi* | [**listAgentBrowserTimeline**](docs/AgentApi.md#listagentbrowsertimeline) | **GET** /v1/agent/runs/{run_id}/browser-session/timeline | The owner browser timeline of a run, oldest first, one page at a time
*AgentApi* | [**listAgentDevtoolsEvents**](docs/AgentApi.md#listagentdevtoolsevents) | **GET** /v1/agent/runs/{run_id}/devtools | Read a filtered window of transient browser DevTools events
*AgentApi* | [**listAgentEgressReceipts**](docs/AgentApi.md#listagentegressreceipts) | **GET** /v1/agent/runs/{run_id}/egress-receipts | Read redacted live browser egress decisions
*AgentApi* | [**listAgentReceipts**](docs/AgentApi.md#listagentreceipts) | **GET** /v1/agent/runs/{run_id}/receipts | Read tenant-scoped immutable action receipts
*AgentApi* | [**listAgentRunDownloads**](docs/AgentApi.md#listagentrundownloads) | **GET** /v1/agent/runs/{run_id}/downloads | List the admitted downloads of a run
*AgentApi* | [**listAgentTabs**](docs/AgentApi.md#listagenttabs) | **GET** /v1/agent/runs/{run_id}/tabs | List the live browser tabs of a run
*AgentApi* | [**markAgentTab**](docs/AgentApi.md#markagenttaboperation) | **POST** /v1/agent/runs/{run_id}/tabs/{tab_id}/mark | Nominate the delegate\&#39;s attached tab for handoff (dark; the delegate only)
*AgentApi* | [**openAgentTab**](docs/AgentApi.md#openagenttaboperation) | **POST** /v1/agent/runs/{run_id}/tabs | Open a browser tab on a run
*AgentApi* | [**qualityCheckAgentProcedure**](docs/AgentApi.md#qualitycheckagentprocedure) | **POST** /v1/agent/procedures/quality-check | Evaluate deterministic procedure promotion evidence
*AgentApi* | [**releaseAgentAttach**](docs/AgentApi.md#releaseagentattach) | **DELETE** /v1/agent/runs/{run_id}/attach/{attach_id} | End an agent attachment (the delegate ends its own; the owner stops it)
*AgentApi* | [**selectAgentTab**](docs/AgentApi.md#selectagenttab) | **POST** /v1/agent/runs/{run_id}/tabs/{tab_id}/select | Make a tab the active one
*AgentApi* | [**setAgentBrowserSessionControl**](docs/AgentApi.md#setagentbrowsersessioncontrol) | **POST** /v1/agent/runs/{run_id}/browser-session/control | Hand the run browser to the person, or back to the agent
*AgentApi* | [**setAgentTabHolder**](docs/AgentApi.md#setagenttabholderoperation) | **POST** /v1/agent/runs/{run_id}/tabs/{tab_id}/control | Hand a tab to the agent or take it back (the owner\&#39;s own token only)
*AgentApi* | [**startAgentRun**](docs/AgentApi.md#startagentrunoperation) | **POST** /v1/agent/runs | Acquire a governed browser-agent run
*AgentApi* | [**streamAgentLiveFrames**](docs/AgentApi.md#streamagentliveframes) | **GET** /v1/agent/runs/{run_id}/frames/stream | Transient live frames of the run browser as Server-Sent Events
*AgentApi* | [**streamAgentRunEvents**](docs/AgentApi.md#streamagentrunevents) | **GET** /v1/agent/runs/{run_id}/events/stream | Typed agent-run events as Server-Sent Events, with resume (WS10.S3)
*AudioApi* | [**audio**](docs/AudioApi.md#audiooperation) | **POST** /v1/audio | Speech synthesis or transcription through the Model Plane
*BatchApi* | [**batchHandoff**](docs/BatchApi.md#batchhandoff) | **POST** /v1/batch | Start a batch scrape job
*BatchApi* | [**getBatchResults**](docs/BatchApi.md#getbatchresults) | **GET** /v1/batch/{job_id}/results | The pages a batch fetched, with their artifact ids (INNH-12)
*ChangeTrackingApi* | [**changeCheck**](docs/ChangeTrackingApi.md#changecheckoperation) | **POST** /v1/change/check | Re-fetch a URL and compare against its baseline
*ChangeTrackingApi* | [**changeDiff**](docs/ChangeTrackingApi.md#changediff) | **GET** /v1/change/diff | The two versions of a tracked page and the paragraph diff between them (INNH-4)
*ChangeTrackingApi* | [**changeHistory**](docs/ChangeTrackingApi.md#changehistory) | **GET** /v1/change/history | Ordered baseline chain for a tracked URL
*ChangeTrackingApi* | [**changeLatest**](docs/ChangeTrackingApi.md#changelatest) | **GET** /v1/change/latest | Most recent baseline for a tracked URL
*ChangeTrackingApi* | [**promoteTrackedResultToSnapshot**](docs/ChangeTrackingApi.md#promotetrackedresulttosnapshot) | **POST** /v1/change/snapshot | Promote the latest baseline to the public Snapshot shape
*ChangeTrackingApi* | [**scheduleRefreshRun**](docs/ChangeTrackingApi.md#schedulerefreshrunoperation) | **POST** /v1/change/refresh | Enqueue an immediate re-check onto the durable frontier
*CrawlApi* | [**crawlHandoff**](docs/CrawlApi.md#crawlhandoff) | **POST** /v1/crawl | Start a crawl job
*CrawlApi* | [**getCrawlResults**](docs/CrawlApi.md#getcrawlresults) | **GET** /v1/crawl/{job_id}/results | The pages a crawl fetched, with their artifact ids (INNH-12)
*DefaultApi* | [**cancelBatch**](docs/DefaultApi.md#cancelbatch) | **DELETE** /v1/batch/{job_id} | Cancel a batch (WS15.L2)
*DefaultApi* | [**cancelCrawl**](docs/DefaultApi.md#cancelcrawl) | **DELETE** /v1/crawl/{job_id} | Cancel a crawl (WS15.L2)
*DefaultApi* | [**clearProfileData**](docs/DefaultApi.md#clearprofiledataoperation) | **POST** /v1/profiles/{profile_id}/clear-data | Remove a stored profile\&#39;s cookies, web storage and IndexedDB (F-t2-068). Off unless QUARRY_EDGE__PROFILE_CLEAR_DATA is on.
*DefaultApi* | [**createProfile**](docs/DefaultApi.md#createprofileoperation) | **POST** /v1/profiles/create | Create a named, scoped profile owned by the caller (WS17.P0 guard)
*DefaultApi* | [**deleteProfile**](docs/DefaultApi.md#deleteprofile) | **DELETE** /v1/profiles/{profile_id} | Erase a profile. Never refused on the ZDR side (wind-down).
*DefaultApi* | [**getBatchErrors**](docs/DefaultApi.md#getbatcherrors) | **GET** /v1/batch/{job_id}/errors | Final page failures and blocked pages of a batch, paged by event seq (WS15.L1)
*DefaultApi* | [**getBatchStatus**](docs/DefaultApi.md#getbatchstatus) | **GET** /v1/batch/{job_id} | Status document of a batch (WS15.L1)
*DefaultApi* | [**getCrawlErrors**](docs/DefaultApi.md#getcrawlerrors) | **GET** /v1/crawl/{job_id}/errors | Final page failures and blocked pages of a crawl, paged by event seq (WS15.L1)
*DefaultApi* | [**getCrawlStatus**](docs/DefaultApi.md#getcrawlstatus) | **GET** /v1/crawl/{job_id} | Status document of a crawl (WS15.L1)
*DefaultApi* | [**getJobErrors**](docs/DefaultApi.md#getjoberrors) | **GET** /v1/jobs/{job_id}/errors | Final page failures and blocked pages of any durable job (WS15.L1)
*DefaultApi* | [**getJobStatus**](docs/DefaultApi.md#getjobstatus) | **GET** /v1/jobs/{job_id} | Status document of a durable job (WS15.L1)
*DefaultApi* | [**getProfile**](docs/DefaultApi.md#getprofile) | **GET** /v1/profiles/{profile_id} | Redacted view of one profile (WS17.P0). No cookie value, no storage value, no IndexedDB value.
*DefaultApi* | [**listOngoingBatches**](docs/DefaultApi.md#listongoingbatches) | **GET** /v1/batch/ongoing | Batches still in flight for the organization, oldest first (WS15.L1)
*DefaultApi* | [**listOngoingCrawls**](docs/DefaultApi.md#listongoingcrawls) | **GET** /v1/crawl/ongoing | Crawls still in flight for the organization, oldest first (WS15.L1)
*DefaultApi* | [**listProfiles**](docs/DefaultApi.md#listprofiles) | **GET** /v1/profiles | Profiles the caller may see (WS17.P0 principal and ZDR guard; WS17.P2 owner filter)
*DefaultApi* | [**pauseBatch**](docs/DefaultApi.md#pausebatch) | **POST** /v1/batch/{job_id}/pause | Pause a running batch (WS15.L2)
*DefaultApi* | [**pauseCrawl**](docs/DefaultApi.md#pausecrawl) | **POST** /v1/crawl/{job_id}/pause | Pause a running crawl (WS15.L2)
*DefaultApi* | [**restoreProbeProfile**](docs/DefaultApi.md#restoreprobeprofile) | **POST** /v1/profiles/{profile_id}/restore_probe | Whether a profile can be restored at this instant. No browser is started (WS17.P0).
*DefaultApi* | [**resumeBatch**](docs/DefaultApi.md#resumebatch) | **POST** /v1/batch/{job_id}/resume | Resume a paused batch (WS15.L2)
*DefaultApi* | [**resumeCrawl**](docs/DefaultApi.md#resumecrawl) | **POST** /v1/crawl/{job_id}/resume | Resume a paused crawl (WS15.L2)
*DefaultApi* | [**saveProfile**](docs/DefaultApi.md#saveprofileoperation) | **POST** /v1/profiles | Save a profile snapshot (the import write path). Needs the quarry:profiles:import scope (WS17.P0).
*DefaultApi* | [**updateProfile**](docs/DefaultApi.md#updateprofileoperation) | **PATCH** /v1/profiles/{profile_id} | Rename and/or rescope a profile. Owner only on an org-shared row (WS17.P2).
*ExtractApi* | [**extract**](docs/ExtractApi.md#extractoperation) | **POST** /v1/extract | Multi-URL structured extraction
*MapApi* | [**map**](docs/MapApi.md#mapoperation) | **POST** /v1/map | Whole-site URL discovery
*OpsApi* | [**health**](docs/OpsApi.md#health) | **GET** /health | Liveness probe
*OpsApi* | [**ready**](docs/OpsApi.md#ready) | **GET** /ready | Readiness probe
*ParseApi* | [**parseDocument**](docs/ParseApi.md#parsedocument) | **POST** /v1/parse | Parse an uploaded PDF or DOCX to text and store nothing (INNH-11)
*ResourcesApi* | [**createSource**](docs/ResourcesApi.md#createsourceoperation) | **POST** /v1/sources | Register an ingestion source
*ResourcesApi* | [**deleteSource**](docs/ResourcesApi.md#deletesource) | **DELETE** /v1/sources/{source_id} | Soft-delete an ingestion source
*ResourcesApi* | [**getArtifact**](docs/ResourcesApi.md#getartifact) | **GET** /v1/artifacts/{artifact_id} | Fetch artifact bytes or metadata
*ResourcesApi* | [**getJobResults**](docs/ResourcesApi.md#getjobresults) | **GET** /v1/jobs/{job_id}/results | The pages any durable job fetched, with their artifact ids (INNH-12)
*ResourcesApi* | [**getSource**](docs/ResourcesApi.md#getsource) | **GET** /v1/sources/{source_id} | Read one ingestion source
*ResourcesApi* | [**listArtifacts**](docs/ResourcesApi.md#listartifacts) | **GET** /v1/artifacts | List stored artifacts (served locally by edge)
*ResourcesApi* | [**listBatchEvents**](docs/ResourcesApi.md#listbatchevents) | **GET** /v1/batch/{job_id}/events | Job events of a batch job (same contract as /v1/jobs/{job_id}/events)
*ResourcesApi* | [**listBenchmarks**](docs/ResourcesApi.md#listbenchmarks) | **GET** /v1/benchmarks | List benchmark runs (placeholder)
*ResourcesApi* | [**listCrawlEvents**](docs/ResourcesApi.md#listcrawlevents) | **GET** /v1/crawl/{job_id}/events | Job events of a crawl job (same contract as /v1/jobs/{job_id}/events)
*ResourcesApi* | [**listJobEvents**](docs/ResourcesApi.md#listjobevents) | **GET** /v1/jobs/{job_id}/events | Durable events keyed by handoff job_id, or a live tail with follow&#x3D;true
*ResourcesApi* | [**listJobsByKind**](docs/ResourcesApi.md#listjobsbykind) | **GET** /v1/{kind}/jobs | List jobs of one kind
*ResourcesApi* | [**listRequestQueues**](docs/ResourcesApi.md#listrequestqueues) | **GET** /v1/request-queues | List request queues with live stats
*ResourcesApi* | [**listRunEvents**](docs/ResourcesApi.md#listrunevents) | **GET** /v1/runs/{run_id}/events | Durable event history for one run
*ResourcesApi* | [**listSnapshots**](docs/ResourcesApi.md#listsnapshots) | **GET** /v1/snapshots | List captured page snapshots
*ResourcesApi* | [**listSources**](docs/ResourcesApi.md#listsources) | **GET** /v1/sources | List ingestion sources
*ResourcesApi* | [**putSourceDefinition**](docs/ResourcesApi.md#putsourcedefinition) | **PUT** /v1/sources/{source_id}/definition | Replace or clear a source\&#39;s saved job definition
*ResourcesApi* | [**updateSourceTag**](docs/ResourcesApi.md#updatesourcetag) | **PATCH** /v1/sources/{source_id} | Set or clear a source\&#39;s monitor tag
*SchedulesApi* | [**backfillSchedule**](docs/SchedulesApi.md#backfillscheduleoperation) | **POST** /v1/schedules/{schedule_id}/backfill | Backfill missed runs
*SchedulesApi* | [**createSchedule**](docs/SchedulesApi.md#createscheduleoperation) | **POST** /v1/schedules | Create a cron / one-shot / change-monitor schedule
*SchedulesApi* | [**deleteSchedule**](docs/SchedulesApi.md#deleteschedule) | **DELETE** /v1/schedules/{schedule_id} | Delete a schedule
*SchedulesApi* | [**getScheduleRequest**](docs/SchedulesApi.md#getschedulerequest) | **GET** /v1/schedules/{schedule_id}/requests/{request_id} | Read one trigger or backfill request row
*SchedulesApi* | [**listScheduleRuns**](docs/SchedulesApi.md#listscheduleruns) | **GET** /v1/schedules/{schedule_id}/runs | List the fires of a schedule
*SchedulesApi* | [**listSchedules**](docs/SchedulesApi.md#listschedules) | **GET** /v1/schedules | List schedules
*SchedulesApi* | [**pauseSchedule**](docs/SchedulesApi.md#pauseschedule) | **POST** /v1/schedules/{schedule_id}/pause | Pause a schedule
*SchedulesApi* | [**triggerSchedule**](docs/SchedulesApi.md#triggerschedule) | **POST** /v1/schedules/{schedule_id}/trigger | Trigger an immediate scheduled run
*SchedulesApi* | [**unpauseSchedule**](docs/SchedulesApi.md#unpauseschedule) | **POST** /v1/schedules/{schedule_id}/unpause | Unpause a schedule
*SchedulesApi* | [**updateScheduleTag**](docs/SchedulesApi.md#updatescheduletag) | **PATCH** /v1/schedules/{schedule_id} | Set or clear a schedule\&#39;s monitor tag
*ScrapeApi* | [**auditPage**](docs/ScrapeApi.md#auditpageoperation) | **POST** /v1/audit/page | Bounded public-page evidence for the audit rules
*ScrapeApi* | [**getScrape**](docs/ScrapeApi.md#getscrape) | **GET** /v1/scrape/{id} | Read back an earlier scrape
*ScrapeApi* | [**scrape**](docs/ScrapeApi.md#scrapeoperation) | **POST** /v1/scrape | Scrape a single URL
*ScrapeApi* | [**scrapeStream**](docs/ScrapeApi.md#scrapestream) | **POST** /v1/scrape/stream | Scrape with SSE event stream
*SearchApi* | [**answer**](docs/SearchApi.md#answeroperation) | **POST** /v1/answer | One-call grounded answer with citations
*SearchApi* | [**answerStream**](docs/SearchApi.md#answerstream) | **POST** /v1/answer/stream | Server-sent events variant of /v1/answer
*SearchApi* | [**search**](docs/SearchApi.md#searchoperation) | **POST** /v1/search | SERP + own-corpus search (hybrid when Data Plane wired)
*SearchApi* | [**searchImages**](docs/SearchApi.md#searchimages) | **POST** /v1/search/images | Image search through the SearXNG image vertical
*SearchApi* | [**searchSimilar**](docs/SearchApi.md#searchsimilar) | **POST** /v1/search/similar | Semantic neighbours of a passage, a page or both
*SearchApi* | [**searchSuggest**](docs/SearchApi.md#searchsuggest) | **POST** /v1/search/suggest | Spelling correction, related searches and a panel for one query
*SearchApi* | [**searchVideos**](docs/SearchApi.md#searchvideos) | **POST** /v1/search/videos | Video search through the SearXNG video vertical
*TeamApi* | [**teamActivity**](docs/TeamApi.md#teamactivity) | **GET** /v1/team/activity | Recent activity feed
*TeamApi* | [**teamConcurrency**](docs/TeamApi.md#teamconcurrency) | **GET** /v1/team/concurrency | Per-org concurrency snapshot
*TeamApi* | [**teamCreditUsage**](docs/TeamApi.md#teamcreditusage) | **GET** /v1/team/credit-usage | Team credit usage for a window
*TeamApi* | [**teamQueueStatus**](docs/TeamApi.md#teamqueuestatus) | **GET** /v1/team/queue-status | Aggregate queue snapshot for the org
*TeamApi* | [**teamTokenUsage**](docs/TeamApi.md#teamtokenusage) | **GET** /v1/team/token-usage | Team token usage for a window
*WebhooksApi* | [**createWebhookSubscription**](docs/WebhooksApi.md#createwebhooksubscription) | **POST** /v1/webhook-subscriptions | Subscribe an HTTPS endpoint to change events (INNH-5)
*WebhooksApi* | [**deleteWebhookSubscription**](docs/WebhooksApi.md#deletewebhooksubscription) | **DELETE** /v1/webhook-subscriptions/{subscription_id} | Delete a subscription and its delivery log (INNH-5)
*WebhooksApi* | [**getWebhookSubscription**](docs/WebhooksApi.md#getwebhooksubscription) | **GET** /v1/webhook-subscriptions/{subscription_id} | Read one webhook subscription (INNH-5)
*WebhooksApi* | [**listWebhookDeliveries**](docs/WebhooksApi.md#listwebhookdeliveries) | **GET** /v1/webhook-subscriptions/{subscription_id}/deliveries | The delivery log of a subscription (INNH-5)
*WebhooksApi* | [**listWebhookSubscriptions**](docs/WebhooksApi.md#listwebhooksubscriptions) | **GET** /v1/webhook-subscriptions | List the organization\&#39;s webhook subscriptions (INNH-5)
*WebhooksApi* | [**retryWebhookDelivery**](docs/WebhooksApi.md#retrywebhookdelivery) | **POST** /v1/webhook-subscriptions/{subscription_id}/deliveries/{delivery_id}/retry | Re-queue a failed delivery (INNH-5)
*WebhooksApi* | [**rotateWebhookSubscriptionSecret**](docs/WebhooksApi.md#rotatewebhooksubscriptionsecret) | **POST** /v1/webhook-subscriptions/{subscription_id}/rotate-secret | Mint a new secret for a subscription (INNH-5)
*WebhooksApi* | [**updateWebhookSubscription**](docs/WebhooksApi.md#updatewebhooksubscription) | **PATCH** /v1/webhook-subscriptions/{subscription_id} | Pause, resume or change the events of a subscription (INNH-5)


### Models

- [AccessibilityNode](docs/AccessibilityNode.md)
- [AccessibilityProjection](docs/AccessibilityProjection.md)
- [ActionOutcome](docs/ActionOutcome.md)
- [AgentAction](docs/AgentAction.md)
- [AgentConstraints](docs/AgentConstraints.md)
- [AgentDownloadKnowledgeRequest](docs/AgentDownloadKnowledgeRequest.md)
- [AgentHitTest200Response](docs/AgentHitTest200Response.md)
- [AgentIdentity](docs/AgentIdentity.md)
- [AgentRunDownload](docs/AgentRunDownload.md)
- [AgentStepRequest](docs/AgentStepRequest.md)
- [AgentTab](docs/AgentTab.md)
- [AiFormatReport](docs/AiFormatReport.md)
- [AiFormatReportPrefilledInner](docs/AiFormatReportPrefilledInner.md)
- [AiFormatReportSchemaIssuesInner](docs/AiFormatReportSchemaIssuesInner.md)
- [AiFormatReportUsage](docs/AiFormatReportUsage.md)
- [AiReport](docs/AiReport.md)
- [AiReportTotal](docs/AiReportTotal.md)
- [AnswerCitation](docs/AnswerCitation.md)
- [AnswerErrorBody](docs/AnswerErrorBody.md)
- [AnswerRequest](docs/AnswerRequest.md)
- [AnswerResponse](docs/AnswerResponse.md)
- [ApplyAgentOriginExtension200Response](docs/ApplyAgentOriginExtension200Response.md)
- [ApplyAgentOriginExtension200ResponseData](docs/ApplyAgentOriginExtension200ResponseData.md)
- [ApplyAgentOriginExtensionRequest](docs/ApplyAgentOriginExtensionRequest.md)
- [ArtifactSummary](docs/ArtifactSummary.md)
- [AttachAgentCurrentTab200Response](docs/AttachAgentCurrentTab200Response.md)
- [AttachAgentCurrentTab200ResponseData](docs/AttachAgentCurrentTab200ResponseData.md)
- [AttachAgentCurrentTabRequest](docs/AttachAgentCurrentTabRequest.md)
- [AttachAgentCurrentTabRequestAttachTab](docs/AttachAgentCurrentTabRequestAttachTab.md)
- [AttachAgentRun200Response](docs/AttachAgentRun200Response.md)
- [AttachAgentRun200ResponseData](docs/AttachAgentRun200ResponseData.md)
- [AttachAgentRunRequest](docs/AttachAgentRunRequest.md)
- [AttachAgentRunRequestConstraints](docs/AttachAgentRunRequestConstraints.md)
- [AudioPolicyRefusal](docs/AudioPolicyRefusal.md)
- [AudioRequest](docs/AudioRequest.md)
- [AudioSpeechRequest](docs/AudioSpeechRequest.md)
- [AudioTranscriptionRequest](docs/AudioTranscriptionRequest.md)
- [AudioUsageError](docs/AudioUsageError.md)
- [AuditPage](docs/AuditPage.md)
- [AuditPageAlternate](docs/AuditPageAlternate.md)
- [AuditPageHeading](docs/AuditPageHeading.md)
- [AuditPageLimits](docs/AuditPageLimits.md)
- [AuditPageLink](docs/AuditPageLink.md)
- [AuditPageMetadata](docs/AuditPageMetadata.md)
- [AuditPageRedirect](docs/AuditPageRedirect.md)
- [AuditPageRequest](docs/AuditPageRequest.md)
- [AuditPageSeo](docs/AuditPageSeo.md)
- [AuditPageStructuredData](docs/AuditPageStructuredData.md)
- [BackAction](docs/BackAction.md)
- [BackfillScheduleRequest](docs/BackfillScheduleRequest.md)
- [BaselineSnapshot](docs/BaselineSnapshot.md)
- [BatchRequest](docs/BatchRequest.md)
- [BatchUrlsBound](docs/BatchUrlsBound.md)
- [BenchmarkSummary](docs/BenchmarkSummary.md)
- [BrowserDialog](docs/BrowserDialog.md)
- [BrowserDriverCapabilities](docs/BrowserDriverCapabilities.md)
- [BrowserEgressReceipt](docs/BrowserEgressReceipt.md)
- [BrowserFrame](docs/BrowserFrame.md)
- [BrowserObservation](docs/BrowserObservation.md)
- [BrowserProcedure](docs/BrowserProcedure.md)
- [BrowserProcedureStepsInner](docs/BrowserProcedureStepsInner.md)
- [BrowserRedirectHop200Response](docs/BrowserRedirectHop200Response.md)
- [BrowserRedirectHopRequest](docs/BrowserRedirectHopRequest.md)
- [BrowserSessionAttach](docs/BrowserSessionAttach.md)
- [BrowserSessionControlRequest](docs/BrowserSessionControlRequest.md)
- [BrowserSessionProjectionData](docs/BrowserSessionProjectionData.md)
- [BrowserSessionProjectionDataViewport](docs/BrowserSessionProjectionDataViewport.md)
- [BrowserSessionTab](docs/BrowserSessionTab.md)
- [BrowserSnapshot](docs/BrowserSnapshot.md)
- [BrowserTelemetry](docs/BrowserTelemetry.md)
- [BrowserTimelineData](docs/BrowserTimelineData.md)
- [BrowserTimelineItem](docs/BrowserTimelineItem.md)
- [BudgetMeta](docs/BudgetMeta.md)
- [CachePolicy](docs/CachePolicy.md)
- [ChallengeSignal](docs/ChallengeSignal.md)
- [ChangeCheck200Response](docs/ChangeCheck200Response.md)
- [ChangeCheckRequest](docs/ChangeCheckRequest.md)
- [ChangeDiff200Response](docs/ChangeDiff200Response.md)
- [ChangeDiffMarkdown](docs/ChangeDiffMarkdown.md)
- [ChangeDiffSide](docs/ChangeDiffSide.md)
- [ChangeDiffUnavailable](docs/ChangeDiffUnavailable.md)
- [ChangeDiffView](docs/ChangeDiffView.md)
- [ChangeHistory200Response](docs/ChangeHistory200Response.md)
- [ChangeInfo](docs/ChangeInfo.md)
- [ChangeLatest200Response](docs/ChangeLatest200Response.md)
- [ChangeRecord](docs/ChangeRecord.md)
- [ClearProfileData200Response](docs/ClearProfileData200Response.md)
- [ClearProfileData200ResponseCounts](docs/ClearProfileData200ResponseCounts.md)
- [ClearProfileDataRequest](docs/ClearProfileDataRequest.md)
- [ClickAction](docs/ClickAction.md)
- [ClickPointAction](docs/ClickPointAction.md)
- [ClickRefAction](docs/ClickRefAction.md)
- [ClickSemanticAction](docs/ClickSemanticAction.md)
- [CompileProcedureRequest](docs/CompileProcedureRequest.md)
- [CrawlBound](docs/CrawlBound.md)
- [CrawlRequest](docs/CrawlRequest.md)
- [CrawlRequestScrapeOptions](docs/CrawlRequestScrapeOptions.md)
- [CreateProfileRequest](docs/CreateProfileRequest.md)
- [CreateSchedule200Response](docs/CreateSchedule200Response.md)
- [CreateScheduleRequest](docs/CreateScheduleRequest.md)
- [CreateSource200Response](docs/CreateSource200Response.md)
- [CreateSourceRequest](docs/CreateSourceRequest.md)
- [CreateSourceRequestConfig](docs/CreateSourceRequestConfig.md)
- [CreateWebhookSubscription](docs/CreateWebhookSubscription.md)
- [CreateWebhookSubscription201Response](docs/CreateWebhookSubscription201Response.md)
- [DeleteSchedule200Response](docs/DeleteSchedule200Response.md)
- [DeleteSource200Response](docs/DeleteSource200Response.md)
- [DialogResponseAction](docs/DialogResponseAction.md)
- [DownloadRefAction](docs/DownloadRefAction.md)
- [DriverInfo](docs/DriverInfo.md)
- [DriverSignals](docs/DriverSignals.md)
- [ElementFingerprint](docs/ElementFingerprint.md)
- [EnvelopeAgentDownloadKnowledge](docs/EnvelopeAgentDownloadKnowledge.md)
- [EnvelopeAgentDownloadKnowledgeData](docs/EnvelopeAgentDownloadKnowledgeData.md)
- [EnvelopeAgentRunDownloads](docs/EnvelopeAgentRunDownloads.md)
- [EnvelopeAgentTab](docs/EnvelopeAgentTab.md)
- [EnvelopeAgentTabData](docs/EnvelopeAgentTabData.md)
- [EnvelopeAgentTabs](docs/EnvelopeAgentTabs.md)
- [EnvelopeAgentTabsData](docs/EnvelopeAgentTabsData.md)
- [EnvelopeBrowserObservation](docs/EnvelopeBrowserObservation.md)
- [EnvelopeBrowserObservationMeta](docs/EnvelopeBrowserObservationMeta.md)
- [EnvelopeBrowserProcedure](docs/EnvelopeBrowserProcedure.md)
- [EnvelopeClosedRun](docs/EnvelopeClosedRun.md)
- [EnvelopeClosedRunData](docs/EnvelopeClosedRunData.md)
- [EnvelopeClosedRunDataUsage](docs/EnvelopeClosedRunDataUsage.md)
- [EnvelopeEgressReceipts](docs/EnvelopeEgressReceipts.md)
- [EnvelopeEgressReceiptsData](docs/EnvelopeEgressReceiptsData.md)
- [EnvelopeError](docs/EnvelopeError.md)
- [EnvelopeErrorError](docs/EnvelopeErrorError.md)
- [EnvelopeHandoffAck](docs/EnvelopeHandoffAck.md)
- [EnvelopeNormalizedOutput](docs/EnvelopeNormalizedOutput.md)
- [EnvelopeProcedureImpact](docs/EnvelopeProcedureImpact.md)
- [EnvelopeProcedureQuality](docs/EnvelopeProcedureQuality.md)
- [EnvelopeReplayDecision](docs/EnvelopeReplayDecision.md)
- [EnvelopeScrapeResponse](docs/EnvelopeScrapeResponse.md)
- [EnvelopeStartAgentRun](docs/EnvelopeStartAgentRun.md)
- [EnvelopeStartAgentRunData](docs/EnvelopeStartAgentRunData.md)
- [EnvelopeStartAgentRunDataClampedInner](docs/EnvelopeStartAgentRunDataClampedInner.md)
- [EnvelopeStartAgentRunDataConstraints](docs/EnvelopeStartAgentRunDataConstraints.md)
- [EnvelopeStepReceipts](docs/EnvelopeStepReceipts.md)
- [ErrorBody](docs/ErrorBody.md)
- [ErrorDetail](docs/ErrorDetail.md)
- [EvaluateAction](docs/EvaluateAction.md)
- [EvidenceDelta](docs/EvidenceDelta.md)
- [EvidenceDeltaDom](docs/EvidenceDeltaDom.md)
- [EvidenceDeltaNetwork](docs/EvidenceDeltaNetwork.md)
- [ExpectCondition](docs/ExpectCondition.md)
- [ExpectLocator](docs/ExpectLocator.md)
- [ExtractItem](docs/ExtractItem.md)
- [ExtractRef](docs/ExtractRef.md)
- [ExtractRequest](docs/ExtractRequest.md)
- [ExtractResponse](docs/ExtractResponse.md)
- [FacetCount](docs/FacetCount.md)
- [FillOtpRefAction](docs/FillOtpRefAction.md)
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
- [GetAgentBrowserSession200Response](docs/GetAgentBrowserSession200Response.md)
- [GetAgentLiveFrame200Response](docs/GetAgentLiveFrame200Response.md)
- [GetAgentLiveView200Response](docs/GetAgentLiveView200Response.md)
- [GetArtifact200Response](docs/GetArtifact200Response.md)
- [GetContentAction](docs/GetContentAction.md)
- [GetCrawlResults200Response](docs/GetCrawlResults200Response.md)
- [GetJobStatus200Response](docs/GetJobStatus200Response.md)
- [GetScheduleRequest200Response](docs/GetScheduleRequest200Response.md)
- [GetSource200Response](docs/GetSource200Response.md)
- [GetWebhookSubscription200Response](docs/GetWebhookSubscription200Response.md)
- [HandoffAck](docs/HandoffAck.md)
- [HandoffAckEffective](docs/HandoffAckEffective.md)
- [HandoffAckEffectiveRequestWarningsInner](docs/HandoffAckEffectiveRequestWarningsInner.md)
- [HandoffAckLimits](docs/HandoffAckLimits.md)
- [HandoffAckRejectedInner](docs/HandoffAckRejectedInner.md)
- [Highlight](docs/Highlight.md)
- [HitTestData](docs/HitTestData.md)
- [HostConcurrency](docs/HostConcurrency.md)
- [ImageResult](docs/ImageResult.md)
- [ImageSearchRequest](docs/ImageSearchRequest.md)
- [ImageSearchResponse](docs/ImageSearchResponse.md)
- [JobCredits](docs/JobCredits.md)
- [JobHistoryEvent](docs/JobHistoryEvent.md)
- [JobResult](docs/JobResult.md)
- [JobResultsView](docs/JobResultsView.md)
- [JobRunOrigin](docs/JobRunOrigin.md)
- [JobStage](docs/JobStage.md)
- [JobStatusView](docs/JobStatusView.md)
- [JobStatusViewControl](docs/JobStatusViewControl.md)
- [JobStatusViewCounts](docs/JobStatusViewCounts.md)
- [JobStatusViewWarningsInner](docs/JobStatusViewWarningsInner.md)
- [JobSummary](docs/JobSummary.md)
- [JobWebhook](docs/JobWebhook.md)
- [LabelLocator](docs/LabelLocator.md)
- [LegacyAgentAction](docs/LegacyAgentAction.md)
- [Link](docs/Link.md)
- [ListAgentBrowserTimeline200Response](docs/ListAgentBrowserTimeline200Response.md)
- [ListAgentDevtoolsEvents200Response](docs/ListAgentDevtoolsEvents200Response.md)
- [ListAgentDevtoolsEvents200ResponseData](docs/ListAgentDevtoolsEvents200ResponseData.md)
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
- [ListScheduleRuns200Response](docs/ListScheduleRuns200Response.md)
- [ListScheduleRuns200ResponseData](docs/ListScheduleRuns200ResponseData.md)
- [ListSchedules200Response](docs/ListSchedules200Response.md)
- [ListSchedules200ResponseData](docs/ListSchedules200ResponseData.md)
- [ListSnapshots200Response](docs/ListSnapshots200Response.md)
- [ListSnapshots200ResponseData](docs/ListSnapshots200ResponseData.md)
- [ListSources200Response](docs/ListSources200Response.md)
- [ListSources200ResponseData](docs/ListSources200ResponseData.md)
- [ListWebhookDeliveries200Response](docs/ListWebhookDeliveries200Response.md)
- [ListWebhookSubscriptions200Response](docs/ListWebhookSubscriptions200Response.md)
- [LiveAgentCursorData](docs/LiveAgentCursorData.md)
- [LiveFrameData](docs/LiveFrameData.md)
- [LiveFrameStreamData](docs/LiveFrameStreamData.md)
- [LiveViewRef](docs/LiveViewRef.md)
- [MapLink](docs/MapLink.md)
- [MapRequest](docs/MapRequest.md)
- [MapResponse](docs/MapResponse.md)
- [MarkAgentTab200Response](docs/MarkAgentTab200Response.md)
- [MarkAgentTab200ResponseData](docs/MarkAgentTab200ResponseData.md)
- [MarkAgentTabRequest](docs/MarkAgentTabRequest.md)
- [MouseWheelAction](docs/MouseWheelAction.md)
- [NavigateAction](docs/NavigateAction.md)
- [NormalizedOutput](docs/NormalizedOutput.md)
- [NotifyOnChangeFilter](docs/NotifyOnChangeFilter.md)
- [NthLocator](docs/NthLocator.md)
- [ObservationDelta](docs/ObservationDelta.md)
- [ObservationWarning](docs/ObservationWarning.md)
- [OpenAgentTabRequest](docs/OpenAgentTabRequest.md)
- [OutputFormats](docs/OutputFormats.md)
- [OverlapPolicy](docs/OverlapPolicy.md)
- [PageMetadata](docs/PageMetadata.md)
- [ParagraphDiff](docs/ParagraphDiff.md)
- [ParseData](docs/ParseData.md)
- [ParseError](docs/ParseError.md)
- [ParseErrorError](docs/ParseErrorError.md)
- [ParseErrorErrorDetails](docs/ParseErrorErrorDetails.md)
- [ParseErrorMeta](docs/ParseErrorMeta.md)
- [ParseResponse](docs/ParseResponse.md)
- [ParseResponseMeta](docs/ParseResponseMeta.md)
- [ParseWarning](docs/ParseWarning.md)
- [PdfAction](docs/PdfAction.md)
- [PlaceholderLocator](docs/PlaceholderLocator.md)
- [PressAction](docs/PressAction.md)
- [ProcedureImpactRequest](docs/ProcedureImpactRequest.md)
- [ProcedureSlot](docs/ProcedureSlot.md)
- [ProcessingContinuesEnvelope](docs/ProcessingContinuesEnvelope.md)
- [ProcessingContinuesEnvelopeError](docs/ProcessingContinuesEnvelopeError.md)
- [ProcessingContinuesEnvelopeErrorDetails](docs/ProcessingContinuesEnvelopeErrorDetails.md)
- [ProcessingContinuesEnvelopeErrorDetailsCounts](docs/ProcessingContinuesEnvelopeErrorDetailsCounts.md)
- [ProfileCookieView](docs/ProfileCookieView.md)
- [ProfileDetail](docs/ProfileDetail.md)
- [ProfileList](docs/ProfileList.md)
- [ProfileOriginView](docs/ProfileOriginView.md)
- [ProfileRestoreProbe](docs/ProfileRestoreProbe.md)
- [ProfileRestoreSummary](docs/ProfileRestoreSummary.md)
- [ProfileSummary](docs/ProfileSummary.md)
- [ProfileView](docs/ProfileView.md)
- [PromoteTrackedResultToSnapshot200Response](docs/PromoteTrackedResultToSnapshot200Response.md)
- [QueueStatusEntry](docs/QueueStatusEntry.md)
- [RateLimitError](docs/RateLimitError.md)
- [Ready200Response](docs/Ready200Response.md)
- [Ready200ResponseBrowser](docs/Ready200ResponseBrowser.md)
- [Ready200ResponseBrowserSlotsInner](docs/Ready200ResponseBrowserSlotsInner.md)
- [Ready200ResponseScrapeBrowser](docs/Ready200ResponseScrapeBrowser.md)
- [Ready503Response](docs/Ready503Response.md)
- [Ready503ResponseBrowser](docs/Ready503ResponseBrowser.md)
- [Ready503ResponseBrowserSlotsInner](docs/Ready503ResponseBrowserSlotsInner.md)
- [RedactionStamp](docs/RedactionStamp.md)
- [RedirectProbeResult](docs/RedirectProbeResult.md)
- [ReloadAction](docs/ReloadAction.md)
- [ReplayCheckRequest](docs/ReplayCheckRequest.md)
- [RequestAsk](docs/RequestAsk.md)
- [RequestQueueSummary](docs/RequestQueueSummary.md)
- [RequestQueueSummaryStats](docs/RequestQueueSummaryStats.md)
- [RerankReport](docs/RerankReport.md)
- [ResponseMeta](docs/ResponseMeta.md)
- [ResponseMetaPage](docs/ResponseMetaPage.md)
- [RetryWebhookDelivery202Response](docs/RetryWebhookDelivery202Response.md)
- [RobotsFetchDenial](docs/RobotsFetchDenial.md)
- [RoleLocator](docs/RoleLocator.md)
- [RotateWebhookSubscriptionSecret200Response](docs/RotateWebhookSubscriptionSecret200Response.md)
- [SaveProfile200Response](docs/SaveProfile200Response.md)
- [SaveProfileRequest](docs/SaveProfileRequest.md)
- [SavedJobDefinition](docs/SavedJobDefinition.md)
- [ScheduleRefreshRun200Response](docs/ScheduleRefreshRun200Response.md)
- [ScheduleRefreshRun200ResponseData](docs/ScheduleRefreshRun200ResponseData.md)
- [ScheduleRefreshRunRequest](docs/ScheduleRefreshRunRequest.md)
- [ScheduleRequestAccepted](docs/ScheduleRequestAccepted.md)
- [ScheduleRequestView](docs/ScheduleRequestView.md)
- [ScheduleSummary](docs/ScheduleSummary.md)
- [ScrapeActionOutcome](docs/ScrapeActionOutcome.md)
- [ScrapeActionOutcomeError](docs/ScrapeActionOutcomeError.md)
- [ScrapeActionOutcomeOutput](docs/ScrapeActionOutcomeOutput.md)
- [ScrapeAiFormat](docs/ScrapeAiFormat.md)
- [ScrapeAiFormatOneOf](docs/ScrapeAiFormatOneOf.md)
- [ScrapeAiFormatOneOf1](docs/ScrapeAiFormatOneOf1.md)
- [ScrapeAiFormatOneOf2](docs/ScrapeAiFormatOneOf2.md)
- [ScrapeAiFormatOneOf3](docs/ScrapeAiFormatOneOf3.md)
- [ScrapeRequest](docs/ScrapeRequest.md)
- [ScrapeRequestActionsInner](docs/ScrapeRequestActionsInner.md)
- [ScrapeRequestFormatsInner](docs/ScrapeRequestFormatsInner.md)
- [ScrapeRequestFormatsInnerOneOf](docs/ScrapeRequestFormatsInnerOneOf.md)
- [ScrapeRequestLocation](docs/ScrapeRequestLocation.md)
- [ScrapeRequestRender](docs/ScrapeRequestRender.md)
- [ScrapeResponse](docs/ScrapeResponse.md)
- [ScrapeResponseAllOfWarnings](docs/ScrapeResponseAllOfWarnings.md)
- [ScreenshotAction](docs/ScreenshotAction.md)
- [ScrollAction](docs/ScrollAction.md)
- [SearchRequest](docs/SearchRequest.md)
- [SearchResponse](docs/SearchResponse.md)
- [SearchResultItem](docs/SearchResultItem.md)
- [SearchScrapeBlock](docs/SearchScrapeBlock.md)
- [SearchScrapeItem](docs/SearchScrapeItem.md)
- [SearchScrapeReport](docs/SearchScrapeReport.md)
- [SelectAction](docs/SelectAction.md)
- [SelectRefAction](docs/SelectRefAction.md)
- [SelectSemanticAction](docs/SelectSemanticAction.md)
- [SemanticLocator](docs/SemanticLocator.md)
- [SemanticTargetAction](docs/SemanticTargetAction.md)
- [SetAgentTabHolderRequest](docs/SetAgentTabHolderRequest.md)
- [SimilarRequest](docs/SimilarRequest.md)
- [Snapshot](docs/Snapshot.md)
- [SnapshotRefAction](docs/SnapshotRefAction.md)
- [SnapshotTarget](docs/SnapshotTarget.md)
- [SnapshotTargetBase](docs/SnapshotTargetBase.md)
- [Source](docs/Source.md)
- [SourceCreatedAt](docs/SourceCreatedAt.md)
- [SourceDefinitionRequest](docs/SourceDefinitionRequest.md)
- [StartAgentRunRequest](docs/StartAgentRunRequest.md)
- [StartAgentRunRequestTrace](docs/StartAgentRunRequestTrace.md)
- [StartAgentRunRequestTraceAgent](docs/StartAgentRunRequestTraceAgent.md)
- [StartAgentRunRequestViewport](docs/StartAgentRunRequestViewport.md)
- [StepExpectation](docs/StepExpectation.md)
- [StepReceipt](docs/StepReceipt.md)
- [StepTarget](docs/StepTarget.md)
- [StructuredData](docs/StructuredData.md)
- [StructuredKeyFigure](docs/StructuredKeyFigure.md)
- [SuggestEntityFact](docs/SuggestEntityFact.md)
- [SuggestEntityPanel](docs/SuggestEntityPanel.md)
- [SuggestRequest](docs/SuggestRequest.md)
- [SuggestResponse](docs/SuggestResponse.md)
- [TabOpener](docs/TabOpener.md)
- [TagPatchRequest](docs/TagPatchRequest.md)
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
- [TextDiffOp](docs/TextDiffOp.md)
- [TextLocator](docs/TextLocator.md)
- [TriggerSchedule202Response](docs/TriggerSchedule202Response.md)
- [TypeAction](docs/TypeAction.md)
- [TypeRefAction](docs/TypeRefAction.md)
- [TypeSemanticAction](docs/TypeSemanticAction.md)
- [UpdateProfileRequest](docs/UpdateProfileRequest.md)
- [UpdateWebhookSubscription](docs/UpdateWebhookSubscription.md)
- [UploadRefAction](docs/UploadRefAction.md)
- [UrlTriple](docs/UrlTriple.md)
- [VideoResult](docs/VideoResult.md)
- [VideoSearchRequest](docs/VideoSearchRequest.md)
- [VideoSearchResponse](docs/VideoSearchResponse.md)
- [WaitAction](docs/WaitAction.md)
- [WaitForAction](docs/WaitForAction.md)
- [WaitForRefAction](docs/WaitForRefAction.md)
- [WaitForSemanticAction](docs/WaitForSemanticAction.md)
- [WebhookDeliveryList](docs/WebhookDeliveryList.md)
- [WebhookDeliveryView](docs/WebhookDeliveryView.md)
- [WebhookLastDelivery](docs/WebhookLastDelivery.md)
- [WebhookSecretRotated](docs/WebhookSecretRotated.md)
- [WebhookSubscription](docs/WebhookSubscription.md)
- [WebhookSubscriptionCreated](docs/WebhookSubscriptionCreated.md)
- [WebhookSubscriptionList](docs/WebhookSubscriptionList.md)

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
