export type ProductRecording = {
	id: string;
	label: string;
	title: string;
	video: string;
	raw: string;
	poster: string;
	captions: string;
	rawCaptions: string;
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

/** A historical run is not publication approval. Only reviewed, complete packs ship. */
export function approvedProductRecordings(
	manifest: { recordingReadiness?: { status: string; gates: Record<string, string> }; tasks: Candidate[] },
	assetExists: (path: string) => boolean,
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
		return { id, label: task.label, title: task.title, ...media, editingNote: task.editingNote };
	});
}
