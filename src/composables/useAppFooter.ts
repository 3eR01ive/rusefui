import {
  computed,
  onUnmounted,
  shallowRef,
  watch,
  type MaybeRefOrGetter,
  toValue,
} from "vue";

export type LedState = "connected" | "scanning" | "error" | "off";

const ledState = shallowRef<LedState>("off");
const ledLabel = shallowRef<string>("");
export const ecuModalOpen = shallowRef(false);
export const footerToggleProtocol = shallowRef<(() => void) | null>(null);

export function setFooterLed(state: LedState, label = ""): void {
  ledState.value = state;
  ledLabel.value = label;
}

export interface FooterSlot {
  text: string;
  error?: boolean;
  warn?: boolean;
  priority: number;
}

const slots = shallowRef(new Map<string, FooterSlot>());

export function setFooterStatus(
  id: string,
  text: string | null | undefined,
  opts?: { error?: boolean; warn?: boolean; priority?: number },
): void {
  const next = new Map(slots.value);
  if (!text) {
    next.delete(id);
  } else {
    next.set(id, {
      text,
      error: opts?.error,
      warn: opts?.warn,
      priority: opts?.priority ?? 0,
    });
  }
  slots.value = next;
}

export function useAppFooter() {
  const segments = computed(() =>
    [...slots.value.entries()]
      .sort((a, b) => b[1].priority - a[1].priority)
      .map(([id, slot]) => ({ id, ...slot })),
  );

  const line = computed(() => segments.value.map((s) => s.text).join(" · "));

  const hasError = computed(() => segments.value.some((s) => s.error));
  const hasWarn = computed(() => segments.value.some((s) => s.warn && !s.error));

  return { segments, line, hasError, hasWarn, setFooterStatus, ledState, ledLabel, ecuModalOpen, footerToggleProtocol };
}

export function useFooterSlot(
  id: string,
  source: MaybeRefOrGetter<string | null | undefined>,
  opts?: MaybeRefOrGetter<
    { error?: boolean; warn?: boolean; priority?: number } | undefined
  >,
): void {
  watch(
    () => [toValue(source), toValue(opts)] as const,
    ([text, o]) => setFooterStatus(id, text ?? null, o),
    { immediate: true },
  );
  onUnmounted(() => setFooterStatus(id, null));
}

