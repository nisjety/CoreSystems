#!/usr/bin/env node

import http from 'node:http';
import * as number from 'lib0/number';
import WebSocket, { WebSocketServer } from 'ws';
import { setupWSConnection } from './utils.js';

const host = process.env.HOST || '0.0.0.0';
const port = number.parseInt(process.env.PORT || '1234');

const wss = new WebSocketServer({ noServer: true });

const server = http.createServer((_request, response) => {
  response.writeHead(200, { 'Content-Type': 'text/plain' });
  response.end('planner-yjs-sync okay');
});

wss.on('connection', setupWSConnection);

server.on('upgrade', (request, socket, head) => {
  wss.handleUpgrade(request, socket, head, (ws) => {
    wss.emit('connection', ws, request);
  });
});

server.listen(port, host, () => {
  console.log(`planner-yjs-sync running at '${host}' on port ${port}`);
});