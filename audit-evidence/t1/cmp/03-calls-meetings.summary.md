# T1 / Category 3: Calls & meetings: cross-source comparison

75 reference records from Teams 33, Slack 21, Grok Bot 10, Buzz 9 and QM 2 were grouped into 26 clusters. 73 records are assigned to clusters. 2 are excluded because they only record that a source lacks a feature (buzz-product-048 and qm-architecture-156); they still count as negative evidence for prevalence. Clusters are in `03-calls-meetings.clusters.json`.

Provenance tags: REA = seen by analysing a shipped app; SRC = read in source or files; DOC = documentation; INF = inference.

## Shared baseline (table stakes)
- **C-001: Calls and huddles started from a room** (Buzz, Slack, Teams). Any channel or chat can start a call, and room membership decides who may join. Buzz is audio only.
- **C-006: Transcript and AI recap saved as an artifact** (Buzz, Slack, Teams). The transcript is posted as messages or a canvas, and the recap lists notes, tasks and chapters.
- **C-014: Calendar access and scheduling** (Teams has a native calendar. Grok Bot, Slack and QM reach a calendar only through integrations or an agent skill). A native calendar is not required to compete.
- QM has none of this. It contributes only a placeholder calendar tab (SRC).

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Calls started from a room (C-001) | Slack (DOC) | The call belongs to the room: one click, optional video and screen share, its own notes thread and canvas, ends when empty. Teams adds "ring others" (REA). |
| Media engine and call lifecycle (C-002) | Teams SlimCore (SRC); Grok Bot for the client side | The engine can preload, unload, recover after crashes and sleep, stop restart loops and fall back to WebRTC. Grok Bot has a clear call state machine with explicit phases (connecting, live, reconnecting, ending, failed). |
| Meeting as a room object (C-003) | Buzz (SRC) | Each meeting is a short-lived private child channel. Signed start and end events link it to the parent room, so it inherits the parent's access rules. |
| Screen sharing (C-004) | Slack (SRC) | A border follows the shared window, and the app's own sensitive windows are left out of the picker. |
| Recap and meeting artifacts (C-006) | Teams (REA) | Notes, tasks, chapters and a speaker timeline, with citations back to the recording. Only people who can see the transcript see the summary. Also attendance and a list of who lacks access to shared content. |
| AI controls in meetings (C-007) | Teams (REA) | Per-meeting AI on/off, a privacy mode that blocks every agent, answers that disappear when the meeting ends, and settings an admin can lock. |
| Agents speaking in rooms (C-010) | Buzz (SRC) | Agents are named audio participants. They hear humans through speech-to-text, answer with speech, and stop when a human starts talking. |
| Voice chat with a personal agent (C-011) | Grok Bot (SRC) | A fast voice front hands real work to the main agent through `send_task`. It can search earlier text chats and calls together, and each agent has its own voice settings. |
| Agents attending outside meetings (C-012) | Grok Bot (SRC, medium confidence) | The agent joins Zoom or Meet as a visible guest after consent, reads the transcript, and posts in meeting chat only through drafts the user approves. |
| Calendar-triggered agent work (C-014) | Grok Bot (SRC) | Agent work starts at a calendar event, prep packs are built before meetings, and scheduling needs approval. |
| Device and permission checks (C-015) | Slack (SRC) | Permission checks run in a separate process with a timeout and cache, and the app tests that a device actually opens. |

## Differentiators
- **Edge for an agent-first product:**
  - Grok Bot's voice-front/worker split and its agents attending outside meetings (C-011, C-012).
  - Buzz's agents as voice participants and its meeting-as-child-room model (C-010, C-003).
  - Teams' AI privacy and consent controls (C-007).
- **Edge for human calling only:** Teams captions and interpretation, telephony, webinars, hardware support and on-device audio/video processing (C-008, C-016, C-022, C-025, C-026). Slack's restart veto and keep-awake handling (C-019).

## Verevon's biggest gaps (status from code; nothing in this category is shipped)
1. **C-001 Calls: absent, priority, significance 5.** There is no WebRTC, SFU or call code (baseline v-space-backend-035 and v-space-ui-036). The Space definition doc lists huddles as a non-goal (`docs/space-defenition.md:352`), so adding calls first needs a product decision. Recommendation: buy an SFU (LiveKit or similar). Calls become an Application Plane projection, and the gateway issues a call token only after checking Space membership.
2. **C-006 Transcript and recap: partial, priority, significance 5.** The only piece today is the text-only Meeting Scribe template (`SpaceCreateAgentDialog.tsx:43-54`). The WhisperX speech-to-text and recap pipeline exists only in docs (`Channel Plane/docs/meeting-intelligence.md`). Build it in the Model Plane, with transcripts in the Data Plane and the recap saved as a Space document.
3. **C-012 Agents attend outside meetings: planned, priority, significance 4.** This is meeting-intelligence phase 4 and has no code. The capture worker belongs in Ingestion/Quarry: the agent proposes, Quarry executes.
4. **C-007 AI consent and privacy mode: absent, priority, significance 4.** The Control Plane should own the policy and deny by default, following the ZDR pattern. It must ship together with items 2 and 3.
5. **Recommended next:**
   - C-003 meeting as a child Space
   - C-004 screen sharing
   - C-010 agents speaking in rooms
   - C-011 duplex voice with delegation
   - C-014 calendar triggers
   - C-015 device selection
   - C-002 call state machine

## Verevon strengths to keep
- **Realtime voice routes exist** in the Model Gateway: `/v1/ai/realtime*` at `http_routes.rs:448-450`.
- **Voice mode is built and tested** (`features/voice/*`): tap-to-talk dictation, a state machine that reports ZDR errors, a mic-error classifier, and read-aloud through Azure Speech. It is only mounted in the dashboard composer, not in Spaces. It gives C-002, C-011 and C-015 a head start.
- **The meeting-intelligence design is sound.** It treats a meeting as a source type that reuses existing retrieval, graph, wiki, ZDR and org scoping. That is architecturally better than separate meeting stores.
- **Agents act only when explicitly invoked**, and there is an agent run live view (`FleetRunConsole.tsx`). Both are good foundations for C-007 and for an "agent is acting here" indicator.
- No cluster is at parity yet, so none is tier n/a.

## Caveats
- Slack's huddle features, AI notes and meeting-aware Slackbot come from documentation. The Slackbot evidence is low confidence. Teams E2EE (C-023) is documentation only.
- Grok Bot's meeting-bot records are SRC but medium confidence (they come from strings and RPC names).
- Buzz mobile huddles are DOC.
- Prevalence for calendar (C-014) counts integration-only support in Slack and QM.
- Global hotkeys (C-018) list Buzz on the strength of a record clustered in C-002.
- Verevon is web-only (no desktop shell under `verevonv3/apps`), so shell features (C-018, C-019, C-020) need web substitutes such as the Wake Lock API, in-tab push-to-talk and protocol handlers.
- C-019 has one item to check: whether service-worker updates are deferred while an agent run is in progress.
- Statuses for clusters with no direct baseline record (C-007, C-009, C-013, C-016 to C-026) are inferred from the absent-calls baseline v-space-backend-035. C-013 and C-015 were also checked by grep.
