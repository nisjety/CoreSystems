import { readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import manifest from '../../plans/product-recordings/manifest.json';
import { approvedProductRecordings } from './product-recording-contract';

// The server strips private run metadata; the browser receives only public media
// and the cue texts of the approved captions (the text alternative for the silent films).
export function getProductRecordings() {
	return approvedProductRecordings(manifest, path => {
		try { const file = statSync(join(process.cwd(), 'public', path)); return file.isFile() && file.size > 0; }
		catch { return false; }
	}, path => {
		// The contract passes only a caption path that already proved to be an existing public .vtt file.
		try { return readFileSync(join(process.cwd(), 'public', path), 'utf8'); }
		catch { return null; }
	});
}
