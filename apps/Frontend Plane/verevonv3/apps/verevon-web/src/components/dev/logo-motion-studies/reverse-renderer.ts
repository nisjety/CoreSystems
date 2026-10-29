import { BODY_MESH, RIBBON_AXIS, RIBBON_LENGTH, RIBBON_MESH, RIBBON_NORMAL, RIBBON_ORIGIN, RIBBON_THICKNESS } from "./ribbon-material";
import { REVEAL_HOLD_MS, TURN_DURATION_MS, TURN_TOP_EXTRA_MS, TURN_TRAVEL_MS, ribbonViewShear } from "./reveal-motion";

const VERTEX_SHADER = `
attribute vec3 aMaterial;
uniform bool uBody;
uniform float uTime, uLength, uView, uHalfThickness;
uniform vec4 uTiming;
uniform vec2 uAxis, uAcross, uOrigin;
varying float vFacing;
void main() {
  vec3 p = aMaterial;
  vFacing = 1.0;
  if (!uBody) {
    float s = p.x, q = p.y;
    float u = clamp(s / uLength, 0.0, 1.0);
    float duration = uTiming.z + uTiming.w * u;
    float t = clamp((uTime - uTiming.x - uTiming.y * u) / duration, 0.0, 1.0);
    float theta = -6.28318530718 * t*t*t * (t*(t*6.0-15.0)+10.0);
    float gradient = 6.28318530718 * 30.0*t*t*(1.0-t)*(1.0-t)
      * (uTiming.y + uTiming.w*t) / (uLength*duration);
    float c = cos(theta), sn = sin(theta);
    vec3 normal = normalize(vec3(-sn*uAcross - q*gradient*uAxis, c));
    p = vec3(uOrigin + s*uAxis + q*c*uAcross, q*sn) + p.z*normal;
    p.z -= uHalfThickness;
    vFacing = normal.z;
  }
  float depth = p.z - uView*dot(p.xy-uOrigin, uAxis);
  vec2 screen = p.xy + uView*p.z*uAxis;
  gl_Position = vec4((screen.x-20.0)/82.0*2.0-1.0, 1.0-(screen.y-26.0)/68.0*2.0, -depth/64.0, 1.0);
}`;

const FRAGMENT_SHADER = `
precision mediump float;
uniform vec3 uColor;
uniform float uRole;
varying float vFacing;
void main() {
  vec3 color = uColor;
  if (uRole == 1.0) color = mix(uColor, vec3(0.25, 0.24, 0.22), 0.85*clamp(0.5-0.5*vFacing, 0.0, 1.0));
  if (uRole == 2.0) color = mix(uColor, vec3(0.42, 0.40, 0.37), 0.8*(1.0-abs(vFacing)));
  if (uRole == 0.0) color *= 0.9 + 0.1*abs(vFacing);
  gl_FragColor = vec4(color, 1.0);
}`;

export type RibbonRenderer = { draw: (timeMs: number) => void; dispose: () => void };

function compile(gl: WebGLRenderingContext, type: number, source: string) {
	const shader = gl.createShader(type);
	if (!shader) throw new Error("Ribbon shader unavailable");
	gl.shaderSource(shader, source);
	gl.compileShader(shader);
	if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
		const message = gl.getShaderInfoLog(shader);
		gl.deleteShader(shader);
		throw new Error(`Ribbon shader compilation failed: ${message}`);
	}
	return shader;
}

function upload(gl: WebGLRenderingContext, target: number, data: Float32Array | Uint16Array) {
	const buffer = gl.createBuffer();
	if (!buffer) throw new Error("Ribbon buffer unavailable");
	gl.bindBuffer(target, buffer);
	gl.bufferData(target, data, gl.STATIC_DRAW);
	return buffer;
}

