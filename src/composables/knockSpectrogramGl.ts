import { ref } from "vue";
import type { KnockCylinderMarker } from "./knockSpectrogramMarkers";
import { knockMarkersForGpu } from "./knockSpectrogramMarkers";

/** Спектрограмма: один WebGL2 canvas — heatmap, colorbar, линии цилиндров. */

export const KNOCK_SPECTROGRAM_GPU_HEADER = 16;
export const KNOCK_SPECTROGRAM_GPU_PATCH_HEADER = 24;
export const KNOCK_SPECTROGRAM_SAMPLE_RATE_HZ = 218_750;
/** Оценка столбцов/с при ~800 rpm, 6 цил. (один столбец на knock-окно на хосте). */
export const KNOCK_SPECTROGRAM_COLS_PER_SEC = 40;

export const KNOCK_SPECTROGRAM_FREQ_MIN_HZ = 0;
export const KNOCK_SPECTROGRAM_FREQ_MAX_HZ = 20_000;
export const KNOCK_SPECTROGRAM_DBFS_MIN = -100;
export const KNOCK_SPECTROGRAM_DBFS_MAX = -20;

/** Автоконтраст и яркость отображения (не меняют Rust/dBFS на проводе). */
export type KnockSpectrogramDisplay = {
  autocontrast: boolean;
  /** 1…400 %, 100 = без изменений */
  gainPercent: number;
};

export const SPECTROGRAM_MARGINS = {
  left: 52,
  right: 56,
  top: 12,
  bottom: 32,
} as const;

const VS = `#version 300 es
in vec2 aPos;
out vec2 vUv;
void main() {
  vUv = aPos * 0.5 + 0.5;
  gl_Position = vec4(aPos, 0.0, 1.0);
}`;

const FS = `#version 300 es
precision mediump float;

uniform sampler2D uTex;
uniform vec2 uCanvas;
uniform vec4 uPlot;
uniform vec4 uBar;
uniform float uHasTex;
uniform float uAutoContrast;
uniform float uDispMin;
uniform float uDispMax;
uniform float uGainScale;

in vec2 vUv;
out vec4 outColor;

vec3 inferno(float t) {
  t = clamp(t, 0.0, 1.0);
  vec3 c;
  if (t < 0.13) {
    c = mix(vec3(0.0, 0.0, 0.016), vec3(0.157, 0.043, 0.271), t / 0.13);
  } else if (t < 0.25) {
    c = mix(vec3(0.157, 0.043, 0.271), vec3(0.388, 0.094, 0.4), (t - 0.13) / 0.12);
  } else if (t < 0.38) {
    c = mix(vec3(0.388, 0.094, 0.4), vec3(0.588, 0.157, 0.314), (t - 0.25) / 0.13);
  } else if (t < 0.5) {
    c = mix(vec3(0.588, 0.157, 0.314), vec3(0.784, 0.235, 0.196), (t - 0.38) / 0.12);
  } else if (t < 0.63) {
    c = mix(vec3(0.784, 0.235, 0.196), vec3(0.902, 0.392, 0.118), (t - 0.5) / 0.13);
  } else if (t < 0.75) {
    c = mix(vec3(0.902, 0.392, 0.118), vec3(0.961, 0.627, 0.157), (t - 0.63) / 0.12);
  } else if (t < 0.88) {
    c = mix(vec3(0.961, 0.627, 0.157), vec3(0.98, 0.863, 0.471), (t - 0.75) / 0.13);
  } else {
    c = mix(vec3(0.98, 0.863, 0.471), vec3(1.0, 1.0, 1.0), (t - 0.88) / 0.12);
  }
  return c;
}

bool inRect(vec2 p, vec4 r) {
  return p.x >= r.x && p.x <= r.x + r.z && p.y >= r.y && p.y <= r.y + r.w;
}

float mapDisplay(float v) {
  float t;
  if (uAutoContrast > 0.5) {
    float span = max(uDispMax - uDispMin, 1.0 / 255.0);
    t = clamp((v - uDispMin) / span, 0.0, 1.0);
  } else {
    t = clamp(v, 0.0, 1.0);
  }
  return clamp(t * uGainScale, 0.0, 1.0);
}

void main() {
  // vUv: WebGL origin снизу → переводим в CSS (y вниз).
  vec2 px = vec2(vUv.x * uCanvas.x, (1.0 - vUv.y) * uCanvas.y);
  vec3 color = vec3(0.0);

  if (inRect(px, uBar)) {
    float t = 1.0 - (px.y - uBar.y) / uBar.w;
    color = inferno(mapDisplay(mix(uDispMin, uDispMax, t)));
  } else if (inRect(px, uPlot) && uHasTex > 0.5) {
    vec2 rel = (px - uPlot.xy) / uPlot.zw;
    vec2 tuv = vec2(clamp(rel.x, 0.0, 1.0), clamp(1.0 - rel.y, 0.0, 1.0));
    float v = texture(uTex, tuv).r;
    color = inferno(mapDisplay(v));
  }

  vec4 plotB = vec4(uPlot.x - 0.5, uPlot.y - 0.5, uPlot.z + 1.0, uPlot.w + 1.0);
  vec4 barB = vec4(uBar.x - 0.5, uBar.y - 0.5, uBar.z + 1.0, uBar.w + 1.0);
  float onPlotEdge =
    (inRect(px, plotB) && !inRect(px, uPlot)) ? 1.0 : 0.0;
  float onBarEdge =
    (inRect(px, barB) && !inRect(px, uBar)) ? 1.0 : 0.0;
  if (onPlotEdge + onBarEdge > 0.0) {
    color = vec3(1.0);
  }

  outColor = vec4(color, 1.0);
}`;

