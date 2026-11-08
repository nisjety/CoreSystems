import sys
sys.path.insert(0, r'C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\w314guard')
from edit import edit

edit('docker-compose.yml', [
("""      MODEL_GATEWAY_WEB_SEARCH_BATCH: ${MODEL_GATEWAY_WEB_SEARCH_BATCH:-false}
""", """      MODEL_GATEWAY_WEB_SEARCH_BATCH: ${MODEL_GATEWAY_WEB_SEARCH_BATCH:-false}
      # EP-2-S4: cited passages on citations and the research report's provenance stamp. Only the literal true.
      MODEL_GATEWAY_CITATION_PASSAGES: ${MODEL_GATEWAY_CITATION_PASSAGES:-false}
""", 1),
("""      MODEL_GATEWAY_SUBAGENT_ROLE_CATALOGUE: ${MODEL_GATEWAY_SUBAGENT_ROLE_CATALOGUE:-false}
""", """      MODEL_GATEWAY_SUBAGENT_ROLE_CATALOGUE: ${MODEL_GATEWAY_SUBAGENT_ROLE_CATALOGUE:-false}
      # EP-2 S8: the composer's wide-research entry (estimate route + wide_research on a Do run). Only the literal true.
      WIDE_RESEARCH_ENTRY_ENABLED: ${WIDE_RESEARCH_ENTRY_ENABLED:-false}
""", 1),
("""      APPROVAL_STEP_UP_ENFORCED: ${APPROVAL_STEP_UP_ENFORCED:-off}
""", """      APPROVAL_STEP_UP_ENFORCED: ${APPROVAL_STEP_UP_ENFORCED:-off}
      # EP-10-S1: the owner-only decrypted preview of a pending write approval, never stored. Only the literal true;
      # the SPA's VITE_APPROVAL_WRITE_PREVIEW must be on too for the disclosure to show.
      APPROVAL_WRITE_PREVIEW_ENABLED: ${APPROVAL_WRITE_PREVIEW_ENABLED:-false}
""", 1),
("""      EXECUTION_DO_BROWSER_ENABLED: ${EXECUTION_DO_BROWSER_ENABLED:-0}
""", """      EXECUTION_DO_BROWSER_ENABLED: ${EXECUTION_DO_BROWSER_ENABLED:-0}
      # EP-1 S2: a wall the person may clear (sign-in, challenge) pauses the run for them. Only the literal true.
      EXECUTION_BROWSER_HUMAN_ASSIST: ${EXECUTION_BROWSER_HUMAN_ASSIST:-false}
      # EP-1 S5: how long a takeover pause waits for the browser back, whole seconds clamped to 300-3600. A setting,
      # not a switch: only a takeover pause reads it.
      EXECUTION_BROWSER_ASSIST_TTL_SECONDS: ${EXECUTION_BROWSER_ASSIST_TTL_SECONDS:-1800}
""", 1),
])
