#!/usr/bin/env node

import http from 'node:http';
import * as number from 'lib0/number';
import WebSocket, { WebSocketServer } from 'ws';
import { authenticatePlannerRequest, parsePlannerRequest, rejectUpgrade } from './auth.js';
import { closeEventing, publishPlannerEvent } from './eventing.js';
import { setupWSConnection } from './utils.js';

const host = process.env.HOST || '0.0.0.0';
const port = number.parseInt(process.env.PORT || '1234');

const wss = new WebSocketServer({ noServer: true });

const server = http.createServer((_request, response) => {
  response.writeHead(200, { 'Content-Type': 'application/json' });
  response.end(JSON.stringify({ status: 'ok', service: 'planner-sync-core' }));
});

wss.on('connection', setupWSConnection);

server.on('upgrade', async (request, socket, head) => {
  const plannerRequest = parsePlannerRequest(request);
  const authenticated = await authenticatePlannerRequest(request);
  if (!authenticated.ok) {
    void publishPlannerEvent('aqencia.application.planner.transport.denied', {
      room: plannerRequest.room,
      workspace_id: plannerRequest.workspaceId || null,
      document_id: plannerRequest.documentId || null,
      source: plannerRequest.source,
      reason: authenticated.error,
      occurred_at: new Date().toISOString(),
    });
    rejectUpgrade(socket, authenticated.status, authenticated.error);
    return;
  }

  request.aqenciaAuthContext = authenticated.context;
  wss.handleUpgrade(request, socket, head, (ws) => {
    wss.emit('connection', ws, request);
  });
});

server.listen(port, host, () => {
  console.log(`planner-sync-core running at '${host}' on port ${port}`);
});

const shutdown = async () => {
  wss.close();
  server.close();
  await closeEventing();
};

process.on('SIGINT', () => {
  void shutdown();
});

process.on('SIGTERM', () => {
  void shutdown();
});
