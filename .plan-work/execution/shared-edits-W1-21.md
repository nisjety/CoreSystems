## WS08-S4.code
shared-edit apps/Model Plane/deploy/docker-compose.yml: add `CHAT_VISION_AS_PARTS_ENABLED: ${CHAT_VISION_AS_PARTS_ENABLED:-}` to the model-gateway environment block. The default must stay unset or off.

## WS08-S4.code
shared-edit apps/Frontend Plane/verevonv3/docker-compose.yml and .env.example: add VITE_CHAT_VISION_AS_PARTS_ENABLED (default empty or false) beside VITE_PLAN_EXECUTE_ENABLED. It must be flipped together with the gateway switch.

