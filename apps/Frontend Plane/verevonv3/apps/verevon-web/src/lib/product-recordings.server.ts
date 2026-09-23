import { statSync } from 'node:fs';
import { join } from 'node:path';
import manifest from '../../plans/product-recordings/manifest.json';
import { approvedProductRecordings } from './product-recording-contract';

// The server strips private run metadata; the browser receives only public media.
export function getProductRecordings() {
	return approvedProductRecordings(manifest, path => {
		try { const file = statSync(join(process.cwd(), 'public', path)); return file.isFile() && file.size > 0; }
		catch { return false; }
	});
}
