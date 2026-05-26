import type { DynoRunPoint } from "./drawDynoChart";

/** Одна итерация: среднее соседей; крайние точки не меняются. */
function smoothInterior(values: number[]): number[] {
  if (values.length < 3) return values;
  const out = values.slice();
  for (let i = 1; i < values.length - 1; i += 1) {
    out[i] = (values[i - 1]! + values[i]! + values[i + 1]!) / 3;
  }
  return out;
}

/**
 * Сглаживание кривой dyno для отображения.
 * @param strength 0 = без сглаживания; 1–20 = число проходов 3-точечного фильтра.
 * Первая и последняя точки всегда из исходных данных.
 */
export function smoothDynoPoints(
  points: DynoRunPoint[],
  strength: number,
): DynoRunPoint[] {
  const passes = Math.round(strength);
  if (passes <= 0 || points.length < 3) {
    return points;
  }

  let torque = points.map((p) => p.torqueNm);
  let hp = points.map((p) => p.hp);

  for (let pass = 0; pass < passes; pass += 1) {
    torque = smoothInterior(torque);
    hp = smoothInterior(hp);
  }

  return points.map((p, i) => ({
    rpm: p.rpm,
    torqueNm: torque[i]!,
    hp: hp[i]!,
  }));
}

export function clampSmoothStrength(n: number): number {
  if (!Number.isFinite(n)) return 0;
  return Math.min(20, Math.max(0, Math.round(n)));
}
