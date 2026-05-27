/** Простой график сырых knock ADC (индекс сэмпла по X). */
export function drawKnockWaveform(
  canvas: HTMLCanvasElement,
  samples: number[],
  opts: {
    min?: number;
    max?: number;
    title?: string;
  } = {},
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

  const margin = { top: 16, right: 12, bottom: 28, left: 48 };
  const plotW = w - margin.left - margin.right;
  const plotH = h - margin.top - margin.bottom;

  ctx.fillStyle = getComputedStyle(canvas).getPropertyValue("--color-bg-panel").trim()
    || "#1a1d24";
  ctx.fillRect(0, 0, w, h);

  if (samples.length < 2) {
    ctx.fillStyle = getComputedStyle(canvas).getPropertyValue("--color-text-muted").trim()
      || "#889";
    ctx.font = "13px system-ui, sans-serif";
    ctx.fillText("Нет данных (ждём knockScopeReady…)", margin.left, margin.top + 24);
    return;
  }

  let yMin = opts.min ?? Math.min(...samples);
  let yMax = opts.max ?? Math.max(...samples);
  if (yMax <= yMin) {
    yMax = yMin + 1;
  }
  const pad = (yMax - yMin) * 0.08;
  yMin -= pad;
  yMax += pad;

  const toX = (i: number) =>
    margin.left + (i / (samples.length - 1)) * plotW;
  const toY = (v: number) =>
    margin.top + plotH - ((v - yMin) / (yMax - yMin)) * plotH;

  ctx.strokeStyle = getComputedStyle(canvas).getPropertyValue("--color-border").trim()
    || "#3a3f4a";
  ctx.lineWidth = 1;
  ctx.strokeRect(margin.left, margin.top, plotW, plotH);

  ctx.strokeStyle = getComputedStyle(canvas).getPropertyValue("--color-accent").trim()
    || "#5b9fd4";
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let i = 0; i < samples.length; i++) {
    const x = toX(i);
    const y = toY(samples[i]!);
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.stroke();

  ctx.fillStyle = getComputedStyle(canvas).getPropertyValue("--color-text-muted").trim()
    || "#aab";
  ctx.font = "10px ui-monospace, Menlo, monospace";
  ctx.fillText(`${yMin.toFixed(0)}`, 4, margin.top + 10);
  ctx.fillText(`${yMax.toFixed(0)}`, 4, margin.top + plotH);
  ctx.fillText("0", margin.left, h - 6);
  ctx.fillText(String(samples.length - 1), margin.left + plotW - 24, h - 6);

  if (opts.title) {
    ctx.fillStyle = getComputedStyle(canvas).getPropertyValue("--color-text").trim()
      || "#e8eaed";
    ctx.font = "11px system-ui, sans-serif";
    ctx.fillText(opts.title, margin.left, 12);
  }
}
