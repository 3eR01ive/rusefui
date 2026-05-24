<script setup lang="ts">
import { computed } from "vue";
import { useConfig } from "../composables/useConfig";

const { snapshot } = useConfig();

const visible = computed(() => snapshot.value.loading);

const percent = computed(() =>
  Math.round((snapshot.value.progress ?? 0) * 100),
);

const progressLabel = computed(() => {
  const { bytesLoaded, bytesTotal } = snapshot.value;
  if (bytesTotal > 0) {
    const kb = (n: number) => (n / 1024).toFixed(1);
    return `${kb(bytesLoaded)} / ${kb(bytesTotal)} КБ`;
  }
  return "Чтение настроек с ECU…";
});
</script>

<template>
  <Teleport to="body">
    <div
      v-if="visible"
      class="config-load-overlay"
      role="dialog"
      aria-modal="true"
      aria-busy="true"
      aria-label="Загрузка настроек ECU"
    >
      <div class="config-load-panel">
        <p class="config-load-title">Загрузка настроек</p>
        <p class="config-load-subtitle">{{ progressLabel }}</p>
        <div
          class="config-load-track"
          role="progressbar"
          :aria-valuenow="percent"
          aria-valuemin="0"
          aria-valuemax="100"
        >
          <div class="config-load-fill" :style="{ width: `${percent}%` }" />
        </div>
        <p class="config-load-percent">{{ percent }}%</p>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.config-load-overlay {
  position: fixed;
  inset: 0;
  z-index: 10000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(58, 53, 48, 0.45);
  backdrop-filter: blur(2px);
  pointer-events: all;
  user-select: none;
}

.config-load-panel {
  min-width: min(22rem, calc(100vw - 2rem));
  padding: 1.5rem 1.75rem;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card);
  text-align: center;
}

.config-load-title {
  margin: 0 0 0.35rem;
  font-size: 1.05rem;
  font-weight: 600;
  color: var(--color-text);
}

.config-load-subtitle {
  margin: 0 0 1rem;
  font-size: 0.85rem;
  color: var(--color-text-muted);
}

.config-load-track {
  height: 0.55rem;
  border-radius: 999px;
  background: var(--color-bg-muted);
  overflow: hidden;
}

.config-load-fill {
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(
    90deg,
    var(--color-accent-muted) 0%,
    var(--color-accent) 100%
  );
  transition: width 0.15s ease-out;
}

.config-load-percent {
  margin: 0.65rem 0 0;
  font-size: 0.8rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--color-accent-hover);
}
</style>
