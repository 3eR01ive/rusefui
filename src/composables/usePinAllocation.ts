import { computed } from "vue";
import {
  describePinOption,
  pinUsageFromSnapshot,
  type PinOptionAllocation,
  type PinUsageIndex,
} from "./pinAllocation";
import { useConfig } from "./useConfig";

export function usePinAllocation() {
  const { snapshot } = useConfig();

  const usageIndex = computed((): PinUsageIndex =>
    pinUsageFromSnapshot(snapshot.value.pinUsage),
  );

  return {
    usageIndex,
    describeOption(
      pool: string | undefined | null,
      fieldName: string,
      pinValue: number,
      pinLabel: string,
    ): PinOptionAllocation {
      return describePinOption(
        usageIndex.value,
        pool,
        fieldName,
        pinValue,
        pinLabel,
      );
    },
  };
}
