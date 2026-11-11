# WebhooksApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**createWebhookSubscription**](WebhooksApi.md#createwebhooksubscription) | **POST** /v1/webhook-subscriptions | Subscribe an HTTPS endpoint to change events (INNH-5) |
| [**deleteWebhookSubscription**](WebhooksApi.md#deletewebhooksubscription) | **DELETE** /v1/webhook-subscriptions/{subscription_id} | Delete a subscription and its delivery log (INNH-5) |
| [**getWebhookSubscription**](WebhooksApi.md#getwebhooksubscription) | **GET** /v1/webhook-subscriptions/{subscription_id} | Read one webhook subscription (INNH-5) |
| [**listWebhookDeliveries**](WebhooksApi.md#listwebhookdeliveries) | **GET** /v1/webhook-subscriptions/{subscription_id}/deliveries | The delivery log of a subscription (INNH-5) |
| [**listWebhookSubscriptions**](WebhooksApi.md#listwebhooksubscriptions) | **GET** /v1/webhook-subscriptions | List the organization\&#39;s webhook subscriptions (INNH-5) |
| [**retryWebhookDelivery**](WebhooksApi.md#retrywebhookdelivery) | **POST** /v1/webhook-subscriptions/{subscription_id}/deliveries/{delivery_id}/retry | Re-queue a failed delivery (INNH-5) |
| [**rotateWebhookSubscriptionSecret**](WebhooksApi.md#rotatewebhooksubscriptionsecret) | **POST** /v1/webhook-subscriptions/{subscription_id}/rotate-secret | Mint a new secret for a subscription (INNH-5) |
| [**updateWebhookSubscription**](WebhooksApi.md#updatewebhooksubscription) | **PATCH** /v1/webhook-subscriptions/{subscription_id} | Pause, resume or change the events of a subscription (INNH-5) |



## createWebhookSubscription

> CreateWebhookSubscription201Response createWebhookSubscription(createWebhookSubscription)

Subscribe an HTTPS endpoint to change events (INNH-5)

Registers a webhook for the caller\&#39;s organization. Quarry sends the endpoint a signed &#x60;POST&#x60; for every event type named in &#x60;events&#x60;; v1 has one: &#x60;change_detected&#x60; (the stored event type; the &#x60;payload.subject&#x60; of the delivery stays &#x60;quarry.change.detected&#x60;). The delivery format, the signature and the retry rules are in &#x60;docs/CONTRACTS.md&#x60;, section 4.  The response carries the HMAC &#x60;secret&#x60; (&#x60;whsec_&#x60; and 64 hex characters). It is shown here once and by &#x60;rotate-secret&#x60;; no read returns it again, only &#x60;secret_hint&#x60;.  The URL must be &#x60;https&#x60;, carry no credentials or fragment, be at most 2048 characters, and resolve only to public addresses (a private, loopback, link-local, metadata or carrier-grade-NAT address is refused; every delivery is vetted again at connect time). An organization holds at most 10 subscriptions. Unknown body fields are a 400. The optional &#x60;notify_on_change&#x60; narrows which change types the subscription takes (&#x60;NotifyOnChangeFilter&#x60;).  Off unless the edge runs with &#x60;QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS&#x60; on and control with &#x60;QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS&#x60; on (501 otherwise). A Zero Data Retention caller gets 403 (&#x60;details.reason&#x60; is &#x60;webhook_off_process&#x60;): a subscription pushes page URLs off-process. 

### Example

```ts
import {
  Configuration,
  WebhooksApi,
} from '@quarry/client';
import type { CreateWebhookSubscriptionRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new WebhooksApi();

  const body = {
    // CreateWebhookSubscription
    createWebhookSubscription: ...,
  } satisfies CreateWebhookSubscriptionRequest;

  try {
    const data = await api.createWebhookSubscription(body);
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
| **createWebhookSubscription** | [CreateWebhookSubscription](CreateWebhookSubscription.md) |  | |

### Return type

[**CreateWebhookSubscription201Response**](CreateWebhookSubscription201Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **201** | The subscription and its secret, once. |  -  |
| **400** | A field is invalid. &#x60;error.details.field&#x60; is &#x60;url&#x60;, &#x60;events&#x60;, &#x60;description&#x60; or &#x60;body&#x60;; &#x60;error.details.reason&#x60; is one of &#x60;url_invalid&#x60;, &#x60;url_too_long&#x60;, &#x60;url_not_https&#x60;, &#x60;url_userinfo&#x60;, &#x60;url_fragment&#x60;, &#x60;url_private_address&#x60;, &#x60;url_unresolvable&#x60;, &#x60;events_empty&#x60;, &#x60;events_unsupported&#x60;, &#x60;description_too_long&#x60;, &#x60;body_invalid&#x60;. |  -  |
| **403** | The token\&#39;s ZDR floor is set (&#x60;error.details.reason&#x60; is &#x60;webhook_off_process&#x60;), or the caller is a service principal. |  -  |
| **409** | The organization already has a subscription with this URL and these events (&#x60;error.details.reason&#x60; is &#x60;subscription_exists&#x60;). |  -  |
| **429** | The organization holds the maximum of 10 subscriptions (&#x60;error.details.reason&#x60; is &#x60;subscription_limit&#x60;, &#x60;error.details.limit&#x60; is 10). |  -  |
| **501** | Not enabled (QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS on the edge, QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS on control; the message names the one that is off). |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## deleteWebhookSubscription

> deleteWebhookSubscription(subscriptionId)

Delete a subscription and its delivery log (INNH-5)

A hard delete. Pending deliveries of the subscription are dropped with it. Open to a Zero Data Retention caller (wind-down).

### Example

```ts
import {
  Configuration,
  WebhooksApi,
} from '@quarry/client';
import type { DeleteWebhookSubscriptionRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new WebhooksApi();

  const body = {
    // string | A webhook subscription id (`whk_...`) of the caller\'s organization
    subscriptionId: subscriptionId_example,
  } satisfies DeleteWebhookSubscriptionRequest;

  try {
    const data = await api.deleteWebhookSubscription(body);
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
| **subscriptionId** | `string` | A webhook subscription id (&#x60;whk_...&#x60;) of the caller\&#39;s organization | [Defaults to `undefined`] |

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
| **204** | Deleted. No body. |  -  |
| **404** | Unknown or foreign id. |  -  |
| **501** | Not enabled. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getWebhookSubscription

> GetWebhookSubscription200Response getWebhookSubscription(subscriptionId)

Read one webhook subscription (INNH-5)

### Example

```ts
import {
  Configuration,
  WebhooksApi,
} from '@quarry/client';
import type { GetWebhookSubscriptionRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new WebhooksApi();

  const body = {
    // string | A webhook subscription id (`whk_...`) of the caller\'s organization
    subscriptionId: subscriptionId_example,
  } satisfies GetWebhookSubscriptionRequest;

  try {
    const data = await api.getWebhookSubscription(body);
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
| **subscriptionId** | `string` | A webhook subscription id (&#x60;whk_...&#x60;) of the caller\&#39;s organization | [Defaults to `undefined`] |

### Return type

[**GetWebhookSubscription200Response**](GetWebhookSubscription200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The subscription. No secret. |  -  |
| **404** | Unknown id, an id of another organization (the same answer), or a malformed id. |  -  |
| **501** | Not enabled. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listWebhookDeliveries

> ListWebhookDeliveries200Response listWebhookDeliveries(subscriptionId, status, limit, cursor)

The delivery log of a subscription (INNH-5)

Newest first. A delivery shows its status and attempt count, never the payload or the raw error: &#x60;last_error&#x60; is &#x60;http_&lt;status&gt;&#x60;, &#x60;timeout&#x60;, &#x60;blocked_target&#x60;, &#x60;transport&#x60;, &#x60;inactive&#x60; or &#x60;error&#x60;. A delivery is &#x60;dlq&#x60; after five failed attempts or at once on a 4xx other than 408 and 429. Finished deliveries are deleted after 14 days. 

### Example

```ts
import {
  Configuration,
  WebhooksApi,
} from '@quarry/client';
import type { ListWebhookDeliveriesRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new WebhooksApi();

  const body = {
    // string | A webhook subscription id (`whk_...`) of the caller\'s organization
    subscriptionId: subscriptionId_example,
    // 'pending' | 'in_flight' | 'success' | 'failed' | 'dlq' (optional)
    status: status_example,
    // number (optional)
    limit: 56,
    // string (optional)
    cursor: cursor_example,
  } satisfies ListWebhookDeliveriesRequest;

  try {
    const data = await api.listWebhookDeliveries(body);
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
| **subscriptionId** | `string` | A webhook subscription id (&#x60;whk_...&#x60;) of the caller\&#39;s organization | [Defaults to `undefined`] |
| **status** | `pending`, `in_flight`, `success`, `failed`, `dlq` |  | [Optional] [Defaults to `undefined`] [Enum: pending, in_flight, success, failed, dlq] |
| **limit** | `number` |  | [Optional] [Defaults to `50`] |
| **cursor** | `string` |  | [Optional] [Defaults to `undefined`] |

### Return type

[**ListWebhookDeliveries200Response**](ListWebhookDeliveries200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | One page of deliveries. |  -  |
| **400** | A bad &#x60;status&#x60;, &#x60;limit&#x60; or &#x60;cursor&#x60; (&#x60;error.details.field&#x60;). |  -  |
| **404** | Unknown or foreign subscription id. |  -  |
| **501** | Not enabled. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listWebhookSubscriptions

> ListWebhookSubscriptions200Response listWebhookSubscriptions(limit, cursor)

List the organization\&#39;s webhook subscriptions (INNH-5)

Newest first. Never returns a secret. Each item carries the status of its latest delivery in &#x60;last_delivery&#x60;.

### Example

```ts
import {
  Configuration,
  WebhooksApi,
} from '@quarry/client';
import type { ListWebhookSubscriptionsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new WebhooksApi();

  const body = {
    // number (optional)
    limit: 56,
    // string | The `next_cursor` of the previous page. (optional)
    cursor: cursor_example,
  } satisfies ListWebhookSubscriptionsRequest;

  try {
    const data = await api.listWebhookSubscriptions(body);
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
| **limit** | `number` |  | [Optional] [Defaults to `50`] |
| **cursor** | `string` | The &#x60;next_cursor&#x60; of the previous page. | [Optional] [Defaults to `undefined`] |

### Return type

[**ListWebhookSubscriptions200Response**](ListWebhookSubscriptions200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | One page of subscriptions. |  -  |
| **400** | A bad &#x60;limit&#x60; or &#x60;cursor&#x60; (&#x60;error.details.field&#x60;). |  -  |
| **501** | Not enabled. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## retryWebhookDelivery

> RetryWebhookDelivery202Response retryWebhookDelivery(subscriptionId, deliveryId)

Re-queue a failed delivery (INNH-5)

Only a &#x60;failed&#x60; or &#x60;dlq&#x60; delivery can be retried (409 &#x60;delivery_not_retryable&#x60; otherwise). The attempt count restarts at zero. Refused for a Zero Data Retention caller.

### Example

```ts
import {
  Configuration,
  WebhooksApi,
} from '@quarry/client';
import type { RetryWebhookDeliveryRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new WebhooksApi();

  const body = {
    // string | A webhook subscription id (`whk_...`) of the caller\'s organization
    subscriptionId: subscriptionId_example,
    // string
    deliveryId: deliveryId_example,
  } satisfies RetryWebhookDeliveryRequest;

  try {
    const data = await api.retryWebhookDelivery(body);
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
| **subscriptionId** | `string` | A webhook subscription id (&#x60;whk_...&#x60;) of the caller\&#39;s organization | [Defaults to `undefined`] |
| **deliveryId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**RetryWebhookDelivery202Response**](RetryWebhookDelivery202Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **202** | The delivery, now pending. |  -  |
| **403** | Zero Data Retention (&#x60;webhook_off_process&#x60;), or a service principal. |  -  |
| **404** | Unknown or foreign subscription or delivery id. |  -  |
| **409** | The delivery is pending, in flight or delivered (&#x60;error.details.reason&#x60; is &#x60;delivery_not_retryable&#x60;). |  -  |
| **501** | Not enabled. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## rotateWebhookSubscriptionSecret

> RotateWebhookSubscriptionSecret200Response rotateWebhookSubscriptionSecret(subscriptionId)

Mint a new secret for a subscription (INNH-5)

The old secret stops working at once; deliveries already queued are signed with the new one. The new secret is in this response only.

### Example

```ts
import {
  Configuration,
  WebhooksApi,
} from '@quarry/client';
import type { RotateWebhookSubscriptionSecretRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new WebhooksApi();

  const body = {
    // string | A webhook subscription id (`whk_...`) of the caller\'s organization
    subscriptionId: subscriptionId_example,
  } satisfies RotateWebhookSubscriptionSecretRequest;

  try {
    const data = await api.rotateWebhookSubscriptionSecret(body);
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
| **subscriptionId** | `string` | A webhook subscription id (&#x60;whk_...&#x60;) of the caller\&#39;s organization | [Defaults to `undefined`] |

### Return type

[**RotateWebhookSubscriptionSecret200Response**](RotateWebhookSubscriptionSecret200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The new secret, once. |  -  |
| **404** | Unknown or foreign id. |  -  |
| **501** | Not enabled. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## updateWebhookSubscription

> GetWebhookSubscription200Response updateWebhookSubscription(subscriptionId, updateWebhookSubscription)

Pause, resume or change the events of a subscription (INNH-5)

Names at least one of &#x60;active&#x60;, &#x60;events&#x60;, &#x60;description&#x60;, &#x60;notify_on_change&#x60;. A &#x60;null&#x60; &#x60;notify_on_change&#x60; removes the filter. The URL is not editable: delete the subscription and create a new one, so every target goes through the create-time check. Refused for a Zero Data Retention caller (403, &#x60;webhook_off_process&#x60;), because it can resume pushing. 

### Example

```ts
import {
  Configuration,
  WebhooksApi,
} from '@quarry/client';
import type { UpdateWebhookSubscriptionRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new WebhooksApi();

  const body = {
    // string | A webhook subscription id (`whk_...`) of the caller\'s organization
    subscriptionId: subscriptionId_example,
    // UpdateWebhookSubscription
    updateWebhookSubscription: ...,
  } satisfies UpdateWebhookSubscriptionRequest;

  try {
    const data = await api.updateWebhookSubscription(body);
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
| **subscriptionId** | `string` | A webhook subscription id (&#x60;whk_...&#x60;) of the caller\&#39;s organization | [Defaults to `undefined`] |
| **updateWebhookSubscription** | [UpdateWebhookSubscription](UpdateWebhookSubscription.md) |  | |

### Return type

[**GetWebhookSubscription200Response**](GetWebhookSubscription200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The subscription after the change. |  -  |
| **400** | No field named (&#x60;reason&#x60; is &#x60;empty_patch&#x60;), an unsupported event, a description over 200 characters, a &#x60;notify_on_change&#x60; refused for &#x60;notify_types_empty&#x60;, &#x60;notify_type_unsupported&#x60; or &#x60;notify_min_delta_out_of_range&#x60;, or an unknown field (&#x60;body_invalid&#x60;). |  -  |
| **403** | Zero Data Retention (&#x60;error.details.reason&#x60; is &#x60;webhook_off_process&#x60;), or a service principal. |  -  |
| **404** | Unknown or foreign id. |  -  |
| **501** | Not enabled. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

