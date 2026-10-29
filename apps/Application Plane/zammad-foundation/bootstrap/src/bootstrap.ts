import { readFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

// ─── Config input types ───────────────────────────────────────────────────────

type GroupConfig = {
  name: string;
  signatureId?: number | null;
  emailAddressId?: number | null;
  assignmentTimeout?: number | null;
  followUpPossible?: "yes" | "new_ticket";
  followUpAssignment?: boolean;
  active?: boolean;
  note?: string | null;
};

type ScreenRule = {
  shown?: boolean;
  required?: boolean;
  item_class?: string;
  null?: boolean;
};

type TicketFieldConfig = {
  name: string;
  object: "Ticket";
  display: string;
  active: boolean;
  position: number;
  data_type: "boolean" | "input" | "integer" | "select";
  data_option: Record<string, unknown>;
  screens: Record<string, Record<string, ScreenRule>>;
};

type WebhookConfig = {
  name: string;
  /** Target URL. Falls back to ZAMMAD_WEBHOOK_ENDPOINT env var. */
  endpoint?: string;
  /** HMAC signing token. Falls back to ZAMMAD_WEBHOOK_TOKEN env var. */
  token?: string;
  sslVerify?: boolean;
  active?: boolean;
  note?: string | null;
};

type TriggerConfig = {
  name: string;
  /** Must match a WebhookConfig name in the same config file. */
  webhookName: string;
  /** Zammad condition object — see Zammad docs for shape. */
  condition: Record<string, unknown>;
  active?: boolean;
  note?: string | null;
};

type BootstrapConfig = {
  groups: GroupConfig[];
  ticketFields: TicketFieldConfig[];
  webhooks?: WebhookConfig[];
  triggers?: TriggerConfig[];
};

// ─── Zammad API response types ────────────────────────────────────────────────

type ZammadGroup = {
  id: number;
  name: string;
  signature_id: number | null;
  email_address_id: number | null;
  assignment_timeout: number | null;
  follow_up_possible: "yes" | "new_ticket";
  follow_up_assignment: boolean;
  active: boolean;
  note: string | null;
};

type ZammadObjectAttribute = {
  id: number;
  name: string;
  object: string;
  display: string;
  active: boolean;
  position: number;
  data_type: string;
  data_option: Record<string, unknown>;
  screens: Record<string, Record<string, unknown>>;
};

type ZammadWebhook = {
  id: number;
  name: string;
  endpoint: string;
  ssl_verify: boolean;
  active: boolean;
  note: string | null;
  token?: string;
};

type ZammadTrigger = {
  id: number;
  name: string;
  active: boolean;
  note: string | null;
  condition: Record<string, unknown>;
  perform: Record<string, unknown>;
};

// ─── CLI flags ────────────────────────────────────────────────────────────────

const args = new Set(process.argv.slice(2));
const applyChanges = args.has("--apply");
const executeMigrations = args.has("--execute-migrations");
const dryRun = !applyChanges;

// ─── Environment ──────────────────────────────────────────────────────────────

const baseUrl = requireEnv("ZAMMAD_BASE_URL").replace(/\/+$/, "");
const token = requireEnv("ZAMMAD_TOKEN");
const configPath = path.resolve(
  process.cwd(),
  process.env.ZAMMAD_CONFIG_PATH ?? "../config/bootstrap.example.json",
);
// Optional: override webhook endpoint from env (useful for different environments).
const webhookEndpointOverride = process.env.ZAMMAD_WEBHOOK_ENDPOINT ?? "";
// Optional: HMAC signing token — must match ZAMMAD_WEBHOOK_TOKEN in integration-core.
const webhookTokenOverride = process.env.ZAMMAD_WEBHOOK_TOKEN ?? "";

// ─── Entry point ─────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  const config = await loadConfig(configPath);

  console.log(dryRun ? "Running in dry-run mode." : "Applying changes to Zammad.");
  console.log(`Using config: ${configPath}`);

  // ── Groups ──────────────────────────────────────────────────────────────────
  const existingGroups = await api<ZammadGroup[]>("GET", "/api/v1/groups");
  await syncGroups(existingGroups, config.groups);

  // ── Ticket fields ────────────────────────────────────────────────────────────
  const existingObjectAttributes = await api<ZammadObjectAttribute[]>(
    "GET",
    "/api/v1/object_manager_attributes",
  );
  const ticketAttributes = existingObjectAttributes.filter(
    (attribute) => attribute.object === "Ticket",
  );
  const objectChanges = await syncTicketFields(ticketAttributes, config.ticketFields);

  if (objectChanges > 0) {
    if (dryRun) {
      console.log(
        "Object changes detected. Re-run with --apply --execute-migrations to execute database migrations.",
      );
    } else if (executeMigrations) {
      console.log("Executing object manager migrations.");
      await api("POST", "/api/v1/object_manager_attributes_execute_migrations");
      console.log(
        "Migrations executed. Restart zammad-railsserver, zammad-scheduler, and zammad-websocket before continuing.",
      );
    } else {
      console.log(
        "Object changes were applied, but migrations were not executed. Run again with --execute-migrations or trigger the migration in the admin UI.",
      );
    }
  }

  // ── Webhooks + triggers ──────────────────────────────────────────────────────
  if (config.webhooks?.length) {
    const existingWebhooks = await api<ZammadWebhook[]>("GET", "/api/v1/webhooks");
    const webhookIds = await syncWebhooks(existingWebhooks, config.webhooks);

    if (config.triggers?.length) {
      const existingTriggers = await api<ZammadTrigger[]>("GET", "/api/v1/triggers");
      await syncTriggers(existingTriggers, config.triggers, webhookIds);
    }
  }
}

