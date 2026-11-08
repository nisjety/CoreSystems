## WS27-S10
shared-edit C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\apps\Frontend Plane\verevonv3\src\features\chat\components\ChatPanels.tsx near line 914 (StepsPanel, which has threadId and activeTask()): change `<RunCostMeter meter={props.meter} />` to `<RunCostMeter meter={props.meter} quotaRefreshKey={`${props.threadId ?? ''}:${activeTask() ? 'live' : 'settled'}`} />`. This reads on Work open and at each task start and settle, never per frame.

## WS27-S10
shared-edit C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\apps\Frontend Plane\verevonv3\src\features\chat\components\ChatLiveRunPanel.tsx near line 590 (`fallback={<RunCostMeter meter={props.meter} />}`): add `quotaRefreshKey={props.runId ?? undefined}`. This panel carries no settle signal, so it reads once per watched run.

## WS27-S10
shared-edit C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\apps\Frontend Plane\verevonv3\src\features\agents\components\AgentRunConsole.tsx near line 1492 (`<RunCostMeter meter={usage()} />`): add a key from state.runId and a settled flag on state.status (RunStatus), for example `quotaRefreshKey={`${state.runId ?? ''}:${state.status}`}`.

## WS18-S5
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway service environment, after the MODEL_GATEWAY_SPACE_APPEND_RETRIEVAL line (~line 361), add a WS18-S5 comment and the line `AGENTIC_SPACE_RETRIEVAL_ENABLED: ${AGENTIC_SPACE_RETRIEVAL_ENABLED:-false}`. Default off.

