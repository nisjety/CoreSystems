import { lookup } from 'node:dns/promises';
import { createServer, request } from 'node:http';
import { BlockList, connect, isIP } from 'node:net';

const blocked = new BlockList();
for (const [network, prefix] of [
  ['0.0.0.0', 8], ['10.0.0.0', 8], ['100.64.0.0', 10], ['127.0.0.0', 8],
  ['169.254.0.0', 16], ['172.16.0.0', 12], ['192.0.0.0', 24], ['192.0.2.0', 24],
  ['192.88.99.0', 24], ['192.168.0.0', 16], ['198.18.0.0', 15], ['198.51.100.0', 24],
  ['203.0.113.0', 24], ['224.0.0.0', 3],
]) blocked.addSubnet(network, prefix, 'ipv4');
for (const [network, prefix] of [
  ['2001::', 23], ['2001:db8::', 32], ['2002::', 16], ['3fff::', 20],
]) blocked.addSubnet(network, prefix, 'ipv6');

const globalV6 = new BlockList();
globalV6.addSubnet('2000::', 3, 'ipv6');

export function isPublicAddress(address) {
  const family = isIP(address);
  if (family === 4) return !blocked.check(address, 'ipv4');
  return family === 6 && globalV6.check(address, 'ipv6') && !blocked.check(address, 'ipv6');
}

export function publicWebUrl(value, httpsOnly = false) {
  if (typeof value !== 'string' || value.length > 4096) throw new Error('Ugyldig nettadresse.');
  const url = new URL(value);
  const allowedProtocol = url.protocol === 'https:' || (!httpsOnly && url.protocol === 'http:');
  const host = url.hostname.replace(/^\[|\]$/g, '').replace(/\.$/, '');
  if (!allowedProtocol || url.username || url.password || url.port || !host ||
      (isIP(host) ? !isPublicAddress(host) : !host.includes('.') ||
        /(^|\.)(localhost|local|internal|invalid|test|onion|arpa)$/.test(host))) {
    throw new Error('Nettleseren kan bare åpne offentlige nettadresser.');
  }
  return url;
}

async function resolvePublic(url) {
  const host = url.hostname.replace(/^\[|\]$/g, '');
  const addresses = isIP(host) ? [{ address: host }] : await lookup(host, { all: true });
  if (addresses.length === 0 || addresses.some(({ address }) => !isPublicAddress(address))) {
    throw new Error('Nettadressen viser til et privat nettverk.');
  }
  return addresses;
}

function cleanHeaders(headers) {
  const cleaned = { ...headers };
  const connection = String(headers.connection ?? '').split(',').map((key) => key.trim().toLowerCase());
  for (const key of [...connection, 'connection', 'keep-alive', 'proxy-authorization',
    'proxy-authenticate', 'proxy-connection', 'te', 'trailer', 'transfer-encoding', 'upgrade']) {
    delete cleaned[key];
  }
  return cleaned;
}

// The desktop Chromium session uses this CONNECT proxy without a DIRECT fallback.
// Resolve once, validate every answer, then dial the approved IP to prevent DNS rebinding.
export async function startPublicWebProxy() {
  const sockets = new Set();
  const server = createServer(async (incoming, response) => {
    let url;
    let addresses;
    try {
      url = publicWebUrl(incoming.url);
      if (url.protocol !== 'http:') throw new Error('Expected HTTP');
      addresses = await resolvePublic(url);
    } catch {
      response.writeHead(403).end();
      return;
    }
    if (incoming.destroyed) return;
    const upstream = request({
      host: addresses[0].address,
      port: 80,
      method: incoming.method,
      path: `${url.pathname}${url.search}`,
      headers: { ...cleanHeaders(incoming.headers), host: url.host },
      timeout: 20_000,
    }, (result) => {
      response.writeHead(result.statusCode ?? 502, cleanHeaders(result.headers));
      result.pipe(response);
    });
    upstream.on('timeout', () => upstream.destroy());
    upstream.on('error', () => {
      if (!response.headersSent) response.writeHead(502);
      response.end();
    });
    response.on('close', () => upstream.destroy());
    incoming.pipe(upstream);
  });

  server.on('connection', (socket) => {
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
    socket.on('error', () => socket.destroy());
  });

  server.on('connect', async (incoming, socket, head) => {
    socket.pause();
    let addresses;
    try {
      if (!/^(?:[a-z0-9.-]+|\[[a-f0-9:]+\]):443$/i.test(incoming.url ?? '')) {
        throw new Error('Invalid CONNECT authority');
      }
      const url = publicWebUrl(`https://${incoming.url}`, true);
      addresses = await resolvePublic(url);
    } catch {
      socket.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');
      return;
    }
    for (const { address } of addresses) {
      if (socket.destroyed) return;
      const upstream = connect({ host: address, port: 443 });
      socket.once('close', () => upstream.destroy());
      const connected = await new Promise((resolve) => {
        const timer = setTimeout(() => upstream.destroy(), 3_000);
        upstream.once('connect', () => { clearTimeout(timer); resolve(true); });
        upstream.once('close', () => { clearTimeout(timer); resolve(false); });
        upstream.on('error', () => upstream.destroy());
      });
      if (!connected) continue;
      socket.write('HTTP/1.1 200 Connection Established\r\n\r\n');
      if (head.length) upstream.write(head);
      upstream.pipe(socket);
      socket.pipe(upstream);
      socket.resume();
      return;
    }
    socket.end('HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n');
  });

  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  return {
    port: server.address().port,
    close() {
      for (const socket of sockets) socket.destroy();
      server.close();
    },
  };
}
