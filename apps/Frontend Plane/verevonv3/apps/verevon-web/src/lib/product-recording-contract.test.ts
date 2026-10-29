import { describe, expect, it } from 'vitest';
import { approvedProductRecordings, parseCaptionCues, recordingGates } from './product-recording-contract';
import manifest from '../../plans/product-recordings/manifest.json';

const approved = () => ({
	recordingReadiness: { status: 'approved', gates: Object.fromEntries(recordingGates.map(gate => [gate, 'passed'])) },
	tasks: manifest.tasks.map(task => ({ ...task, status: 'media-approved', approval: { reviewedOn: '2026-09-19', evidence: 'review.md', consecutivePasses: 5, build: 'release-image' }, editingNote: 'Tidskutt er merket.',
		media: { video: `/verevon-product-recordings/${task.id}.mp4`, raw: `/verevon-product-recordings/${task.id}-raw.webm`, poster: `/verevon-product-recordings/${task.id}.jpg`, captions: `/verevon-product-recordings/${task.id}.vtt`, rawCaptions: `/verevon-product-recordings/${task.id}-raw.vtt` } })),
});

// Same shape as the exporter writes (scripts/prepare-product-recording.mjs): `WEBVTT`, a blank line, then `start --> end` and one line of text per cue.
const stamp = (seconds: number) => `${new Date(seconds * 1000).toISOString().slice(11, 19)}.000`;
const vtt = (...texts: string[]) => `WEBVTT\n\n${texts.map((text, index) => `${stamp(index)} --> ${stamp(index + 1)}\n${text}\n`).join('\n')}`;
const cuesFor = (path: string) => [`Beskrivelse av ${path}, første steg.`, `Beskrivelse av ${path}, andre steg.`];
const reader = (path: string) => vtt(...cuesFor(path));

describe('product recording publication gate', () => {
	it('keeps the current unapproved pack off the public player', () => expect(approvedProductRecordings(manifest, () => true, reader)).toEqual([]));
	it('rejects missing media even when the manifest claims approval', () => expect(() => approvedProductRecordings(approved(), () => false, reader)).toThrow('existing public media'));
	it('rejects an unpassed performance gate', () => { const pack = approved(); pack.recordingReadiness.gates.performance = 'pending'; expect(() => approvedProductRecordings(pack, () => true, reader)).toThrow('every readiness gate'); });
	it('rejects an insufficient consecutive pass count', () => { const pack = approved(); pack.tasks[0]!.approval.consecutivePasses = 1; expect(() => approvedProductRecordings(pack, () => true, reader)).toThrow('repeatability'); });
	it.each([NaN, Infinity, 5.5, undefined, '5'])('rejects malformed repeatability evidence: %s', count => {
		const pack = approved();
		Object.assign(pack.tasks[0]!.approval, { consecutivePasses: count });
		expect(() => approvedProductRecordings(pack, () => true, reader)).toThrow('repeatability');
	});
	it('rejects private, remote or traversal paths', () => {
		for (const path of ['https://example.com/movie.mp4', '/verevon-product-recordings/../private.mp4', '/verevon-product-recordings/%2e%2e/private.mp4', 'C:/private/trace.mp4']) {
			const pack = approved(); pack.tasks[0]!.media.video = path; expect(() => approvedProductRecordings(pack, () => true, reader)).toThrow('existing public media');
		}
	});
	it('requires all four tasks and matching caption tracks for the full recordings', () => {
		const pack = approved(); pack.tasks.pop(); expect(() => approvedProductRecordings(pack, () => true, reader)).toThrow('all four');
		const missingCaptions = approved(); missingCaptions.tasks[0]!.media.rawCaptions = ''; expect(() => approvedProductRecordings(missingCaptions, () => true, reader)).toThrow('rawCaptions');
	});
	it('projects only safe media, labels and disclosure after approval', () => {
		const result = approvedProductRecordings(approved(), () => true, reader);
		expect(result).toHaveLength(4); expect(result[0]!.label).toBe('Kundesvar');
		expect(JSON.stringify(result)).not.toMatch(/threadId|verifiedRun|approval|evidence|release-image/);
	});
});

