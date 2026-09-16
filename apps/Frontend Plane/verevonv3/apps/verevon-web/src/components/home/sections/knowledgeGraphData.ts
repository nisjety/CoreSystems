export const knowledgeDepartments = [
	{ name: "Økonomi", x: -0.55, y: 0.55, items: ["Budsjett og prognoser", "Rapporter og regnskap"] },
	{ name: "Salg", x: 0.46, y: 0.62, items: ["Kunder og muligheter", "Tilbud og avtaler"] },
	{ name: "HR", x: 0.7, y: 0.12, items: ["Ansatte og onboarding", "Personalhåndbok"] },
	{ name: "Drift", x: 0.46, y: -0.55, items: ["Rutiner og prosesser", "Prosjekter og oppgaver"] },
	{ name: "IT", x: -0.12, y: -0.72, items: ["Systemer og tilganger", "Teknisk dokumentasjon"] },
	{ name: "Logistikk", x: -0.7, y: -0.24, items: ["Ordre og leveranser", "Lager og innkjøp"] },
	{ name: "Marked", x: -0.2, y: 0.12, items: ["Innhold og kampanjer", "Merkevare og innsikt"] },
	{ name: "Kundeservice", x: 0.24, y: -0.16, items: ["Kundesaker og historikk", "Svar og veiledninger"] },
];

/** Fine threads converge on a few hubs inside a lightly irregular spherical net. */
export function createKnowledgeGraphData() {
	let state = 437;
	const random = () => {
		state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
		return state / 4294967296;
	};
	const count = 760;
	const hubCount = 36;
	const positions = new Float32Array(count * 3);
	const edges: number[] = [];
	const seen = new Set<number>();
	const degree = new Float32Array(count);
	const addEdge = (a: number, b: number) => {
		const key = Math.min(a, b) * count + Math.max(a, b);
		if (a === b || seen.has(key)) return;
		seen.add(key);
		edges.push(a, b);
		degree[a]++;
		degree[b]++;
	};
	// Department labels belong to actual high-degree nodes, not arbitrary points
	// overlaid on the sphere. The remaining hubs cover the rear and outer shell.
	const goldenAngle = Math.PI * (3 - Math.sqrt(5));
	for (let node = 0; node < count; node++) {
		const department = knowledgeDepartments[node];
		if (department) {
			positions.set([department.x, department.y, Math.sqrt(1 - department.x ** 2 - department.y ** 2)], node * 3);
			continue;
		}
		const isHub = node < hubCount;
		const offset = isHub ? knowledgeDepartments.length : hubCount;
		const samples = isHub ? hubCount - offset : count - hubCount;
		const sample = node - offset;
		const y = 1 - 2 * (sample + 0.5) / samples;
		const longitude = sample * goldenAngle + (isHub ? 1.7 : 0);
		const r = Math.sqrt(1 - y * y);
		const point = [Math.cos(longitude) * r, y, Math.sin(longitude) * r]
			.map((value) => value + (random() - 0.5) * 0.085);
		const length = Math.hypot(...point);
		const surface = 0.985 + Math.sin(point[0] * 5 + point[2] * 3) * Math.cos(point[1] * 4) * 0.023 + (random() - 0.5) * 0.028;
		// A sparse inner layer adds depth without filling the centre with ink.
		const radius = !isHub && node % 11 === 0 ? 0.65 + random() * 0.22 : surface;
		point.forEach((value, axis) => { positions[node * 3 + axis] = value / length * radius; });
	}
	const distance = (a: number, b: number) => {
		let d = 0;
		for (let axis = 0; axis < 3; axis++) d += (positions[a * 3 + axis] - positions[b * 3 + axis]) ** 2;
		return d;
	};
	for (let node = hubCount; node < count; node++) {
		// A bounded nearest-neighbour scan avoids sorting the entire graph for
		// every point. Two short threads retain a light, open surface mesh.
		const neighbors: { node: number; distance: number }[] = [];
		for (let other = 0; other < count; other++) {
			if (node === other) continue;
			const d = distance(node, other);
			if (neighbors.length === 2 && d >= neighbors[1].distance) continue;
			const index = neighbors.findIndex((item) => d < item.distance);
			neighbors.splice(index < 0 ? neighbors.length : index, 0, { node: other, distance: d });
			if (neighbors.length > 2) neighbors.pop();
		}
		neighbors.forEach((other) => addEdge(node, other.node));
	}
	for (let hub = 0; hub < hubCount; hub++) {
		const neighborhood = Array.from({ length: count }, (_, node) => node)
			.filter((node) => node !== hub)
			.sort((a, b) => distance(hub, a) - distance(hub, b));
		// Long, overlapping spokes make each hub legible as a convergence point.
		neighborhood.slice(0, 38).forEach((node) => addEdge(hub, node));
		for (let spoke = 0; spoke < 10; spoke++) addEdge(hub, neighborhood[38 + Math.floor(random() * 110)]);
	}
	// Wispy cross-sphere threads sit behind the more legible hub spokes.
	while (edges.length < 7200) {
		const a = Math.floor(random() * count);
		const b = Math.floor(random() * count);
		if (distance(a, b) < 3.2) addEdge(a, b);
	}
	return { positions, edges, degree, count, hubCount };
}

export function knowledgeGraphFallbackPath() {
	const { positions, edges } = createKnowledgeGraphData();
	const project = (index: number) => {
		const x = positions[index * 3];
		const y = positions[index * 3 + 1];
		const z = positions[index * 3 + 2];
		return `${(150 + (x * 0.93 + z * 0.37) * 122).toFixed(1)},${(150 - y * 122).toFixed(1)}`;
	};
	let path = "";
	for (let i = 0; i < edges.length; i += 4) path += `M${project(edges[i])}L${project(edges[i + 1])}`;
	return path;
}
