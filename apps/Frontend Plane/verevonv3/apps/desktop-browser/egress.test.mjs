import assert from 'node:assert/strict';
import { connect } from 'node:net';
import { test } from 'node:test';
import {
  isPublicAddress, publicWebUrl, startPublicWebProxy,
} from '../../../../Ingestion Plane/Quarry-v2/adapters/desktop-browser/egress.mjs';

test('desktop browsing refuses local, private, and special-use endpoints', () => {
  for (const url of ['https://localhost/', 'https://127.0.0.1/', 'https://2130706433/',
    'https://[::ffff:127.0.0.1]/', 'https://169.254.169.254/', 'https://host.internal/',
    'https://example.com:8443/', 'https://user:password@example.com/', 'file:///etc/passwd']) {
    assert.throws(() => publicWebUrl(url), undefined, url);
  }
  assert.throws(() => publicWebUrl('http://example.com', true));
  assert.equal(publicWebUrl('https://www.finn.no/', true).hostname, 'www.finn.no');
  assert.equal(isPublicAddress('2606:4700:4700::1111'), true);
  assert.equal(isPublicAddress('8.8.8.8'), true);
});

test('the native Chromium CONNECT path rejects private addresses before dialing', async () => {
  const proxy = await startPublicWebProxy();
  try {
    const response = await new Promise((resolve, reject) => {
      const socket = connect(proxy.port, '127.0.0.1');
      socket.on('error', reject);
      socket.on('data', (chunk) => resolve(chunk.toString()));
      socket.on('connect', () => socket.write('CONNECT 127.0.0.1:443 HTTP/1.1\r\nHost: 127.0.0.1:443\r\n\r\n'));
    });
    assert.match(response, /^HTTP\/1\.1 403 /);
  } finally {
    proxy.close();
  }
});
