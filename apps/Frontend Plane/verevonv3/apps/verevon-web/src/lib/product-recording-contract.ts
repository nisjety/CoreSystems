export type ProductRecording = {
	id: string;
	label: string;
	title: string;
	video: string;
	raw: string;
	poster: string;
	captions: string;
	rawCaptions: string;
	/** Cue texts of `captions` in order. The films are silent, so this is the text alternative of the short film. */
	transcript: string[];
	/** Cue texts of `rawCaptions` in order: the text alternative of the full task. */
	rawTranscript: string[];
	editingNote: string;
};

type Candidate = {
	id: string;
	label: string;
	title: string;
	status: string;
	approval?: { reviewedOn: string; evidence: string; consecutivePasses: number; build: string };
	media: { raw: string | null; video: string | null; poster: string | null; captions: string | null; rawCaptions?: string | null };
	editingNote?: string;
};

const ids = ['01-kundesvar', '02-salgsrapport', '03-kampanje', '04-prosjektplan'];
export const recordingGates = ['correctness', 'repeatability', 'durability', 'recovery', 'trust', 'experience', 'performance'] as const;

const cueTiming = /^(?:\d{2,}:)?[0-5]\d:[0-5]\d\.\d{3}[ \t]+-->[ \t]+(?:\d{2,}:)?[0-5]\d:[0-5]\d\.\d{3}(?:[ \t].*)?$/;
const cueEntities: Record<string, string> = { '&lt;': '<', '&gt;': '>', '&nbsp;': ' ', '&lrm;': '', '&rlm;': '', '&amp;': '&' };
const maxCues = 100;
const maxCueLength = 600;

/** The text of every cue of a WebVTT file, in order, as the captions track would show it. Throws on anything it cannot read faithfully. */
export function parseCaptionCues(file: string): string[] {
	const [header = '', ...blocks] = file.replace(/^﻿/, '').replace(/\r\n?/g, '\n').split(/\n{2,}/).map(block => block.replace(/^\n+|\n+$/g, '')).filter(Boolean);
	if (!/^WEBVTT(?:[ \t\n]|$)/.test(header)) throw new Error('it is not a WebVTT file');
	const cues: string[] = [];
	for (const block of blocks) {
		if (/^(?:NOTE|STYLE|REGION)(?:[ \t\n]|$)/.test(block)) continue;
		const lines = block.split('\n');
		const timing = lines.findIndex(line => line.includes('-->'));
		if (timing < 0 || timing > 1 || !cueTiming.test(lines[timing]!)) throw new Error('a cue has no valid timing line');
		const raw = lines.slice(timing + 1);
		// A second timing line inside the text means a separator was not a blank line (a line of spaces is not one): refuse rather than merge two cues.
		if (raw.some(line => line.includes('-->'))) throw new Error('a cue contains a second timing line');
		// Bound the raw text before the tag strip: `<[^>]*>` is quadratic on a long run of `<`, and the 600 character limit below only sees the stripped text.
		const joined = raw.join(' ');
		if (joined.length > maxCueLength * 8) throw new Error('a cue is too long');
		// Tags go first so that an escaped `&lt;` survives as text.
		const text = joined.replace(/<[^>]*>/g, '').replace(/&(?:lt|gt|nbsp|lrm|rlm|amp);/g, entity => cueEntities[entity]!).replace(/\s+/g, ' ').trim();
		if (!text) throw new Error('a cue has no text');
		if (text.length > maxCueLength) throw new Error('a cue is too long');
		if (cues.length >= maxCues) throw new Error('it has too many cues');
		cues.push(text);
	}
	if (!cues.length) throw new Error('it has no cues');
	return cues;
}

function transcriptOf(readCaptions: (path: string) => string | null, path: string, name: string): string[] {
	try {
		const file = readCaptions(path);
		if (!file) throw new Error('the file could not be read');
		return parseCaptionCues(file);
	} catch (error) {
		throw new Error(`Product recordings: ${name} transcript: ${error instanceof Error ? error.message : 'unreadable'}.`);
	}
}

/**
 * A historical run is not publication approval. Only reviewed, complete packs ship.
 * `readCaptions` receives only a caption path that already passed the public-media checks below.
 */
export function approvedProductRecordings(
	manifest: { recordingReadiness?: { status: string; gates: Record<string, string> }; tasks: Candidate[] },
	assetExists: (path: string) => boolean,
	readCaptions: (path: string) => string | null,
): ProductRecording[] {
	if (manifest.recordingReadiness?.status !== 'approved') return [];
	if (recordingGates.some(gate => manifest.recordingReadiness?.gates[gate] !== 'passed')) {
		throw new Error('Product recordings: every readiness gate must pass before approval.');
	}
	if (manifest.tasks.length !== 4 || new Set(manifest.tasks.map(task => task.id)).size !== 4) {
		throw new Error('Product recordings: all four distinct scenarios are required.');
	}
	return ids.map(id => {
		const task = manifest.tasks.find(task => task.id === id);
		if (!task || task.status !== 'media-approved' || !task.approval?.reviewedOn || !task.approval.evidence || !task.approval.build || !Number.isInteger(task.approval.consecutivePasses) || task.approval.consecutivePasses < 5 || !task.editingNote?.trim()) {
			throw new Error(`Product recordings: ${id} is missing factual review, repeatability or editing disclosure.`);
		}
		const media = {} as Pick<ProductRecording, 'raw' | 'video' | 'poster' | 'captions' | 'rawCaptions'>;
		for (const key of ['raw', 'video', 'poster', 'captions', 'rawCaptions'] as const) {
			const path = task.media[key];
			const extension = key === 'poster' ? /\.(png|jpg|webp)$/ : key.endsWith('Captions') || key === 'captions' ? /\.vtt$/ : /\.(mp4|webm)$/;
			// Public media only. Reject remote URLs, encoded traversal and private evidence paths.
			if (!path || !/^\/verevon-product-recordings\/[a-zA-Z0-9_/-]+\.[a-z0-9]+$/.test(path) || !extension.test(path) || !assetExists(path)) {
				throw new Error(`Product recordings: ${id}/${key} must name an existing public media file.`);
			}
			media[key] = path;
		}
		const transcript = transcriptOf(readCaptions, media.captions, `${id}/captions`);
		const rawTranscript = transcriptOf(readCaptions, media.rawCaptions, `${id}/rawCaptions`);
		return { id, label: task.label, title: task.title, ...media, transcript, rawTranscript, editingNote: task.editingNote };
	});
}
