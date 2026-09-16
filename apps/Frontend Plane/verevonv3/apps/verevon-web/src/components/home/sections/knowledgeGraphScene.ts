import {
	BufferGeometry, Color, Float32BufferAttribute, Group, LineSegments,
	PerspectiveCamera, Points, Scene, ShaderMaterial, Vector3, WebGLRenderer,
} from "three";
import { createKnowledgeGraphData } from "./knowledgeGraphData";

// The same displacement is used for nodes and edge endpoints so the net stays joined.
const vertexShader = `
	uniform float uTime;
	uniform float uPixelRatio;
	uniform vec2 uPointer;
	uniform float uHover;
	attribute float weight;
	varying float vAlpha;
	varying float vAccent;
	void main() {
		vec3 direction = normalize(position);
		float wave = sin(direction.x * 5.1 + uTime * 0.24)
			* cos(direction.y * 4.3 - uTime * 0.19)
			* sin(direction.z * 4.7 + uTime * 0.16);
		float detail = sin(dot(direction, vec3(13.0, 7.0, 9.0)) + uTime * 0.32);
		vec3 displaced = position + direction * (wave * 0.023 + detail * 0.006);
		vec4 view = modelViewMatrix * vec4(displaced, 1.0);
		gl_Position = projectionMatrix * view;
		vec2 screen = gl_Position.xy / gl_Position.w;
		float proximity = (1.0 - smoothstep(0.0, 0.5, distance(screen, uPointer))) * uHover;
		// Front threads carry more ink; the rear remains visible through the net.
		// This is independent of camera distance, including on narrow cards.
		float facing = normalize(normalMatrix * direction).z;
		float depth = smoothstep(-0.35, 0.85, facing);
		float rim = pow(1.0 - abs(facing), 3.0);
		vAlpha = 0.022 + depth * 0.085 + rim * 0.009 + weight * depth * 0.038;
		vAccent = proximity * 0.32;
		gl_PointSize = (0.7 + weight * 2.3) * uPixelRatio * (3.8 / -view.z);
	}
`;
const fragmentShader = `
	uniform vec3 uInk;
	uniform vec3 uAccent;
	varying float vAlpha;
	varying float vAccent;
	void main() {
		float alpha = vAlpha;
		#ifdef IS_POINT
			float radius = length(gl_PointCoord - 0.5);
			alpha *= (1.0 - smoothstep(0.15, 0.5, radius)) * 2.4;
		#endif
		gl_FragColor = vec4(mix(uInk, uAccent, vAccent), alpha);
		#include <tonemapping_fragment>
		#include <colorspace_fragment>
	}
`;

