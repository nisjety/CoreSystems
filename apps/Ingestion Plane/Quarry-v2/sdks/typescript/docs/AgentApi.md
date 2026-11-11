# AgentApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**agentHitTest**](AgentApi.md#agenthittest) | **GET** /v1/agent/runs/{run_id}/hit-test | Map a viewport point to the observed target under it (dark) |
| [**agentInteract**](AgentApi.md#agentinteract) | **POST** /v1/agent/runs/{run_id}/interact | Ergonomic alias for one governed browser action |
| [**agentStep**](AgentApi.md#agentstepoperation) | **POST** /v1/agent/runs/{run_id}/step | Execute one governed browser action |
| [**applyAgentOriginExtension**](AgentApi.md#applyagentoriginextensionoperation) | **POST** /v1/agent/runs/{run_id}/origin-extension | Apply a person\&#39;s per-origin consent to a live run (dark) |
| [**attachAgentCurrentTab**](AgentApi.md#attachagentcurrenttaboperation) | **POST** /v1/agent/runs/{run_id}/attach-current-tab | Attach an agent to a tab the person already has open (dark; service principal only) |
| [**attachAgentRun**](AgentApi.md#attachagentrunoperation) | **POST** /v1/agent/runs/{run_id}/attach | Attach an agent to a person\&#39;s live run (dark; service principal only) |
| [**browserRedirectHop**](AgentApi.md#browserredirecthopoperation) | **POST** /v1/browser/redirect-hop | One credentialless redirect hop for assistant navigation |
| [**checkAgentProcedureReplay**](AgentApi.md#checkagentprocedurereplay) | **POST** /v1/agent/procedures/replay-check | Compare actions with a procedure without executing effects |
| [**closeAgentRun**](AgentApi.md#closeagentrun) | **DELETE** /v1/agent/runs/{run_id} | Close a browser-agent run and release its lease |
| [**closeAgentTab**](AgentApi.md#closeagenttab) | **DELETE** /v1/agent/runs/{run_id}/tabs/{tab_id} | Close a tab |
| [**compileAgentProcedure**](AgentApi.md#compileagentprocedure) | **POST** /v1/agent/runs/{run_id}/procedure | Compile verified receipts into a replay candidate |
| [**connectAgentLiveFrameSocket**](AgentApi.md#connectagentliveframesocket) | **GET** /v1/agent/runs/{run_id}/frames/ws | Live frames and browser actions over a WebSocket (the GET answers 101) |
| [**getAgentBrowserSession**](AgentApi.md#getagentbrowsersession) | **GET** /v1/agent/runs/{run_id}/browser-session | The owner browser-session projection of a run |
| [**getAgentLiveFrame**](AgentApi.md#getagentliveframe) | **GET** /v1/agent/runs/{run_id}/frame | One transient live frame of the run browser |
| [**getAgentLiveView**](AgentApi.md#getagentliveview) | **GET** /v1/agent/runs/{run_id}/live-view | The run live-view reference, read once; the App Shell asks again when it expires |
| [**handOffAgentRunDownload**](AgentApi.md#handoffagentrundownload) | **POST** /v1/agent/runs/{run_id}/downloads/{artifact_id}/knowledge | Hand one admitted download of a run to Data Plane as a document |
| [**impactCheckAgentProcedure**](AgentApi.md#impactcheckagentprocedure) | **POST** /v1/agent/procedures/impact-check | Identify changed sources that require procedure quarantine |
| [**listAgentBrowserTimeline**](AgentApi.md#listagentbrowsertimeline) | **GET** /v1/agent/runs/{run_id}/browser-session/timeline | The owner browser timeline of a run, oldest first, one page at a time |
| [**listAgentDevtoolsEvents**](AgentApi.md#listagentdevtoolsevents) | **GET** /v1/agent/runs/{run_id}/devtools | Read a filtered window of transient browser DevTools events |
| [**listAgentEgressReceipts**](AgentApi.md#listagentegressreceipts) | **GET** /v1/agent/runs/{run_id}/egress-receipts | Read redacted live browser egress decisions |
| [**listAgentReceipts**](AgentApi.md#listagentreceipts) | **GET** /v1/agent/runs/{run_id}/receipts | Read tenant-scoped immutable action receipts |
| [**listAgentRunDownloads**](AgentApi.md#listagentrundownloads) | **GET** /v1/agent/runs/{run_id}/downloads | List the admitted downloads of a run |
| [**listAgentTabs**](AgentApi.md#listagenttabs) | **GET** /v1/agent/runs/{run_id}/tabs | List the live browser tabs of a run |
| [**markAgentTab**](AgentApi.md#markagenttaboperation) | **POST** /v1/agent/runs/{run_id}/tabs/{tab_id}/mark | Nominate the delegate\&#39;s attached tab for handoff (dark; the delegate only) |
| [**openAgentTab**](AgentApi.md#openagenttaboperation) | **POST** /v1/agent/runs/{run_id}/tabs | Open a browser tab on a run |
| [**qualityCheckAgentProcedure**](AgentApi.md#qualitycheckagentprocedure) | **POST** /v1/agent/procedures/quality-check | Evaluate deterministic procedure promotion evidence |
| [**releaseAgentAttach**](AgentApi.md#releaseagentattach) | **DELETE** /v1/agent/runs/{run_id}/attach/{attach_id} | End an agent attachment (the delegate ends its own; the owner stops it) |
| [**selectAgentTab**](AgentApi.md#selectagenttab) | **POST** /v1/agent/runs/{run_id}/tabs/{tab_id}/select | Make a tab the active one |
| [**setAgentBrowserSessionControl**](AgentApi.md#setagentbrowsersessioncontrol) | **POST** /v1/agent/runs/{run_id}/browser-session/control | Hand the run browser to the person, or back to the agent |
| [**setAgentTabHolder**](AgentApi.md#setagenttabholderoperation) | **POST** /v1/agent/runs/{run_id}/tabs/{tab_id}/control | Hand a tab to the agent or take it back (the owner\&#39;s own token only) |
| [**startAgentRun**](AgentApi.md#startagentrunoperation) | **POST** /v1/agent/runs | Acquire a governed browser-agent run |
| [**streamAgentLiveFrames**](AgentApi.md#streamagentliveframes) | **GET** /v1/agent/runs/{run_id}/frames/stream | Transient live frames of the run browser as Server-Sent Events |
| [**streamAgentRunEvents**](AgentApi.md#streamagentrunevents) | **GET** /v1/agent/runs/{run_id}/events/stream | Typed agent-run events as Server-Sent Events, with resume (WS10.S3) |



## agentHitTest

> AgentHitTest200Response agentHitTest(runId, x, y)

Map a viewport point to the observed target under it (dark)

E3.3-h. A read-only point hit test: it executes nothing, navigates nowhere, focuses or scrolls nothing, and writes nothing (no receipt, timeline row, artifact or cache entry). Off unless QUARRY_EDGE__HIT_TEST is on (default off): off, the call is refused with 403 hit_test_disabled before the run is looked up. &#x60;x&#x60; and &#x60;y&#x60; are CSS pixels from the viewport\&#39;s top-left corner, rounded to whole pixels. A missing or non-finite value, or a point outside the run\&#39;s viewport (1280 by 800 when its lease names none), is a 400. The browser\&#39;s hit test finds the DOM node under the point and its accessibility ancestors. The answer names an observed target in &#x60;ref&#x60; only when one of those nodes is a target of the caller\&#39;s last observation of the run; then &#x60;snapshotId&#x60; and &#x60;generation&#x60; name that snapshot, so the caller can tell that it is stale. Without a &#x60;ref&#x60;, &#x60;reason&#x60; says why: no_node (nothing under the point), no_snapshot (a node is there, but the run has observed no page) or not_observed (a node is there, but no observed target is on its ancestors). The answer carries no backend node id and no frame id. A ZDR run may use it: nothing is stored or logged, and &#x60;zdr&#x60; echoes the run\&#39;s flag. Only the actor that started the run, in its organisation, may call it; anyone else gets 404.

### Example

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

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **x** | `number` | The point\&#39;s horizontal position, in CSS pixels from the viewport\&#39;s left edge. | [Defaults to `undefined`] |
| **y** | `number` | The point\&#39;s vertical position, in CSS pixels from the viewport\&#39;s top edge. | [Defaults to `undefined`] |

### Return type

[**AgentHitTest200Response**](AgentHitTest200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The node under the point, and the observed target it names when there is one |  -  |
| **400** | x or y is missing, not finite, or outside the viewport; a value that is not a number gets the framework\&#39;s plain-text rejection |  -  |
| **403** | The switch is off (hit_test_disabled) |  -  |
| **404** | Run not found, not started by the caller, or no longer live |  -  |
| **501** | The browser driver has no point hit test (UNSUPPORTED) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## agentInteract

> EnvelopeBrowserObservation agentInteract(runId, agentStepRequest)

Ergonomic alias for one governed browser action

### Example

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

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **agentStepRequest** | [AgentStepRequest](AgentStepRequest.md) |  | |

### Return type

[**EnvelopeBrowserObservation**](EnvelopeBrowserObservation.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Observation and immutable outcome proof |  -  |
| **403** | Domain, SSRF, policy, or grant denial |  -  |
| **409** | Snapshot target is stale, absent, ambiguous, or changed; re-observe before acting |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## agentStep

> EnvelopeBrowserObservation agentStep(runId, agentStepRequest)

Execute one governed browser action

For an API-key caller (INNH-10) the grant is revalidated on every step and must still be the key\&#39;s own; a dialog reply, an upload or a download is refused with 403 &#x60;error.details.reason&#x60; &#x60;api_key_sensitive_action_refused&#x60; before the driver or the broker is asked, and a run belongs to the key that started it (another caller gets 404).

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { AgentStepOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // AgentStepRequest
    agentStepRequest: ...,
  } satisfies AgentStepOperationRequest;

  try {
    const data = await api.agentStep(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **agentStepRequest** | [AgentStepRequest](AgentStepRequest.md) |  | |

### Return type

[**EnvelopeBrowserObservation**](EnvelopeBrowserObservation.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Observation and immutable outcome proof |  -  |
| **403** | Domain, SSRF, policy, or grant denial |  -  |
| **409** | Either the snapshot target is stale, absent, ambiguous, or changed (re-observe before acting), or USER_TOOK_OVER: the person using the browser has taken it over and this action was proposed by an agent (a service principal, or &#x60;initiated_by: agent&#x60;). &#x60;error.details&#x60; is &#x60;{control_mode: \&quot;human_takeover\&quot;, control_epoch, taken_over_at}&#x60; (&#x60;taken_over_at&#x60;, RFC 3339, is absent for a run resumed into a takeover). Not retryable until control is handed back. Enforced only when QUARRY_EDGE__TAKEOVER_ENFORCEMENT&#x3D;1; otherwise the edge logs what it would have refused. |  -  |
| **402** | BUDGET_EXHAUSTED. An agent run reached its step, attempt, runtime or estimated-cost bound (error.details.dimension is steps, attempts, runtime or cost, with limit, spent and unit). Not retryable: the budget does not refill, and a resumed run keeps what it spent. Only returned when QUARRY_EDGE__BUDGET_ENFORCEMENT&#x3D;on; in shadow mode the same condition is only recorded. |  -  |
| **429** | Step budget exhausted (the step limit the request itself set) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## applyAgentOriginExtension

> ApplyAgentOriginExtension200Response applyAgentOriginExtension(runId, applyAgentOriginExtensionRequest)

Apply a person\&#39;s per-origin consent to a live run (dark)

Q3-E3.2-e1w. Off unless QUARRY_EDGE__ORIGIN_CONSENT is on: off, the call is refused with 403 origin_consent_disabled before any broker call. The caller is the run\&#39;s owner or the service principal that started it. The origin-extension grant (kind origin_extension, a child of the run\&#39;s grant, owned by the run\&#39;s owner, the start host plus at most eight listed hosts, one extend_origin:turn, extend_origin:conversation or extend_origin:always action; a ZDR run takes turn only) is validated, the org\&#39;s consent caps are applied, and any web-policy, grant or denied-list deny beats the approval; only then is the run\&#39;s browser egress widened. The extension reaches what a page loads or is redirected through, and never widens the run\&#39;s own navigation allow list. Quarry reads the grant again before every step, before a tab is opened and before an attachment starts; when it is no longer usable the run\&#39;s own egress policy is restored and the extension is dropped. Nothing is persisted, and a resumed run starts from its own grant. The answer never names a host. Refusal reasons (in error.details.reason): origin_consent_disabled (403), origin_extension_api_key_run (403), origin_extension_grant_invalid, origin_extension_host_invalid, origin_extension_too_many_hosts, origin_extension_zdr_scope, org_origin_consent_disabled, org_origin_consent_scope, org_origin_consent_unreadable, origin_extension_denied (SECURITY_BLOCKED).

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ApplyAgentOriginExtensionOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // ApplyAgentOriginExtensionRequest
    applyAgentOriginExtensionRequest: ...,
  } satisfies ApplyAgentOriginExtensionOperationRequest;

  try {
    const data = await api.applyAgentOriginExtension(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **applyAgentOriginExtensionRequest** | [ApplyAgentOriginExtensionRequest](ApplyAgentOriginExtensionRequest.md) |  | |

### Return type

[**ApplyAgentOriginExtension200Response**](ApplyAgentOriginExtension200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The extension was applied |  -  |
| **400** | extension_grant_id is empty or longer than 256 characters |  -  |
| **403** | Origin consent is off (origin_consent_disabled) or the run is an API-key run |  -  |
| **404** | Run not found, not the caller\&#39;s, or no longer live |  -  |
| **422** | The body does not parse |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## attachAgentCurrentTab

> AttachAgentCurrentTab200Response attachAgentCurrentTab(runId, attachAgentCurrentTabRequest)

Attach an agent to a tab the person already has open (dark; service principal only)

WS8.F, the hand-over of a tab the person already has open. Off unless QUARRY_EDGE__ATTACH is on. The same gates as &#x60;POST .../attach&#x60; apply: the caller is a service principal holding browser:execute, it holds an attach grant (a child of the run\&#39;s grant, for this run, owned by the run\&#39;s owner, one to eight hosts), and the run is not ZDR. The body must name the tab in &#x60;attach_tab&#x60; and must not carry &#x60;start_url&#x60; (both are a 400). The delegate gets that tab, not a new one, so &#x60;attached_existing_tab&#x60; is true. The tab must be the person\&#39;s active tab, at &#x60;expected_epoch&#x60; and &#x60;expected_origin&#x60;, held by the person and with no attachment, and the grant must allow exactly that tab\&#39;s host; otherwise the call is 409 browser_continue_tab_changed. Upload and download are disabled for this attachment whatever &#x60;policy&#x60; asks: browser.act.upload and browser.act.download are added to the ask\&#39;s &#x60;disable&#x60; list. Refusal reasons (in error.message): attach_disabled, attach_requires_service_principal, attach_zdr_refused, attach_current_tab_requires_personal_browser (the run is not on the open web), attach_requires_grant, attach_grant_invalid, attach_driver_unsupported (UNSUPPORTED), attach_exists (CONFLICT) and browser_continue_tab_changed (CONFLICT).

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { AttachAgentCurrentTabOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // AttachAgentCurrentTabRequest
    attachAgentCurrentTabRequest: ...,
  } satisfies AttachAgentCurrentTabOperationRequest;

  try {
    const data = await api.attachAgentCurrentTab(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **attachAgentCurrentTabRequest** | [AttachAgentCurrentTabRequest](AttachAgentCurrentTabRequest.md) |  | |

### Return type

[**AttachAgentCurrentTab200Response**](AttachAgentCurrentTab200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The attachment, bound to the tab the person already has open |  -  |
| **400** | attach_tab is missing, start_url is present, or a constraint is outside its bounds |  -  |
| **403** | Refused (the reason is in error.message); a service principal without browser:execute gets a bare 403 |  -  |
| **404** | Run not found, in another organisation, or no longer live |  -  |
| **409** | An attachment is already live on the run (attach_exists), or the tab changed (browser_continue_tab_changed) |  -  |
| **501** | The browser driver cannot hand over a tab (attach_driver_unsupported) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## attachAgentRun

> AttachAgentRun200Response attachAgentRun(runId, attachAgentRunRequest)

Attach an agent to a person\&#39;s live run (dark; service principal only)

WS8.F. Off unless QUARRY_EDGE__ATTACH is on. A service principal holding an attach grant (a child of the run\&#39;s grant, for this run, owned by the run\&#39;s owner, one to eight hosts) gets one agent tab, opened in the background: the person\&#39;s active tab does not change. Refused for ZDR runs. Refusal reasons (in error.message): attach_disabled, attach_requires_service_principal, attach_zdr_refused, attach_requires_grant, attach_grant_invalid, attach_driver_unsupported (UNSUPPORTED), attach_exists (CONFLICT).

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { AttachAgentRunOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // AttachAgentRunRequest
    attachAgentRunRequest: ...,
  } satisfies AttachAgentRunOperationRequest;

  try {
    const data = await api.attachAgentRun(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **attachAgentRunRequest** | [AttachAgentRunRequest](AttachAgentRunRequest.md) |  | |

### Return type

[**AttachAgentRun200Response**](AttachAgentRun200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The attachment and its agent tab |  -  |
| **400** | A constraint is outside its bounds |  -  |
| **403** | Refused (the reason is in error.message) |  -  |
| **404** | Run not found |  -  |
| **409** | An attachment is already live on the run |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## browserRedirectHop

> BrowserRedirectHop200Response browserRedirectHop(browserRedirectHopRequest)

One credentialless redirect hop for assistant navigation

Inspects one public HTTP(S) response and answers where its redirect points, if it redirects at all. The probe sends no cookies, reads no storage and reads the response headers only; the URL\&#39;s fragment is dropped. The probe pins public DNS addresses and refuses a URL that is not public or is blocked (403 SECURITY_BLOCKED). The web policy is checked for the URL and again for the target (403 FORBIDDEN). &#x60;target&#x60; is the absolute URL of a validated 301, 302, 303, 307 or 308 redirect, and null for any other status; a redirect without exactly one usable Location, or one that downgrades https to http, is refused (403 SECURITY_BLOCKED). A DNS or transport failure is answered with the probe\&#39;s own code and a generic message that never names an internal host or address. A service principal needs browser:execute (a bare 403 otherwise); a person\&#39;s token passes this gate. At most 16 probes run per process, and an excess request is 429 RATE_LIMITED without queueing. The whole probe has a 4 second deadline, past which the answer is 504 TIMEOUT.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { BrowserRedirectHopOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // BrowserRedirectHopRequest
    browserRedirectHopRequest: ...,
  } satisfies BrowserRedirectHopOperationRequest;

  try {
    const data = await api.browserRedirectHop(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **browserRedirectHopRequest** | [BrowserRedirectHopRequest](BrowserRedirectHopRequest.md) |  | |

### Return type

[**BrowserRedirectHop200Response**](BrowserRedirectHop200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The redirect target, or null when the response is not a redirect |  -  |
| **403** | Blocked, not a public URL, refused by the web policy, or a redirect that is refused (SECURITY_BLOCKED or FORBIDDEN) |  -  |
| **400** | The body is not JSON (the framework\&#39;s plain-text rejection) |  -  |
| **422** | The body does not match the schema (an unknown field, or a wrong type); the framework answers in plain text |  -  |
| **429** | Probe capacity is busy (RATE_LIMITED); retry later |  -  |
| **502** | The public URL could not be resolved or read (DRIVER_FAILED, or the probe\&#39;s own code) |  -  |
| **504** | The probe passed its 4 second deadline (TIMEOUT) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## checkAgentProcedureReplay

> EnvelopeReplayDecision checkAgentProcedureReplay(replayCheckRequest)

Compare actions with a procedure without executing effects

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { CheckAgentProcedureReplayRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // ReplayCheckRequest
    replayCheckRequest: ...,
  } satisfies CheckAgentProcedureReplayRequest;

  try {
    const data = await api.checkAgentProcedureReplay(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **replayCheckRequest** | [ReplayCheckRequest](ReplayCheckRequest.md) |  | |

### Return type

[**EnvelopeReplayDecision**](EnvelopeReplayDecision.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Exact, repair-required, or refused decision |  -  |
| **400** | &#x60;prefix_needs_v2&#x60; (details.reason) when &#x60;prefix&#x60; is true and the procedure is version 1. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## closeAgentRun

> EnvelopeClosedRun closeAgentRun(runId)

Close a browser-agent run and release its lease

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { CloseAgentRunRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
  } satisfies CloseAgentRunRequest;

  try {
    const data = await api.closeAgentRun(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |

### Return type

[**EnvelopeClosedRun**](EnvelopeClosedRun.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Run closed |  -  |
| **404** | Run not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## closeAgentTab

> EnvelopeAgentTabs closeAgentTab(runId, tabId, initiatedBy, aiRunId, agentLabel)

Close a tab

Has no body, so the hint and the advisory fields are query parameters. Same take-over rule and recording as opening a tab; the tab\&#39;s opener record is dropped.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { CloseAgentTabRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | A tab id from the run\'s tab list
    tabId: tabId_example,
    // 'human' | 'agent' | Who is making the call. Can only tighten: `agent` on a person\'s token makes it an agent\'s, and a service principal is always an agent. `initiated_by` is accepted too (both in one request is an error). (optional)
    initiatedBy: initiatedBy_example,
    // string | ADVISORY. The AI run the caller says it acts for. Recorded, never trusted. `ai_run_id` is accepted too. (optional)
    aiRunId: aiRunId_example,
    // string | ADVISORY. A short label the caller gave its agent. Recorded, never trusted. `agent_label` is accepted too. (optional)
    agentLabel: agentLabel_example,
  } satisfies CloseAgentTabRequest;

  try {
    const data = await api.closeAgentTab(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **tabId** | `string` | A tab id from the run\&#39;s tab list | [Defaults to `undefined`] |
| **initiatedBy** | `human`, `agent` | Who is making the call. Can only tighten: &#x60;agent&#x60; on a person\&#39;s token makes it an agent\&#39;s, and a service principal is always an agent. &#x60;initiated_by&#x60; is accepted too (both in one request is an error). | [Optional] [Defaults to `undefined`] [Enum: human, agent] |
| **aiRunId** | `string` | ADVISORY. The AI run the caller says it acts for. Recorded, never trusted. &#x60;ai_run_id&#x60; is accepted too. | [Optional] [Defaults to `undefined`] |
| **agentLabel** | `string` | ADVISORY. A short label the caller gave its agent. Recorded, never trusted. &#x60;agent_label&#x60; is accepted too. | [Optional] [Defaults to `undefined`] |

### Return type

[**EnvelopeAgentTabs**](EnvelopeAgentTabs.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The run\&#39;s tabs |  -  |
| **400** | A hint that is not &#x60;human&#x60; or &#x60;agent&#x60; |  -  |
| **404** | Run not found, not owned by the caller, or no longer live |  -  |
| **409** | USER_TOOK_OVER, as for opening a tab |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## compileAgentProcedure

> EnvelopeBrowserProcedure compileAgentProcedure(runId, compileProcedureRequest)

Compile verified receipts into a replay candidate

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { CompileAgentProcedureRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // CompileProcedureRequest
    compileProcedureRequest: ...,
  } satisfies CompileAgentProcedureRequest;

  try {
    const data = await api.compileAgentProcedure(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **compileProcedureRequest** | [CompileProcedureRequest](CompileProcedureRequest.md) |  | |

### Return type

[**EnvelopeBrowserProcedure**](EnvelopeBrowserProcedure.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Deterministic procedure candidate |  -  |
| **400** | With &#x60;canonical&#x60; true while QUARRY_EDGE__PROCEDURE_CANONICAL is off (the default), &#x60;procedure_canonical_disabled&#x60; (details.reason, details.env). See ERROR_REASONS.md. |  -  |
| **409** | Receipt stream contains unverified or incomplete effects. With &#x60;canonical&#x60; true, a ref or semantic step with no storable target answers TARGET_REPAIR_REQUIRED (details.step). |  -  |
| **501** | With &#x60;canonical&#x60; true, a receipt holds an action outside the canonical vocabulary, or a key or option value carries a redaction marker: UNSUPPORTED (details.step). |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## connectAgentLiveFrameSocket

> connectAgentLiveFrameSocket(runId, tabId, format, quality, maxWidth, maxHeight, everyNthFrame, timeoutMs, intervalMs, maxFrames, xQuarryLiveScope)

Live frames and browser actions over a WebSocket (the GET answers 101)

A WebSocket upgrade. OpenAPI cannot describe the messages, so they are set out here. The owner, gating, scope and query rules are those of &#x60;GET .../frames/stream&#x60;. A refusal of them (400, 403, 404, 501 or 502) is a plain JSON answer, given before the upgrade. A socket that names a &#x60;tabId&#x60; is view-only. Gate: none as a route. Once the socket is open, the server sends JSON text messages, each with a &#x60;type&#x60;: &#x60;frame&#x60; (&#x60;sequence&#x60;, &#x60;mimeType&#x60;, &#x60;dataBase64&#x60; and &#x60;zdr&#x60;) every &#x60;intervalMs&#x60;; &#x60;devtools&#x60; (&#x60;events&#x60;, the same objects as &#x60;GET .../devtools&#x60;, and &#x60;zdr&#x60;) when new events exist; &#x60;observation&#x60; (&#x60;observation&#x60;, a BrowserObservation) as the answer to an action; &#x60;pong&#x60; as the answer to a ping; &#x60;error&#x60; (&#x60;code&#x60;, &#x60;message&#x60;, &#x60;retryable&#x60;, and &#x60;details&#x60; when there are some) for a refused or failed message; and &#x60;done&#x60; (&#x60;reason&#x60; &#x60;max_frames&#x60; or &#x60;shutdown&#x60;) before the server closes. A frame that cannot be read sends an &#x60;error&#x60; and closes the socket, and other errors leave it open. The client may send &#x60;{\&quot;type\&quot;:\&quot;action\&quot;,\&quot;action\&quot;:{...},\&quot;instruction\&quot;:\&quot;...\&quot;,\&quot;actor\&quot;:\&quot;human\&quot;}&#x60;, where &#x60;action&#x60; is an action object of &#x60;POST .../step&#x60;, &#x60;instruction&#x60; and &#x60;actor&#x60; are optional, and &#x60;actor&#x60; (&#x60;human&#x60; or &#x60;agent&#x60;) can only tighten who the action is attributed to. The action runs as one step, through the same admission and refusals as &#x60;POST .../step&#x60;. The client may send &#x60;{\&quot;type\&quot;:\&quot;ping\&quot;}&#x60; to keep the socket alive, and transport pings are answered with pongs. On a view-only socket every well-formed message other than a ping is refused with an &#x60;error&#x60; frame, with code &#x60;FORBIDDEN&#x60; and &#x60;details.reason&#x60; &#x60;live_view_only&#x60;, and the socket stays open. Binary messages are ignored, and text that is not one of these messages answers &#x60;BAD_REQUEST&#x60;. Frames are transient, as on &#x60;frames/stream&#x60;, and an action\&#39;s receipt, timeline item and observation follow the rules of a step.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ConnectAgentLiveFrameSocketRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | One of the run\'s tabs, read without being selected. The driver must be able to pin a tab. A tab the run does not hold is 404. A WebSocket that names a tab is view-only. (optional)
    tabId: tabId_example,
    // 'jpeg' | 'jpg' | 'png' | `jpeg` (also `jpg`), the default, or `png`. Any other value is 400. (optional)
    format: format_example,
    // number | JPEG or PNG quality, clamped to 1 to 100. Default 65. (optional)
    quality: 56,
    // number | Width of the frame, clamped to at least 320 and to the run\'s viewport width. Default the viewport width, which is 1280 when the lease names none. (optional)
    maxWidth: 56,
    // number | Height of the frame, clamped to at least 240 and to the run\'s viewport height. Default the viewport height, which is 800 when the lease names none. (optional)
    maxHeight: 56,
    // number | The screencast\'s frame thinning, clamped to 1 to 10. Default 1. (optional)
    everyNthFrame: 56,
    // number | How long the driver may take to produce a frame, clamped to 100 to 5000 milliseconds. Default 1000. (optional)
    timeoutMs: 56,
    // number | Milliseconds between frames, clamped to 75 to 2000. Default 250. Frame stream and WebSocket only. (optional)
    intervalMs: 56,
    // number | Frames before the stream or socket ends with done (max_frames). 0 or absent is no cap, and a larger value is capped at 7200. Frame stream and WebSocket only. (optional)
    maxFrames: 56,
    // string | `full` (the default) or `view_only`; `view-only` and `view` are read the same way, ignoring case. The header is read only while QUARRY_EDGE__LIVE_VIEW_ONLY is on, and it is ignored while that switch is off. A view-only caller gets no input and no interactive live-view reference, and a view-only share of a ZDR run is refused with 403. Any other value is 400. The Verevon gateway sets it from its own route and strips a value that a client sends. A caller can only narrow the scope with it, never widen it. (optional)
    xQuarryLiveScope: xQuarryLiveScope_example,
  } satisfies ConnectAgentLiveFrameSocketRequest;

  try {
    const data = await api.connectAgentLiveFrameSocket(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **tabId** | `string` | One of the run\&#39;s tabs, read without being selected. The driver must be able to pin a tab. A tab the run does not hold is 404. A WebSocket that names a tab is view-only. | [Optional] [Defaults to `undefined`] |
| **format** | `jpeg`, `jpg`, `png` | &#x60;jpeg&#x60; (also &#x60;jpg&#x60;), the default, or &#x60;png&#x60;. Any other value is 400. | [Optional] [Defaults to `undefined`] [Enum: jpeg, jpg, png] |
| **quality** | `number` | JPEG or PNG quality, clamped to 1 to 100. Default 65. | [Optional] [Defaults to `undefined`] |
| **maxWidth** | `number` | Width of the frame, clamped to at least 320 and to the run\&#39;s viewport width. Default the viewport width, which is 1280 when the lease names none. | [Optional] [Defaults to `undefined`] |
| **maxHeight** | `number` | Height of the frame, clamped to at least 240 and to the run\&#39;s viewport height. Default the viewport height, which is 800 when the lease names none. | [Optional] [Defaults to `undefined`] |
| **everyNthFrame** | `number` | The screencast\&#39;s frame thinning, clamped to 1 to 10. Default 1. | [Optional] [Defaults to `undefined`] |
| **timeoutMs** | `number` | How long the driver may take to produce a frame, clamped to 100 to 5000 milliseconds. Default 1000. | [Optional] [Defaults to `undefined`] |
| **intervalMs** | `number` | Milliseconds between frames, clamped to 75 to 2000. Default 250. Frame stream and WebSocket only. | [Optional] [Defaults to `undefined`] |
| **maxFrames** | `number` | Frames before the stream or socket ends with done (max_frames). 0 or absent is no cap, and a larger value is capped at 7200. Frame stream and WebSocket only. | [Optional] [Defaults to `undefined`] |
| **xQuarryLiveScope** | `string` | &#x60;full&#x60; (the default) or &#x60;view_only&#x60;; &#x60;view-only&#x60; and &#x60;view&#x60; are read the same way, ignoring case. The header is read only while QUARRY_EDGE__LIVE_VIEW_ONLY is on, and it is ignored while that switch is off. A view-only caller gets no input and no interactive live-view reference, and a view-only share of a ZDR run is refused with 403. Any other value is 400. The Verevon gateway sets it from its own route and strips a value that a client sends. A caller can only narrow the scope with it, never widen it. | [Optional] [Defaults to `undefined`] |

### Return type

`void` (Empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **101** | Switching Protocols. The socket is open and carries the JSON messages that the description sets out. |  -  |
| **400** | A format other than jpeg, jpg or png, a scope value other than full or view_only, an empty tabId or one over 128 characters, or a tabId on a driver that cannot pin tabs |  -  |
| **403** | A view-only share of a ZDR run (live_view_only_zdr, while QUARRY_EDGE__LIVE_VIEW_ONLY is on), or an API-key token on a route that its key may not use (api_key_route_refused) |  -  |
| **404** | The edge holds no such run, or the caller does not own it; tab not found when tabId is not one of the run\&#39;s tabs |  -  |
| **501** | UNSUPPORTED, when the selected driver cannot produce governed full-fidelity live frames |  -  |
| **502** | DRIVER_FAILED, when the tab cannot be looked up before the upgrade (a timeout answers 504) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getAgentBrowserSession

> GetAgentBrowserSession200Response getAgentBrowserSession(runId)

The owner browser-session projection of a run

The projection the owner\&#39;s browser shows for one run: its profile, current URL, control mode and epoch, tabs and last observation, and the open-web and attachment fields where they apply. A live run is answered from the edge\&#39;s memory, with &#x60;live&#x60; true. A run the edge no longer holds is answered from its durable checkpoint, with &#x60;live&#x60; false, an empty &#x60;tabs&#x60; list, &#x60;lastObservation&#x60; null, and &#x60;tabControl&#x60; and &#x60;attachAvailable&#x60; false. A ZDR run keeps no checkpoint, so once the edge has released it the read is 404. Gate: none as a route. &#x60;grantKind&#x60; &#x60;human_browse&#x60; marks a person\&#39;s open-web run, which is admitted only while QUARRY_EDGE__HUMAN_BROWSE is on, and the attach fields (&#x60;tabControl&#x60;, &#x60;attachAvailable&#x60; and &#x60;attach&#x60;) are dark while QUARRY_EDGE__ATTACH is off. Only the run\&#39;s owner, the actor that started it in its organisation, may read it; any other caller gets 404. The read writes nothing durable and counts as activity for a live run\&#39;s idle clock.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { GetAgentBrowserSessionRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
  } satisfies GetAgentBrowserSessionRequest;

  try {
    const data = await api.getAgentBrowserSession(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |

### Return type

[**GetAgentBrowserSession200Response**](GetAgentBrowserSession200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The owner projection of the run |  -  |
| **400** | The run id is not a run id (invalid agent run id) |  -  |
| **403** | An API-key token on a route that its key may not use (api_key_route_refused) |  -  |
| **404** | The edge holds no such run and no checkpoint for it, or the caller does not own it |  -  |
| **502** | DRIVER_FAILED, when a driver or store call fails, for example the tab listing of a live run (a timeout answers 504) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getAgentLiveFrame

> GetAgentLiveFrame200Response getAgentLiveFrame(runId, tabId, format, quality, maxWidth, maxHeight, everyNthFrame, timeoutMs, xQuarryLiveScope)

One transient live frame of the run browser

One frame of the run\&#39;s current page, for live preview and human takeover. The frame is returned to the caller and nothing else: no artifact, receipt, timeline item or checkpoint is written from it, whatever the run\&#39;s posture, and the evidence of a step comes from the observation that the step takes, not from a frame. &#x60;zdr&#x60; says whether the run is under zero data retention. The request selects no tab, navigates nowhere and changes no control state. The selected driver must produce full-fidelity frames, or the answer is 501 UNSUPPORTED, and that check comes before the run is looked up. &#x60;format&#x60; is &#x60;jpeg&#x60; by default, or &#x60;png&#x60;. &#x60;quality&#x60;, &#x60;maxWidth&#x60;, &#x60;maxHeight&#x60;, &#x60;everyNthFrame&#x60; and &#x60;timeoutMs&#x60; are clamped rather than refused, as their parameter descriptions say. &#x60;tabId&#x60; reads one of the run\&#39;s tabs without selecting it, and needs a driver that can pin a tab. Gate: none as a route; the scope header is read only while QUARRY_EDGE__LIVE_VIEW_ONLY is on. Only the run\&#39;s owner may read it; any other caller gets 404.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { GetAgentLiveFrameRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | One of the run\'s tabs, read without being selected. The driver must be able to pin a tab. A tab the run does not hold is 404. A WebSocket that names a tab is view-only. (optional)
    tabId: tabId_example,
    // 'jpeg' | 'jpg' | 'png' | `jpeg` (also `jpg`), the default, or `png`. Any other value is 400. (optional)
    format: format_example,
    // number | JPEG or PNG quality, clamped to 1 to 100. Default 65. (optional)
    quality: 56,
    // number | Width of the frame, clamped to at least 320 and to the run\'s viewport width. Default the viewport width, which is 1280 when the lease names none. (optional)
    maxWidth: 56,
    // number | Height of the frame, clamped to at least 240 and to the run\'s viewport height. Default the viewport height, which is 800 when the lease names none. (optional)
    maxHeight: 56,
    // number | The screencast\'s frame thinning, clamped to 1 to 10. Default 1. (optional)
    everyNthFrame: 56,
    // number | How long the driver may take to produce a frame, clamped to 100 to 5000 milliseconds. Default 1000. (optional)
    timeoutMs: 56,
    // string | `full` (the default) or `view_only`; `view-only` and `view` are read the same way, ignoring case. The header is read only while QUARRY_EDGE__LIVE_VIEW_ONLY is on, and it is ignored while that switch is off. A view-only caller gets no input and no interactive live-view reference, and a view-only share of a ZDR run is refused with 403. Any other value is 400. The Verevon gateway sets it from its own route and strips a value that a client sends. A caller can only narrow the scope with it, never widen it. (optional)
    xQuarryLiveScope: xQuarryLiveScope_example,
  } satisfies GetAgentLiveFrameRequest;

  try {
    const data = await api.getAgentLiveFrame(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **tabId** | `string` | One of the run\&#39;s tabs, read without being selected. The driver must be able to pin a tab. A tab the run does not hold is 404. A WebSocket that names a tab is view-only. | [Optional] [Defaults to `undefined`] |
| **format** | `jpeg`, `jpg`, `png` | &#x60;jpeg&#x60; (also &#x60;jpg&#x60;), the default, or &#x60;png&#x60;. Any other value is 400. | [Optional] [Defaults to `undefined`] [Enum: jpeg, jpg, png] |
| **quality** | `number` | JPEG or PNG quality, clamped to 1 to 100. Default 65. | [Optional] [Defaults to `undefined`] |
| **maxWidth** | `number` | Width of the frame, clamped to at least 320 and to the run\&#39;s viewport width. Default the viewport width, which is 1280 when the lease names none. | [Optional] [Defaults to `undefined`] |
| **maxHeight** | `number` | Height of the frame, clamped to at least 240 and to the run\&#39;s viewport height. Default the viewport height, which is 800 when the lease names none. | [Optional] [Defaults to `undefined`] |
| **everyNthFrame** | `number` | The screencast\&#39;s frame thinning, clamped to 1 to 10. Default 1. | [Optional] [Defaults to `undefined`] |
| **timeoutMs** | `number` | How long the driver may take to produce a frame, clamped to 100 to 5000 milliseconds. Default 1000. | [Optional] [Defaults to `undefined`] |
| **xQuarryLiveScope** | `string` | &#x60;full&#x60; (the default) or &#x60;view_only&#x60;; &#x60;view-only&#x60; and &#x60;view&#x60; are read the same way, ignoring case. The header is read only while QUARRY_EDGE__LIVE_VIEW_ONLY is on, and it is ignored while that switch is off. A view-only caller gets no input and no interactive live-view reference, and a view-only share of a ZDR run is refused with 403. Any other value is 400. The Verevon gateway sets it from its own route and strips a value that a client sends. A caller can only narrow the scope with it, never widen it. | [Optional] [Defaults to `undefined`] |

### Return type

[**GetAgentLiveFrame200Response**](GetAgentLiveFrame200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | One frame |  -  |
| **400** | A format other than jpeg, jpg or png, a scope value other than full or view_only, an empty tabId or one over 128 characters, or a tabId on a driver that cannot pin tabs |  -  |
| **403** | A view-only share of a ZDR run (live_view_only_zdr, while QUARRY_EDGE__LIVE_VIEW_ONLY is on), or an API-key token on a route that its key may not use (api_key_route_refused) |  -  |
| **404** | The edge holds no such run, or the caller does not own it; tab not found when tabId is not one of the run\&#39;s tabs |  -  |
| **501** | UNSUPPORTED, when the selected driver cannot produce governed full-fidelity live frames |  -  |
| **502** | DRIVER_FAILED, when the frame cannot be read (a timeout answers 504) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getAgentLiveView

> GetAgentLiveView200Response getAgentLiveView(runId, xQuarryLiveScope)

The run live-view reference, read once; the App Shell asks again when it expires

The run\&#39;s live-view reference: the URL the App Shell renders, its &#x60;kind&#x60;, and when it expires as &#x60;expires_at&#x60;. It is the object that the &#x60;live_view&#x60; event of the frame stream carries. &#x60;data&#x60; is null when the driver has no live-view reference. A reference is not a frame and carries no frame bytes. When &#x60;expires_at&#x60; has passed, the App Shell asks again. Under a view-only scope only a read-only kind, &#x60;hls&#x60; or &#x60;frames&#x60;, is answered: an &#x60;iframe&#x60; or &#x60;provider&#x60; reference, or one with no kind, is answered as null. Gate: none as a route; the scope header is read only while QUARRY_EDGE__LIVE_VIEW_ONLY is on. The read writes nothing. Only the run\&#39;s owner may read it; any other caller gets 404.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { GetAgentLiveViewRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | `full` (the default) or `view_only`; `view-only` and `view` are read the same way, ignoring case. The header is read only while QUARRY_EDGE__LIVE_VIEW_ONLY is on, and it is ignored while that switch is off. A view-only caller gets no input and no interactive live-view reference, and a view-only share of a ZDR run is refused with 403. Any other value is 400. The Verevon gateway sets it from its own route and strips a value that a client sends. A caller can only narrow the scope with it, never widen it. (optional)
    xQuarryLiveScope: xQuarryLiveScope_example,
  } satisfies GetAgentLiveViewRequest;

  try {
    const data = await api.getAgentLiveView(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **xQuarryLiveScope** | `string` | &#x60;full&#x60; (the default) or &#x60;view_only&#x60;; &#x60;view-only&#x60; and &#x60;view&#x60; are read the same way, ignoring case. The header is read only while QUARRY_EDGE__LIVE_VIEW_ONLY is on, and it is ignored while that switch is off. A view-only caller gets no input and no interactive live-view reference, and a view-only share of a ZDR run is refused with 403. Any other value is 400. The Verevon gateway sets it from its own route and strips a value that a client sends. A caller can only narrow the scope with it, never widen it. | [Optional] [Defaults to `undefined`] |

### Return type

[**GetAgentLiveView200Response**](GetAgentLiveView200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The live-view reference, or null when the driver has none |  -  |
| **400** | The scope header holds a value other than full or view_only |  -  |
| **403** | A view-only share of a ZDR run (live_view_only_zdr, while QUARRY_EDGE__LIVE_VIEW_ONLY is on), or an API-key token on a route that its key may not use (api_key_route_refused) |  -  |
| **404** | The edge holds no such run, or the caller does not own it |  -  |
| **502** | DRIVER_FAILED, when the driver cannot give the live-view reference (a timeout answers 504) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## handOffAgentRunDownload

> EnvelopeAgentDownloadKnowledge handOffAgentRunDownload(runId, artifactId, agentDownloadKnowledgeRequest)

Hand one admitted download of a run to Data Plane as a document

Reads the stored bytes under the caller\&#39;s org, derives the text (PDF parser, or UTF-8 for text, Markdown and CSV) and sends it through the one Data Plane ingest contract. The org and user come from the verified token. Off until &#x60;QUARRY_EDGE__DOWNLOADS&#x60; and &#x60;QUARRY_EDGE__DOWNLOAD_HANDOFF&#x60; are on; a ZDR token or run is refused (403, &#x60;download_handoff_zdr_refused&#x60;) before anything is read. A repeat for the same artifact and digest returns the first document id with &#x60;created&#x60; false. Typed refusals (&#x60;download_handoff_*&#x60; in &#x60;details.reason&#x60;) pass through from the hand-off.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { HandOffAgentRunDownloadRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | An artifact id from the run\'s download list
    artifactId: artifactId_example,
    // AgentDownloadKnowledgeRequest
    agentDownloadKnowledgeRequest: ...,
  } satisfies HandOffAgentRunDownloadRequest;

  try {
    const data = await api.handOffAgentRunDownload(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **artifactId** | `string` | An artifact id from the run\&#39;s download list | [Defaults to `undefined`] |
| **agentDownloadKnowledgeRequest** | [AgentDownloadKnowledgeRequest](AgentDownloadKnowledgeRequest.md) |  | |

### Return type

[**EnvelopeAgentDownloadKnowledge**](EnvelopeAgentDownloadKnowledge.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The document the download became |  -  |
| **400** | Invalid &#x60;idempotency_key&#x60;, or the download\&#39;s text or source is not acceptable to Data Plane |  -  |
| **403** | Downloads or the hand-off are not enabled, or the caller or run is ZDR |  -  |
| **404** | Run or download not found, or not owned by the caller |  -  |
| **415** | The download type cannot be handed off, or the file has no text |  -  |
| **501** | Data Plane ingest is not configured |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## impactCheckAgentProcedure

> EnvelopeProcedureImpact impactCheckAgentProcedure(procedureImpactRequest)

Identify changed sources that require procedure quarantine

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ImpactCheckAgentProcedureRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // ProcedureImpactRequest
    procedureImpactRequest: ...,
  } satisfies ImpactCheckAgentProcedureRequest;

  try {
    const data = await api.impactCheckAgentProcedure(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **procedureImpactRequest** | [ProcedureImpactRequest](ProcedureImpactRequest.md) |  | |

### Return type

[**EnvelopeProcedureImpact**](EnvelopeProcedureImpact.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Conservative change-impact report |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listAgentBrowserTimeline

> ListAgentBrowserTimeline200Response listAgentBrowserTimeline(runId, cursor, limit)

The owner browser timeline of a run, oldest first, one page at a time

A bounded page of the run\&#39;s owner timeline. It merges the run\&#39;s step receipts, as &#x60;action&#x60; items, with Quarry\&#39;s compact audit records (&#x60;control&#x60;, &#x60;tab&#x60;, &#x60;tab_control&#x60;, &#x60;attach&#x60;, &#x60;devtools&#x60; and &#x60;lifecycle&#x60; items), and sorts them by &#x60;occurredAt&#x60; and then &#x60;id&#x60;, oldest first. An item carries ids, classes, counts and words, never raw browser contents. Action items are the run\&#39;s step receipts: a step on a run that is not ZDR is recorded, and a person\&#39;s step is recorded as QUARRY_EDGE__HUMAN_RECEIPT_MODE says, where &#x60;none&#x60; records nothing. &#x60;limit&#x60; defaults to 50 and is clamped to 1 to 100. &#x60;cursor&#x60; is the &#x60;id&#x60; of the last item of the previous page, and an id that the timeline does not hold is a 400. &#x60;nextCursor&#x60; is null on the last page. Gate: none as a route. A ZDR run has no timeline, and the read is 409. Only the run\&#39;s owner may read it; any other caller gets 404.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ListAgentBrowserTimelineRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | The `id` of the last item of the previous page. (optional)
    cursor: cursor_example,
    // number | Items per page, clamped to 1 to 100. Default 50. (optional)
    limit: 56,
  } satisfies ListAgentBrowserTimelineRequest;

  try {
    const data = await api.listAgentBrowserTimeline(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **cursor** | `string` | The &#x60;id&#x60; of the last item of the previous page. | [Optional] [Defaults to `undefined`] |
| **limit** | `number` | Items per page, clamped to 1 to 100. Default 50. | [Optional] [Defaults to `undefined`] |

### Return type

[**ListAgentBrowserTimeline200Response**](ListAgentBrowserTimeline200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | One page of the timeline, oldest first |  -  |
| **400** | The run id is not a run id, or the cursor is not an item on the timeline (browser timeline cursor is invalid) |  -  |
| **403** | An API-key token on a route that its key may not use (api_key_route_refused) |  -  |
| **404** | The edge holds no such run and no checkpoint for it, or the caller does not own it |  -  |
| **409** | CONFLICT for a ZDR run (browser timeline is unavailable for a Zero Data Retention run) |  -  |
| **502** | DRIVER_FAILED, when the stored receipts or timeline events cannot be read (a timeout answers 504) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listAgentDevtoolsEvents

> ListAgentDevtoolsEvents200Response listAgentDevtoolsEvents(runId, afterSequence, limit, levels, filter, category, tabId)

Read a filtered window of transient browser DevTools events

Returns events newer than afterSequence from the driver\&#39;s bounded buffer (512 events). Filters are optional and combine with AND. levels accepts debug, info, log, warn, warning, error and verbose (at most five; warn is warning, log is info, verbose is debug). filter is a case-insensitive substring over text, url and name only, never payload, at most 200 characters without control characters. nextSequence is the last event examined; pass it back as afterSequence to continue even when no event matched.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ListAgentDevtoolsEventsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // number (optional)
    afterSequence: 56,
    // number (optional)
    limit: 56,
    // string (optional)
    levels: warn,error,
    // string (optional)
    filter: filter_example,
    // 'console' | 'network' | 'lifecycle' (optional)
    category: category_example,
    // string (optional)
    tabId: tabId_example,
  } satisfies ListAgentDevtoolsEventsRequest;

  try {
    const data = await api.listAgentDevtoolsEvents(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **afterSequence** | `number` |  | [Optional] [Defaults to `0`] |
| **limit** | `number` |  | [Optional] [Defaults to `100`] |
| **levels** | `string` |  | [Optional] [Defaults to `undefined`] |
| **filter** | `string` |  | [Optional] [Defaults to `undefined`] |
| **category** | `console`, `network`, `lifecycle` |  | [Optional] [Defaults to `undefined`] [Enum: console, network, lifecycle] |
| **tabId** | `string` |  | [Optional] [Defaults to `undefined`] |

### Return type

[**ListAgentDevtoolsEvents200Response**](ListAgentDevtoolsEvents200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Matching events in sequence order, with the resume cursor and ZDR flag |  -  |
| **400** | A filter value is outside the documented allowlist |  -  |
| **404** | Run not found or not owned by the caller |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listAgentEgressReceipts

> EnvelopeEgressReceipts listAgentEgressReceipts(runId, afterSequence, limit)

Read redacted live browser egress decisions

Returns bounded, driver-authored policy receipts for the live session. URLs contain only origin and path; query strings, headers, bodies, credentials, resolved IP addresses, and proxy details are omitted.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ListAgentEgressReceiptsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // number (optional)
    afterSequence: 56,
    // number (optional)
    limit: 56,
  } satisfies ListAgentEgressReceiptsRequest;

  try {
    const data = await api.listAgentEgressReceipts(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **afterSequence** | `number` |  | [Optional] [Defaults to `0`] |
| **limit** | `number` |  | [Optional] [Defaults to `100`] |

### Return type

[**EnvelopeEgressReceipts**](EnvelopeEgressReceipts.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Ordered, redacted egress decisions |  -  |
| **404** | Run not found or not owned by the caller |  -  |
| **409** | Requested receipt cursor is no longer continuous in the bounded proof buffer |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listAgentReceipts

> EnvelopeStepReceipts listAgentReceipts(runId)

Read tenant-scoped immutable action receipts

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ListAgentReceiptsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
  } satisfies ListAgentReceiptsRequest;

  try {
    const data = await api.listAgentReceipts(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |

### Return type

[**EnvelopeStepReceipts**](EnvelopeStepReceipts.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Ordered action receipt stream |  -  |
| **404** | Run or receipt stream not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listAgentRunDownloads

> EnvelopeAgentRunDownloads listAgentRunDownloads(runId)

List the admitted downloads of a run

Lists the files a &#x60;download_ref&#x60; step stored for the caller\&#39;s own run, each already checked against the download type allow-list (PDF, plain text, Markdown, CSV). An entry names the artifact, its type, size and SHA-256 digest; it carries no URL and no file name. A ZDR run lists nothing. Off (403 &#x60;downloads_disabled&#x60;) until &#x60;QUARRY_EDGE__DOWNLOADS&#x60; is on.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ListAgentRunDownloadsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
  } satisfies ListAgentRunDownloadsRequest;

  try {
    const data = await api.listAgentRunDownloads(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |

### Return type

[**EnvelopeAgentRunDownloads**](EnvelopeAgentRunDownloads.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The run\&#39;s admitted downloads, oldest first (at most 64) |  -  |
| **403** | Downloads are not enabled (&#x60;downloads_disabled&#x60;) |  -  |
| **404** | Run not found or not owned by the caller |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listAgentTabs

> EnvelopeAgentTabs listAgentTabs(runId)

List the live browser tabs of a run

The driver\&#39;s current tabs for the run. A tab carries no opener here; the owner projection (&#x60;GET .../browser-session&#x60;, see CONTRACTS.md) names who opened each tab as &#x60;openedBy&#x60;.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ListAgentTabsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
  } satisfies ListAgentTabsRequest;

  try {
    const data = await api.listAgentTabs(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |

### Return type

[**EnvelopeAgentTabs**](EnvelopeAgentTabs.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The run\&#39;s tabs |  -  |
| **404** | Run not found or not owned by the caller |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## markAgentTab

> MarkAgentTab200Response markAgentTab(runId, tabId, markAgentTabRequest)

Nominate the delegate\&#39;s attached tab for handoff (dark; the delegate only)

WS8.F. The service principal that holds a live attachment on this tab, with its attach grant, nominates the tab. The nomination hands the tab to the person at once: its holder becomes human, so the delegate takes no further step on it. A successful release (&#x60;DELETE .../attach/{attach_id}&#x60; with the default &#x60;release&#x60;) then keeps an agent-opened tab open for the owner, if the grant still passes the check made at release; a borrowed tab always goes back to its owner. Both &#x60;mark&#x60; values are accepted and echoed. The ledger keeps the mark on the tab, and the owner projection (&#x60;GET .../browser-session&#x60;, see CONTRACTS.md) shows it as the tab\&#39;s &#x60;mark&#x60;; the two values have the same effect on the holder and on release today. A closed run, a ZDR run, or a tab that is not this delegate\&#39;s attached tab is 404. A grant that is no longer usable for the run is 403 attach_grant_invalid.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { MarkAgentTabOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | A tab id from the run\'s tab list
    tabId: tabId_example,
    // MarkAgentTabRequest
    markAgentTabRequest: ...,
  } satisfies MarkAgentTabOperationRequest;

  try {
    const data = await api.markAgentTab(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **tabId** | `string` | A tab id from the run\&#39;s tab list | [Defaults to `undefined`] |
| **markAgentTabRequest** | [MarkAgentTabRequest](MarkAgentTabRequest.md) |  | |

### Return type

[**MarkAgentTab200Response**](MarkAgentTab200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The tab and the mark that was recorded |  -  |
| **400** | mark is neither deliverable nor handoff (invalid tab mark) |  -  |
| **403** | The attach grant is no longer usable for this run (attach_grant_invalid) |  -  |
| **404** | Run not found, closed or under ZDR, or the tab is not this delegate\&#39;s attached tab |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## openAgentTab

> EnvelopeAgentTab openAgentTab(runId, openAgentTabRequest)

Open a browser tab on a run

Opens a tab (optionally navigated to &#x60;url&#x60;) and records who did it. Subject to the same gates as a step: the run\&#39;s domains, its grant, and the take-over rule. Who is calling comes from the token (a service principal is an agent whatever it sends; a person is a person unless &#x60;initiatedBy&#x60; says &#x60;agent&#x60;), so the hint only tightens. The recorded identity of an agent is on the timeline item and the owner projection; see CONTRACTS.md.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { OpenAgentTabOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // OpenAgentTabRequest (optional)
    openAgentTabRequest: ...,
  } satisfies OpenAgentTabOperationRequest;

  try {
    const data = await api.openAgentTab(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **openAgentTabRequest** | [OpenAgentTabRequest](OpenAgentTabRequest.md) |  | [Optional] |

### Return type

[**EnvelopeAgentTab**](EnvelopeAgentTab.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The new tab and the run\&#39;s tabs |  -  |
| **403** | Domain (including a &#x60;url&#x60; outside the run\&#39;s allowed domains), SSRF, grant or tier refusal |  -  |
| **404** | Run not found, not owned by the caller, or no longer live |  -  |
| **409** | USER_TOOK_OVER: the person using the browser has taken it over and this call is an agent\&#39;s. &#x60;error.details&#x60; is &#x60;{control_mode: \&quot;human_takeover\&quot;, control_epoch, taken_over_at}&#x60; (&#x60;taken_over_at&#x60;, RFC 3339, is absent for a run resumed into a takeover). Enforced only when QUARRY_EDGE__TAKEOVER_ENFORCEMENT is on. |  -  |
| **422** | The body does not parse: both spellings of the hint in one body, or a hint that is neither &#x60;human&#x60; nor &#x60;agent&#x60;. The answer is the HTTP framework\&#39;s plain-text rejection, not an error envelope. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## qualityCheckAgentProcedure

> EnvelopeProcedureQuality qualityCheckAgentProcedure(browserProcedure)

Evaluate deterministic procedure promotion evidence

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { QualityCheckAgentProcedureRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // BrowserProcedure
    browserProcedure: ...,
  } satisfies QualityCheckAgentProcedureRequest;

  try {
    const data = await api.qualityCheckAgentProcedure(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **browserProcedure** | [BrowserProcedure](BrowserProcedure.md) |  | |

### Return type

[**EnvelopeProcedureQuality**](EnvelopeProcedureQuality.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Quality report; does not change rollout state |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## releaseAgentAttach

> releaseAgentAttach(runId, attachId, disposition)

End an agent attachment (the delegate ends its own; the owner stops it)

Closes the agent tab and records the end on the timeline and as agent.detached. Does not revoke the grant.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { ReleaseAgentAttachRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string
    attachId: attachId_example,
    // 'release' | 'abort' (optional)
    disposition: disposition_example,
  } satisfies ReleaseAgentAttachRequest;

  try {
    const data = await api.releaseAgentAttach(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **attachId** | `string` |  | [Defaults to `undefined`] |
| **disposition** | `release`, `abort` |  | [Optional] [Defaults to `undefined`] [Enum: release, abort] |

### Return type

`void` (Empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: Not defined


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The attachment ended |  -  |
| **400** | Unknown disposition |  -  |
| **404** | Not the owner, not the live attachment\&#39;s delegate, or no such attachment |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## selectAgentTab

> EnvelopeAgentTabs selectAgentTab(runId, tabId, initiatedBy, aiRunId, agentLabel)

Make a tab the active one

Has no body, so the hint and the advisory fields are query parameters. Same take-over rule and recording as opening a tab.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { SelectAgentTabRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | A tab id from the run\'s tab list
    tabId: tabId_example,
    // 'human' | 'agent' | Who is making the call. Can only tighten: `agent` on a person\'s token makes it an agent\'s, and a service principal is always an agent. `initiated_by` is accepted too (both in one request is an error). (optional)
    initiatedBy: initiatedBy_example,
    // string | ADVISORY. The AI run the caller says it acts for. Recorded, never trusted. `ai_run_id` is accepted too. (optional)
    aiRunId: aiRunId_example,
    // string | ADVISORY. A short label the caller gave its agent. Recorded, never trusted. `agent_label` is accepted too. (optional)
    agentLabel: agentLabel_example,
  } satisfies SelectAgentTabRequest;

  try {
    const data = await api.selectAgentTab(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **tabId** | `string` | A tab id from the run\&#39;s tab list | [Defaults to `undefined`] |
| **initiatedBy** | `human`, `agent` | Who is making the call. Can only tighten: &#x60;agent&#x60; on a person\&#39;s token makes it an agent\&#39;s, and a service principal is always an agent. &#x60;initiated_by&#x60; is accepted too (both in one request is an error). | [Optional] [Defaults to `undefined`] [Enum: human, agent] |
| **aiRunId** | `string` | ADVISORY. The AI run the caller says it acts for. Recorded, never trusted. &#x60;ai_run_id&#x60; is accepted too. | [Optional] [Defaults to `undefined`] |
| **agentLabel** | `string` | ADVISORY. A short label the caller gave its agent. Recorded, never trusted. &#x60;agent_label&#x60; is accepted too. | [Optional] [Defaults to `undefined`] |

### Return type

[**EnvelopeAgentTabs**](EnvelopeAgentTabs.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The run\&#39;s tabs |  -  |
| **400** | A hint that is not &#x60;human&#x60; or &#x60;agent&#x60; |  -  |
| **404** | Run not found, not owned by the caller, or no longer live |  -  |
| **409** | USER_TOOK_OVER, as for opening a tab |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## setAgentBrowserSessionControl

> GetAgentBrowserSession200Response setAgentBrowserSessionControl(runId, browserSessionControlRequest)

Hand the run browser to the person, or back to the agent

Sets who may drive the run\&#39;s browser. &#x60;human_takeover&#x60; gives it to the person, so an action that an agent proposes is refused with 409 USER_TOOK_OVER, and that refusal is enforced only while QUARRY_EDGE__TAKEOVER_ENFORCEMENT is on. &#x60;agent_control&#x60; gives it back. Only a person may make the change: a service principal\&#39;s token gets 403 control_transition_requires_human, and an API-key token is refused by the route fence (api_key_route_refused). The change does not wait for an action that is still running. The control epoch advances only on a real change, so a request for the mode the run already has answers 200 and changes nothing. &#x60;expected_epoch&#x60;, when sent, makes the change a compare-and-set: while the run\&#39;s epoch is another number, the answer is 409 CONFLICT with &#x60;details.control_epoch&#x60; and &#x60;details.expected_epoch&#x60;. A real change on a run that is not ZDR writes one control item on the owner timeline. Every real change emits a &#x60;browser.control&#x60; event on the run\&#39;s trace, and on a ZDR run that event reaches live subscribers only, with nothing durable written. A takeover pauses the run\&#39;s budget clock, and handing the browser back resumes it. The answer is the owner projection, as &#x60;GET .../browser-session&#x60; gives it. Gate: none as a route. Only the run\&#39;s owner may call it; any other caller, and a run the edge does not hold, gets 404.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { SetAgentBrowserSessionControlRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // BrowserSessionControlRequest
    browserSessionControlRequest: ...,
  } satisfies SetAgentBrowserSessionControlRequest;

  try {
    const data = await api.setAgentBrowserSessionControl(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **browserSessionControlRequest** | [BrowserSessionControlRequest](BrowserSessionControlRequest.md) |  | |

### Return type

[**GetAgentBrowserSession200Response**](GetAgentBrowserSession200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The owner projection, after the change |  -  |
| **400** | The body is not JSON (the framework\&#39;s plain-text rejection) |  -  |
| **403** | control_transition_requires_human for an agent\&#39;s token, or api_key_route_refused for an API-key token |  -  |
| **404** | The edge holds no such run, or the caller does not own it |  -  |
| **409** | CONFLICT when expected_epoch is not the run\&#39;s control epoch (details carry control_epoch and expected_epoch) |  -  |
| **415** | The request is not sent as application/json (the framework\&#39;s plain-text rejection) |  -  |
| **422** | The body is JSON but not a control request, for example an unknown mode (the framework\&#39;s plain-text rejection) |  -  |
| **502** | DRIVER_FAILED, when the driver cannot list the tabs after the change, or the change cannot be recorded (a timeout answers 504) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## setAgentTabHolder

> setAgentTabHolder(runId, tabId, setAgentTabHolderRequest)

Hand a tab to the agent or take it back (the owner\&#39;s own token only)

holder agent needs a live attachment on this tab (no_attached_agent, 409). holder human takes the tab back. Compare-and-set on the tab epoch: a stale expected_epoch is stale_tab_epoch (409). A change writes a tab_control timeline item.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { SetAgentTabHolderOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | A tab id from the run\'s tab list
    tabId: tabId_example,
    // SetAgentTabHolderRequest
    setAgentTabHolderRequest: ...,
  } satisfies SetAgentTabHolderOperationRequest;

  try {
    const data = await api.setAgentTabHolder(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **tabId** | `string` | A tab id from the run\&#39;s tab list | [Defaults to `undefined`] |
| **setAgentTabHolderRequest** | [SetAgentTabHolderRequest](SetAgentTabHolderRequest.md) |  | |

### Return type

`void` (Empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: Not defined


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The tab\&#39;s holder and epochs |  -  |
| **403** | A service principal or an agent token may not change the holder (control_transition_requires_human) |  -  |
| **404** | Run not found, or the tab is closed (pinned_tab_closed) |  -  |
| **409** | Stale epoch (stale_tab_epoch) or no live attachment (no_attached_agent) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## startAgentRun

> EnvelopeStartAgentRun startAgentRun(startAgentRunRequest)

Acquire a governed browser-agent run

Creates a tenant-owned browser lease. BrowserBroker grants are revalidated at run start and before every action; non-ZDR runs receive a durable continuation checkpoint. API-key callers (INNH-10, off until &#x60;QUARRY_EDGE__API_KEY_AGENT_RUNS&#x60; is on): the bearer is a short-lived token of the &#x60;api_key&#x60; principal type (&#x60;sub&#x60; &#x60;apikey:&lt;key_id&gt;&#x60;, scope &#x60;browser:agent&#x60;) that Auth Core minted for the key; the key itself never reaches Quarry. A key run needs a &#x60;grant_id&#x60; in every environment, issued to that key (an allow-list session grant with hosts and a budget; its hosts replace &#x60;constraints.allowed_domains&#x60;), takes no &#x60;resume_run_id&#x60;, &#x60;profile_id&#x60; or &#x60;persist_profile&#x60;, is always an agent run, and may call only the routes of the key allow-list (see CONTRACTS.md, \&quot;API-key agent runs\&quot;). The refusals are 403 with &#x60;error.details.reason&#x60; &#x60;api_key_*&#x60; and 429 &#x60;per_key_run_cap&#x60;; see ERROR_REASONS.md.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { StartAgentRunOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // StartAgentRunRequest
    startAgentRunRequest: ...,
  } satisfies StartAgentRunOperationRequest;

  try {
    const data = await api.startAgentRun(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **startAgentRunRequest** | [StartAgentRunRequest](StartAgentRunRequest.md) |  | |

### Return type

[**EnvelopeStartAgentRun**](EnvelopeStartAgentRun.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Browser run acquired |  -  |
| **400** | Invalid constraints or persistence request |  -  |
| **403** | Missing or invalid BrowserBroker grant, or &#x60;policy_denied&#x60;: the request\&#39;s &#x60;policy&#x60; or the operator baseline refuses a capability the run asked for (&#x60;error.details.reason&#x60; &#x60;policy_denied&#x60;, with &#x60;capability&#x60;, &#x60;decision_source&#x60; and &#x60;retryable: false&#x60;). Nothing is acquired. A person\&#39;s &#x60;human_browse&#x60; grant (WS8.B; refused unless &#x60;QUARRY_EDGE__HUMAN_BROWSE&#x60; is on) is refused with 403 and the reason as the message: &#x60;human_browse_disabled&#x60;, &#x60;human_browse_service_refused&#x60;, &#x60;human_browse_owner_mismatch&#x60;, &#x60;human_browse_zdr_posture&#x60; or &#x60;human_browse_policy_unknown&#x60;. A resumed run whose stored policy mode differs is refused with \&quot;resumed run policy no longer matches its broker grant\&quot;. An API-key caller (INNH-10) is refused with &#x60;error.details.reason&#x60; one of &#x60;api_key_agent_runs_disabled&#x60;, &#x60;api_key_scope_missing&#x60;, &#x60;api_key_route_refused&#x60;, &#x60;api_key_grant_required&#x60;, &#x60;api_key_grant_kind_refused&#x60;, &#x60;api_key_open_web_refused&#x60;, &#x60;api_key_grant_owner_mismatch&#x60;, &#x60;api_key_grant_budget_required&#x60;, &#x60;api_key_grant_hosts_required&#x60;, &#x60;api_key_profile_refused&#x60;, &#x60;api_key_resume_refused&#x60; or &#x60;api_key_budget_not_enforced&#x60;. |  -  |
| **429** | RATE_LIMITED. A run cap is full: &#x60;error.details.reason&#x60; is &#x60;global_run_cap&#x60;, &#x60;org_run_cap&#x60;, &#x60;org_interactive_run_cap&#x60;, &#x60;org_plan_cap&#x60; (the organization\&#39;s plan bounds this class of run) or &#x60;per_key_run_cap&#x60; (an API key has its maximum of live runs), with &#x60;scope&#x60;, &#x60;limit&#x60;, &#x60;active&#x60; and &#x60;retry_after_seconds&#x60;. Retryable. See ERROR_REASONS.md. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## streamAgentLiveFrames

> string streamAgentLiveFrames(runId, tabId, format, quality, maxWidth, maxHeight, everyNthFrame, timeoutMs, intervalMs, maxFrames, xQuarryLiveScope)

Transient live frames of the run browser as Server-Sent Events

A stream of transient frames of the run\&#39;s current page, for live preview. Each &#x60;frame&#x60; event carries one frame, and its SSE &#x60;id&#x60; is the frame\&#39;s &#x60;sequence&#x60;, counted from 1. Frames are never persisted: the stream writes no artifact, receipt, timeline item or checkpoint, and it is a read-only visual channel, since actions still go through &#x60;POST .../step&#x60;. Without &#x60;tabId&#x60; the stream opens with one &#x60;live_view&#x60; event when the driver has a live-view reference. That event is the same object as &#x60;GET .../live-view&#x60;, and under a view-only scope only a read-only kind is sent. Frames then come every &#x60;intervalMs&#x60;. &#x60;maxFrames&#x60; ends the stream with &#x60;done&#x60; &#x60;max_frames&#x60; after that many frames. The stream ends with &#x60;done&#x60; &#x60;shutdown&#x60; when the edge drains. A driver failure sends one &#x60;error&#x60; event, whose data is the error\&#39;s text and not a wire code, and then ends the stream. &#x60;agentCursor&#x60; appears only on a &#x60;tabId&#x60; stream of a tab that an agent holds through a live attachment, which needs QUARRY_EDGE__ATTACH. Gate: none as a route, and the frame options and scope are those of &#x60;GET .../frame&#x60;. The owner, gating, scope and option rules are checked before the stream opens, and only the run\&#39;s owner may open it; any other caller gets 404.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { StreamAgentLiveFramesRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // string | One of the run\'s tabs, read without being selected. The driver must be able to pin a tab. A tab the run does not hold is 404. A WebSocket that names a tab is view-only. (optional)
    tabId: tabId_example,
    // 'jpeg' | 'jpg' | 'png' | `jpeg` (also `jpg`), the default, or `png`. Any other value is 400. (optional)
    format: format_example,
    // number | JPEG or PNG quality, clamped to 1 to 100. Default 65. (optional)
    quality: 56,
    // number | Width of the frame, clamped to at least 320 and to the run\'s viewport width. Default the viewport width, which is 1280 when the lease names none. (optional)
    maxWidth: 56,
    // number | Height of the frame, clamped to at least 240 and to the run\'s viewport height. Default the viewport height, which is 800 when the lease names none. (optional)
    maxHeight: 56,
    // number | The screencast\'s frame thinning, clamped to 1 to 10. Default 1. (optional)
    everyNthFrame: 56,
    // number | How long the driver may take to produce a frame, clamped to 100 to 5000 milliseconds. Default 1000. (optional)
    timeoutMs: 56,
    // number | Milliseconds between frames, clamped to 75 to 2000. Default 250. Frame stream and WebSocket only. (optional)
    intervalMs: 56,
    // number | Frames before the stream or socket ends with done (max_frames). 0 or absent is no cap, and a larger value is capped at 7200. Frame stream and WebSocket only. (optional)
    maxFrames: 56,
    // string | `full` (the default) or `view_only`; `view-only` and `view` are read the same way, ignoring case. The header is read only while QUARRY_EDGE__LIVE_VIEW_ONLY is on, and it is ignored while that switch is off. A view-only caller gets no input and no interactive live-view reference, and a view-only share of a ZDR run is refused with 403. Any other value is 400. The Verevon gateway sets it from its own route and strips a value that a client sends. A caller can only narrow the scope with it, never widen it. (optional)
    xQuarryLiveScope: xQuarryLiveScope_example,
  } satisfies StreamAgentLiveFramesRequest;

  try {
    const data = await api.streamAgentLiveFrames(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **tabId** | `string` | One of the run\&#39;s tabs, read without being selected. The driver must be able to pin a tab. A tab the run does not hold is 404. A WebSocket that names a tab is view-only. | [Optional] [Defaults to `undefined`] |
| **format** | `jpeg`, `jpg`, `png` | &#x60;jpeg&#x60; (also &#x60;jpg&#x60;), the default, or &#x60;png&#x60;. Any other value is 400. | [Optional] [Defaults to `undefined`] [Enum: jpeg, jpg, png] |
| **quality** | `number` | JPEG or PNG quality, clamped to 1 to 100. Default 65. | [Optional] [Defaults to `undefined`] |
| **maxWidth** | `number` | Width of the frame, clamped to at least 320 and to the run\&#39;s viewport width. Default the viewport width, which is 1280 when the lease names none. | [Optional] [Defaults to `undefined`] |
| **maxHeight** | `number` | Height of the frame, clamped to at least 240 and to the run\&#39;s viewport height. Default the viewport height, which is 800 when the lease names none. | [Optional] [Defaults to `undefined`] |
| **everyNthFrame** | `number` | The screencast\&#39;s frame thinning, clamped to 1 to 10. Default 1. | [Optional] [Defaults to `undefined`] |
| **timeoutMs** | `number` | How long the driver may take to produce a frame, clamped to 100 to 5000 milliseconds. Default 1000. | [Optional] [Defaults to `undefined`] |
| **intervalMs** | `number` | Milliseconds between frames, clamped to 75 to 2000. Default 250. Frame stream and WebSocket only. | [Optional] [Defaults to `undefined`] |
| **maxFrames** | `number` | Frames before the stream or socket ends with done (max_frames). 0 or absent is no cap, and a larger value is capped at 7200. Frame stream and WebSocket only. | [Optional] [Defaults to `undefined`] |
| **xQuarryLiveScope** | `string` | &#x60;full&#x60; (the default) or &#x60;view_only&#x60;; &#x60;view-only&#x60; and &#x60;view&#x60; are read the same way, ignoring case. The header is read only while QUARRY_EDGE__LIVE_VIEW_ONLY is on, and it is ignored while that switch is off. A view-only caller gets no input and no interactive live-view reference, and a view-only share of a ZDR run is refused with 403. Any other value is 400. The Verevon gateway sets it from its own route and strips a value that a client sends. A caller can only narrow the scope with it, never widen it. | [Optional] [Defaults to `undefined`] |

### Return type

**string**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `text/event-stream`, `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | text/event-stream. Named events: live_view (data is a LiveViewRef, first, and only without tabId), frame (data is LiveFrameStreamData, id is sequence), error (data is the error\&#39;s text; the stream ends) and done (data is max_frames or shutdown). |  -  |
| **400** | A format other than jpeg, jpg or png, a scope value other than full or view_only, an empty tabId or one over 128 characters, or a tabId on a driver that cannot pin tabs |  -  |
| **403** | A view-only share of a ZDR run (live_view_only_zdr, while QUARRY_EDGE__LIVE_VIEW_ONLY is on), or an API-key token on a route that its key may not use (api_key_route_refused) |  -  |
| **404** | The edge holds no such run, or the caller does not own it; tab not found when tabId is not one of the run\&#39;s tabs |  -  |
| **501** | UNSUPPORTED, when the selected driver cannot produce governed full-fidelity live frames |  -  |
| **502** | DRIVER_FAILED, when the tab cannot be looked up before the stream opens (a timeout answers 504) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## streamAgentRunEvents

> string streamAgentRunEvents(runId, fromSeq, maxEvents, lastEventID)

Typed agent-run events as Server-Sent Events, with resume (WS10.S3)

Streams the run\&#39;s user-facing events (agent.started, action.started, action.completed, action.failed, observation.ready, agent.completed, agent.expired, agent.budget_exhausted, agent.failed) in the order of the run\&#39;s trace, and the browser trace events (WS10.S4): browser.session.opened, browser.session.closed, browser.control, browser.tab, browser.receipt, browser.egress and browser.artifact. The browser events are agent class and never reach the Ingestion bus (see CONTRACTS.md, \&quot;Agent run trace: browser events and run origin\&quot;). Each SSE &#x60;id:&#x60; is the event\&#39;s per-run &#x60;seq&#x60; (dense, starting at 1 for an agent run). Resume with the &#x60;Last-Event-ID&#x60; header a reconnecting EventSource sends, or with &#x60;from_seq&#x60;; &#x60;from_seq&#x60; wins when both are present. The stream replays the run\&#39;s in-memory replay ring after that seq, then tails the live channel with no gap and no duplicate. The ring holds &#x60;QUARRY_EDGE__TRACE_RING_EVENTS&#x60; events (default 512; 0 disables it) within &#x60;QUARRY_EDGE__TRACE_RING_MAX_BYTES&#x60; (default 1 MiB) and lives only as long as the run. A &#x60;replay_gap&#x60; event comes first when the ring no longer holds the events after the resume point; its fallback is the durable browser-session timeline. The stream ends with &#x60;event: done&#x60; and &#x60;data: run_closed&#x60; when the run closes, or &#x60;data: max_events&#x60; when &#x60;max_events&#x60; is reached. A slow consumer gets &#x60;event: lagged&#x60; and the stream resumes from the ring. Only the owner of the run may read it; any other caller gets 404.

### Example

```ts
import {
  Configuration,
  AgentApi,
} from '@quarry/client';
import type { StreamAgentRunEventsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new AgentApi();

  const body = {
    // string | Tenant-owned Quarry run identifier
    runId: runId_example,
    // number | Replay the retained events with seq greater than this value before the live tail. Wins over Last-Event-ID. (optional)
    fromSeq: 56,
    // number | Close the stream after this many user-facing events (0 or absent, no cap). (optional)
    maxEvents: 56,
    // number | The seq of the last event the client received. Used when from_seq is absent. (optional)
    lastEventID: 56,
  } satisfies StreamAgentRunEventsRequest;

  try {
    const data = await api.streamAgentRunEvents(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **runId** | `string` | Tenant-owned Quarry run identifier | [Defaults to `undefined`] |
| **fromSeq** | `number` | Replay the retained events with seq greater than this value before the live tail. Wins over Last-Event-ID. | [Optional] [Defaults to `undefined`] |
| **maxEvents** | `number` | Close the stream after this many user-facing events (0 or absent, no cap). | [Optional] [Defaults to `undefined`] |
| **lastEventID** | `number` | The seq of the last event the client received. Used when from_seq is absent. | [Optional] [Defaults to `undefined`] |

### Return type

**string**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `text/event-stream`, `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | text/event-stream. Named events: agent.started, action.started, action.completed, action.failed, observation.ready, agent.completed, agent.expired, agent.budget_exhausted, agent.failed (data is the Event envelope, id is seq), replay_gap (data is {requested, oldest_retained, fallback}), lagged (data is {skipped}), done (data is run_closed or max_events). |  -  |
| **404** | Run not found, or not owned by the caller |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

