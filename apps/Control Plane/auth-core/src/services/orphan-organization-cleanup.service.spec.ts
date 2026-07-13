import { ownerlessOrganizationMode } from './orphan-organization-cleanup.service';

describe('ownerless organization safety policy', () => {
  it.each([undefined, '', 'delete', 'true', '1', 'unexpected'])(
    'defaults %p to off and never enables automatic deletion',
    (value) => {
      expect(ownerlessOrganizationMode(value)).toBe('off');
    },
  );

  it('supports an explicit read-only report mode', () => {
    expect(ownerlessOrganizationMode('report')).toBe('report');
    expect(ownerlessOrganizationMode(' REPORT ')).toBe('report');
  });
});
