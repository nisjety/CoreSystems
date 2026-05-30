"use client";

import { useSyncExternalStore } from "react";
import type { Agent, CustomerContext, Group, Macro } from "@/features/inbox-v2/lib/inbox-model";

type StoreRecord<T> = {
  listeners: Set<() => void>;
  request: Promise<void> | null;
  state: T;
};

type SupportReferenceData = {
  agents: Agent[];
  groups: Group[];
};

const emptyReferenceData: SupportReferenceData = {
  agents: [],
  groups: [],
};

const emptyMacros: Macro[] = [];
const referenceDataRecords = new Map<string, StoreRecord<SupportReferenceData>>();
const customerContextRecords = new Map<string, StoreRecord<CustomerContext | null>>();
const macroRecords = new Map<string, StoreRecord<Macro[]>>();

function getRecord<T>(records: Map<string, StoreRecord<T>>, key: string, initialState: T) {
  let record = records.get(key);

  if (!record) {
    record = {
      listeners: new Set<() => void>(),
      request: null,
      state: initialState,
    };
    records.set(key, record);
  }

  return record;
}

function notifyRecord<T>(record: StoreRecord<T>) {
  for (const listener of record.listeners) {
    listener();
  }
}

async function fetchJson<T>(url: string, init?: RequestInit) {
  const response = await fetch(url, init);

  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }

  return response.json() as Promise<T>;
}

async function loadReferenceData(record: StoreRecord<SupportReferenceData>) {
  const [agentResult, groupResult] = await Promise.allSettled([
    fetchJson<Agent[]>("/api/support/agents"),
    fetchJson<Group[]>("/api/support/groups"),
  ]);

  record.state = {
    agents: agentResult.status === "fulfilled" && Array.isArray(agentResult.value) ? agentResult.value : [],
    groups: groupResult.status === "fulfilled" && Array.isArray(groupResult.value) ? groupResult.value : [],
  };
  notifyRecord(record);
}

async function loadCustomerContext(record: StoreRecord<CustomerContext | null>, key: string) {
  const [customerId, orgId = ""] = key.split("::");

  try {
    record.state = await fetchJson<CustomerContext>(
      `/api/support/customers/${encodeURIComponent(customerId)}/context?orgId=${encodeURIComponent(orgId)}`,
    );
  } catch {
    record.state = null;
  }

  notifyRecord(record);
}

async function loadMacros(record: StoreRecord<Macro[]>) {
  try {
    const payload = await fetchJson<Macro[]>("/api/support/macros");
    record.state = Array.isArray(payload) ? payload : [];
  } catch {
    record.state = [];
  }

  notifyRecord(record);
}

function ensureRecordRequest<T>(
  record: StoreRecord<T>,
  load: () => Promise<void>,
) {
  record.request ??= load().finally(() => {
    record.request = null;
  });
}

function subscribeRecord<T>(
  record: StoreRecord<T>,
  load: () => Promise<void>,
  listener: () => void,
) {
  record.listeners.add(listener);
  ensureRecordRequest(record, load);

  return () => {
    record.listeners.delete(listener);
  };
}

function useStoreSnapshot<T>(
  key: string | null,
  records: Map<string, StoreRecord<T>>,
  initialState: T,
  load: (record: StoreRecord<T>, key: string) => Promise<void>,
) {
  return useSyncExternalStore(
    (listener) => {
      if (!key) {
        return () => undefined;
      }

      const record = getRecord(records, key, initialState);
      return subscribeRecord(record, () => load(record, key), listener);
    },
    () => key ? getRecord(records, key, initialState).state : initialState,
    () => initialState,
  );
}

export function useSupportReferenceData() {
  return useStoreSnapshot(
    "support-reference-data",
    referenceDataRecords,
    emptyReferenceData,
    (record) => loadReferenceData(record),
  );
}

export function useCustomerContext(customerId: number | null | undefined, orgId: number | null | undefined) {
  const key = customerId ? `${customerId}::${orgId ?? ""}` : null;

  return useStoreSnapshot(key, customerContextRecords, null, loadCustomerContext);
}

export function useSupportMacros() {
  return useStoreSnapshot(
    "support-macros",
    macroRecords,
    emptyMacros,
    (record) => loadMacros(record),
  );
}
