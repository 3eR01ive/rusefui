/** Только отрисовка heatmap; FFT — в Rust (`knock_spectrogram.rs`). */

export interface KnockSpectrogramView {
  width: number;
  height: number;
  freqStartHz: number;
  freqStepHz: number;
  pixels: number[];
}

function dbToColor(db: number): [number, number, number] {
  const t = db / 255;
  const r = Math.round(20 + 180 * t);
  const g = Math.round(30 + 90 * (1 - Math.abs(t - 0.55) * 2));
  const b = Math.round(80 + 120 * (1 - t));
  return [r, g, b];
}

export function drawKnockSpectrogram(
  canvas: HTMLCanvasElement,
  view: KnockSpectrogramView,
  opts: { title?: string } = {},
): void {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (w < 8 || h < 8) return;

  canvas.width = Math.round(w * dpr);
  canvas.height = Math.round(h * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  const bg =
    getComputedStyle(canvas).getPropertyValue("--color-bg-panel").trim() ||
    "#1a1d24";
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, w, h);

  const { width, height, pixels, freqStartHz, freqStepHz } = view;
  if (width < 1 || height < 1 || pixels.length < width * height) {
    ctx.fillStyle =
      getComputedStyle(canvas).getPropertyValue("--color-text-muted").trim() ||
      "#889";
    ctx.font = "13px system-ui, sans-serif";
    ctx.fillText("Спектрограмма (ждём FFT-столбцы…)", 12, 28);
    return;
  }

  const margin = { top: 16, right: 12, bottom: 28, left: 52 };
  const plotW = w - margin.left - margin.right;
  const plotH = h - margin.top - margin.bottom;

  const img = ctx.createImageData(width, height);
  for (let col = 0; col < width; col++) {
    for (let row = 0; row < height; row++) {
      const db = pixels[col * height + row] ?? 0;
      const [r, g, b] = dbToColor(db);
      const di = (row * width + col) * 4;
      img.data[di] = r;
      img.data[di + 1] = g;
      img.data[di + 2] = b;
      img.data[di + 3] = 255;
    }
  }

  const off = document.createElement("canvas");
  off.width = width;
  off.height = height;
  const offCtx = off.getContext("2d");
  if (!offCtx) return;
  offCtx.putImageData(img, 0, 0);

  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(off, 0, 0, width, height, margin.left, margin.top, plotW, plotH);

  ctx.strokeStyle =
    getComputedStyle(canvas).getPropertyValue("--color-border").trim() ||
    "#3a3f4a";
  ctx.lineWidth = 1;
  ctx.strokeRect(margin.left, margin.top, plotW, plotH);

  const fTop = freqStartHz + freqStepHz * (height - 1);
  ctx.fillStyle =
    getComputedStyle(canvas).getPropertyValue("--color-text-muted").trim() ||
    "#aab";
  ctx.font = "10px ui-monospace, Menlo, monospace";
  ctx.fillText(`${Math.round(freqStartHz)} Hz`, 4, margin.top + plotH);
  ctx.fillText(`${Math.round(fTop)} Hz`, 4, margin.top + 10);

  if (opts.title) {
    ctx.fillStyle =
      getComputedStyle(canvas).getPropertyValue("--color-text").trim() ||
      "#e8eaed";
    ctx.font = "11px system-ui, sans-serif";
    ctx.fillText(opts.title, margin.left, 12);
  }
}
