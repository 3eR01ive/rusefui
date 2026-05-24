<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import TabWorkspace from "./TabWorkspace.vue";
import ProtocolLogSheet from "./ProtocolLogSheet.vue";
import ConfigLoadOverlay from "./ConfigLoadOverlay.vue";
import { createDataContext, provideDataContext } from "../core/data-context";
import { initOutputChannels } from "../composables/useOutputChannels";
import { initConfig, useConfig } from "../composables/useConfig";
import { useProtocolLog, useProtocolLogLifecycle } from "../composables/useProtocolLog";

const dataCtx = createDataContext();
provideDataContext(dataCtx);

const appTitle = ref("rusefui");
const { togglePanel } = useProtocolLog();
const { snapshot: configSnap, burn: burnConfig } = useConfig();

const burning = ref(false);
const burnError = ref<string | null>(null);

const canBurn = computed(
  () =>
    dataCtx.connection.value.connected &&
    configSnap.value.loaded &&
    !configSnap.value.loading &&
    !burning.value,
);

useProtocolLogLifecycle();

onMounted(() => {
  void initOutputChannels();
  void initConfig();
});

async function onBurn() {
  if (!canBurn.value) return;
  burning.value = true;
  burnError.value = null;
  try {
    await burnConfig();
  } catch (e) {
    burnError.value = e instanceof Error ? e.message : String(e);
  } finally {
    burning.value = false;
  }
}
</script>

<template>
  <div class="app-shell">
    <header class="app-header">
      <div class="brand-mark" aria-hidden="true" />
      <div>
        <h1 class="app-title">{{ appTitle }}</h1>
        <span class="app-subtitle">rusEFI · декларативный UI</span>
      </div>
      <div v-if="dataCtx.connection.value.connected" class="header-actions">
        <button
          type="button"
          class="burn-btn"
          :disabled="!canBurn"
          :title="
            burnError ??
            'Записать конфигурацию во flash (команда B, как Burn в TunerStudio)'
          "
          @click="onBurn"
        >
          {{ burning ? "Burn…" : "Burn" }}
        </button>
        <button
          type="button"
          class="conn-badge"
          title="Протокол ECU — команды и ответы"
          @click="togglePanel"
        >
          ECU
        </button>
      </div>
      <p v-if="burnError" class="burn-error" role="alert">{{ burnError }}</p>
    </header>
    <TabWorkspace />
    <ProtocolLogSheet />
    <ConfigLoadOverlay />
  </div>
</template>

<style scoped>
.app-shell {
  width: 100%;
  max-width: var(--content-max);
  margin: 0;
  padding: var(--app-padding-y) var(--app-padding-x) calc(var(--app-padding-y) + 0.5rem);
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
}

.app-header {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.85rem;
  margin-bottom: 0.5rem;
  padding-bottom: 1rem;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.brand-mark {
  width: 4px;
  height: 2.5rem;
  border-radius: 2px;
  background: linear-gradient(
    180deg,
    var(--color-accent) 0%,
    var(--color-accent-muted) 100%
  );
  flex-shrink: 0;
}

.app-title {
  margin: 0;
  font-size: 1.5rem;
  font-weight: 600;
  letter-spacing: -0.02em;
  color: var(--color-text);
}

.app-subtitle {
  display: block;
  margin-top: 0.2rem;
  font-size: 0.85rem;
  color: var(--color-text-muted);
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-left: auto;
}

.burn-btn {
  padding: 0.35rem 0.75rem;
  font-size: 0.72rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-on-accent);
  background: var(--color-accent);
  border: 1px solid var(--color-accent);
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.burn-btn:hover:not(:disabled) {
  filter: brightness(1.05);
}

.burn-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.burn-error {
  flex: 1 1 100%;
  margin: 0.35rem 0 0;
  font-size: 0.82rem;
  color: var(--color-error);
}

.conn-badge {
  padding: 0.25rem 0.55rem;
  font-size: 0.72rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--color-accent-hover);
  background: var(--color-bg-accent-soft);
  border: 1px solid var(--color-success-border);
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.conn-badge:hover {
  background: #fce8d8;
}
</style>
