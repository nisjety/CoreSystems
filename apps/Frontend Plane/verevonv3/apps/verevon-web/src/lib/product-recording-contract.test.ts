import { describe, expect, it } from 'vitest';
import { approvedProductRecordings, recordingGates } from './product-recording-contract';
import manifest from '../../plans/product-recordings/manifest.json';

const approved = () => ({
	recordingReadiness: { status: 'approved', gates: Object.fromEntries(recordingGates.map(gate => [gate, 'passed'])) },
	tasks: manifest.tasks.map(task => ({ ...task, status: 'media-approved', approval: { reviewedOn: '2026-09-19', evidence: 'review.md', consecutivePasses: 5, build: 'release-image' }, editingNote: 'Tidskutt er merket.',
		media: { video: `/verevon-product-recordings/${task.id}.mp4`, raw: `/verevon-product-recordings/${task.id}-raw.webm`, poster: `/verevon-product-recordings/${task.id}.jpg`, captions: `/verevon-product-recordings/${task.id}.vtt`, rawCaptions: `/verevon-product-recordings/${task.id}-raw.vtt` } })),
});

describe('product recording publication gate', () => {
	it('keeps the current unapproved pack off the public player', () => expect(approvedProductRecordings(manifest, () => true)).toEqual([]));
	it('rejects missing media even when the manifest claims approval', () => expect(() => approvedProductRecordings(approved(), () => false)).toThrow('existing public media'));
	it('rejects an unpassed performance gate', () => { const pack = approved(); pack.recordingReadiness.gates.performance = 'pending'; expect(() => approvedProductRecordings(pack, () => true)).toThrow('every readiness gate'); });
	it('rejects an insufficient consecutive pass count', () => { const pack = approved(); pack.tasks[0]!.approval.consecutivePasses = 1; expect(() => approvedProductRecordings(pack, () => true)).toThrow('repeatability'); });
	it.each([NaN, Infinity, 5.5, undefined, '5'])('rejects malformed repeatability evidence: %s', count => {
		const pack = approved();
		Object.assign(pack.tasks[0]!.approval, { consecutivePasses: count });
		expect(() => approvedProductRecordings(pack, () => true)).toThrow('repeatability');
	});
	it('rejects private, remote or traversal paths', () => {
		for (const path of ['https://example.com/movie.mp4', '/verevon-product-recordings/../private.mp4', '/verevon-product-recordings/%2e%2e/private.mp4', 'C:/private/trace.mp4']) {
			const pack = approved(); pack.tasks[0]!.media.video = path; expect(() => approvedProductRecordings(pack, () => true)).toThrow('existing public media');
		}
	});
	it('requires all four tasks and matching caption tracks for the full recordings', () => {
		const pack = approved(); pack.tasks.pop(); expect(() => approvedProductRecordings(pack, () => true)).toThrow('all four');
		const missingCaptions = approved(); missingCaptions.tasks[0]!.media.rawCaptions = ''; expect(() => approvedProductRecordings(missingCaptions, () => true)).toThrow('rawCaptions');
	});
	it('projects only safe media, labels and disclosure after approval', () => {
		const result = approvedProductRecordings(approved(), () => true);
		expect(result).toHaveLength(4); expect(result[0]!.label).toBe('Kundesvar');
		expect(JSON.stringify(result)).not.toMatch(/threadId|verifiedRun|approval|evidence|release-image/);
	});
});