// ─── Group sync ───────────────────────────────────────────────────────────────

async function syncGroups(existingGroups: ZammadGroup[], groups: GroupConfig[]): Promise<void> {
  for (const group of groups) {
    const existing = existingGroups.find((candidate) => candidate.name === group.name);
    const payload = buildGroupPayload(group, existing);

    if (!existing) {
      await maybeApply("create group", group.name, "POST", "/api/v1/groups", payload);
      continue;
    }

    await maybeApply(
      "update group",
      `${group.name} (#${existing.id})`,
      "PUT",
      `/api/v1/groups/${existing.id}`,
      payload,
    );
  }
}

function buildGroupPayload(group: GroupConfig, existing?: ZammadGroup): Record<string, unknown> {
  return {
    name: group.name,
    signature_id:
      group.signatureId !== undefined ? group.signatureId : existing?.signature_id ?? null,
    email_address_id:
      group.emailAddressId !== undefined
        ? group.emailAddressId
        : existing?.email_address_id ?? null,
    assignment_timeout:
      group.assignmentTimeout !== undefined
        ? group.assignmentTimeout
        : existing?.assignment_timeout ?? null,
    follow_up_possible: group.followUpPossible ?? existing?.follow_up_possible ?? "yes",
    follow_up_assignment:
      group.followUpAssignment ?? existing?.follow_up_assignment ?? true,
    active: group.active ?? existing?.active ?? true,
    note: group.note ?? existing?.note ?? null,
  };
}

// ─── Ticket field sync ────────────────────────────────────────────────────────

async function syncTicketFields(
  existingFields: ZammadObjectAttribute[],
  fields: TicketFieldConfig[],
): Promise<number> {
  let changes = 0;

  for (const field of fields) {
    const existing = existingFields.find((candidate) => candidate.name === field.name);
    const payload = buildFieldPayload(field, existing);

    if (!existing) {
      changes += 1;
      await maybeApply(
        "create ticket field",
        field.name,
        "POST",
        "/api/v1/object_manager_attributes",
        payload,
      );
      continue;
    }

    changes += 1;
    await maybeApply(
      "update ticket field",
      `${field.name} (#${existing.id})`,
      "PUT",
      `/api/v1/object_manager_attributes/${existing.id}`,
      payload,
    );
  }

  return changes;
}

function buildFieldPayload(
  field: TicketFieldConfig,
  existing?: ZammadObjectAttribute,
): Record<string, unknown> {
  return {
    ...(existing ? { id: existing.id } : {}),
    name: field.name,
    object: "Ticket",
    display: field.display,
    active: field.active,
    position: field.position,
    data_type: field.data_type,
    data_option: field.data_option,
    screens: field.screens,
  };
}

// ─── Webhook sync ─────────────────────────────────────────────────────────────

/**
 * Creates or updates webhooks. Returns a map of webhook name → Zammad ID.
 * On dry-run, IDs are -1 (placeholders).
 */
async function syncWebhooks(
  existingWebhooks: ZammadWebhook[],
  configs: WebhookConfig[],
): Promise<Map<string, number>> {
  const nameToId = new Map<string, number>();

  for (const cfg of configs) {
    const endpoint = webhookEndpointOverride || cfg.endpoint || "";
    const signingToken = webhookTokenOverride || cfg.token || "";
    const found = existingWebhooks.find((w) => w.name === cfg.name);
    const payload = buildWebhookPayload(cfg, endpoint, signingToken, found);

    if (!found) {
      console.log(`${dryRun ? "DRY RUN" : "APPLY"} create webhook: ${cfg.name}`);
      console.log(JSON.stringify(payload, null, 2));
      if (!dryRun) {
        const created = await api<ZammadWebhook>("POST", "/api/v1/webhooks", payload);
        nameToId.set(cfg.name, created.id);
        console.log(`  → webhook #${created.id} created`);
      } else {
        nameToId.set(cfg.name, -1);
      }
    } else {
      nameToId.set(cfg.name, found.id);
      console.log(`${dryRun ? "DRY RUN" : "APPLY"} update webhook: ${cfg.name} (#${found.id})`);
      console.log(JSON.stringify(payload, null, 2));
      if (!dryRun) {
        await api("PUT", `/api/v1/webhooks/${found.id}`, payload);
      }
    }
  }

  return nameToId;
}

