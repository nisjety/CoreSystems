import { Logger } from '@nestjs/common';

const mockFlushInvitationAcceptanceRepairs = jest.fn();
jest.mock('../auth/invitation-acceptance-repair', () => ({
  flushInvitationAcceptanceRepairs: mockFlushInvitationAcceptanceRepairs,
}));

import { InvitationAcceptanceRepairService } from './invitation-acceptance-repair.service';

describe('InvitationAcceptanceRepairService', () => {
  let log: jest.SpyInstance;
  let error: jest.SpyInstance;
  let warn: jest.SpyInstance;

  beforeEach(() => {
    jest.clearAllMocks();
    log = jest.spyOn(Logger.prototype, 'log').mockImplementation();
    error = jest.spyOn(Logger.prototype, 'error').mockImplementation();
    warn = jest.spyOn(Logger.prototype, 'warn').mockImplementation();
  });

  afterEach(() => {
    log.mockRestore();
    error.mockRestore();
    warn.mockRestore();
  });

  it('records completed repair counts without invitation identifiers', async () => {
    mockFlushInvitationAcceptanceRepairs.mockResolvedValue({
      claimed: 3,
      completed: 2,
      superseded: 0,
      notRepairable: 0,
      retried: 1,
      deadLettered: 0,
    });
    await new InvitationAcceptanceRepairService().reconcileAcceptedInvitations();
    expect(log).toHaveBeenCalledWith({
      event: 'invitation_acceptance_repair_sweep',
      claimed: 3,
      completed: 2,
      superseded: 0,
      notRepairable: 0,
      retried: 1,
      deadLettered: 0,
    });
    expect(warn).not.toHaveBeenCalled();
  });

  it('stays quiet when there is no work', async () => {
    mockFlushInvitationAcceptanceRepairs.mockResolvedValue({
      claimed: 0,
      completed: 0,
      superseded: 0,
      notRepairable: 0,
      retried: 0,
      deadLettered: 0,
    });
    await new InvitationAcceptanceRepairService().reconcileAcceptedInvitations();
    expect(log).not.toHaveBeenCalled();
    expect(error).not.toHaveBeenCalled();
    expect(warn).not.toHaveBeenCalled();
  });

  it('raises a structured operator warning when work reaches dead letter', async () => {
    mockFlushInvitationAcceptanceRepairs.mockResolvedValue({
      claimed: 1,
      completed: 0,
      superseded: 0,
      notRepairable: 0,
      retried: 0,
      deadLettered: 1,
    });
    await new InvitationAcceptanceRepairService().reconcileAcceptedInvitations();
    expect(warn).toHaveBeenCalledWith({
      event: 'invitation_acceptance_repair_dead_letter',
      deadLettered: 1,
      action:
        'inspect invitation_acceptance_repair without exposing invitation identifiers',
    });
  });

  it('contains sweep failures inside the scheduler', async () => {
    const failure = new Error('database unavailable');
    mockFlushInvitationAcceptanceRepairs.mockRejectedValue(failure);
    await new InvitationAcceptanceRepairService().reconcileAcceptedInvitations();
    expect(error).toHaveBeenCalledWith(
      'Invitation acceptance repair sweep failed',
      failure,
    );
  });
});
