## EP-14-S4
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx (1 of 3): in the useChatController() destructure, next to 'uiEvents,' (about line 125), add 'ledgerInspect,' and 'inspectLedgerCall,'.

## EP-14-S4
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx (2 of 3): on the <StepsPanel> mount (about line 937, after 'onStopTask={handleStop}') add 'onInspectCall={inspectLedgerCall}'.

## EP-14-S4
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx (3 of 3): change the Trace mount (about line 956) to <TracePanel runId={liveRunId()} events={uiEvents()} replayTruncated={traceReplayTruncated()} inspectRequest={ledgerInspect()} />. Apply all three together; line 3 reads ledgerInspect from line 1. Props are optional, so the ledger still works without them, but Inspect is absent and the Trace opens unselected.

## EP-14-S4
shared-edit apps/Frontend Plane/verevonv3/src/styles/global.css: append these rules after '.verevon-chat-trace-log__empty' (about line 23829):

