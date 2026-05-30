import { ref } from "vue";

/** WebGL heatmap: Rust → ArrayBuffer → texture. Full init (16 B hdr) или patch (24 B hdr). */

export const KNOCK_SPECTROGRAM_GPU_HEADER = 16;
export const KNOCK_SPECTROGRAM_GPU_PATCH_HEADER = 24;
/** FFT hop=256 @ 218.75 kHz → столбцов на секунду данных. */
export const KNOCK_SPECTROGRAM_COLS_PER_SEC = 218_750 / 256;

const VS = `
attribute vec2 aPos;
varying vec2 vUv;
void main() {
  vUv = aPos * 0.5 + 0.5;
  gl_Position = vec4(aPos, 0.0, 1.0);
}`;

const FS = `
precision mediump float;
uniform sampler2D uTex;
uniform float uPeak;
uniform float uTexCols;
uniform float uColsPerSec;
varying vec2 vUv;
vec3 heat(float t) {
  t = clamp(t, 0.0, 1.0);
  return vec3(
    (20.0 + 180.0 * t) / 255.0,
    (30.0 + 90.0 * (1.0 - abs(t - 0.55) * 2.0)) / 255.0,
    (80.0 + 120.0 * (1.0 - t)) / 255.0
  );
}
void main() {
  vec2 uv = vec2(vUv.x, 1.0 - vUv.y);
  float v = texture2D(uTex, uv).r;
  float t = v / max(uPeak, 1.0 / 255.0);
  vec3 color = heat(t);

  if (uTexCols > 1.0) {
    float col = uv.x * uTexCols;
    float sec = max(uColsPerSec, 1.0);
    float phase = mod(col, sec);
    if (phase < 1.25) {
      color = mix(color, vec3(0.95, 0.92, 0.35), 0.9);
    }
    if (col >= uTexCols - 1.25) {
      color = mix(color, vec3(0.25, 0.95, 1.0), 0.92);
    }
  }

  gl_FragColor = vec4(color, 1.0);
}`;

function compileShader(gl: WebGLRenderingContext, type: number, src: string): WebGLShader {
  const sh = gl.createShader(type)!;
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
    throw new Error(gl.getShaderInfoLog(sh) ?? "shader compile failed");
  }
  return sh;
}

function linkProgram(gl: WebGLRenderingContext, vs: WebGLShader, fs: WebGLShader): WebGLProgram {
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

function rowMajorToRgba(gray: Uint8Array): Uint8Array {
  const rgba = new Uint8Array(gray.length * 4);
  for (let i = 0; i < gray.length; i += 1) {
    const v = gray[i]!;
    const j = i * 4;
    rgba[j] = v;
    rgba[j + 1] = v;
    rgba[j + 2] = v;
    rgba[j + 3] = 255;
  }
  return rgba;
}

function uploadRowMajor(
  ctx: WebGLRenderingContext,
  tex: WebGLTexture,
  texW: number,
  texH: number,
  pixels: Uint8Array,
): boolean {
  ctx.bindTexture(ctx.TEXTURE_2D, tex);
  if (pixels.length !== texW * texH) return false;
  const rgba = rowMajorToRgba(pixels);
  ctx.texImage2D(
    ctx.TEXTURE_2D,
    0,
    ctx.RGBA,
    texW,
    texH,
    0,
    ctx.RGBA,
    ctx.UNSIGNED_BYTE,
    rgba,
  );
  return true;
}

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
    nonzeroPixels: 0,
    uploads: 0,
    fullSynced: false,
  };
}