const MARKER_LINE_VS = `#version 300 es
in vec2 aPos;
in vec3 aColor;
uniform vec2 uCanvas;
out vec3 vColor;
void main() {
  vec2 clip = vec2(aPos.x / uCanvas.x * 2.0 - 1.0, 1.0 - aPos.y / uCanvas.y * 2.0);
  gl_Position = vec4(clip, 0.0, 1.0);
  vColor = aColor;
}`;

const MARKER_LINE_FS = `#version 300 es
precision mediump float;
in vec3 vColor;
out vec4 outColor;
void main() {
  outColor = vec4(vColor, 0.92);
}`;

const MARKER_COLORS: ReadonlyArray<[number, number, number]> = [
  [1.0, 0.92, 0.35],
  [0.45, 0.82, 1.0],
  [0.55, 1.0, 0.55],
  [1.0, 0.55, 0.75],
  [0.75, 0.65, 1.0],
  [1.0, 0.7, 0.4],
  [0.5, 0.95, 0.9],
  [0.95, 0.55, 0.45],
];

function markerRgb(cylinder: number): [number, number, number] {
  return MARKER_COLORS[cylinder % MARKER_COLORS.length]!;
}

function compileShader(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader {
  const sh = gl.createShader(type)!;
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
    throw new Error(gl.getShaderInfoLog(sh) ?? "shader compile failed");
  }
  return sh;
}

function linkProgram(gl: WebGL2RenderingContext, vs: WebGLShader, fs: WebGLShader): WebGLProgram {
  const prog = gl.createProgram()!;
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
    throw new Error(gl.getProgramInfoLog(prog) ?? "program link failed");
  }
  return prog;
}

export function b64ToArrayBuffer(b64: string): ArrayBuffer {
  const bin = atob(b64);
  const buf = new ArrayBuffer(bin.length);
  const u8 = new Uint8Array(buf);
  for (let i = 0; i < bin.length; i += 1) u8[i] = bin.charCodeAt(i);
  return buf;
}

/** u8 из Rust → dBFS (Rust: dbfs_to_u8, диапазон -100…-20). */
export function knockSpectrogramU8ToDbfs(u8: number): number {
  const t = u8 / 255;
  return KNOCK_SPECTROGRAM_DBFS_MIN + t * (KNOCK_SPECTROGRAM_DBFS_MAX - KNOCK_SPECTROGRAM_DBFS_MIN);
}

function scanBytes(bytes: Uint8Array): { min: number; max: number; nz: number } {
  if (bytes.length === 0) return { min: 0, max: 0, nz: 0 };
  let min = 255;
  let max = 0;
  let nz = 0;
  for (let i = 0; i < bytes.length; i += 1) {
    const v = bytes[i]!;
    if (v > 0) nz += 1;
    if (v < min) min = v;
    if (v > max) max = v;
  }
  return { min, max, nz };
}

