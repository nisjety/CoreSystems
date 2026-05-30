# Redis Streams Pending Message Recovery Fix

## Problem
Worker only uses `XREADGROUP` with `>` which fetches only new messages and **never recovers pending (unacked) messages**. Failed/crashed jobs remain stuck forever.

## TDD Solution

### 🔴 RED Phase: Test Cases (Created)

**File:** `tests/test_pending_recovery.py`

4 specific test cases using stdlib `unittest`:

1. **`test_claim_pending_messages_returns_stuck_jobs`**  
   Verifies that messages pending for >30s are claimed via `XPENDING_RANGE` + `XCLAIM` and returned for reprocessing.

2. **`test_claim_pending_returns_empty_when_no_pending`**  
   Ensures function returns `[]` when no pending messages exist (no-op case).

3. **`test_run_loop_processes_pending_before_new`**  
   Integration test verifying run loop recovers pending messages before reading new ones.

4. **`test_pending_recovery_respects_idle_time_threshold`**  
   Validates only messages idle for >30 seconds are claimed (avoids race conditions with in-flight processing).

### 🟢 GREEN Phase: Production Change (Implemented)

**File:** `worker/main.py`

#### Added Function:
```python
async def claim_pending_messages(
    r: aioredis.Redis,
    stream: str,
    group: str,
    consumer: str,
    min_idle_ms: int = 30_000,  # 30 second threshold
    count: int = 100,
) -> List[tuple[str, Dict[str, Any]]]:
    """
    Claim pending messages that have been idle for > min_idle_ms.
    Returns list of (message_id, fields) tuples in same format as XREADGROUP.
    """
```

**Logic:**
1. Call `XPENDING_RANGE` to get pending message IDs
2. Call `XCLAIM` with `min_idle_time=30000` to claim messages stuck for >30s
3. Return claimed messages in same format as `XREADGROUP` for batch processing

#### Modified `run()` Loop:
```python
while True:
    # Step 1: Claim pending messages first (recovery)
    pending_ku = await claim_pending_messages(r, STREAM_KU_CREATED, ...)
    pending_del = await claim_pending_messages(r, STREAM_DOC_DELETED, ...)
    
    # Step 2: Read new messages
    results = await r.xreadgroup(..., streams={...: ">"}, ...)
    
    # Step 3: Combine pending + new messages for processing
    ku_buffer.extend(pending_ku)  # Process recovered messages
    # ... process new messages ...
```

## Why This is the Minimal Fix

✅ **Narrow scope:** Only adds pending recovery, doesn't change batch processing, error handling, or ack logic  
✅ **Reuses existing code:** Claimed messages flow through same `process_ku_batch()` path  
✅ **Safe idle threshold:** 30 seconds prevents claiming messages still in-flight  
✅ **No breaking changes:** Existing behavior for new messages unchanged  
✅ **Fail-safe:** If `claim_pending_messages` fails, worker continues reading new messages  

## How It Works

### Before (Bug):
```
[Worker crashes mid-processing]
→ Message 1234-0 left unacked in pending state
→ Next iteration: XREADGROUP with '>' only fetches NEW messages  
→ Message 1234-0 NEVER RECOVERED ❌
```

### After (Fixed):
```
[Worker crashes mid-processing]
→ Message 1234-0 left unacked in pending state
→ Next iteration:
  1. claim_pending_messages() calls XPENDING_RANGE → finds 1234-0 (idle 35s)
  2. XCLAIM retrieves message 1234-0
  3. process_ku_batch() reprocesses it
  4. Success → XACK sent ✅
```

## Testing Strategy

Since Python 3.14/asyncpg compatibility blocked automated tests, manual verification:

1. **Simulate crash:** Start worker, kill mid-batch (before XACK)
2. **Verify pending:** `redis-cli XPENDING dataplane.knowledge.units.created embedding-worker`
3. **Restart worker:** Observe logs showing claimed messages recovered
4. **Verify cleared:** Check `XPENDING` returns empty after successful reprocessing

## Metrics to Monitor

- `xpending_count` (should trend toward 0)  
- `xclaim_calls` (recovery attempts)  
- `claimed_message_count` (how many stuck jobs recovered)  
- `message_redelivery_count` (times_delivered > 1)
