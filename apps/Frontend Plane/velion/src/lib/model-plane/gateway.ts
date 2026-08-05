// Typed client wrapper around the model-gateway gRPC surface.
//
// Generated message types live under `./gen/model_plane/v1/`; this
// file ties them together with a Connect-RPC transport so callers can
// just do `await gateway.webSearch({...})`.
//
// We deliberately keep this file thin — no auth wrapping, no
// retry/back-off, no telemetry. Pages that need cross-cutting concerns
// should layer them via tanstack-query / SWR. For one-off calls inside
// a server action the bare client is the right shape.
//
// Transport: Connect-RPC over HTTP, hitting the gateway's gRPC port.
// In Docker/compose the URL is the cluster-internal service name
// (`http://model-gateway:9090`); in dev the gateway exposes :9090 on
// the host directly.
//
// Note: not every RPC in `ModelGatewayService` is re-exported here —
// only the Wave 9/10 surfaces verevon actually touches. Add re-exports
// as needed; the typed client supports them all out of the box.

import { createClient, type Client } from "@connectrpc/connect";
import { createConnectTransport } from "@connectrpc/connect-web";

// connect-es v2 `createClient` consumes the protobuf-es v2 `GenService`
// descriptor from the generated `_pb` module — not the legacy `_connect`
// `ServiceType` (which v1's `createPromiseClient` used).
import { ModelGateway } from "./gen/model_plane/v1/gateway_pb.js";

const DEFAULT_BASE_URL =
  process.env.MODEL_GATEWAY_URL ?? "http://localhost:9090";

let _client: Client<typeof ModelGateway> | null = null;

/**
 * Lazily-constructed model-gateway client. Cached at module scope so
 * repeated calls within one Node process share a connection pool.
 *
 * Override the URL by setting `MODEL_GATEWAY_URL` in the env. The
 * client throws if the URL isn't a valid http(s) endpoint at first
 * call; we don't pre-validate at import time so this module can be
 * imported during build (where the env may be empty).
 */
export function gatewayClient(): Client<typeof ModelGateway> {
  if (_client) return _client;
  const transport = createConnectTransport({
    baseUrl: DEFAULT_BASE_URL,
    // grpc-web works against tonic without extra config; the gateway
    // serves it on the same port as native gRPC. If we ever need
    // streaming RPCs from verevon we may need to switch this to the
    // `useHttpGet`/`useBinaryFormat` knobs — for unary it's irrelevant.
    useBinaryFormat: true,
  });
  _client = createClient(ModelGateway, transport);
  return _client;
}

/**
 * Convenience re-export of the service definition. Lets callers do
 *
 *     import { ModelGateway } from "@/lib/model-plane/gateway";
 *
 * if they need the lower-level Connect APIs (interceptors, abort
 * signals, custom transports).
 */
export { ModelGateway };