export function mountKnowledgeGraph(host: HTMLDivElement, reducedMotion: boolean) {
	const renderer = new WebGLRenderer({ alpha: true, antialias: true, powerPreference: "low-power" });
	renderer.setClearColor(0x000000, 0);
	renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.75));
	renderer.domElement.setAttribute("aria-hidden", "true");
	renderer.domElement.style.cssText = "display:block;width:100%;height:100%;position:absolute;inset:0";
	const scene = new Scene();
	const camera = new PerspectiveCamera(36, 1, 0.1, 20);
	const group = new Group();
	const { positions, edges, degree, hubCount } = createKnowledgeGraphData();
	const departmentNodes = Array.from(host.querySelectorAll<HTMLButtonElement>("[data-business-node]")).map((element) => {
		const x = Number(element.dataset.nodeX);
		const y = Number(element.dataset.nodeY);
		const target = new Vector3(x, y, Math.sqrt(1 - x * x - y * y));
		let nearest = 0;
		let distance = Infinity;
		const candidate = new Vector3();
		for (let node = 0; node < hubCount; node++) {
			candidate.fromArray(positions, node * 3);
			const nextDistance = candidate.distanceToSquared(target);
			if (nextDistance < distance) { distance = nextDistance; nearest = node; }
		}
		const point = new Vector3().fromArray(positions, nearest * 3);
		element.style.left = "0";
		element.style.top = "0";
		return { element, point, direction: point.clone().normalize(), projected: new Vector3(), previewOffset: Infinity };
	});
	const weights = Float32Array.from(degree, (d) => Math.min(d / 52, 1));
	const nodeGeometry = new BufferGeometry();
	nodeGeometry.setAttribute("position", new Float32BufferAttribute(positions, 3));
	nodeGeometry.setAttribute("weight", new Float32BufferAttribute(weights, 1));
	const edgePositions = new Float32Array(edges.length * 3);
	const edgeWeights = new Float32Array(edges.length);
	edges.forEach((node, i) => {
		edgePositions.set(positions.subarray(node * 3, node * 3 + 3), i * 3);
		edgeWeights[i] = weights[node];
	});
	const edgeGeometry = new BufferGeometry();
	edgeGeometry.setAttribute("position", new Float32BufferAttribute(edgePositions, 3));
	edgeGeometry.setAttribute("weight", new Float32BufferAttribute(edgeWeights, 1));
	const uniforms = {
		uTime: { value: 0 }, uPixelRatio: { value: renderer.getPixelRatio() },
		uInk: { value: new Color("#625f55") }, uAccent: { value: new Color("#c36b4c") },
		uPointer: { value: [10, 10] }, uHover: { value: 0 },
	};
	const lineMaterial = new ShaderMaterial({ vertexShader, fragmentShader, uniforms, transparent: true, depthWrite: false });
	const pointMaterial = new ShaderMaterial({ vertexShader, fragmentShader, uniforms, defines: { IS_POINT: 1 }, transparent: true, depthWrite: false });
	group.add(new LineSegments(edgeGeometry, lineMaterial), new Points(nodeGeometry, pointMaterial));
	scene.add(group);
	let frame = 0;
	let lastTime = 0;
	let elapsed = 0;
	let inView = false;
	let contextAvailable = true;
	let disposed = false;
	let pointerX = 0;
	let pointerY = 0;
	let hovering = false;
	let width = 1;
	let height = 1;
	const render = () => {
		group.updateMatrixWorld();
		camera.updateMatrixWorld();
		let closest = departmentNodes[0];
		let closestDistance = Infinity;
		for (const node of departmentNodes) {
			// Match the shader displacement so labels remain attached to the live net.
			const direction = node.direction;
			const wave = Math.sin(direction.x * 5.1 + elapsed * 0.24) * Math.cos(direction.y * 4.3 - elapsed * 0.19) * Math.sin(direction.z * 4.7 + elapsed * 0.16);
			const detail = Math.sin(direction.x * 13 + direction.y * 7 + direction.z * 9 + elapsed * 0.32);
			node.projected.copy(node.point).addScaledVector(direction, wave * 0.023 + detail * 0.006).applyMatrix4(group.matrixWorld).project(camera);
			const x = (node.projected.x + 1) * width / 2;
			const y = (1 - node.projected.y) * height / 2;
			node.element.style.transform = `translate3d(${x.toFixed(2)}px, ${y.toFixed(2)}px, 0) translate(-50%, -50%)`;
			const previewMargin = width < 350 ? 84 : 100;
			const previewX = Math.max(previewMargin, Math.min(width - previewMargin, x));
			const offset = Math.round(previewX - x);
			if (offset !== node.previewOffset) {
				node.element.style.setProperty("--node-preview-offset", `${offset}px`);
				node.previewOffset = offset;
			}
			const lower = String(y > height * 0.55);
			if (node.element.dataset.lower !== lower) node.element.dataset.lower = lower;
			const distance = ((node.projected.x - pointerX) * width / 2) ** 2 + ((node.projected.y - pointerY) * height / 2) ** 2;
			if (distance < closestDistance) { closestDistance = distance; closest = node; }
		}
		for (const node of departmentNodes) {
			const highlight = String(host.dataset.selectedNode ? node.element.dataset.businessNode === host.dataset.selectedNode : hovering && closestDistance < 52 ** 2 && node === closest);
			if (node.element.dataset.highlight !== highlight) node.element.dataset.highlight = highlight;
		}
		renderer.render(scene, camera);
	};
	const resize = () => {
		width = host.clientWidth;
		height = host.clientHeight;
		if (!width || !height) return;
		camera.aspect = width / height;
		// A true circle with breathing room, including on narrow phone cards.
		camera.position.z = 3.65 / Math.min(camera.aspect, 1);
		camera.updateProjectionMatrix();
		renderer.setSize(width, height, false);
		render();
	};
	const animate = (now: number) => {
		if (disposed) return;
		frame = requestAnimationFrame(animate);
		const delta = now - lastTime;
		if (delta < 1000 / 30) return;
		const seconds = Math.min(delta, 65) / 1000;
		const blend = 1 - Math.exp(-seconds * 4.5);
		elapsed += seconds;
		const inspecting = host.dataset.expanded === "true";
		lastTime = now;
		uniforms.uTime.value = elapsed;
		uniforms.uHover.value += ((hovering ? 1 : 0) - uniforms.uHover.value) * blend;
		// Free rotation reveals the rear layer. Inspection gently presents the
		// department hubs; wrapping the angle prevents a long spin on re-entry.
		if (inspecting) group.rotation.y += ((hovering ? pointerX * 0.08 : 0) - group.rotation.y) * blend;
		else group.rotation.y = (group.rotation.y + seconds * 0.075 + Math.PI) % (Math.PI * 2) - Math.PI;
		const targetTilt = Math.sin(elapsed * 0.12) * 0.065 + (inspecting && hovering ? -pointerY * 0.045 : 0);
		group.rotation.x += (targetTilt - group.rotation.x) * blend;
		group.rotation.z = Math.sin(elapsed * 0.08) * 0.018;
		render();
	};
	const syncAnimation = () => {
		cancelAnimationFrame(frame);
		if (inView && contextAvailable && !document.hidden && !reducedMotion && !disposed) {
			lastTime = performance.now();
			frame = requestAnimationFrame(animate);
		}
	};
	const visibility = new IntersectionObserver(([entry]) => { inView = entry.isIntersecting; syncAnimation(); }, { threshold: 0.01 });
	const sizing = new ResizeObserver(resize);
	const expandedState = new MutationObserver(() => { if (contextAvailable) render(); });
	const pointerMove = (event: PointerEvent) => {
		if (event.pointerType === "touch" || reducedMotion) return;
		const rect = host.getBoundingClientRect();
		pointerX = ((event.clientX - rect.left) / rect.width) * 2 - 1;
		pointerY = -((event.clientY - rect.top) / rect.height) * 2 + 1;
		uniforms.uPointer.value = [pointerX, pointerY];
		hovering = true;
	};
	const pointerLeave = () => { hovering = false; pointerX = 0; pointerY = 0; };
	const contextLost = (event: Event) => {
		event.preventDefault();
		contextAvailable = false;
		cancelAnimationFrame(frame);
		host.dataset.graphReady = "false";
	};
	const contextRestored = () => { contextAvailable = true; resize(); host.dataset.graphReady = "true"; syncAnimation(); };
	host.appendChild(renderer.domElement);
	resize();
	host.dataset.graphReady = "true";
	visibility.observe(host);
	sizing.observe(host);
	expandedState.observe(host, { attributes: true, attributeFilter: ["data-expanded", "data-selected-node"] });
	document.addEventListener("visibilitychange", syncAnimation);
	host.addEventListener("pointermove", pointerMove);
	host.addEventListener("pointerleave", pointerLeave);
	renderer.domElement.addEventListener("webglcontextlost", contextLost);
	renderer.domElement.addEventListener("webglcontextrestored", contextRestored);
	return () => {
		disposed = true;
		cancelAnimationFrame(frame);
		visibility.disconnect(); sizing.disconnect(); expandedState.disconnect();
		document.removeEventListener("visibilitychange", syncAnimation);
		host.removeEventListener("pointermove", pointerMove);
		host.removeEventListener("pointerleave", pointerLeave);
		renderer.domElement.removeEventListener("webglcontextlost", contextLost);
		renderer.domElement.removeEventListener("webglcontextrestored", contextRestored);
		nodeGeometry.dispose(); edgeGeometry.dispose(); lineMaterial.dispose(); pointMaterial.dispose();
		renderer.dispose(); renderer.forceContextLoss(); renderer.domElement.remove();
		delete host.dataset.graphReady;
	};
}
