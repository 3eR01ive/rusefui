import { onMounted, onUnmounted, type Ref } from "vue";

export const KEYBOARD_SINK_ATTR = "data-keyboard-sink";

/** Фокус внутри зоны, где клавиши обрабатывает сам виджет (Monaco, кастомный редактор…). */
export function isKeyboardSinkTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el?.closest) return false;
  return el.closest(`[${KEYBOARD_SINK_ATTR}]`) !== null;
}

/**
 * Помечает корень компонента: глобальная nav-навигация вкладки не перехватывает
 * клавиши при фокусе внутри (без привязки router к конкретному типу компонента).
 */
export function useKeyboardSink(root: Ref<HTMLElement | null>): void {
  onMounted(() => {
    root.value?.setAttribute(KEYBOARD_SINK_ATTR, "");
  });
  onUnmounted(() => {
    root.value?.removeAttribute(KEYBOARD_SINK_ATTR);
  });
}
