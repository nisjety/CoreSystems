import { Logger } from '@nestjs/common';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

// ZDR-Z-03b: with AUTH_CORE_ZDR_EFFECTIVE_POSTURE_ENABLED on, Auth Core reads
// org-core's effectiveZdr instead of computing intent && entitled. Every
// org_posture_rows row of the vendored matrix is driven through the real
// resolver; only fetch (org-core) is a double, and it serves exactly what the
// matrix row says org-core answers (state and lapsed), never a rule restated
// here.
const SERVICE_TOKEN = 'unit-spec-service-token-0123456789abcdef0123';
process.env.ORG_CORE_SERVICE_TOKEN = SERVICE_TOKEN;

import {
  RetentionPostureUnavailableError,
  resetEffectivePostureReadForTests,
  resetInteractiveRetentionCacheForTests,
  resolveInteractiveRetentionPosture,
} from './interactive-retention-policy';

const FIXTURES = join(__dirname, 'fixtures');

type OrgRow = {
  id: string;
  inputs: {
    intent: boolean | null;
    entitled: boolean | null;
    lookup: 'ok' | 'unavailable';
  };
  expect: { state: 'zdr' | 'not_zdr' | 'unknown'; lapsed: boolean };
};
type Matrix = {
  policy: { plan_lapse: string };
  org_posture_rows: OrgRow[];
};

const MATRIX_PATH = join(FIXTURES, 'zdr-posture-matrix.json');
const matrix = JSON.parse(readFileSync(MATRIX_PATH, 'utf8')) as Matrix;
const ORG = 'org-matrix-1';

function okReply(body: unknown) {
  return { ok: true, status: 200, json: () => Promise.resolve(body) };
}

/** org-core's answer for a lookup-ok row, as ZDR-Z-03a's handler serves it. */
function orgCoreBody(row: OrgRow): Record<string, unknown> {
  return {
    zdrIntent: row.inputs.intent === true,
    entitled: row.inputs.entitled === true,
    effectiveZdr: row.expect.state === 'zdr',
    lapsed: row.expect.lapsed,
    planLapsePolicy: matrix.policy.plan_lapse,
  };
}

function setSwitch(value: string | undefined): void {
  if (value === undefined)
    delete process.env.AUTH_CORE_ZDR_EFFECTIVE_POSTURE_ENABLED;
  else process.env.AUTH_CORE_ZDR_EFFECTIVE_POSTURE_ENABLED = value;
  resetEffectivePostureReadForTests();
  resetInteractiveRetentionCacheForTests();
}

