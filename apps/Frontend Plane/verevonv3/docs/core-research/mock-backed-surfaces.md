# Verevon v3 Mock-Backed Workspace Surfaces

> Historical snapshot — do not use this file as current runtime status.
> The inventory was written before the gateway/API-client rollout. Current
> ownership and live-contract status is maintained in
> [`../gateway-integration-plan.md`](../gateway-integration-plan.md), including
> the 2026-08-11 authority hardening update. This document remains useful only
> for finding legacy mock imports that still need removal.

## Current State

Most non-onboarding workspace surfaces are backed by `src/shared/mocks/verevon-operating-model.ts`.

This includes:

- dashboard metrics and live runs
- chat conversation turns
- inbox ticket rows
- knowledge sources
- agent role cards
- workflow policies
- visible context items for the AI rail

## Surface Inventory

- `DashboardPage` uses `operatingMetrics`, `liveRuns`, and `actionRegistry`.
- `ChatPage` uses `conversationTurns` and local action commands.
- `InboxPage` uses `inboxItems` and local action commands.
- `KnowledgePage` uses `knowledgeSources` and `fetchKnowledgeGraphSnapshot()`.
- `AgentsPage` uses `agentRoles` and local action commands.
- `SettingsPage` renders the server-owned Router Policy controls; it does not
  select a concrete model in the browser.

## GraphREST State

`src/shared/graphrest/graphrest-client.ts` currently returns a hard-coded graph:

- `src_website`
- `topic_returns`
- `agent_support`

It does not call Data Plane v2 graph-index or any Application Plane API.

## Empty Planned Surfaces

The README names future shared transport areas. Current file inventory shows:

- `src/shared/api` is empty.
- `src/shared/rpc` is empty.
- `src/shared/workers` is empty.

## Risk

These mock-backed surfaces are useful for UX and architectural prototyping, but they should not be read as evidence that the v3 workspace has live operational parity with `verevonv2`.