/** Min/max u8 для автоконтраста (2–98 перцентиль среди ненулевых). */
function displayRangeU8(bytes: Uint8Array): { min: number; max: number } {
  const vals: number[] = [];
  for (let i = 0; i < bytes.length; i += 1) {
    const v = bytes[i]!;
    if (v > 0) vals.push(v);
  }
  if (vals.length === 0) return { min: 0, max: 255 };
  vals.sort((a, b) => a - b);
  const pick = (pct: number) => vals[Math.floor((vals.length - 1) * pct)]!;
  let min = pick(0.02);
  let max = pick(0.98);
  if (max <= min) max = Math.min(255, min + 1);
  return { min, max };
}

function gainPercentToScale(gainPercent: number): number {
  return Math.max(1, Math.min(400, gainPercent)) / 100;
}

type PlotLayout = {
  cssW: number;
  cssH: number;
  plot: { x: number; y: number; w: number; h: number };
  bar: { x: number; y: number; w: number; h: number };
};

function computeLayout(cssW: number, cssH: number): PlotLayout {
  const m = SPECTROGRAM_MARGINS;
  const plot = {
    x: m.left,
    y: m.top,
    w: Math.max(1, cssW - m.left - m.right),
    h: Math.max(1, cssH - m.top - m.bottom),
  };
  const bar = {
    x: plot.x + plot.w + 10,
    y: plot.y,
    w: 14,
    h: plot.h,
  };
  return { cssW, cssH, plot, bar };
};

export type KnockSpectrogramGlStats = {
  packetKind: "none" | "full" | "patch";
  packetBytes: number;
  payloadMax: number;
  texW: number;
  texH: number;
  shiftLeft: number;
  newCols: number;
  pixelMin: number;
  pixelMax: number;
  displayMinU8: number;
  displayMaxU8: number;
  displayGainScale: number;
  nonzeroPixels: number;
  uploads: number;
  fullSynced: boolean;
};

export const knockSpectrogramGlStats = ref<KnockSpectrogramGlStats>({
  packetKind: "none",
  packetBytes: 0,
  payloadMax: 0,
  texW: 0,
  texH: 0,
  shiftLeft: 0,
  newCols: 0,
  pixelMin: 0,
  pixelMax: 0,
  displayMinU8: 0,
  displayMaxU8: 255,
  displayGainScale: 1,
  nonzeroPixels: 0,
  uploads: 0,
  fullSynced: false,
});

function recordStats(partial: Omit<KnockSpectrogramGlStats, "uploads"> & { uploads?: number }): void {
  const prev = knockSpectrogramGlStats.value;
  knockSpectrogramGlStats.value = {
    ...partial,
    uploads: partial.uploads ?? prev.uploads + 1,
  };
}

export function resetKnockSpectrogramGlStats(): void {
  knockSpectrogramGlStats.value = {
    packetKind: "none",
    packetBytes: 0,
    payloadMax: 0,
    texW: 0,
    texH: 0,
    shiftLeft: 0,
    newCols: 0,
    pixelMin: 0,
    pixelMax: 0,
    displayMinU8: 0,
    displayMaxU8: 255,
    displayGainScale: 1,
    nonzeroPixels: 0,
    uploads: 0,
    fullSynced: false,
  };
}

export type KnockSpectrogramGl = {
  applyBuffer: (buf: ArrayBuffer) => void;
  draw: () => void;
  setDisplay: (display: KnockSpectrogramDisplay) => void;
  setMarkers: (markers: readonly KnockCylinderMarker[], texWidth: number) => void;
  reset: () => void;
  destroy: () => void;
};

type FullBufferFetcher = () => Promise<ArrayBuffer | null>;

let fullBufferFetcher: FullBufferFetcher | null = null;

export function registerKnockSpectrogramFullBuffer(fetch: FullBufferFetcher): void {
  fullBufferFetcher = fetch;
}