export type KnockSpectrogramGl = {
  applyBuffer: (buf: ArrayBuffer) => void;
  draw: () => void;
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

function createKnockSpectrogramGl(
  ctx: WebGLRenderingContext,
  canvas: HTMLCanvasElement,
): KnockSpectrogramGl {
  let prog: WebGLProgram | null = null;
  let aPos = -1;
  let uTex: WebGLUniformLocation | null = null;
  let uPeak: WebGLUniformLocation | null = null;
  let uTexCols: WebGLUniformLocation | null = null;
  let uColsPerSec: WebGLUniformLocation | null = null;
  let vbo: WebGLBuffer | null = null;
  let tex: WebGLTexture | null = null;

  let texW = 0;
  let texH = 0;
  let cpuTex: Uint8Array | null = null;
  let texPeak = 0;
  let fullSynced = false;
  let fullLoadInFlight = false;
  const pendingPatches: ArrayBuffer[] = [];

  function initGlResources(): void {
    if (prog) {
      ctx.deleteProgram(prog);
      if (vbo) ctx.deleteBuffer(vbo);
      if (tex) ctx.deleteTexture(tex);
    }
    prog = linkProgram(
      ctx,
      compileShader(ctx, ctx.VERTEX_SHADER, VS),
      compileShader(ctx, ctx.FRAGMENT_SHADER, FS),
    );
    ctx.useProgram(prog);
    aPos = ctx.getAttribLocation(prog, "aPos");
    uTex = ctx.getUniformLocation(prog, "uTex");
    uPeak = ctx.getUniformLocation(prog, "uPeak");
    uTexCols = ctx.getUniformLocation(prog, "uTexCols");
    uColsPerSec = ctx.getUniformLocation(prog, "uColsPerSec");

    vbo = ctx.createBuffer()!;
    ctx.bindBuffer(ctx.ARRAY_BUFFER, vbo);
    ctx.bufferData(ctx.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), ctx.STATIC_DRAW);
    ctx.enableVertexAttribArray(aPos);
    ctx.vertexAttribPointer(aPos, 2, ctx.FLOAT, false, 0, 0);

    tex = ctx.createTexture()!;
    ctx.activeTexture(ctx.TEXTURE0);
    ctx.bindTexture(ctx.TEXTURE_2D, tex);
    ctx.uniform1i(uTex, 0);
    ctx.texParameteri(ctx.TEXTURE_2D, ctx.TEXTURE_MIN_FILTER, ctx.NEAREST);
    ctx.texParameteri(ctx.TEXTURE_2D, ctx.TEXTURE_MAG_FILTER, ctx.NEAREST);
    ctx.texParameteri(ctx.TEXTURE_2D, ctx.TEXTURE_WRAP_S, ctx.CLAMP_TO_EDGE);
    ctx.texParameteri(ctx.TEXTURE_2D, ctx.TEXTURE_WRAP_T, ctx.CLAMP_TO_EDGE);
  }

  function refreshPeak(): void {
    if (!cpuTex || cpuTex.length === 0) {
      texPeak = 0;
      return;
    }
    texPeak = scanBytes(cpuTex).max;
  }

  function syncTexture(): void {
    if (!cpuTex || texW < 1 || texH < 1 || !tex) return;
    uploadRowMajor(ctx, tex, texW, texH, cpuTex);
  }

  function flushPendingPatches(): void {
    const batch = pendingPatches.splice(0, pendingPatches.length);
    for (const buf of batch) applyPatchNow(buf);
  }

  function requestFullSync(onDone?: () => void): void {
    if (fullSynced || fullLoadInFlight || !fullBufferFetcher) {
      onDone?.();
      return;
    }
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
        onDone?.();
      });
  }

  initGlResources();

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
    refreshPeak();
    syncTexture();
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
    refreshPeak();
    syncTexture();
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
      nonzeroPixels: scan.nz,
      fullSynced,
    });
  }

  function applyFull(buf: ArrayBuffer): void {
    applyFullNow(buf);
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
        applyFull(buf);
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
        return;
      }
    }
  }

  function draw(): void {
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.floor(canvas.clientWidth * dpr));
    const h = Math.max(1, Math.floor(canvas.clientHeight * dpr));
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
      initGlResources();
      syncTexture();
    }
    if (!prog || !uPeak || !uTexCols || !uColsPerSec) return;
    ctx.useProgram(prog);
    ctx.viewport(0, 0, w, h);
    ctx.clearColor(0.1, 0.11, 0.14, 1);
    ctx.clear(ctx.COLOR_BUFFER_BIT);
    if (texW > 0 && texH > 0 && cpuTex) {
      ctx.uniform1f(uPeak, texPeak > 0 ? texPeak / 255 : 1 / 255);
      ctx.uniform1f(uTexCols, texW);
      ctx.uniform1f(uColsPerSec, KNOCK_SPECTROGRAM_COLS_PER_SEC);
      ctx.drawArrays(ctx.TRIANGLE_STRIP, 0, 4);
    }
  }

  function reset(): void {
    texW = 0;
    texH = 0;
    cpuTex = null;
    texPeak = 0;
    fullSynced = false;
    fullLoadInFlight = false;
    pendingPatches.length = 0;
    resetKnockSpectrogramGlStats();
  }

  function destroy(): void {
    reset();
    if (prog) ctx.deleteProgram(prog);
    if (vbo) ctx.deleteBuffer(vbo);
    if (tex) ctx.deleteTexture(tex);
    prog = null;
    vbo = null;
    tex = null;
  }

  return { applyBuffer, draw, reset, destroy };
}

export function mountKnockSpectrogramGl(canvas: HTMLCanvasElement): KnockSpectrogramGl | null {
  const ctx = canvas.getContext("webgl", { alpha: false, antialias: false });
  if (!ctx) return null;
  return createKnockSpectrogramGl(ctx, canvas);
}

export async function fetchKnockSpectrogramFullBuffer(): Promise<ArrayBuffer | null> {
  if (!fullBufferFetcher) return null;
  return fullBufferFetcher();
}