function buildWebhookPayload(
  cfg: WebhookConfig,
  endpoint: string,
  signingToken: string,
  existing?: ZammadWebhook,
): Record<string, unknown> {
  const payload: Record<string, unknown> = {
    name: cfg.name,
    endpoint: endpoint || existing?.endpoint || "",
    ssl_verify: cfg.sslVerify ?? existing?.ssl_verify ?? false,
    http_method: "post",
    active: cfg.active ?? existing?.active ?? true,
    note: cfg.note ?? existing?.note ?? null,
    customized_messaging: false,
  };
  // Zammad uses this token for HMAC-SHA1 signatures sent in X-Hub-Signature.
  if (signingToken) {
    payload.token = signingToken;
  }
  return payload;
}

// ─── Trigger sync ─────────────────────────────────────────────────────────────

async function syncTriggers(
  existingTriggers: ZammadTrigger[],
  configs: TriggerConfig[],
  webhookIds: Map<string, number>,
): Promise<void> {
  for (const cfg of configs) {
    const webhookId = webhookIds.get(cfg.webhookName);
    if (webhookId === undefined || webhookId < 0) {
      console.warn(
        `  Skipping trigger "${cfg.name}" — webhook "${cfg.webhookName}" has no ID (dry-run or missing).`,
      );
      continue;
    }

    const found = existingTriggers.find((t) => t.name === cfg.name);
    const payload = buildTriggerPayload(cfg, webhookId, found);

    if (!found) {
      console.log(`${dryRun ? "DRY RUN" : "APPLY"} create trigger: ${cfg.name}`);
      console.log(JSON.stringify(payload, null, 2));
      if (!dryRun) {
        await api("POST", "/api/v1/triggers", payload);
      }
    } else {
      console.log(`${dryRun ? "DRY RUN" : "APPLY"} update trigger: ${cfg.name} (#${found.id})`);
      console.log(JSON.stringify(payload, null, 2));
      if (!dryRun) {
        await api("PUT", `/api/v1/triggers/${found.id}`, payload);
      }
    }
  }
}

function buildTriggerPayload(
  cfg: TriggerConfig,
  webhookId: number,
  existing?: ZammadTrigger,
): Record<string, unknown> {
  return {
    name: cfg.name,
    active: cfg.active ?? existing?.active ?? true,
    condition: cfg.condition,
    perform: {
      "notification.webhook": {
        // Zammad stores webhook_id as a string in some versions
        webhook_id: String(webhookId),
      },
    },
    execution_condition_mode: "selective",
    note: cfg.note ?? existing?.note ?? null,
  };
}

// ─── Shared helpers ───────────────────────────────────────────────────────────

function requireEnv(name: string): string {
  const value = process.env[name];

  if (!value) {
    throw new Error(`Missing required environment variable: ${name}`);
  }

  return value;
}

async function loadConfig(filePath: string): Promise<BootstrapConfig> {
  const raw = await readFile(filePath, "utf8");
  return JSON.parse(raw) as BootstrapConfig;
}

async function maybeApply(
  action: string,
  subject: string,
  method: "POST" | "PUT",
  endpoint: string,
  payload: Record<string, unknown>,
): Promise<void> {
  console.log(`${dryRun ? "DRY RUN" : "APPLY"} ${action}: ${subject}`);
  console.log(JSON.stringify(payload, null, 2));

  if (dryRun) {
    return;
  }

  await api(method, endpoint, payload);
}

async function api<T = unknown>(
  method: "GET" | "POST" | "PUT",
  endpoint: string,
  body?: Record<string, unknown>,
): Promise<T> {
  const response = await fetch(`${baseUrl}${endpoint}`, {
    method,
    headers: {
      Authorization: `Token token=${token}`,
      "Content-Type": "application/json",
    },
    body: body ? JSON.stringify(body) : undefined,
  });

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(`${method} ${endpoint} failed: ${response.status} ${errorText}`);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  const text = await response.text();
  if (!text) {
    return undefined as T;
  }

  return JSON.parse(text) as T;
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
