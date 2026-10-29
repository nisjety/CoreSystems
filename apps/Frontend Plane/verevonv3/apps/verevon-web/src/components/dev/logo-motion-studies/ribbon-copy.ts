import type { RibbonRenderer } from "./ribbon-renderer";

// Copy the approved B's freshly drawn GPU frame before the browser composites
// it. Scaling this presentation never resizes or alters the original mesh.
export function createRibbonCopy(canvas: HTMLCanvasElement, source: HTMLCanvasElement): RibbonRenderer | null {
	try {
		const context = canvas.getContext("2d");
		if (!context) return null;
		let disposed = false;
		return {
			draw: () => {
				if (disposed) return;
				if (canvas.width !== source.width) canvas.width = source.width;
				if (canvas.height !== source.height) canvas.height = source.height;
				context.clearRect(0, 0, canvas.width, canvas.height);
				context.drawImage(source, 0, 0);
			},
			dispose: () => { disposed = true; },
		};
	} catch (error) {
		console.warn("Verevon ribbon copy unavailable; showing the approved static logo.", error);
		return null;
	}
}
