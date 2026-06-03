import { SPECTROGRAM_MARGINS } from "./knockSpectrogramGl";

export interface KnockCylinderMarker {
  column: number;
  cylinder: number;
  channel?: number;
}

export interface KnockMarkerOverlayItem {
  x: number;
  label: string;
  cylinder: number;
}

/** X в CSS px внутри heatmap-wrap (совпадает с plot area WebGL). */
export function knockSpectrogramMarkerX(
  column: number,
  texWidth: number,
  cssWidth: number,
): number {
  const m = SPECTROGRAM_MARGINS;
  const plotW = Math.max(1, cssWidth - m.left - m.right);
  if (texWidth < 1) {
    return m.left;
  }
  const t = Math.max(0, Math.min(column, texWidth)) / texWidth;
  return m.left + t * plotW;
}

/** Метка цилиндра для UI: ECU нумерует с 0, показываем «C1»… */
export function knockCylinderLabel(cylinder: number): string {
  return `C${cylinder + 1}`;
}

/** Все захваты: одна подпись на каждый столбец+цилиндр (без слияния оборотов). */
export function buildKnockMarkerOverlay(
  markers: readonly KnockCylinderMarker[],
  texWidth: number,
  cssWidth: number,
): KnockMarkerOverlayItem[] {
  if (texWidth < 1 || markers.length === 0 || cssWidth < 1) {
    return [];
  }
  const seen = new Set<string>();
  const out: KnockMarkerOverlayItem[] = [];
  for (const mk of markers) {
    const key = `${mk.column}:${mk.cylinder}`;
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    out.push({
      x: knockSpectrogramMarkerX(mk.column, texWidth, cssWidth),
      label: knockCylinderLabel(mk.cylinder),
      cylinder: mk.cylinder,
    });
  }
  return out.sort((a, b) => a.x - b.x);
}

/** Список для WebGL-линий — каждый захват. */
export function knockMarkersForGpu(
  markers: readonly KnockCylinderMarker[],
): KnockCylinderMarker[] {
  if (markers.length === 0) return [];
  const seen = new Set<string>();
  const out: KnockCylinderMarker[] = [];
  for (const mk of markers) {
    const key = `${mk.column}:${mk.cylinder}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ column: mk.column, cylinder: mk.cylinder, channel: mk.channel });
  }
  return out.sort((a, b) => a.column - b.column || a.cylinder - b.cylinder);
}