describe('interactive retention policy against the ZDR posture matrix (ZDR-Z-03b)', () => {
  const original = process.env.AUTH_CORE_ZDR_EFFECTIVE_POSTURE_ENABLED;
  let fetchSpy: jest.SpyInstance | undefined;
  let warned: string[];
  let errored: string[];

  beforeEach(() => {
    warned = [];
    errored = [];
    jest
      .spyOn(Logger.prototype, 'warn')
      .mockImplementation((message: unknown) => {
        warned.push(String(message));
      });
    jest
      .spyOn(Logger.prototype, 'error')
      .mockImplementation((message: unknown) => {
        errored.push(String(message));
      });
  });

  afterEach(() => {
    fetchSpy?.mockRestore();
    fetchSpy = undefined;
    jest.restoreAllMocks();
  });

  afterAll(() => {
    setSwitch(original);
  });

  function serve(body: unknown): void {
    fetchSpy = jest
      .spyOn(global, 'fetch')
      .mockImplementation(async () => okReply(body) as unknown as Response);
  }

  it('the vendored matrix hashes to its pin (CRLF normalised to LF)', () => {
    const bytes = readFileSync(MATRIX_PATH, 'utf8').replace(/\r\n/g, '\n');
    const pin = readFileSync(`${MATRIX_PATH}.sha256`, 'utf8')
      .trim()
      .split(/\s+/)[0];
    expect(createHash('sha256').update(bytes, 'utf8').digest('hex')).toBe(pin);
  });

  it('the matrix has the rows this spec relies on', () => {
    const ids = matrix.org_posture_rows.map((row) => row.id);
    expect(ids).toContain('o_plan_lapsed_keeps_zdr');
    expect(
      matrix.org_posture_rows.some(
        (row) => row.inputs.lookup === 'unavailable',
      ),
    ).toBe(true);
    expect(matrix.policy.plan_lapse).toBe('intent_stays_zdr');
  });

  describe('switch on', () => {
    beforeEach(() => setSwitch('true'));

    it.each(matrix.org_posture_rows.map((row) => [row.id, row] as const))(
      '%s',
      async (_id, row) => {
        if (row.inputs.lookup === 'unavailable') {
          fetchSpy = jest
            .spyOn(global, 'fetch')
            .mockRejectedValue(new TypeError('fetch failed'));
          await expect(
            resolveInteractiveRetentionPosture(ORG),
          ).rejects.toBeInstanceOf(RetentionPostureUnavailableError);
          return;
        }
        serve(orgCoreBody(row));
        const posture = await resolveInteractiveRetentionPosture(ORG);
        expect(posture.zdr).toBe(row.expect.state === 'zdr');
      },
    );

    it('follows effectiveZdr even where intent && entitled would say otherwise', async () => {
      // A lapsed org: intent on, entitlement gone, org-core says ZDR.
      serve({
        zdrIntent: true,
        entitled: false,
        effectiveZdr: true,
        lapsed: true,
      });
      expect((await resolveInteractiveRetentionPosture(ORG)).zdr).toBe(true);
    });

    it('logs plan_lapsed_zdr_kept with the org id only for a lapsed org', async () => {
      serve({
        zdrIntent: true,
        entitled: false,
        effectiveZdr: true,
        lapsed: true,
      });
      await resolveInteractiveRetentionPosture(ORG);
      const line = warned.find((message) => message.includes('plan_lapsed'));
      expect(line).toBe(`plan_lapsed_zdr_kept: org=${ORG}`);
      expect(
        warned.some((message) => message.includes('plan_not_entitled')),
      ).toBe(false);
    });

    it.each([
      [
        'effectiveZdr missing',
        { zdrIntent: true, entitled: true, lapsed: false },
      ],
      [
        'lapsed missing',
        { zdrIntent: true, entitled: true, effectiveZdr: true },
      ],
      [
        'effectiveZdr not a boolean',
        {
          zdrIntent: false,
          entitled: true,
          effectiveZdr: 'true',
          lapsed: false,
        },
      ],
      [
        'lapsed not a boolean',
        { zdrIntent: false, entitled: true, effectiveZdr: false, lapsed: 0 },
      ],
    ])(
      '%s is a malformed answer: 503, nothing guessed',
      async (_case, body) => {
        serve(body);
        const error = await resolveInteractiveRetentionPosture(ORG).then(
          () => undefined,
          (caught: unknown) => caught,
        );
        expect(error).toBeInstanceOf(RetentionPostureUnavailableError);
        expect(
          JSON.stringify(
            (error as RetentionPostureUnavailableError).getResponse(),
          ),
        ).toContain('retention_posture_unavailable');
      },
    );

    it('a cached entry written with the switch off is a cache miss once it is on', async () => {
      // Remember an old-shape entry (no effectiveZdr) with the switch off.
      setSwitch(undefined);
      serve({ zdrIntent: true, entitled: true });
      expect((await resolveInteractiveRetentionPosture(ORG)).zdr).toBe(true);
      fetchSpy?.mockRestore();
      // Flip on without clearing the cache: the old entry must not be served,
      // fresh or stale, so an org-core outage now refuses.
      process.env.AUTH_CORE_ZDR_EFFECTIVE_POSTURE_ENABLED = 'true';
      resetEffectivePostureReadForTests();
      fetchSpy = jest
        .spyOn(global, 'fetch')
        .mockRejectedValue(new TypeError('fetch failed'));
      await expect(
        resolveInteractiveRetentionPosture(ORG),
      ).rejects.toBeInstanceOf(RetentionPostureUnavailableError);
      expect(fetchSpy).toHaveBeenCalledTimes(1);
    });
  });

  describe('switch off (byte-for-byte the old rule)', () => {
    it.each([undefined, '', 'TRUE', 'True', '1', 'yes', ' true'])(
      'value %p keeps intent && entitled: the lapsed row is not ZDR',
      async (value) => {
        setSwitch(value);
        const lapsedRow = matrix.org_posture_rows.find(
          (row) => row.id === 'o_plan_lapsed_keeps_zdr',
        )!;
        serve(orgCoreBody(lapsedRow));
        expect((await resolveInteractiveRetentionPosture(ORG)).zdr).toBe(false);
        expect(
          warned.some((message) => message.startsWith('plan_not_entitled')),
        ).toBe(true);
      },
    );

    it('an old org-core answer without effectiveZdr is still accepted', async () => {
      setSwitch(undefined);
      serve({ zdrIntent: true, entitled: true });
      expect((await resolveInteractiveRetentionPosture(ORG)).zdr).toBe(true);
    });

    it('ignores effectiveZdr even when org-core sends it', async () => {
      setSwitch(undefined);
      serve({
        zdrIntent: false,
        entitled: true,
        effectiveZdr: true,
        lapsed: false,
      });
      expect((await resolveInteractiveRetentionPosture(ORG)).zdr).toBe(false);
    });
  });
});
