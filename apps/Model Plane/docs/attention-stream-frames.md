# Attention stream frames

The per-person attention stream (build plan WS16 S5). One stream per signed-in person carries the closed
attention state of that person's threads. It is built behind `SESSION_CORE_ATTENTION_STREAM_ENABLED`, which
defaults off. No live proof is recorded here. The Session Core and Model Gateway parts compile only after the
`attention.proto` frame messages land.

## Path

| Hop | Surface |
|---|---|
| Session Core | `AttentionService.StreamAttention` (server stream, `proto/model_plane/v1/attention.proto`) |
| Model Gateway | `GET /v1/attention/stream` (server-sent events) |
| Frontend BFF | `GET /api/v1/attention/stream` (relayed as the gateway sends it) |

The live session runs browser to BFF to Model Gateway to Session Core. Nothing else is on the path.

## Who may open it

Only the verified caller, for that caller's own threads. The organization and user come from the verified
caller, never from the request. A caller whose retention class is not `persistent` is refused: a
zero-retention or waived caller keeps its attention on the client (DEC-32). The gate is the one MarkThreadSeen
uses, and the posture matrix test pins both call sites.

## Refusals, before the first frame

| Session Core status | Meaning |
|---|---|
| `failed_precondition`, `attention_stream_not_permitted` | The caller is not persistent. |
| `failed_precondition`, `attention_stream_disabled` | The attention switch or the stream switch is off. |
| `resource_exhausted`, `attention_stream_limit` | This replica already holds the cap for the person. |

Model Gateway relays a `failed_precondition` as 412. It maps other codes through its shared thread-error
mapping, so a cap refusal reaches the browser as a server error for now. The BFF holds its own cap of four streams per person per process and answers 429
`attention_stream_limit` before it calls Model Gateway.

## Wire format

Every frame is one server-sent event. The event name is the frame's kind, and the id is `<epoch>:<seq>`:

- `epoch` names one Session Core process (its process id and start time, in hex). A restarted replica answers
  with a new epoch.
- `seq` starts at 1 on each stream and rises by one for every frame that stream sends.

| Event | When | Data |
|---|---|---|
| `attention_snapshot` | The first frame of every stream. Also sent when only the counts changed. | `{"items": [Item, ...], "counts": Counts}` |
| `attention_changed` | One thread's item changed, or the thread left the listing (its item then has state `idle`). | `{"item": Item, "counts": Counts}` |
| `attention_resync` | Session Core could not read the rows. Discard the view; a snapshot follows. | `{}` |
| `attention_heartbeat` | Every 20 seconds on a quiet stream. | `{}` |

An `Item` is `{"thread_id", "state", "unseen", "reason"?, "since"?, "run_id"?}`. `state` is one of
`needs_approval`, `needs_input`, `plan_ready`, `working`, `completed`, `failed`, `archived`, `idle`. `reason`
is `approval`, `plan` or `input`, and is omitted when no pending row set the state. `since` is RFC 3339, and
`run_id` is the thread's latest run; both are omitted when they have no value.

`Counts` is `{"needs_you", "working", "unseen"}`, the counts behind the badge, each thread counted once per group.

The keys are snake_case. The BFF relays the bytes unchanged, so the stream keeps Model Gateway's names, while
the BFF listing `GET /api/v1/attention` is camelCase. Key order is not part of the contract.

An example of the bytes for one thread that waits on an approval:

```
event: attention_snapshot
id: 4f2a-19c3e0a1b2c3d4e5:1
data: {"items":[{"thread_id":"thread-1","state":"needs_approval","reason":"approval","unseen":false,"run_id":"run-1"}],"counts":{"needs_you":1,"working":0,"unseen":0}}

```

## Rules

- A stream opens with a snapshot. Changed frames follow in the order Session Core read them.
- A frame whose sequence number is not newer than the last one applied is stale and is dropped. A gap in the
  sequence, or a frame from a new epoch, means frames were lost: the client reads the stream again.
- A reconnect starts with a fresh snapshot. There is no replay buffer, so frames missed while disconnected are
  not sent again; the snapshot is the truth.
- Frames carry identifiers and the closed state only. No title, preview, tool name, argument or reason text is
  ever in a frame, and frame bodies are not logged.
- Each stream reads the owner's threads once per tick. `SESSION_CORE_ATTENTION_TICK_SECONDS` sets the tick,
  five seconds by default and between one and sixty. A change appears within one tick.
- A person holds at most four streams per Session Core replica and four per BFF process.

## Switches

- `SESSION_CORE_ATTENTION_ENABLED` must be on. The stream reads the attention state it serves (WS16 S1).
- `SESSION_CORE_ATTENTION_STREAM_ENABLED` turns the stream on. Only the literal `true`, in any case with
  surrounding whitespace allowed, enables it. The default is off.
- `ListAttentionResponse.stream_enabled` tells Model Gateway whether to name the stream. The listing then
  carries `stream: true`, and the BFF listing sets `capabilities.stream` from it. With the switch off, the
  listing has no `stream` key and every other response is unchanged.

## Browser behaviour

- One connection per browser profile. The tab that holds the `verevon-attention-stream` lock opens the stream
  and shares its frames over a `BroadcastChannel`. Without the lock API, each tab streams alone.
- While the stream is live, the navbar refreshes every 300 seconds as a backstop. Otherwise it refreshes every
  60 seconds visible and 180 seconds hidden, as before.
- A thread that starts waiting refreshes the navbar once, however many threads changed together. It also sets
  a polite live-region notice in a visible tab that is not showing that thread.

## Not in this slice

- A visible toast. The SPA has no toast component; the notice is a live region for now.
- Web push (WS16 S6). The BFF reports `capabilities.push` as off.
- Proof on a running stack: rebuilt images, `curl -N` through the BFF, and the two-tab and restart checks.
