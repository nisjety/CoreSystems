import { describe, expect, it } from 'vitest';
import { MessageInbox } from '../../../src/protocol/rustdesk/MessageInbox.js';

interface Message {
  kind: string;
  n?: number;
}

const never = () => new Error('timeout');

describe('MessageInbox', () => {
  it('delivers a message pushed BEFORE anyone waits (the race it exists for)', async () => {
    const inbox = new MessageInbox<Message>();
    inbox.push({ kind: 'hash' });

    await expect(inbox.next((m) => m.kind === 'hash', 0, never)).resolves.toEqual({ kind: 'hash' });
  });

  it('delivers a message pushed AFTER a waiter is registered', async () => {
    const inbox = new MessageInbox<Message>();
    const pending = inbox.next((m) => m.kind === 'hash', 0, never);
    inbox.push({ kind: 'hash' });

    await expect(pending).resolves.toEqual({ kind: 'hash' });
  });

  it('leaves non-matching messages queued for a later waiter', async () => {
    const inbox = new MessageInbox<Message>();
    inbox.push({ kind: 'video' });
    inbox.push({ kind: 'hash' });

    await expect(inbox.next((m) => m.kind === 'hash', 0, never)).resolves.toEqual({ kind: 'hash' });
    await expect(inbox.next((m) => m.kind === 'video', 0, never)).resolves.toEqual({ kind: 'video' });
  });

  it('consumes each queued message only once', async () => {
    const inbox = new MessageInbox<Message>();
    inbox.push({ kind: 'hash', n: 1 });
    inbox.push({ kind: 'hash', n: 2 });

    await expect(inbox.next((m) => m.kind === 'hash', 0, never)).resolves.toEqual({ kind: 'hash', n: 1 });
    await expect(inbox.next((m) => m.kind === 'hash', 0, never)).resolves.toEqual({ kind: 'hash', n: 2 });
  });

  it('rejects with the supplied error when it times out', async () => {
    const inbox = new MessageInbox<Message>();

    await expect(
      inbox.next((m) => m.kind === 'never-arrives', 5, () => new Error('waited too long')),
    ).rejects.toThrow('waited too long');
  });

  it('does not time out a waiter that was already satisfied', async () => {
    const inbox = new MessageInbox<Message>();
    const pending = inbox.next((m) => m.kind === 'hash', 5, () => new Error('should not fire'));
    inbox.push({ kind: 'hash' });

    await expect(pending).resolves.toEqual({ kind: 'hash' });
    // Give the (now cleared) timer a chance to misfire.
    await new Promise((resolve) => setTimeout(resolve, 15));
  });

  it('close() rejects pending waiters and all later calls', async () => {
    const inbox = new MessageInbox<Message>();
    const pending = inbox.next((m) => m.kind === 'hash', 0, never);

    inbox.close(new Error('connection lost'));

    await expect(pending).rejects.toThrow('connection lost');
    await expect(inbox.next((m) => m.kind === 'hash', 0, never)).rejects.toThrow('connection lost');
  });
});
