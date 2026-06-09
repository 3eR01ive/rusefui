import { onMounted, onUnmounted } from "vue";
import { activeTabId } from "./useTabState";
import {
  activePath,
  deactivateComponent,
  isFilterNavPath,
  navMode,
  selectedPath,
} from "./useWorkspaceNav";
import {
  burnCallback,
  openProjectCallback,
  redoCallback,
  saveProjectCallback,
  tabEnterHandlers,
  tabOrder,
  undoCallback,
} from "./useHotkeys";
import { isKeyboardSinkTarget } from "./useKeyboardSink";

export type KeyboardHandler = (e: KeyboardEvent) => boolean;

const componentBindings = new Map<string, KeyboardHandler>();
let tabBinding: KeyboardHandler | null = null;

function isEditableTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  return el.isContentEditable;
}

function escapeNavPath(path: string): string {
  return typeof CSS !== "undefined" && "escape" in CSS ? CSS.escape(path) : path;
}

function isEditableInNavPath(e: KeyboardEvent, path: string): boolean {
  if (!path || !isEditableTarget(e.target)) return false;
  const node = document.querySelector<HTMLElement>(
    `[data-nav-path="${escapeNavPath(path)}"]`,
  );
  return node?.contains(e.target as Node) ?? false;
}

function isEditableInActiveComponent(e: KeyboardEvent): boolean {
  return isEditableInNavPath(e, activePath.value);
}

/** Глобальные сочетания — работают всегда, поверх активного компонента. */
export function isGlobalBinding(e: KeyboardEvent): boolean {
  if (e.altKey && !e.ctrlKey && !e.metaKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
    return true;
  }
  if (!(e.ctrlKey || e.metaKey)) return false;
  if (e.code === "KeyZ" || e.code === "KeyY" || e.code === "KeyS" || e.code === "KeyO") {
    return true;
  }
  if (e.code === "Enter") return true;
  return false;
}

function handleGlobalBinding(e: KeyboardEvent): boolean {
  if (e.altKey && !e.ctrlKey && !e.metaKey) {
    return false;
  }

  if (e.ctrlKey || e.metaKey) {
    if (e.code === "KeyZ" && !e.shiftKey) {
      if (!isEditableTarget(e.target)) {
        e.preventDefault();
        void undoCallback.value?.();
        return true;
      }
      return false;
    }
    if (e.code === "KeyZ" && e.shiftKey) {
      e.preventDefault();
      void redoCallback.value?.();
      return true;
    }
    if (e.code === "KeyY") {
      e.preventDefault();
      void redoCallback.value?.();
      return true;
    }
    if (e.code === "KeyS") {
      e.preventDefault();
      saveProjectCallback.value?.();
      return true;
    }
    if (e.code === "KeyO") {
      e.preventDefault();
      openProjectCallback.value?.();
      return true;
    }
    if (e.code === "Enter") {
      e.preventDefault();
      burnCallback.value?.();
      return true;
    }
  }

  return false;
}

export function registerTabBinding(handler: KeyboardHandler): void {
  tabBinding = handler;
}

export function unregisterTabBinding(): void {
  tabBinding = null;
}

export function registerComponentBinding(path: string, handler: KeyboardHandler): void {
  componentBindings.set(path, handler);
}

export function unregisterComponentBinding(path: string): void {
  componentBindings.delete(path);
}

function hasNoModifiers(e: KeyboardEvent): boolean {
  return !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey;
}

function isArrowKey(key: string): boolean {
  return key === "ArrowUp" || key === "ArrowDown" || key === "ArrowLeft" || key === "ArrowRight";
}

/** Нативное поведение стрелок в поле ввода (кaret, select) — не глушим. */
function shouldAllowArrowDefault(e: KeyboardEvent): boolean {
  if (!isEditableTarget(e.target)) return false;
  const el = e.target as HTMLElement;
  if (el.tagName === "SELECT") return isArrowKey(e.key);
  if (el.tagName === "TEXTAREA") return isArrowKey(e.key);
  if (el.tagName === "INPUT") {
    const type = (el as HTMLInputElement).type;
    if (type === "text" || type === "search" || type === "number" || type === "password") {
      return e.key === "ArrowLeft" || e.key === "ArrowRight";
    }
  }
  return false;
}

function blockKey(e: KeyboardEvent): void {
  e.preventDefault();
  e.stopPropagation();
}

function isFilterTypingKey(e: KeyboardEvent): boolean {
  if (e.ctrlKey || e.metaKey || e.altKey) return false;
  if (e.key === "Backspace" || e.key === "Delete") return true;
  return e.key.length === 1;
}

function tryFilterTyping(e: KeyboardEvent, path: string): boolean {
  if (!isFilterNavPath(path) || !isFilterTypingKey(e)) return false;
  return componentBindings.get(path)?.(e) ?? false;
}

