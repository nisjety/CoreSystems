import { join } from 'node:path';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// The wiring between the contract and the public folder: media are checked with statSync and
// the transcript is read from the same `public/<path>` file the browser's <track> loads.
const state = vi.hoisted(() => {
	const ids = ['01-kundesvar', '02-salgsrapport', '03-kampanje', '04-prosjektplan'];
	const gates = ['correctness', 'repeatability', 'durability', 'recovery', 'trust', 'experience', 'performance'];
	const manifest = {
		recordingReadiness: { status: 'approved', gates: Object.fromEntries(gates.map(gate => [gate, 'passed'])) },
		tasks: ids.map(id => ({
			id, label: id, title: id, status: 'media-approved', editingNote: 'Tidskutt er merket.',
			approval: { reviewedOn: '2026-09-19', evidence: 'review.md', consecutivePasses: 5, build: 'release-image' },
			media: { video: `/verevon-product-recordings/${id}.mp4`, raw: `/verevon-product-recordings/${id}-raw.webm`, poster: `/verevon-product-recordings/${id}.jpg`, captions: `/verevon-product-recordings/${id}.vtt`, rawCaptions: `/verevon-product-recordings/${id}-raw.vtt` },
		})),
	};
	return { manifest, files: new Map<string, string>(), unreadable: new Set<string>() };
});

vi.mock('../../plans/product-recordings/manifest.json', () => ({ default: state.manifest }));
// Vitest resolves named imports of a node builtin through its default export, so the fake replaces both.
vi.mock('node:fs', async importOriginal => {
	const fake = {
		...await importOriginal<typeof import('node:fs')>(),
		statSync: (path: string) => {
			const text = state.files.get(path);
			if (text === undefined) throw new Error('ENOENT');
			return { isFile: () => true, size: text.length };
		},
		readFileSync: (path: string) => {
			if (state.unreadable.has(path)) throw new Error('EACCES');
			return state.files.get(path);
		},
	};
	return { ...fake, default: fake };
});

import { getProductRecordings } from './product-recordings.server';

const onDisk = (path: string) => join(process.cwd(), 'public', path);
const vtt = (text: string) => `WEBVTT\n\n00:00:00.000 --> 00:00:02.000\n${text}\n`;

beforeEach(() => {
	state.files.clear();
	state.unreadable.clear();
	for (const task of state.manifest.tasks) {
		for (const path of Object.values(task.media)) state.files.set(onDisk(path), path.endsWith('.vtt') ? vtt(`Beskrivelse fra ${path}`) : 'media');
	}
});

describe('getProductRecordings text alternative', () => {
	it('reads each transcript from the matching public vtt file', () => {
		const [first] = getProductRecordings();
		expect(first!.transcript).toEqual(['Beskrivelse fra /verevon-product-recordings/01-kundesvar.vtt']);
		expect(first!.rawTranscript).toEqual(['Beskrivelse fra /verevon-product-recordings/01-kundesvar-raw.vtt']);
	});
	it('refuses to publish when a caption file is missing, empty or unreadable', () => {
		const missing = onDisk('/verevon-product-recordings/02-salgsrapport.vtt');
		const content = state.files.get(missing)!;
		state.files.delete(missing);
		expect(() => getProductRecordings()).toThrow('02-salgsrapport/captions must name an existing public media file');
		state.files.set(missing, '');
		expect(() => getProductRecordings()).toThrow('02-salgsrapport/captions must name an existing public media file');
		state.files.set(missing, content);
		state.unreadable.add(missing);
		expect(() => getProductRecordings()).toThrow('02-salgsrapport/captions transcript: the file could not be read');
	});
});
