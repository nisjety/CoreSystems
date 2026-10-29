export type ProjectionAction = "upsert" | "remove";

export type ProjectionEvent = Readonly<{
  action: ProjectionAction;
  revision: number;
  eventId: string;
  fingerprint: string;
}>;

export type ProjectionState = Readonly<{
  kind: "active" | "removed";
  revision: number;
  eventId: string;
  fingerprint: string;
}>;

function assertEvent(event: ProjectionEvent): void {
  if (!Number.isSafeInteger(event.revision) || event.revision <= 0) {
    throw new Error("projection revision must be a positive safe integer");
  }
  if (!event.eventId || !event.fingerprint) {
    throw new Error("projection identity is required");
  }
}

function applyRevision(
  state: ProjectionState | undefined,
  event: ProjectionEvent,
): ProjectionState {
  assertEvent(event);
  if (state && event.revision < state.revision) return state;
  if (state && event.revision === state.revision) {
    if (
      state.eventId === event.eventId &&
      state.fingerprint === event.fingerprint &&
      state.kind === (event.action === "remove" ? "removed" : "active")
    ) {
      return state;
    }
    throw new Error("same-revision projection conflict");
  }
  return {
    kind: event.action === "remove" ? "removed" : "active",
    revision: event.revision,
    eventId: event.eventId,
    fingerprint: event.fingerprint,
  };
}

export function applyMembershipProjectionState(
  state: ProjectionState | undefined,
  event: ProjectionEvent,
): ProjectionState {
  return applyRevision(state, event);
}

export function applyOrganizationProjectionState(
  state: ProjectionState | undefined,
  event: ProjectionEvent,
): ProjectionState {
  if (state?.kind === "removed" && event.action === "upsert") {
    return state;
  }
  return applyRevision(state, event);
}

export function organizationProjectionFingerprint(event: {
  action: ProjectionAction;
  name?: string;
  slug?: string;
}): string {
  return event.action === "remove"
    ? "remove"
    : JSON.stringify(["upsert", event.name, event.slug]);
}

export function membershipProjectionFingerprint(event: {
  action: ProjectionAction;
  email?: string;
  role?: string;
  organizationRevision: number;
}): string {
  return event.action === "remove"
    ? JSON.stringify(["remove", event.organizationRevision])
    : JSON.stringify([
        "upsert",
        event.email,
        event.role,
        event.organizationRevision,
      ]);
}
