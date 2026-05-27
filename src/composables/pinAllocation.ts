import type { ConfigSnapshot } from "./useConfig";

/** Значение «пин не выбран» в rusEFI INI. */
export const PIN_NONE_VALUE = 0;

export function isIniPlaceholderLabel(label: string): boolean {
  return label.trim().toUpperCase() === "INVALID";
}

/** pool → pin value → имена полей config, которые его заняли. */
export type PinUsageIndex = Map<string, Map<number, string[]>>;

/** Десериализованный `pinUsage` из Rust-снимка (ключи значений — строки в JSON). */
export function pinUsageFromSnapshot(
  pinUsage: ConfigSnapshot["pinUsage"],
): PinUsageIndex {
  const index: PinUsageIndex = new Map();
  if (!pinUsage) return index;
  for (const [pool, poolMap] of Object.entries(pinUsage)) {
    const inner = new Map<number, string[]>();
    for (const [valueKey, users] of Object.entries(poolMap)) {
      inner.set(Number(valueKey), users);
    }
    index.set(pool, inner);
  }
  return index;
}

export interface PinOptionAllocation {
  /** Можно выбрать в списке. */
  selectable: boolean;
  /** Подпись: « — fanPin, vvtPins2». */
  suffix: string;
  title: string;
  cssClass: string;
}

export function describePinOption(
  index: PinUsageIndex,
  pool: string | undefined | null,
  fieldName: string,
  pinValue: number,
  pinLabel: string,
): PinOptionAllocation {
  const empty: PinOptionAllocation = {
    selectable: true,
    suffix: "",
    title: "",
    cssClass: "",
  };
  if (!pool || pinValue === PIN_NONE_VALUE || isIniPlaceholderLabel(pinLabel)) {
    return empty;
  }

  const users = index.get(pool)?.get(pinValue) ?? [];
  const others = users.filter((f) => f !== fieldName);
  if (others.length === 0) {
    return empty;
  }

  const usedIn = others.join(", ");
  return {
    selectable: false,
    suffix: ` — занят: ${usedIn}`,
    title: `Пин уже назначен: ${usedIn}`,
    cssClass: "pin-option--conflict",
  };
}
