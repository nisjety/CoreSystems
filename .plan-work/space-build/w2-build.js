export const meta = {
  name: 'space-w2-build',
  description: 'Build wave W2 (M2.1 + M2.2 + M2.3) of the approved Space plan unit by unit: Sonnet high builders, lock groups keep concurrent builders on disjoint files, compile-check only, tests and depth come after the wave',
  phases: [
    { title: 'Build', detail: 'one builder per slice, at most 5 at a time, dependency order, commit per slice', model: 'claude-sonnet-5-5' },
  ],
}

const MODEL = 'claude-sonnet-5-5'
const UNITS = [
 {
  "id": "WS12-S4",
  "what": "Session Core revisions, fold, ReviseMessage",
  "lane": "L1",
  "group": "sc",
  "size": "L",
  "level": 0,
  "deps": [
   "WS01-S1"
  ],
  "ws": "WS12",
  "ms": "M2.2",
  "planStart": 2143,
  "planEnd": 2278
 },
 {
  "id": "WS13-S1",
  "what": "Session Core SpaceHistoryService, FTS index",
  "lane": "L1",
  "group": "sc",
  "size": "L",
  "level": 0,
  "deps": [
   "P-SC1"
  ],
  "ws": "WS13",
  "ms": "M2.3",
  "planStart": 2279,
  "planEnd": 2389
 },
 {
  "id": "WS14-S2",
  "what": "Control grants: owner-asserted, Space subject",
  "lane": "L4",
  "group": "ctl",
  "size": "L",
  "level": 0,
  "deps": [
   "P-UC1"
  ],
  "ws": "WS14",
  "ms": "M2.3",
  "planStart": 2390,
  "planEnd": 2501
 },
 {
  "id": "WS04-S8",
  "what": "visibility, directory policy, self-join",
  "lane": "L4",
  "group": "ctl",
  "size": "M/L",
  "level": 0,
  "deps": [
   "WS04-S4",
   "P-UC1",
   "WS04-S5",
   "WS04-S6"
  ],
  "ws": "WS04",
  "ms": "M2.2",
  "planStart": 1029,
  "planEnd": 1142
 },
 {
  "id": "WS04-S9",
  "what": "composed people and agent directory",
  "lane": "L7",
  "group": "gw",
  "size": "M",
  "level": 0,
  "deps": [
   "P-GW1",
   "WS04-S3"
  ],
  "ws": "WS04",
  "ms": "M2.2",
  "planStart": 1029,
  "planEnd": 1142
 },
 {
  "id": "WS05-S4",
  "what": "audit list v2, admin view (no export)",
  "lane": "L5",
  "group": "idn",
  "size": "M",
  "level": 0,
  "deps": [
   "WS05-S1"
  ],
  "ws": "WS05",
  "ms": "M2.3",
  "planStart": 1151,
  "planEnd": 1281
 },
 {
  "id": "WS05-S5",
  "what": "Space decision audit events",
  "lane": "L4",
  "group": "ctl",
  "size": "M",
  "level": 0,
  "deps": [
   "WS03-S1",
   "P-UC1"
  ],
  "ws": "WS05",
  "ms": "M2.3",
  "planStart": 1151,
  "planEnd": 1281
 },
 {
  "id": "WS05-S6",
  "what": "DNS domain verification, identity audit outbox",
  "lane": "L5",
  "group": "idn",
  "size": "M",
  "level": 0,
  "deps": [],
  "ws": "WS05",
  "ms": "M2.3",
  "planStart": 1151,
  "planEnd": 1281
 },
 {
  "id": "WS06-S6",
  "what": "sandbox-manager ZDR gate interceptor",
  "lane": "L3",
  "group": "run",
  "size": "M",
  "level": 0,
  "deps": [
   "WS06-S5"
  ],
  "ws": "WS06",
  "ms": "M2.3",
  "planStart": 1282,
  "planEnd": 1418
 },
 {
  "id": "WS06-S8",
  "what": "per-spawn prlimit, limit classification",
  "lane": "L3",
  "group": "run",
  "size": "M",
  "level": 0,
  "deps": [
   "WS06-S7"
  ],
  "ws": "WS06",
  "ms": "M2.3",
  "planStart": 1282,
  "planEnd": 1418
 },
 {
  "id": "WS06-S9",
  "what": "SpawnAdmission, shell lease, cleared env",
  "lane": "L3",
  "group": "run",
  "size": "M",
  "level": 0,
  "deps": [
   "WS06-S5"
  ],
  "ws": "WS06",
  "ms": "M2.3",
  "planStart": 1282,
  "planEnd": 1418
 },
 {
  "id": "WS08-S1",
  "what": "Control model.run.wake decision",
  "lane": "L4",
  "group": "ctl",
  "size": "M",
  "level": 0,
  "deps": [
   "P-UC1"
  ],
  "ws": "WS08",
  "ms": "M2.1",
  "planStart": 1691,
  "planEnd": 1799
 },
 {
  "id": "WS08-S5",
  "what": "binding wake policy (Convex, PATCH route, retire group)",
  "lane": "L6",
  "group": "app",
  "size": "M",
  "level": 0,
  "deps": [
   "P-CX1"
  ],
  "ws": "WS08",
  "ms": "M2.1",
  "planStart": 1691,
  "planEnd": 1799
 },
 {
  "id": "WS09-S3",
  "what": "room-safe projector, run pointer, work summary",
  "lane": "L2",
  "group": "sc",
  "size": "M",
  "level": 0,
  "deps": [],
  "ws": "WS09",
  "ms": "M2.1",
  "planStart": 1800,
  "planEnd": 1913
 },
 {
  "id": "WS09-S7",
  "what": "Control model.run.control and entitlement bit",
  "lane": "L4",
  "group": "ctl",
  "size": "M",
  "level": 0,
  "deps": [
   "P-UC1"
  ],
  "ws": "WS09",
  "ms": "M2.1",
  "planStart": 1800,
  "planEnd": 1913
 },
 {
  "id": "WS10-S1",
  "what": "honest approval card, typed context, derived continuation",
  "lane": "L2",
  "group": "sc",
  "size": "M",
  "level": 0,
  "deps": [],
  "ws": "WS10",
  "ms": "M2.1",
  "planStart": 1914,
  "planEnd": 2023
 },
 {
  "id": "WS10-S2",
  "what": "Control grants, approval policy, gate/use endpoints",
  "lane": "L4",
  "group": "ctl",
  "size": "M",
  "level": 0,
  "deps": [
   "P-UC1"
  ],
  "ws": "WS10",
  "ms": "M2.1",
  "planStart": 1914,
  "planEnd": 2023
 },
 {
  "id": "WS11-S5",
  "what": "Session Core attention outbox, NATS stream",
  "lane": "L2",
  "group": "sc",
  "size": "M",
  "level": 0,
  "deps": [
   "WS02-S1"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS12-S1",
  "what": "Control DM kinds, participant_hash, eligibility",
  "lane": "L4",
  "group": "ctl",
  "size": "M",
  "level": 0,
  "deps": [
   "P-UC1"
  ],
  "ws": "WS12",
  "ms": "M2.2",
  "planStart": 2143,
  "planEnd": 2278
 },
 {
  "id": "WS13-S6",
  "what": "command layer: registry, palette, switcher",
  "lane": "L9",
  "group": "spa-shell",
  "size": "M",
  "level": 0,
  "deps": [
   "P-SPA1"
  ],
  "ws": "WS13",
  "ms": "M2.3",
  "planStart": 2279,
  "planEnd": 2389
 },
 {
  "id": "WS13-S8",
  "what": "room pins (Convex, routes)",
  "lane": "L6",
  "group": "app",
  "size": "M",
  "level": 0,
  "deps": [
   "P-CX1"
  ],
  "ws": "WS13",
  "ms": "M2.3",
  "planStart": 2279,
  "planEnd": 2389
 },
 {
  "id": "WS14-S1",
  "what": "Control shared import decision",
  "lane": "L4",
  "group": "ctl",
  "size": "M",
  "level": 0,
  "deps": [
   "P-UC1"
  ],
  "ws": "WS14",
  "ms": "M2.3",
  "planStart": 2390,
  "planEnd": 2501
 },
 {
  "id": "WS14-S8",
  "what": "SPA per-turn attachments, chips, typed ZDR copy",
  "lane": "L8",
  "group": "spa-room",
  "size": "M",
  "level": 0,
  "deps": [
   "P-SPA2",
   "P-SPA1"
  ],
  "ws": "WS14",
  "ms": "M2.3",
  "planStart": 2390,
  "planEnd": 2501
 },
 {
  "id": "WS15-S3",
  "what": "markdown toolbar, preview, counter",
  "lane": "L8",
  "group": "spa-room",
  "size": "M",
  "level": 0,
  "deps": [
   "P-SPA2",
   "P-SPA1"
  ],
  "ws": "WS15",
  "ms": "M2.3",
  "planStart": 2502,
  "planEnd": 2614
 },
 {
  "id": "WS15-S5",
  "what": "Session Core thread activity summary",
  "lane": "L1",
  "group": "sc",
  "size": "M",
  "level": 0,
  "deps": [
   "P-SC1"
  ],
  "ws": "WS15",
  "ms": "M2.2",
  "planStart": 2502,
  "planEnd": 2614
 },
 {
  "id": "WS16-S1",
  "what": "locale-correct Space copy and dates",
  "lane": "L9",
  "group": "spa-shell",
  "size": "M",
  "level": 0,
  "deps": [],
  "ws": "WS16",
  "ms": "M2.3",
  "planStart": 1555,
  "planEnd": 1682
 },
 {
  "id": "WS16-S7",
  "what": "Convex spaceSidebarPreferences",
  "lane": "L6",
  "group": "app",
  "size": "M",
  "level": 0,
  "deps": [
   "P-CX1"
  ],
  "ws": "WS16",
  "ms": "M2.3",
  "planStart": 1555,
  "planEnd": 1682
 },
 {
  "id": "WS04-S10",
  "what": "durable membership events",
  "lane": "L4",
  "group": "ctl",
  "size": "S/M",
  "level": 0,
  "deps": [
   "WS04-S4",
   "P-UC1"
  ],
  "ws": "WS04",
  "ms": "M2.2",
  "planStart": 1029,
  "planEnd": 1142
 },
 {
  "id": "WS04-S12",
  "what": "retire Application member list",
  "lane": "L6",
  "group": "app",
  "size": "S",
  "level": 0,
  "deps": [
   "WS04-S6",
   "P-CX1"
  ],
  "ws": "WS04",
  "ms": "M2.2",
  "planStart": 1029,
  "planEnd": 1142
 },
 {
  "id": "WS09-S2",
  "what": "owner mid-run steer in room composer",
  "lane": "L8",
  "group": "spa-room",
  "size": "S",
  "level": 0,
  "deps": [
   "P-SPA2",
   "P-SPA1"
  ],
  "ws": "WS09",
  "ms": "M2.1",
  "planStart": 1800,
  "planEnd": 1913
 },
 {
  "id": "WS11-S1",
  "what": "Control bulk read decisions",
  "lane": "L4",
  "group": "ctl",
  "size": "S",
  "level": 0,
  "deps": [
   "P-UC1"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS13-S4a",
  "what": "SPA entry links, entity router, highlight",
  "lane": "L8",
  "group": "spa-room",
  "size": "S",
  "level": 0,
  "deps": [
   "WS01-S7a",
   "P-SPA1"
  ],
  "ws": "WS13",
  "ms": "M2.3",
  "planStart": 2279,
  "planEnd": 2389
 },
 {
  "id": "WS15-S1",
  "what": "client timeline: truncation fix, anchoring, jump pill",
  "lane": "L8",
  "group": "spa-room",
  "size": "S",
  "level": 0,
  "deps": [
   "P-SPA1"
  ],
  "ws": "WS15",
  "ms": "M2.3",
  "planStart": 2502,
  "planEnd": 2614
 },
 {
  "id": "WS15-S2",
  "what": "per-room per-thread drafts",
  "lane": "L8",
  "group": "spa-room",
  "size": "S",
  "level": 0,
  "deps": [
   "P-SPA2",
   "P-SPA1"
  ],
  "ws": "WS15",
  "ms": "M2.3",
  "planStart": 2502,
  "planEnd": 2614
 },
 {
  "id": "WS15-S4",
  "what": "human posts rendered as markdown",
  "lane": "L8",
  "group": "spa-room",
  "size": "S",
  "level": 0,
  "deps": [
   "WS07-S5",
   "P-SPA1"
  ],
  "ws": "WS15",
  "ms": "M2.3",
  "planStart": 2502,
  "planEnd": 2614
 },
 {
  "id": "WS16-S2",
  "what": "Control preference contract v2",
  "lane": "L4",
  "group": "ctl",
  "size": "S",
  "level": 0,
  "deps": [
   "P-UC1"
  ],
  "ws": "WS16",
  "ms": "M2.3",
  "planStart": 1555,
  "planEnd": 1682
 },
 {
  "id": "WS16-S6",
  "what": "viewer locale hint on Space turns",
  "lane": "L7",
  "group": "gw",
  "size": "S",
  "level": 0,
  "deps": [
   "P-GW1"
  ],
  "ws": "WS16",
  "ms": "M2.3",
  "planStart": 1555,
  "planEnd": 1682
 },
 {
  "id": "WS04-S11",
  "what": "opt-in history window (since_join)",
  "lane": "L4",
  "group": "ctl",
  "size": "L",
  "level": 1,
  "deps": [
   "WS04-S10",
   "P-UC1",
   "WS04-S5",
   "WS04-S8",
   "WS04-S8.gw",
   "WS04-S8.spa",
   "WS04-S9.spa"
  ],
  "ws": "WS04",
  "ms": "M2.2",
  "planStart": 1029,
  "planEnd": 1142
 },
 {
  "id": "WS05-S7",
  "what": "SSO policy, break-glass, enforcement hook",
  "lane": "L5",
  "group": "idn",
  "size": "L",
  "level": 1,
  "deps": [
   "WS05-S3",
   "WS05-S6"
  ],
  "ws": "WS05",
  "ms": "M2.3",
  "planStart": 1151,
  "planEnd": 1281
 },
 {
  "id": "WS08-S2",
  "what": "Session Core mentions, wake verification, run columns",
  "lane": "L1",
  "group": "sc",
  "size": "L",
  "level": 1,
  "deps": [
   "WS01-S1",
   "WS09-S1",
   "WS08-S1"
  ],
  "ws": "WS08",
  "ms": "M2.1",
  "planStart": 1691,
  "planEnd": 1799
 },
 {
  "id": "WS09-S4",
  "what": "member live attach (Session Core, SSE, gateway)",
  "lane": "L2",
  "group": "sc",
  "size": "L",
  "level": 1,
  "deps": [
   "WS09-S3"
  ],
  "ws": "WS09",
  "ms": "M2.1",
  "planStart": 1800,
  "planEnd": 1913
 },
 {
  "id": "WS10-S3",
  "what": "execution-core gate, once-token, grant pass",
  "lane": "L3",
  "group": "run",
  "size": "L",
  "level": 1,
  "deps": [
   "WS10-S1",
   "WS10-S2"
  ],
  "ws": "WS10",
  "ms": "M2.1",
  "planStart": 1914,
  "planEnd": 2023
 },
 {
  "id": "WS11-S2",
  "what": "Session Core SpaceAttention RPC",
  "lane": "L2",
  "group": "sc",
  "size": "L",
  "level": 1,
  "deps": [
   "WS15-S5",
   "WS11-S1"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS14-S3",
  "what": "documents-api space upload",
  "lane": "L10",
  "group": "dat",
  "size": "L",
  "level": 1,
  "deps": [
   "WS14-S1",
   "WS14-S2"
  ],
  "ws": "WS14",
  "ms": "M2.3",
  "planStart": 2390,
  "planEnd": 2501
 },
 {
  "id": "WS15-S6",
  "what": "ListSpaceTimeline, tail transcript",
  "lane": "L1",
  "group": "sc",
  "size": "L",
  "level": 1,
  "deps": [
   "WS15-S5",
   "WS01-S1"
  ],
  "ws": "WS15",
  "ms": "M2.3",
  "planStart": 2502,
  "planEnd": 2614
 },
 {
  "id": "WS06-S10",
  "what": "production-profile verification and calibration",
  "lane": "L3",
  "group": "run",
  "size": "M",
  "level": 1,
  "deps": [
   "WS06-S7",
   "WS06-S8",
   "WS06-S9"
  ],
  "ws": "WS06",
  "ms": "M2.3",
  "planStart": 1282,
  "planEnd": 1418
 },
 {
  "id": "WS10-S5a",
  "what": "tool policy store (Space ceiling, agent layer)",
  "lane": "L4",
  "group": "ctl",
  "size": "M",
  "level": 1,
  "deps": [
   "WS10-S2",
   "P-UC1"
  ],
  "ws": "WS10",
  "ms": "M2.1",
  "planStart": 1914,
  "planEnd": 2023
 },
 {
  "id": "WS12-S2",
  "what": "Application DM open, hide, legacy guards",
  "lane": "L6",
  "group": "app",
  "size": "M",
  "level": 1,
  "deps": [
   "P-CX1",
   "WS12-S1"
  ],
  "ws": "WS12",
  "ms": "M2.2",
  "planStart": 2143,
  "planEnd": 2278
 },
 {
  "id": "WS12-S5",
  "what": "Control revise decision, message policies",
  "lane": "L4",
  "group": "ctl",
  "size": "M",
  "level": 1,
  "deps": [
   "P-UC1",
   "WS12-S4"
  ],
  "ws": "WS12",
  "ms": "M2.2",
  "planStart": 2143,
  "planEnd": 2278
 },
 {
  "id": "WS14-S4",
  "what": "sharing un-fence (documents-api, gateway)",
  "lane": "L10",
  "group": "dat",
  "size": "M",
  "level": 1,
  "deps": [
   "WS14-S2"
  ],
  "ws": "WS14",
  "ms": "M2.3",
  "planStart": 2390,
  "planEnd": 2501
 },
 {
  "id": "WS14-S5",
  "what": "retrieval-engine listing provenance, grants union",
  "lane": "L10",
  "group": "dat",
  "size": "M",
  "level": 1,
  "deps": [
   "WS14-S2"
  ],
  "ws": "WS14",
  "ms": "M2.3",
  "planStart": 2390,
  "planEnd": 2501
 },
 {
  "id": "WS14-S6",
  "what": "imports-core space-upload",
  "lane": "L10",
  "group": "dat",
  "size": "M",
  "level": 1,
  "deps": [
   "WS14-S1"
  ],
  "ws": "WS14",
  "ms": "M2.3",
  "planStart": 2390,
  "planEnd": 2501
 },
 {
  "id": "WS16-S3",
  "what": "preferences bootstrap, locale, density",
  "lane": "L9",
  "group": "spa-shell",
  "size": "M",
  "level": 1,
  "deps": [
   "WS16-S1",
   "P-SPA1"
  ],
  "ws": "WS16",
  "ms": "M2.3",
  "planStart": 1555,
  "planEnd": 1682
 },
 {
  "id": "WS16-S4",
  "what": "shared announcer, Space narration",
  "lane": "L9",
  "group": "spa-shell",
  "size": "M",
  "level": 1,
  "deps": [
   "WS16-S1",
   "P-SPA1"
  ],
  "ws": "WS16",
  "ms": "M2.3",
  "planStart": 1555,
  "planEnd": 1682
 },
 {
  "id": "WS16-S8",
  "what": "BFF sidebar routes, ZDR split",
  "lane": "L7",
  "group": "gw",
  "size": "M",
  "level": 1,
  "deps": [
   "P-GW1",
   "WS16-S7",
   "WS01-S5"
  ],
  "ws": "WS16",
  "ms": "M2.3",
  "planStart": 1555,
  "planEnd": 1682
 },
 {
  "id": "WS13-S2",
  "what": "model-gateway search routes and client",
  "lane": "L1",
  "group": "sc",
  "size": "S/M",
  "level": 1,
  "deps": [
   "WS13-S1"
  ],
  "ws": "WS13",
  "ms": "M2.3",
  "planStart": 2279,
  "planEnd": 2389
 },
 {
  "id": "WS13-S7",
  "what": "platform slash commands in composer",
  "lane": "L8",
  "group": "spa-room",
  "size": "S",
  "level": 1,
  "deps": [
   "WS13-S6",
   "P-SPA2",
   "P-SPA1"
  ],
  "ws": "WS13",
  "ms": "M2.3",
  "planStart": 2279,
  "planEnd": 2389
 },
 {
  "id": "WS16-S11",
  "what": "orphan sweep cron",
  "lane": "L6",
  "group": "app",
  "size": "S",
  "level": 1,
  "deps": [
   "WS16-S7",
   "P-CX1"
  ],
  "ws": "WS16",
  "ms": "M2.3",
  "planStart": 1555,
  "planEnd": 1682
 },
 {
  "id": "WS05-S8",
  "what": "SCIM Users in org-core, deprovision",
  "lane": "L5",
  "group": "idn",
  "size": "L",
  "level": 2,
  "deps": [
   "WS05-S2",
   "WS05-S6",
   "WS05-S7",
   "WS04-S10"
  ],
  "ws": "WS05",
  "ms": "M2.3",
  "planStart": 1151,
  "planEnd": 1281
 },
 {
  "id": "WS08-S3",
  "what": "model-gateway wake mode, skip-append, history splice",
  "lane": "L1",
  "group": "sc",
  "size": "L",
  "level": 2,
  "deps": [
   "WS08-S2",
   "WS01-S4"
  ],
  "ws": "WS08",
  "ms": "M2.1",
  "planStart": 1691,
  "planEnd": 1799
 },
 {
  "id": "WS10-S6",
  "what": "addressee routing, member decide in Session Core",
  "lane": "L2",
  "group": "sc",
  "size": "L",
  "level": 2,
  "deps": [
   "WS10-S1",
   "WS10-S3",
   "WS01-S3"
  ],
  "ws": "WS10",
  "ms": "M2.3",
  "planStart": 1914,
  "planEnd": 2023
 },
 {
  "id": "WS11-S3",
  "what": "Convex read-state extensions, BFF aggregate",
  "lane": "L6",
  "group": "app",
  "size": "L",
  "level": 2,
  "deps": [
   "P-CX1",
   "WS11-S1",
   "WS11-S2"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS12-S3",
  "what": "gateway DM routes, SPA sidebar group, start dialog",
  "lane": "L7",
  "group": "gw",
  "size": "L",
  "level": 2,
  "deps": [
   "P-GW1",
   "WS12-S1",
   "WS12-S2",
   "WS01-S5",
   "WS01-S6",
   "WS01-S7b"
  ],
  "ws": "WS12",
  "ms": "M2.2",
  "planStart": 2143,
  "planEnd": 2278
 },
 {
  "id": "WS16-S9",
  "what": "organiser core, grouped panel, stars, drag",
  "lane": "L9",
  "group": "spa-shell",
  "size": "L",
  "level": 2,
  "deps": [
   "P-SPA1",
   "WS16-S8"
  ],
  "ws": "WS16",
  "ms": "M2.3",
  "planStart": 1555,
  "planEnd": 1682
 },
 {
  "id": "WS09-S5",
  "what": "run card, work disclosure, Work and Activity adapter",
  "lane": "L8",
  "group": "spa-room",
  "size": "M",
  "level": 2,
  "deps": [
   "P-SPA1",
   "WS09-S4"
  ],
  "ws": "WS09",
  "ms": "M2.1",
  "planStart": 1800,
  "planEnd": 1913
 },
 {
  "id": "WS09-S8",
  "what": "manager Stop end to end",
  "lane": "L2",
  "group": "sc",
  "size": "M",
  "level": 2,
  "deps": [
   "WS09-S4",
   "WS09-S7"
  ],
  "ws": "WS09",
  "ms": "M2.1",
  "planStart": 1800,
  "planEnd": 1913
 },
 {
  "id": "WS10-S4",
  "what": "Space decide route, scope picker, standing list",
  "lane": "L7",
  "group": "gw",
  "size": "M",
  "level": 2,
  "deps": [
   "P-GW1",
   "WS10-S1",
   "WS10-S2",
   "WS10-S3"
  ],
  "ws": "WS10",
  "ms": "M2.1",
  "planStart": 1914,
  "planEnd": 2023
 },
 {
  "id": "WS10-S5b",
  "what": "editor routes, Convex projection, Agent-tab editor",
  "lane": "L8",
  "group": "spa-room",
  "size": "M",
  "level": 2,
  "deps": [
   "P-SPA1",
   "WS10-S5a"
  ],
  "ws": "WS10",
  "ms": "M2.1",
  "planStart": 1914,
  "planEnd": 2023
 },
 {
  "id": "WS10-S9",
  "what": "resumable-kind registry and parity test",
  "lane": "L3",
  "group": "run",
  "size": "M",
  "level": 2,
  "deps": [
   "WS10-S1",
   "WS10-S3"
  ],
  "ws": "WS10",
  "ms": "M2.3",
  "planStart": 1914,
  "planEnd": 2023
 },
 {
  "id": "WS12-S6",
  "what": "model-gateway PATCH/DELETE/target routes",
  "lane": "L1",
  "group": "sc",
  "size": "M",
  "level": 2,
  "deps": [
   "WS12-S4",
   "WS12-S5"
  ],
  "ws": "WS12",
  "ms": "M2.2",
  "planStart": 2143,
  "planEnd": 2278
 },
 {
  "id": "WS13-S3",
  "what": "BFF search routes, entries, features",
  "lane": "L7",
  "group": "gw",
  "size": "M",
  "level": 2,
  "deps": [
   "P-GW1",
   "WS13-S2"
  ],
  "ws": "WS13",
  "ms": "M2.3",
  "planStart": 2279,
  "planEnd": 2389
 },
 {
  "id": "WS14-S7",
  "what": "gateway space_files.rs R1-R6",
  "lane": "L7",
  "group": "gw",
  "size": "M",
  "level": 2,
  "deps": [
   "P-GW1",
   "WS14-S1",
   "WS14-S5",
   "WS14-S6"
  ],
  "ws": "WS14",
  "ms": "M2.3",
  "planStart": 2390,
  "planEnd": 2501
 },
 {
  "id": "WS15-S7",
  "what": "BFF timeline routes, legacy fallback",
  "lane": "L7",
  "group": "gw",
  "size": "M",
  "level": 2,
  "deps": [
   "P-GW1",
   "WS15-S6"
  ],
  "ws": "WS15",
  "ms": "M2.3",
  "planStart": 2502,
  "planEnd": 2614
 },
 {
  "id": "WS15-S9",
  "what": "typed entry plumbing and registry",
  "lane": "L1",
  "group": "sc",
  "size": "M",
  "level": 2,
  "deps": [
   "WS01-S1",
   "WS15-S6"
  ],
  "ws": "WS15",
  "ms": "M2.3",
  "planStart": 2502,
  "planEnd": 2614
 },
 {
  "id": "WS16-S5",
  "what": "forced-colors, contrast, density CSS",
  "lane": "L9",
  "group": "spa-shell",
  "size": "S",
  "level": 2,
  "deps": [
   "WS16-S3",
   "P-SPA1"
  ],
  "ws": "WS16",
  "ms": "M2.3",
  "planStart": 1555,
  "planEnd": 1682
 },
 {
  "id": "WS08-S4",
  "what": "BFF plan_mentions, policy split, wake injector",
  "lane": "L7",
  "group": "gw",
  "size": "L",
  "level": 3,
  "deps": [
   "P-GW1",
   "WS08-S1",
   "WS08-S3",
   "WS01-S5"
  ],
  "ws": "WS08",
  "ms": "M2.1",
  "planStart": 1691,
  "planEnd": 1799
 },
 {
  "id": "WS10-S8",
  "what": "quarantine approval kind, hold and release",
  "lane": "L3",
  "group": "run",
  "size": "L",
  "level": 3,
  "deps": [
   "WS10-S1",
   "WS10-S3",
   "WS10-S6"
  ],
  "ws": "WS10",
  "ms": "M2.3",
  "planStart": 1914,
  "planEnd": 2023
 },
 {
  "id": "WS11-S6",
  "what": "Convex ingest, rules, items, follows",
  "lane": "L6",
  "group": "app",
  "size": "L",
  "level": 3,
  "deps": [
   "WS11-S3",
   "P-CX1",
   "WS11-S5",
   "WS11-S3.gw",
   "WS11-S3.spa"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS12-S7",
  "what": "gateway lifecycle routes, SPA toolbar, edit, delete",
  "lane": "L8",
  "group": "spa-room",
  "size": "L",
  "level": 3,
  "deps": [
   "WS01-S7a",
   "P-SPA2",
   "P-SPA1",
   "WS12-S5",
   "WS12-S6",
   "WS01-S7b"
  ],
  "ws": "WS12",
  "ms": "M2.2",
  "planStart": 2143,
  "planEnd": 2278
 },
 {
  "id": "WS14-S9",
  "what": "SPA Files tab, preview, delete",
  "lane": "L8",
  "group": "spa-room",
  "size": "L",
  "level": 3,
  "deps": [
   "P-SPA1",
   "WS14-S7"
  ],
  "ws": "WS14",
  "ms": "M2.3",
  "planStart": 2390,
  "planEnd": 2501
 },
 {
  "id": "WS15-S8",
  "what": "SPA timeline store, paged UI, lazy transcripts",
  "lane": "L8",
  "group": "spa-room",
  "size": "L",
  "level": 3,
  "deps": [
   "WS15-S1",
   "P-SPA1",
   "WS15-S7"
  ],
  "ws": "WS15",
  "ms": "M2.3",
  "planStart": 2502,
  "planEnd": 2614
 },
 {
  "id": "WS11-S4",
  "what": "SPA store, sidebar counts, mark all read",
  "lane": "L9",
  "group": "spa-shell",
  "size": "M",
  "level": 3,
  "deps": [
   "P-SPA1",
   "WS11-S3",
   "WS11-S3.gw",
   "WS11-S3.spa"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS13-S5",
  "what": "in-room search UI",
  "lane": "L8",
  "group": "spa-room",
  "size": "M",
  "level": 3,
  "deps": [
   "WS13-S4a",
   "P-SPA1",
   "WS13-S3"
  ],
  "ws": "WS13",
  "ms": "M2.3",
  "planStart": 2279,
  "planEnd": 2389
 },
 {
  "id": "WS05-S9",
  "what": "deprovision propagation probe, runbook",
  "lane": "L5",
  "group": "idn",
  "size": "S",
  "level": 3,
  "deps": [
   "WS05-S8",
   "WS04-S10"
  ],
  "ws": "WS05",
  "ms": "M2.3",
  "planStart": 1151,
  "planEnd": 1281
 },
 {
  "id": "WS09-S6",
  "what": "activity strip, sidebar working state",
  "lane": "L8",
  "group": "spa-room",
  "size": "S",
  "level": 3,
  "deps": [
   "WS09-S5",
   "P-SPA1"
  ],
  "ws": "WS09",
  "ms": "M2.1",
  "planStart": 1800,
  "planEnd": 1913
 },
 {
  "id": "WS10-S7",
  "what": "turn_origin rules",
  "lane": "L2",
  "group": "sc",
  "size": "S",
  "level": 3,
  "deps": [
   "WS10-S6",
   "WS01-S3",
   "WS08-S2"
  ],
  "ws": "WS10",
  "ms": "M2.3",
  "planStart": 1914,
  "planEnd": 2023
 },
 {
  "id": "WS13-S4b",
  "what": "/m/:messageId permalink route",
  "lane": "L8",
  "group": "spa-room",
  "size": "S",
  "level": 3,
  "deps": [
   "WS13-S4a",
   "P-SPA1",
   "WS13-S3"
  ],
  "ws": "WS13",
  "ms": "M2.3",
  "planStart": 2279,
  "planEnd": 2389
 },
 {
  "id": "WS08-S6",
  "what": "SPA structured mentions, picker, sequential wakes",
  "lane": "L8",
  "group": "spa-room",
  "size": "L",
  "level": 4,
  "deps": [
   "WS01-S6",
   "P-SPA2",
   "P-SPA1",
   "WS08-S4"
  ],
  "ws": "WS08",
  "ms": "M2.1",
  "planStart": 1691,
  "planEnd": 1799
 },
 {
  "id": "WS11-S7",
  "what": "items API, /attention page, follow control",
  "lane": "L9",
  "group": "spa-shell",
  "size": "L",
  "level": 4,
  "deps": [
   "P-SPA1",
   "WS11-S2",
   "WS11-S6"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS11-S10",
  "what": "org status text and agent availability chips",
  "lane": "L6",
  "group": "app",
  "size": "M",
  "level": 4,
  "deps": [
   "WS11-S3",
   "WS11-S6",
   "P-CX1",
   "WS11-S3.gw",
   "WS11-S3.spa"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS11-S9",
  "what": "notification-core principal, allowlist, courier",
  "lane": "L6",
  "group": "app",
  "size": "M",
  "level": 4,
  "deps": [
   "WS11-S6",
   "P-CX1"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS12-S8",
  "what": "reactions (Convex, routes, chips)",
  "lane": "L6",
  "group": "app",
  "size": "M",
  "level": 4,
  "deps": [
   "P-CX1",
   "WS12-S7",
   "WS12-S4",
   "WS01-S5"
  ],
  "ws": "WS12",
  "ms": "M2.2",
  "planStart": 2143,
  "planEnd": 2278
 },
 {
  "id": "WS15-S11",
  "what": "room system rows from Control ledger",
  "lane": "L7",
  "group": "gw",
  "size": "M",
  "level": 4,
  "deps": [
   "P-GW1",
   "WS15-S8",
   "WS15-S9",
   "WS04-S4"
  ],
  "ws": "WS15",
  "ms": "M2.3",
  "planStart": 2502,
  "planEnd": 2614
 },
 {
  "id": "WS16-S10",
  "what": "computed sections, mute entry, more-unread pill",
  "lane": "L9",
  "group": "spa-shell",
  "size": "M",
  "level": 4,
  "deps": [
   "WS16-S9",
   "P-SPA1",
   "WS11-S3",
   "WS11-S4",
   "WS11-S3.gw",
   "WS11-S3.spa"
  ],
  "ws": "WS16",
  "ms": "M2.3",
  "planStart": 1555,
  "planEnd": 1682
 },
 {
  "id": "WS11-S8",
  "what": "invite signal from gateway add-member",
  "lane": "L7",
  "group": "gw",
  "size": "S",
  "level": 4,
  "deps": [
   "P-GW1",
   "WS11-S6"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS11-S11",
  "what": "reminders",
  "lane": "L6",
  "group": "app",
  "size": "M",
  "level": 5,
  "deps": [
   "WS11-S6",
   "P-CX1",
   "WS11-S7"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS15-S12",
  "what": "Application room notices",
  "lane": "L6",
  "group": "app",
  "size": "M",
  "level": 5,
  "deps": [
   "P-CX1",
   "WS15-S11"
  ],
  "ws": "WS15",
  "ms": "M2.3",
  "planStart": 2502,
  "planEnd": 2614
 },
 {
  "id": "WS08-S8",
  "what": "Agent tab policy editor, why-no-answer copy",
  "lane": "L8",
  "group": "spa-room",
  "size": "S",
  "level": 5,
  "deps": [
   "WS08-S6",
   "P-SPA1",
   "WS08-S5"
  ],
  "ws": "WS08",
  "ms": "M2.1",
  "planStart": 1691,
  "planEnd": 1799
 },
 {
  "id": "WS04-S8.gw",
  "what": "LEG gw of WS04-S8 (visibility, directory policy, self-join). The Control leg is committed (10e972aa6); read .plan-work/space-build/units/WS04-S8.json first. Build ONLY: gateway directory.rs rooms route, room_admin.rs join/settings proxy plus audience reconcile, flag SPACE_ROOM_DIRECTORY_API, and the gateway dispatcher side of actions spaces.join_room / spaces.update_room_settings",
  "lane": "L4",
  "group": "gw",
  "size": "M/L",
  "level": 1,
  "deps": [
   "WS04-S8"
  ],
  "ws": "WS04",
  "ms": "M2.2",
  "planStart": 1029,
  "planEnd": 1142
 },
 {
  "id": "WS04-S8.spa",
  "what": "LEG spa of WS04-S8. Control leg 10e972aa6 and the gw leg (units/WS04-S8.gw.json) are done; read both unit files. Build ONLY the SPA: action contracts spaces.join_room / spaces.update_room_settings in src/shared/actions, SpacesDirectoryPage.tsx, SpaceRoomSettings, errors.ts copy; the App.tsx route line and the CoreSidebarSpacesPanel entry are allowed as minimal edits outside your group",
  "lane": "L4",
  "group": "spa-room",
  "size": "M/L",
  "level": 1,
  "deps": [
   "WS04-S8.gw"
  ],
  "ws": "WS04",
  "ms": "M2.2",
  "planStart": 1029,
  "planEnd": 1142
 },
 {
  "id": "WS04-S9.spa",
  "what": "LEG spa of WS04-S9 (composed people and agent directory). The gateway leg is committed (243561a67); read units/WS04-S9.json first. Build ONLY the SPA: spaces-client resolveDirectory, DirectoryPicker.tsx, SpaceMemberControls.tsx",
  "lane": "L7",
  "group": "spa-room",
  "size": "M",
  "level": 1,
  "deps": [
   "WS04-S9"
  ],
  "ws": "WS04",
  "ms": "M2.2",
  "planStart": 1029,
  "planEnd": 1142
 },
 {
  "id": "WS11-S3.gw",
  "what": "LEG of WS11-S3 (Convex read-state extensions, BFF aggregate). The Application half is committed (920bb2ac1). Read .plan-work/space-build/units/WS11-S3.json (and any earlier leg unit files of WS11-S3) first. Build ONLY: the gateway BFF for attention: domains/spaces/attention.rs aggregate (attention list, item state, read-all, unread/mark-unread, level, follow, invite, reason on read), its routes, flags and compose entries, and the gateway dispatchers for the matching spaces.* actions. WS11-S4's SPA (907ee8498) already calls these routes: read units/WS11-S4.json and match its client paths and payloads exactly",
  "lane": "L6",
  "group": "gw",
  "size": "L",
  "level": 3,
  "deps": [
   "WS11-S3"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS11-S3.spa",
  "what": "LEG of WS11-S3 (Convex read-state extensions, BFF aggregate). The Application half (920bb2ac1) and the gateway leg (units/WS11-S3.gw.json) are done. Read .plan-work/space-build/units/WS11-S3.json (and any earlier leg unit files of WS11-S3) first. Build ONLY: the SPA action contracts in src/shared/actions for the attention actions the gw leg dispatches, and any wiring the WS11-S4 SPA (907ee8498) still lacks to reach them",
  "lane": "L6",
  "group": "spa-room",
  "size": "L",
  "level": 3,
  "deps": [
   "WS11-S3.gw"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS11-S10.gw",
  "what": "LEG of WS11-S10 (org status text and agent availability chips). The Application half is committed (c7584a983). Read .plan-work/space-build/units/WS11-S10.json (and any earlier leg unit files of WS11-S10) first. Build ONLY: BFF GET/PUT /api/v1/spaces/status, roster status and agents activity routes, flag and compose entries, and the gateway dispatcher for spaces.set_status",
  "lane": "L6",
  "group": "gw",
  "size": "M",
  "level": 5,
  "deps": [
   "WS11-S10"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS11-S10.spa",
  "what": "LEG of WS11-S10 (org status text and agent availability chips). The Application half (c7584a983) and the gw leg (units/WS11-S10.gw.json) are done. Read .plan-work/space-build/units/WS11-S10.json (and any earlier leg unit files of WS11-S10) first. Build ONLY: action contract spaces.set_status, spaces-client calls, SpaceStatusMenu.tsx and roster chips; a minimal AccountSettingsPage entry is allowed outside your group",
  "lane": "L6",
  "group": "spa-room",
  "size": "M",
  "level": 5,
  "deps": [
   "WS11-S10.gw"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS11-S11.gw",
  "what": "LEG of WS11-S11 (reminders). The Application half is committed (f0860876e). Read .plan-work/space-build/units/WS11-S11.json (and any earlier leg unit files of WS11-S11) first. Build ONLY: BFF reminder routes, flag space_reminders_api with compose entries, and the gateway dispatchers for spaces.create_reminder / spaces.cancel_reminder",
  "lane": "L6",
  "group": "gw",
  "size": "M",
  "level": 6,
  "deps": [
   "WS11-S11"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS11-S11.spa",
  "what": "LEG of WS11-S11 (reminders). The Application half (f0860876e) and the gw leg (units/WS11-S11.gw.json) are done. Read .plan-work/space-build/units/WS11-S11.json (and any earlier leg unit files of WS11-S11) first. Build ONLY: action contracts spaces.create_reminder / spaces.cancel_reminder, spaces-client calls and SpaceRemindMenu.tsx",
  "lane": "L6",
  "group": "spa-room",
  "size": "M",
  "level": 6,
  "deps": [
   "WS11-S11.gw"
  ],
  "ws": "WS11",
  "ms": "M2.2",
  "planStart": 2024,
  "planEnd": 2132
 },
 {
  "id": "WS10-S7.ctl",
  "what": "LEG of WS10-S7 (turn_origin rules). The Session Core half is committed (5d3b6ad83). Read .plan-work/space-build/units/WS10-S7.json (and any earlier leg unit files of WS10-S7) first. Build ONLY: the Control twin in user-core (approval_decide_decision.go and whatever the plan names for turn_origin on the Control side)",
  "lane": "L2",
  "group": "ctl",
  "size": "S",
  "level": 4,
  "deps": [
   "WS10-S7"
  ],
  "ws": "WS10",
  "ms": "M2.3",
  "planStart": 1914,
  "planEnd": 2023
 },
 {
  "id": "WS04-S11.sc",
  "what": "LEG of WS04-S11 (opt-in history window (since_join)). The Control leg is committed (a85ad2d85). Read .plan-work/space-build/units/WS04-S11.json and any earlier WS04-S11 leg unit files first. Build ONLY: session-core history-floor claim and filter (grpc.rs, run_service_grpc.rs) behind SESSION_CORE_SPACE_HISTORY_FLOOR, plus compose entries",
  "lane": "L4",
  "group": "sc",
  "size": "L",
  "level": 2,
  "deps": [
   "WS04-S11"
  ],
  "ws": "WS04",
  "ms": "M2.2",
  "planStart": 1029,
  "planEnd": 1142
 },
 {
  "id": "WS04-S11.gw",
  "what": "LEG of WS04-S11 (opt-in history window (since_join)). The Control leg is committed (a85ad2d85). Read .plan-work/space-build/units/WS04-S11.json and any earlier WS04-S11 leg unit files first. Build ONLY: the gateway settings passthrough for the history floor and any flag/compose entries",
  "lane": "L4",
  "group": "gw",
  "size": "L",
  "level": 2,
  "deps": [
   "WS04-S11.sc"
  ],
  "ws": "WS04",
  "ms": "M2.2",
  "planStart": 1029,
  "planEnd": 1142
 },
 {
  "id": "WS04-S11.spa",
  "what": "LEG of WS04-S11 (opt-in history window (since_join)). The Control leg is committed (a85ad2d85). Read .plan-work/space-build/units/WS04-S11.json and any earlier WS04-S11 leg unit files first. Build ONLY: the SPA timeline notice and the spaces-client types for the history floor",
  "lane": "L4",
  "group": "spa-room",
  "size": "L",
  "level": 2,
  "deps": [
   "WS04-S11.gw"
  ],
  "ws": "WS04",
  "ms": "M2.2",
  "planStart": 1029,
  "planEnd": 1142
 }
]
const LIMIT = (args && args.limit) ? args.limit : 4
const ONLY = (args && args.only) ? args.only : null
const ASSUME_DONE = new Set((args && args.assumeDone) ? args.assumeDone : [])
const PLAN = 'docs/plans/space-hybrid-build-plan-2026-10-14.md'

const RET = {
  type: 'object',
  properties: {
    slice: { type: 'string' },
    status: { type: 'string' },
    commit: { type: 'string' },
    files: { type: 'array', items: { type: 'string' } },
    compile: { type: 'string' },
    migrations: { type: 'array', items: { type: 'string' } },
    flags: { type: 'array', items: { type: 'string' } },
    tests_owed: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string' },
  },
  required: ['slice', 'status'],
}

const GROUP_HINT = {
  plat: 'You own repo-level files for M0: scripts/, .github/workflows/, and test fixture files under apps/Frontend Plane/verevonv3/tests/e2e/. You do not touch product source.',
  sc: 'Your lock group is the Model Plane transcript and run stack: apps/Model Plane/rust/services/session-core, model-gateway, mp-contracts and the Model Plane proto files. Other builders (control, gateway, SPA, identity) run at the same time in other directories.',
  run: 'Your lock group is the Model Plane runtime: execution-core, capability-core, sandbox-manager, browser-broker and their compose/config files. Other builders run at the same time in other directories. mp-contracts/proto belong to the session-core group: if you need a proto field, add it minimally and say so.',
  ctl: 'Your lock group is Control Plane user-core (apps/Control Plane/user-core) and Control config. Other builders run at the same time in other directories.',
  idn: 'Your lock group is Control identity services: org-core, audit-core, auth-core and integration-corev2 where named. Other builders run at the same time in other directories.',
  app: 'Your lock group is the Application Plane: apps/Application Plane/convex-core, notification-core, conversation-core. Other builders run at the same time in other directories.',
  gw: 'Your lock group is the Frontend gateway BFF: apps/Frontend Plane/verevonv3/apps/gateway and its nginx/compose entries. It holds no durable Space state. Other builders run at the same time in other directories.',
  'spa-room': 'Your lock group is the SPA room surfaces: apps/Frontend Plane/verevonv3/src/features/spaces, src/shared/api (spaces-client, chat-client), src/shared/actions, src/shared/i18n. The SPA shell group builds settings, app shell and nginx at the same time.',
  'spa-shell': 'Your lock group is the SPA shell and settings: apps/Frontend Plane/verevonv3/src/app, src/features/settings, src/features/core, nginx.conf, index/vite config. The SPA room group builds src/features/spaces at the same time.',
  dat: 'Your lock group is the Data/Ingestion plane files named by your slice.',
}

function prompt(u) {
  const where = u.ws
    ? `Plan subsection: Grep '#### ${u.ws} ' in ${PLAN} (currently lines ${u.planStart}-${u.planEnd}); design file .plan-work/design/${u.ws}.md.`
    : `This is a seam/baseline row, defined in plan sections 4.1.1 (M0 exit criteria), 4.4.2 (prep refactors) and 4.5-4.6: Grep '${u.id}' in ${PLAN}; related workstream slices that consume it name it in their tables.`
  const deps = u.deps.length ? `Dependencies already built (read .plan-work/space-build/units/<id>.json for each): ${u.deps.join(', ')}.` : 'No slice dependencies.'
  return `Read C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/w2/.plan-work/00-build-rules-w2.md first and follow it exactly; your first shell command is cd "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/w2". You build slice ${u.id}: ${u.what} (lane ${u.lane}, size ${u.size}, milestone ${u.ms} of wave W2).
${where}
${deps}
${GROUP_HINT[u.group] || ''}
SKIP IF DONE: if .plan-work/space-build/units/${u.id}.json exists with status built or already-present, read it and return it without changing anything. If it exists with status partial or blocked, or the working tree already contains uncommitted edits for this slice (git status), continue from there instead of starting over.
Build the slice to the plan's scope (not beyond), compile-check once, commit your files only, write the two result files, and return the result object. Depth findings go to the depth file, not into the code. Tests are NOT written now (list them under tests_owed).`
}

phase('Build')
const units = ONLY ? UNITS.filter(u => ONLY.includes(u.id)) : UNITS
const state = {}
const info = {}
units.forEach(u => { state[u.id] = ASSUME_DONE.has(u.id) ? 'built' : 'pending' })
const OKSTATES = ['built', 'partial', 'already-present']
const FAILSTATES = ['blocked', 'failed', 'skipped']
const busy = new Set()
const running = new Map()

function depsOk(u) {
  return u.deps.every(d => !(d in state) || OKSTATES.includes(state[d]))
}
function depsFailed(u) {
  return u.deps.some(d => (d in state) && FAILSTATES.includes(state[d]))
}

function launch(u) {
  state[u.id] = 'running'
  busy.add(u.group)
  log('start ' + u.id + ' (' + u.group + ')')
  const p = (async () => {
    let r = null
    try {
      r = await agent(prompt(u), { label: 'build-' + u.id, phase: 'Build', model: MODEL, effort: 'high', schema: RET })
    } catch (e) {
      r = null
    }
    if (!r) {
      state[u.id] = 'failed'
    } else {
      const s = (r.status || '').toLowerCase()
      state[u.id] = OKSTATES.includes(s) ? s : (s === 'blocked' ? 'blocked' : 'partial')
      info[u.id] = { status: r.status, commit: r.commit, compile: r.compile, notes: (r.notes || '').slice(0, 300) }
    }
    busy.delete(u.group)
    running.delete(u.id)
    const done = Object.values(state).filter(x => OKSTATES.includes(x)).length
    log(u.id + ' -> ' + state[u.id] + ' (' + done + '/' + units.length + ' built or partial)')
    return u.id
  })()
  running.set(u.id, p)
}

while (true) {
  for (const u of units) {
    if (state[u.id] === 'pending' && depsFailed(u)) state[u.id] = 'skipped'
  }
  for (const u of units) {
    if (running.size >= LIMIT) break
    if (state[u.id] === 'pending' && !busy.has(u.group) && depsOk(u)) launch(u)
  }
  if (running.size === 0) break
  await Promise.race(Array.from(running.values()))
}

const counts = {}
Object.values(state).forEach(s => { counts[s] = (counts[s] || 0) + 1 })
log('W2 batch finished: ' + JSON.stringify(counts))
return { counts, state, info }
