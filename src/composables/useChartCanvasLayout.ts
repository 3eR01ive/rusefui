import { onUnmounted, watch, type Ref } from "vue";

/** Ширина plot-area; 0 если контейнер скрыт (v-show → clientWidth 0). */
export function measureChartWidth(
  el: HTMLElement | null | undefined,
  minWidth = 1,
): number {
  if (!el) return 0;
  const w = el.clientWidth;
  if (w < 1) return 0;
  return Math.max(minWidth, Math.floor(w));
}

export interface ChartCanvasLayoutOptions {
  /** Перерисовывать только при изменении ширины (не высоты flex). */
  widthOnly?: boolean;
  debounceMs?: number;
}

/**
 * ResizeObserver + синхронный measure при появлении контейнера.
 * `onLayout` вызывать redraw (читать ширину через measureChartWidth(containerRef)).
 */
export function useChartCanvasLayout(
  containerRef: Ref<HTMLElement | null | undefined>,
  onLayout: () => void,
  options: ChartCanvasLayoutOptions = {},
): void {
  let ro: ResizeObserver | undefined;
  let lastWidth = -1;
  let debounceTimer = 0;

  function notify(): void {
    const el = containerRef.value;
    if (!el) return;
    const w = el.clientWidth;
    if (w < 1) return;
    if (options.widthOnly && lastWidth >= 1 && Math.abs(w - lastWidth) < 1) return;
    lastWidth = w;
    onLayout();
  }

  function scheduleNotify(): void {
    const ms = options.debounceMs ?? 0;
    if (ms <= 0) {
      notify();
      return;
    }
    if (debounceTimer !== 0) window.clearTimeout(debounceTimer);
    debounceTimer = window.setTimeout(() => {
      debounceTimer = 0;
      notify();
    }, ms);
  }

  function bind(): void {
    ro?.disconnect();
    ro = undefined;
    lastWidth = -1;
    if (debounceTimer !== 0) {
      window.clearTimeout(debounceTimer);
      debounceTimer = 0;
    }
    const el = containerRef.value;
    if (!el) return;

    if (el.clientWidth >= 1) notify();

    if (typeof ResizeObserver === "undefined") return;
    ro = new ResizeObserver(() => scheduleNotify());
    ro.observe(el);
  }

  watch(containerRef, bind, { flush: "post" });
  onUnmounted(() => {
    ro?.disconnect();
    if (debounceTimer !== 0) window.clearTimeout(debounceTimer);
  });
}
