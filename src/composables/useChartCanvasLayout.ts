import { onUnmounted, watch, type Ref } from "vue";

/** Ширина plot-area из контейнера; без ref(640) → медленного «расползания». */
export function measureChartWidth(
  el: HTMLElement | null | undefined,
  minWidth = 280,
): number {
  if (!el) return minWidth;
  const w = el.clientWidth;
  return w > 0 ? Math.max(minWidth, Math.floor(w)) : minWidth;
}

/**
 * ResizeObserver + синхронный measure при появлении контейнера.
 * `onLayout` вызывать redraw (читать ширину через measureChartWidth(containerRef)).
 */
export function useChartCanvasLayout(
  containerRef: Ref<HTMLElement | null | undefined>,
  onLayout: () => void,
): void {
  let ro: ResizeObserver | undefined;

  function bind(): void {
    ro?.disconnect();
    ro = undefined;
    const el = containerRef.value;
    if (!el) return;

    onLayout();

    if (typeof ResizeObserver === "undefined") return;
    ro = new ResizeObserver(() => onLayout());
    ro.observe(el);
  }

  watch(containerRef, bind, { flush: "post" });
  onUnmounted(() => ro?.disconnect());
}
