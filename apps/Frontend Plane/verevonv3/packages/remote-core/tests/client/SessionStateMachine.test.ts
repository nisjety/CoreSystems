import { describe, expect, it } from 'vitest';
import { SessionStateMachine } from '../../src/client/SessionStateMachine.js';
import { ProtocolError } from '../../src/errors/RemoteError.js';

describe('SessionStateMachine', () => {
  it('starts idle and follows the documented happy path', () => {
    const machine = new SessionStateMachine();
    expect(machine.state).toBe('idle');

    machine.transition('connecting');
    machine.transition('rendezvous');
    machine.transition('authenticating');
    machine.transition('connected');

    expect(machine.state).toBe('connected');
  });

  it('rejects illegal transitions instead of silently allowing them', () => {
    const machine = new SessionStateMachine();
    expect(() => machine.transition('connected')).toThrow(ProtocolError);
  });

  it('has no outgoing transitions from terminal states', () => {
    const machine = new SessionStateMachine();
    machine.transition('connecting');
    machine.transition('failed', 'protocol-error');

    expect(machine.canTransition('connecting')).toBe(false);
    expect(machine.canTransition('disconnected')).toBe(false);
  });

  it('supports the reconnecting -> connected recovery path', () => {
    const machine = new SessionStateMachine();
    machine.transition('connecting');
    machine.transition('rendezvous');
    machine.transition('authenticating');
    machine.transition('connected');
    machine.transition('reconnecting', 'transient-network');
    machine.transition('connected');

    expect(machine.state).toBe('connected');
  });

  it('notifies listeners with from/to on every transition', () => {
    const machine = new SessionStateMachine();
    const changes: string[] = [];
    machine.onChange((change) => changes.push(`${change.from}->${change.to}`));

    machine.transition('connecting');

    expect(changes).toEqual(['idle->connecting']);
  });
});