function ensureCpuTex(
  w: number,
  h: number,
  prev: Uint8Array | null,
  prevW: number,
): Uint8Array {
  if (!prev || prevW < 1) {
    return new Uint8Array(w * h);
  }
  if (prev.length === w * h) {
    return prev;
  }
  const next = new Uint8Array(w * h);
  const copyW = Math.min(prevW, w);
  for (let row = 0; row < h; row += 1) {
    next.set(prev.subarray(row * prevW, row * prevW + copyW), row * w);
  }
  return next;
}

function createKnockSpectrogramGl(gl: WebGL2RenderingContext, canvas: HTMLCanvasElement): KnockSpectrogramGl {
  let heatmapProg: WebGLProgram | null = null;
  let aPos = -1;
  let uTex: WebGLUniformLocation | null = null;
  let uCanvas: WebGLUniformLocation | null = null;
  let uPlot: WebGLUniformLocation | null = null;
  let uBar: WebGLUniformLocation | null = null;
  let uHasTex: WebGLUniformLocation | null = null;
  let uAutoContrast: WebGLUniformLocation | null = null;
  let uDispMin: WebGLUniformLocation | null = null;
  let uDispMax: WebGLUniformLocation | null = null;
  let uGainScale: WebGLUniformLocation | null = null;
  let heatmapVao: WebGLVertexArrayObject | null = null;
  let tex: WebGLTexture | null = null;

  let lineProg: WebGLProgram | null = null;
  let lineVao: WebGLVertexArrayObject | null = null;
  let lineVbo: WebGLBuffer | null = null;
  let lineAPos = -1;
  let lineAColor = -1;
  let lineUCanvas: WebGLUniformLocation | null = null;
  let lineVertexCount = 0;

  let markerTexWidth = 0;
  let markerList: KnockCylinderMarker[] = [];

  let display: KnockSpectrogramDisplay = { autocontrast: true, gainPercent: 100 };
  let lastGainScale = 1;

  let texW = 0;
  let texH = 0;
  let cpuTex: Uint8Array | null = null;
  let texDirty = false;
  let fullSynced = false;
  let fullLoadInFlight = false;
  const pendingPatches: ArrayBuffer[] = [];
  let displayMinU8 = 0;
  let displayMaxU8 = 255;

  function refreshDisplayRange(): void {
    if (!cpuTex || cpuTex.length === 0) {
      displayMinU8 = 0;
      displayMaxU8 = 255;
      return;
    }
    const { autocontrast } = display;
    if (!autocontrast) {
      displayMinU8 = 0;
      displayMaxU8 = 255;
      return;
    }
    const r = displayRangeU8(cpuTex);
    displayMinU8 = r.min;
    displayMaxU8 = r.max;
  }

  function initHeatmapGlResources(): void {
    if (heatmapVao) gl.deleteVertexArray(heatmapVao);
    if (heatmapProg) gl.deleteProgram(heatmapProg);
    if (tex) gl.deleteTexture(tex);

    heatmapProg = linkProgram(
      gl,
      compileShader(gl, gl.VERTEX_SHADER, VS),
      compileShader(gl, gl.FRAGMENT_SHADER, FS),
    );
    gl.useProgram(heatmapProg);
    aPos = gl.getAttribLocation(heatmapProg, "aPos");
    uTex = gl.getUniformLocation(heatmapProg, "uTex");
    uCanvas = gl.getUniformLocation(heatmapProg, "uCanvas");
    uPlot = gl.getUniformLocation(heatmapProg, "uPlot");
    uBar = gl.getUniformLocation(heatmapProg, "uBar");
    uHasTex = gl.getUniformLocation(heatmapProg, "uHasTex");
    uAutoContrast = gl.getUniformLocation(heatmapProg, "uAutoContrast");
    uDispMin = gl.getUniformLocation(heatmapProg, "uDispMin");
    uDispMax = gl.getUniformLocation(heatmapProg, "uDispMax");
    uGainScale = gl.getUniformLocation(heatmapProg, "uGainScale");

    const vbo = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);

    heatmapVao = gl.createVertexArray()!;
    gl.bindVertexArray(heatmapVao);
    gl.enableVertexAttribArray(aPos);
    gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);
    gl.bindVertexArray(null);

    tex = gl.createTexture()!;
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.uniform1i(uTex, 0);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  }

  function initLineGlResources(): void {
    if (lineVao) gl.deleteVertexArray(lineVao);
    if (lineProg) gl.deleteProgram(lineProg);
    if (lineVbo) gl.deleteBuffer(lineVbo);

    lineProg = linkProgram(
      gl,
      compileShader(gl, gl.VERTEX_SHADER, MARKER_LINE_VS),
      compileShader(gl, gl.FRAGMENT_SHADER, MARKER_LINE_FS),
    );
    lineAPos = gl.getAttribLocation(lineProg, "aPos");
    lineAColor = gl.getAttribLocation(lineProg, "aColor");
    lineUCanvas = gl.getUniformLocation(lineProg, "uCanvas");

    lineVbo = gl.createBuffer()!;
    lineVao = gl.createVertexArray()!;
    gl.bindVertexArray(lineVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, lineVbo);
    const stride = 5 * 4;
    gl.enableVertexAttribArray(lineAPos);
    gl.vertexAttribPointer(lineAPos, 2, gl.FLOAT, false, stride, 0);
    gl.enableVertexAttribArray(lineAColor);
    gl.vertexAttribPointer(lineAColor, 3, gl.FLOAT, false, stride, 8);
    gl.bindVertexArray(null);
  }

  function initGlResources(): void {
    initHeatmapGlResources();
    initLineGlResources();
  }

  function syncTexture(): void {
    if (!texDirty || !cpuTex || texW < 1 || texH < 1 || !tex) return;
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, texW, texH, 0, gl.RED, gl.UNSIGNED_BYTE, cpuTex);
    texDirty = false;
  }

  function uploadMarkerLines(layout: PlotLayout): void {
    if (!lineProg || !lineVao || !lineVbo) return;
    const tw = markerTexWidth > 0 ? markerTexWidth : texW;
    if (tw < 1 || markerList.length === 0) {
      lineVertexCount = 0;
      return;
    }
    const verts = new Float32Array(markerList.length * 2 * 5);
    let n = 0;
    const { plot } = layout;
    const y0 = plot.y;
    const y1 = plot.y + plot.h;
    for (const mk of markerList) {
      const t = Math.max(0, Math.min(mk.column, tw)) / tw;
      const x = plot.x + t * plot.w;
      const [r, g, b] = markerRgb(mk.cylinder);
      for (const y of [y0, y1]) {
        verts[n++] = x;
        verts[n++] = y;
        verts[n++] = r;
        verts[n++] = g;
        verts[n++] = b;
      }
    }
    lineVertexCount = markerList.length * 2;
    gl.bindBuffer(gl.ARRAY_BUFFER, lineVbo);
    gl.bufferData(gl.ARRAY_BUFFER, verts, gl.DYNAMIC_DRAW);
  }

  function drawMarkerLines(cssW: number, cssH: number): void {
    if (lineVertexCount < 2 || !lineProg || !lineVao) return;
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.useProgram(lineProg);
    gl.bindVertexArray(lineVao);
    if (lineUCanvas) gl.uniform2f(lineUCanvas, cssW, cssH);
    gl.drawArrays(gl.LINES, 0, lineVertexCount);
    gl.bindVertexArray(null);
  }

  function flushPendingPatches(): void {
    for (const buf of pendingPatches.splice(0, pendingPatches.length)) {
      applyPatchNow(buf);
    }
  }

  function requestFullSync(): void {
    if (fullSynced || fullLoadInFlight || !fullBufferFetcher) return;
    fullLoadInFlight = true;
    void fullBufferFetcher()
      .then((buf) => {
        if (buf && buf.byteLength > KNOCK_SPECTROGRAM_GPU_HEADER) {
          applyFullNow(buf);
        }
        fullSynced = true;
        flushPendingPatches();
      })
      .finally(() => {
        fullLoadInFlight = false;
      });
  }

  initGlResources();

  function setDisplay(next: KnockSpectrogramDisplay): void {
    display = {
      autocontrast: next.autocontrast,
      gainPercent: Math.max(1, Math.min(400, next.gainPercent)),
    };
  }

  function setMarkers(markers: readonly KnockCylinderMarker[], texWidth: number): void {
    markerTexWidth = Math.max(0, Math.floor(texWidth));
    markerList = knockMarkersForGpu(markers);
  }

  function applyFullNow(buf: ArrayBuffer): void {
    if (buf.byteLength < KNOCK_SPECTROGRAM_GPU_HEADER) return;
    const dv = new DataView(buf);
    const w = dv.getUint32(0, true);
    const h = dv.getUint32(4, true);
    if (w < 1 || h < 1) return;
    const pixels = new Uint8Array(buf, KNOCK_SPECTROGRAM_GPU_HEADER);
    if (pixels.length < w * h) return;
    texW = w;
    texH = h;
    cpuTex = pixels.slice();
    fullSynced = true;
    texDirty = true;
    refreshDisplayRange();
    const scan = scanBytes(cpuTex);
    recordStats({
      packetKind: "full",
      packetBytes: buf.byteLength,
      payloadMax: scan.max,
      texW: w,
      texH: h,
      shiftLeft: 0,
      newCols: w,
      pixelMin: scan.min,
      pixelMax: scan.max,
      displayMinU8: displayMinU8,
      displayMaxU8: displayMaxU8,
      displayGainScale: gainPercentToScale(display.gainPercent),
      nonzeroPixels: scan.nz,
      fullSynced: true,
    });
  }

  function applyPatchNow(buf: ArrayBuffer): void {
    if (buf.byteLength < KNOCK_SPECTROGRAM_GPU_PATCH_HEADER) return;
    const dv = new DataView(buf);
    const w = dv.getUint32(0, true);
    const h = dv.getUint32(4, true);
    const shift = dv.getUint32(16, true);
    const newCols = dv.getUint32(20, true);
    if (w < 1 || h < 1 || newCols < 1) return;
    const payload = new Uint8Array(buf, KNOCK_SPECTROGRAM_GPU_PATCH_HEADER);
    if (payload.length < newCols * h) return;

    const payloadScan = scanBytes(payload);
    const prevW = texW;
    cpuTex = ensureCpuTex(w, h, cpuTex, prevW);
    texW = w;
    texH = h;

    if (shift > 0) {
      const n = Math.min(shift, w);
      for (let row = 0; row < h; row += 1) {
        const rowStart = row * w;
        cpuTex.copyWithin(rowStart, rowStart + n, rowStart + w);
        cpuTex.fill(0, rowStart + w - n, rowStart + w);
      }
    }

    const startCol = w - newCols;
    for (let c = 0; c < newCols; c += 1) {
      const colOff = c * h;
      for (let row = 0; row < h; row += 1) {
        cpuTex[row * w + startCol + c] = payload[colOff + row]!;
      }
    }
    texDirty = true;
    refreshDisplayRange();
    const scan = scanBytes(cpuTex);
    recordStats({
      packetKind: "patch",
      packetBytes: buf.byteLength,
      payloadMax: payloadScan.max,
      texW: w,
      texH: h,
      shiftLeft: shift,
      newCols,
      pixelMin: scan.min,
      pixelMax: scan.max,
      displayMinU8: displayMinU8,
      displayMaxU8: displayMaxU8,
      displayGainScale: gainPercentToScale(display.gainPercent),
      nonzeroPixels: scan.nz,
      fullSynced,
    });
  }

  function applyPatch(buf: ArrayBuffer): void {
    if (!fullSynced && texW < 1 && fullBufferFetcher) {
      pendingPatches.push(buf.slice(0));
      requestFullSync();
      return;
    }
    applyPatchNow(buf);
  }

  function applyBuffer(buf: ArrayBuffer): void {
    const dv = new DataView(buf);
    if (buf.byteLength >= KNOCK_SPECTROGRAM_GPU_HEADER) {
      const w = dv.getUint32(0, true);
      const h = dv.getUint32(4, true);
      if (w >= 1 && h >= 1 && buf.byteLength === KNOCK_SPECTROGRAM_GPU_HEADER + w * h) {
        applyFullNow(buf);
        return;
      }
    }
    if (buf.byteLength >= KNOCK_SPECTROGRAM_GPU_PATCH_HEADER) {
      const w = dv.getUint32(0, true);
      const h = dv.getUint32(4, true);
      const shift = dv.getUint32(16, true);
      const newCols = dv.getUint32(20, true);
      const expected = KNOCK_SPECTROGRAM_GPU_PATCH_HEADER + newCols * h;
      if (
        w >= 1 &&
        h >= 1 &&
        newCols >= 1 &&
        newCols <= w &&
        shift <= w &&
        buf.byteLength === expected
      ) {
        applyPatch(buf);
      }
    }
  }

  function draw(): void {
    const dpr = window.devicePixelRatio || 1;
    const cssW = Math.max(1, canvas.clientWidth);
    const cssH = Math.max(1, canvas.clientHeight);
    const pixelW = Math.max(1, Math.floor(cssW * dpr));
    const pixelH = Math.max(1, Math.floor(cssH * dpr));
    if (canvas.width !== pixelW || canvas.height !== pixelH) {
      canvas.width = pixelW;
      canvas.height = pixelH;
      initGlResources();
      texDirty = true;
    }

    const layout = computeLayout(cssW, cssH);

    if (!heatmapProg || !heatmapVao || !tex) return;

    syncTexture();
    refreshDisplayRange();
    uploadMarkerLines(layout);

    const dispMinN = display.autocontrast ? displayMinU8 / 255 : 0;
    const dispMaxN = display.autocontrast ? displayMaxU8 / 255 : 1;
    lastGainScale = gainPercentToScale(display.gainPercent);

    gl.viewport(0, 0, pixelW, pixelH);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);

    gl.useProgram(heatmapProg);
    gl.bindVertexArray(heatmapVao);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);

    if (uCanvas) gl.uniform2f(uCanvas, cssW, cssH);
    if (uPlot) gl.uniform4f(uPlot, layout.plot.x, layout.plot.y, layout.plot.w, layout.plot.h);
    if (uBar) gl.uniform4f(uBar, layout.bar.x, layout.bar.y, layout.bar.w, layout.bar.h);
    if (uHasTex) gl.uniform1f(uHasTex, texW > 0 && cpuTex ? 1 : 0);
    if (uAutoContrast) gl.uniform1f(uAutoContrast, display.autocontrast ? 1 : 0);
    if (uDispMin) gl.uniform1f(uDispMin, dispMinN);
    if (uDispMax) gl.uniform1f(uDispMax, dispMaxN);
    if (uGainScale) gl.uniform1f(uGainScale, lastGainScale);

    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    gl.bindVertexArray(null);

    drawMarkerLines(cssW, cssH);
    gl.disable(gl.BLEND);

    const prev = knockSpectrogramGlStats.value;
    if (
      prev.displayMinU8 !== displayMinU8 ||
      prev.displayMaxU8 !== displayMaxU8 ||
      prev.displayGainScale !== lastGainScale
    ) {
      knockSpectrogramGlStats.value = { ...prev, displayMinU8, displayMaxU8, displayGainScale: lastGainScale };
    }
  }

  function reset(): void {
    texW = 0;
    texH = 0;
    cpuTex = null;
    texDirty = false;
    fullSynced = false;
    fullLoadInFlight = false;
    pendingPatches.length = 0;
    markerList = [];
    markerTexWidth = 0;
    lineVertexCount = 0;
    resetKnockSpectrogramGlStats();
  }

  function destroy(): void {
    reset();
    if (heatmapVao) gl.deleteVertexArray(heatmapVao);
    if (heatmapProg) gl.deleteProgram(heatmapProg);
    if (lineVao) gl.deleteVertexArray(lineVao);
    if (lineProg) gl.deleteProgram(lineProg);
    if (lineVbo) gl.deleteBuffer(lineVbo);
    if (tex) gl.deleteTexture(tex);
    heatmapVao = null;
    heatmapProg = null;
    lineVao = null;
    lineProg = null;
    lineVbo = null;
    tex = null;
  }

  return { applyBuffer, draw, setDisplay, setMarkers, reset, destroy };
}

/** Один WebGL2 canvas: heatmap + colorbar. */
export function mountKnockSpectrogramGl(canvas: HTMLCanvasElement): KnockSpectrogramGl | null {
  const gl = canvas.getContext("webgl2", { alpha: false, antialias: false });
  if (!gl) return null;
  return createKnockSpectrogramGl(gl, canvas);
}

export async function fetchKnockSpectrogramFullBuffer(): Promise<ArrayBuffer | null> {
  if (!fullBufferFetcher) return null;
  return fullBufferFetcher();
}