describe('product recording text alternative', () => {
	const publicVtt = /^\/verevon-product-recordings\/[a-zA-Z0-9_/-]+\.vtt$/;
	const recorded = () => { const paths: string[] = []; return { paths, read: (path: string) => { paths.push(path); return reader(path); } }; };

	it('derives the short and the full transcript from the matching approved public vtt files', () => {
		const result = approvedProductRecordings(approved(), () => true, reader);
		for (const recording of result) {
			expect(recording.transcript).toEqual(cuesFor(recording.captions));
			expect(recording.rawTranscript).toEqual(cuesFor(recording.rawCaptions));
			expect(recording.transcript).not.toEqual(recording.rawTranscript);
		}
	});
	it('reads only public vtt paths that already passed the media checks, once per track', () => {
		const { paths, read } = recorded();
		approvedProductRecordings(approved(), () => true, read);
		expect(paths).toHaveLength(8);
		expect(new Set(paths).size).toBe(8);
		expect(paths.every(path => publicVtt.test(path))).toBe(true);
	});
	it.each([
		['remote url', 'https://example.com/captions.vtt'],
		['traversal', '/verevon-product-recordings/../private.vtt'],
		['encoded traversal', '/verevon-product-recordings/%2e%2e/private.vtt'],
		['private evidence path', 'C:/private/captions.vtt'],
		['wrong extension', '/verevon-product-recordings/01-kundesvar.txt'],
		['missing file', '/verevon-product-recordings/01-kundesvar-missing.vtt'],
	])('never reads a caption path that is not an approved public vtt: %s', (_name, path) => {
		const { paths, read } = recorded();
		const pack = approved(); pack.tasks[0]!.media.captions = path;
		expect(() => approvedProductRecordings(pack, candidate => !candidate.includes('missing'), read)).toThrow('existing public media');
		expect(paths).toEqual([]);
		const rawPack = approved(); rawPack.tasks[0]!.media.rawCaptions = path;
		expect(() => approvedProductRecordings(rawPack, candidate => !candidate.includes('missing'), read)).toThrow('existing public media');
		expect(paths).toEqual([]);
	});
	it.each([
		['unreadable file', () => null],
		['empty file', () => ''],
		['not webvtt', () => '1\n00:00:00.000 --> 00:00:01.000\nUten topptekst\n'],
		['no cues', () => 'WEBVTT\n\nNOTE bare en merknad\n'],
		['cue without text', () => 'WEBVTT\n\n00:00:00.000 --> 00:00:01.000\n'],
		['malformed timing', () => 'WEBVTT\n\n0 --> 1\nTekst\n'],
	])('refuses to publish a player without a usable transcript: %s', (_name, read) => {
		for (const track of ['01-kundesvar.vtt', '01-kundesvar-raw.vtt']) {
			expect(() => approvedProductRecordings(approved(), () => true, path => path.endsWith(track) ? read() : reader(path))).toThrow(`01-kundesvar/${track.endsWith('raw.vtt') ? 'rawCaptions' : 'captions'} transcript`);
		}
	});
	it('does not add private run metadata next to the transcript', () => {
		const result = approvedProductRecordings(approved(), () => true, reader);
		expect(Object.keys(result[0]!).sort()).toEqual(['captions', 'editingNote', 'id', 'label', 'poster', 'rawCaptions', 'rawTranscript', 'raw', 'title', 'transcript', 'video'].sort());
	});
});

describe('parseCaptionCues', () => {
	it('returns the cue texts in order and ignores the header, notes, styles and cue identifiers', () => {
		const file = ['WEBVTT - Norsk', '', 'NOTE Bare for redaktøren', '', 'STYLE', '::cue { color: red }', '', 'intro', '00:00:00.000 --> 00:00:02.500 align:start', 'Åpner innboksen.', '', '01:00:01.000 --> 01:00:03.000', 'Skriver svaret.', ''].join('\n');
		expect(parseCaptionCues(file)).toEqual(['Åpner innboksen.', 'Skriver svaret.']);
	});
	it('accepts a byte order mark and CRLF line endings, and joins a cue that wraps over lines', () => {
		expect(parseCaptionCues('\uFEFFWEBVTT\r\n\r\n00:00:00.000 --> 00:00:02.000\r\nFørste linje\r\nandre linje\r\n\r\n00:00:02.000 --> 00:00:03.000\r\nNeste.\r\n')).toEqual(['Første linje andre linje', 'Neste.']);
	});
	it('gives the text a caption would show: tags removed, entities decoded, whitespace collapsed', () => {
		expect(parseCaptionCues('WEBVTT\n\n00:00:00.000 --> 00:00:02.000\n<b>Salg</b> &amp; marked &lt;Q3&gt;\u00a0  opp\n')).toEqual(['Salg & marked <Q3> opp']);
	});
	it('bounds the number of cues and the length of a cue so the page payload stays small', () => {
		expect(() => parseCaptionCues(vtt(...Array.from({ length: 101 }, (_, index) => `Steg ${index}`)))).toThrow('too many cues');
		expect(() => parseCaptionCues(vtt('x'.repeat(601)))).toThrow('too long');
	});
	it('refuses a long run of "<" before the tag strip can go quadratic on it', () => {
		// Without the raw-length bound this input takes over a minute (and trips the test timeout).
		expect(() => parseCaptionCues(vtt('<'.repeat(200_000)))).toThrow('too long');
	});
	it('refuses two cues that were not separated by a blank line instead of merging them', () => {
		expect(() => parseCaptionCues('WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nA\n \n00:00:01.000 --> 00:00:02.000\nB\n')).toThrow('second timing line');
	});
});