export function createReverseRenderer(canvas: HTMLCanvasElement, color: string, onFailure: () => void): RibbonRenderer | null {
	const gl = canvas.getContext("webgl", { alpha: true, antialias: true, premultipliedAlpha: true });
	if (!gl) return null;
	const program = gl.createProgram();
	if (!program) return null;
	let shaders: readonly WebGLShader[] = [];
	let buffers: readonly WebGLBuffer[] = [];
	const release = () => {
		buffers.forEach((buffer) => gl.deleteBuffer(buffer));
		shaders.forEach((shader) => gl.deleteShader(shader));
		gl.deleteProgram(program);
	};
	try {
		shaders = [compile(gl, gl.VERTEX_SHADER, VERTEX_SHADER)];
		shaders = [...shaders, compile(gl, gl.FRAGMENT_SHADER, FRAGMENT_SHADER)];
		shaders.forEach((shader) => gl.attachShader(program, shader));
		gl.linkProgram(program);
		if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(`Ribbon shader link failed: ${gl.getProgramInfoLog(program)}`);
		const buffer = (target: number, data: Float32Array | Uint16Array) => {
			const next = upload(gl, target, data);
			buffers = [...buffers, next];
			return next;
		};
		const ribbon = buffer(gl.ARRAY_BUFFER, new Float32Array(RIBBON_MESH.vertices.flat()));
		const body = buffer(gl.ARRAY_BUFFER, new Float32Array(BODY_MESH.vertices.flat()));
		const faces = RIBBON_MESH.faces.map(({ indices }, role) => ({ role, count: indices.length,
			buffer: buffer(gl.ELEMENT_ARRAY_BUFFER, new Uint16Array(indices)) }));
		const bodyIndices = buffer(gl.ELEMENT_ARRAY_BUFFER, new Uint16Array(BODY_MESH.indices));
		gl.useProgram(program);
		const uniform = (name: string) => gl.getUniformLocation(program, name);
		gl.uniform2fv(uniform("uAxis"), RIBBON_AXIS);
		gl.uniform2fv(uniform("uAcross"), RIBBON_NORMAL);
		gl.uniform2fv(uniform("uOrigin"), RIBBON_ORIGIN);
		gl.uniform1f(uniform("uLength"), RIBBON_LENGTH);
		gl.uniform1f(uniform("uHalfThickness"), RIBBON_THICKNESS / 2);
		gl.uniform4f(uniform("uTiming"), REVEAL_HOLD_MS, TURN_TRAVEL_MS, TURN_DURATION_MS, TURN_TOP_EXTRA_MS);
		const rgb = color.match(/[\d.]+/g)?.slice(0, 3).map(Number) ?? [23, 23, 23];
		gl.uniform3fv(uniform("uColor"), rgb.map((v) => v / 255));
		const timeUniform = uniform("uTime");
		const viewUniform = uniform("uView");
		const bodyUniform = uniform("uBody");
		const roleUniform = uniform("uRole");
		const attribute = gl.getAttribLocation(program, "aMaterial");
		gl.enableVertexAttribArray(attribute);
		gl.enable(gl.DEPTH_TEST);
		gl.depthFunc(gl.LEQUAL);
		gl.disable(gl.CULL_FACE);
		gl.clearColor(0, 0, 0, 0);
		let lastTime = 0;
		let disposed = false;
		const draw = (timeMs: number) => {
			if (disposed || gl.isContextLost()) return;
			lastTime = timeMs;
			const rect = canvas.getBoundingClientRect();
			const ratio = Math.min(window.devicePixelRatio || 1, 2);
			const width = Math.max(1, Math.round(rect.width * ratio));
			const height = Math.max(1, Math.round(rect.height * ratio));
			if (canvas.width !== width || canvas.height !== height) { canvas.width = width; canvas.height = height; }
			gl.viewport(0, 0, width, height);
			gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
			gl.useProgram(program);
			gl.uniform1f(timeUniform, timeMs);
			gl.uniform1f(viewUniform, ribbonViewShear(timeMs));
			const render = (vertexBuffer: WebGLBuffer, indexBuffer: WebGLBuffer, count: number, isBody: boolean, role: number) => {
				gl.bindBuffer(gl.ARRAY_BUFFER, vertexBuffer);
				gl.vertexAttribPointer(attribute, 3, gl.FLOAT, false, 0, 0);
				gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indexBuffer);
				gl.uniform1i(bodyUniform, isBody ? 1 : 0);
				gl.uniform1f(roleUniform, role);
				gl.drawElements(gl.TRIANGLES, count, gl.UNSIGNED_SHORT, 0);
			};
			render(body, bodyIndices, BODY_MESH.indices.length, true, 3);
			faces.forEach((face) => render(ribbon, face.buffer, face.count, false, face.role));
		};
		const lost = (event: Event) => { event.preventDefault(); onFailure(); };
		canvas.addEventListener("webglcontextlost", lost);
		const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => draw(lastTime));
		observer?.observe(canvas);
		return { draw, dispose: () => { disposed = true; observer?.disconnect(); canvas.removeEventListener("webglcontextlost", lost); release(); } };
	} catch (error) {
		release();
		console.warn("Verevon ribbon unavailable; showing the approved static logo.", error);
		return null;
	}
}