function filterNavNode(path: string): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[data-nav-path="${escapeNavPath(path)}"]`);
}

function isFilterInputFocused(path: string): boolean {
  if (!isFilterNavPath(path)) return false;
  const node = filterNavNode(path);
  const active = document.activeElement;
  return !!active && !!node?.contains(active) && isEditableTarget(active);
}

function onKeydownCapture(e: KeyboardEvent): void {
  if (e.defaultPrevented) return;

  if (isGlobalBinding(e) && handleGlobalBinding(e)) {
    return;
  }

  if (e.altKey && !e.ctrlKey && !e.metaKey) {
    const ids = tabOrder.value;
    if (ids.length) {
      const current = ids.indexOf(activeTabId.value);
      if (e.key === "ArrowRight" && current < ids.length - 1) {
        e.preventDefault();
        e.stopPropagation();
        activeTabId.value = ids[current + 1]!;
        return;
      }
      if (e.key === "ArrowLeft" && current > 0) {
        e.preventDefault();
        e.stopPropagation();
        activeTabId.value = ids[current - 1]!;
        return;
      }
    }
  }

  if (isKeyboardSinkTarget(e.target) && !e.ctrlKey && !e.metaKey && !e.altKey) {
    return;
  }

  if (navMode.value === "active" && activePath.value) {
    const arrow = hasNoModifiers(e) && isArrowKey(e.key);

    if (isEditableInActiveComponent(e)) {
      if (e.key === "Escape" && hasNoModifiers(e)) {
        blockKey(e);
        deactivateComponent();
        return;
      }
      const handler = componentBindings.get(activePath.value);
      if (handler?.(e)) {
        blockKey(e);
        return;
      }
      if (arrow && !shouldAllowArrowDefault(e)) {
        blockKey(e);
      }
      return;
    }

    if (e.key === "Escape" && hasNoModifiers(e)) {
      blockKey(e);
      deactivateComponent();
      return;
    }
    if (e.key === "Enter" && hasNoModifiers(e)) {
      const handler = componentBindings.get(activePath.value);
      if (handler?.(e)) {
        blockKey(e);
        return;
      }
      blockKey(e);
      deactivateComponent();
      return;
    }
    const handler = componentBindings.get(activePath.value);
    if (handler?.(e)) {
      blockKey(e);
      return;
    }
    if (arrow) {
      deactivateComponent();
      tabBinding?.(e);
      blockKey(e);
      return;
    }
    if (!isGlobalBinding(e)) {
      blockKey(e);
    }
    return;
  }

  if (navMode.value === "select" && isFilterNavPath(selectedPath.value)) {
    if (isFilterInputFocused(selectedPath.value)) {
      if (
        isFilterTypingKey(e) ||
        (hasNoModifiers(e) && (e.key === "ArrowLeft" || e.key === "ArrowRight"))
      ) {
        return;
      }
    } else if (isFilterTypingKey(e)) {
      if (tryFilterTyping(e, selectedPath.value)) {
        blockKey(e);
      }
      return;
    }
  }

  if (e.key === "Enter" && !e.altKey && !e.shiftKey && !(e.ctrlKey || e.metaKey)) {
    const handler = tabEnterHandlers.get(activeTabId.value);
    if (handler && !isEditableTarget(e.target)) {
      e.preventDefault();
      e.stopPropagation();
      handler();
      return;
    }
  }

  if (
    navMode.value === "select" &&
    selectedPath.value &&
    isEditableInNavPath(e, selectedPath.value)
  ) {
    const handler = componentBindings.get(selectedPath.value);
    if (handler?.(e)) {
      blockKey(e);
      return;
    }
    if (
      hasNoModifiers(e) &&
      (e.key === "Enter" || e.key === "ArrowUp" || e.key === "ArrowDown")
    ) {
      return;
    }
  }

  if (tabBinding?.(e)) {
    blockKey(e);
  } else if (
    hasNoModifiers(e) &&
    isArrowKey(e.key) &&
    navMode.value === "select"
  ) {
    // Режим выбора: стрелки только для nav, без прокрутки страницы.
    blockKey(e);
  }
}

function onDocumentMouseDown(e: MouseEvent): void {
  if (navMode.value !== "active" || !activePath.value) return;
  const target = e.target;
  if (!(target instanceof Node)) return;
  const node = document.querySelector<HTMLElement>(
    `[data-nav-path="${escapeNavPath(activePath.value)}"]`,
  );
  if (node?.contains(target)) return;
  deactivateComponent();
}

/** Подписка компонента на клавиатуру, пока он активен (зелёная рамка). */
export function useComponentBinding(path: string, handler: (e: KeyboardEvent) => boolean): void {
  onMounted(() => registerComponentBinding(path, handler));
  onUnmounted(() => unregisterComponentBinding(path));
}

export function useKeyboardRouter(): void {
  onMounted(() => {
    window.addEventListener("keydown", onKeydownCapture, true);
    window.addEventListener("mousedown", onDocumentMouseDown, true);
  });
  onUnmounted(() => {
    window.removeEventListener("keydown", onKeydownCapture, true);
    window.removeEventListener("mousedown", onDocumentMouseDown, true);
  });
}
