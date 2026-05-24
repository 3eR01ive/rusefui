<script setup lang="ts">
import { onMounted, ref } from "vue";
import TabWorkspace from "./TabWorkspace.vue";
import ProtocolLogSheet from "./ProtocolLogSheet.vue";
import ConfigLoadOverlay from "./ConfigLoadOverlay.vue";
import { createDataContext, provideDataContext } from "../core/data-context";
import { initOutputChannels } from "../composables/useOutputChannels";
import { initConfig } from "../composables/useConfig";
import { useProtocolLog, useProtocolLogLifecycle } from "../composables/useProtocolLog";

const dataCtx = createDataContext();
provideDataContext(dataCtx);

const appTitle = ref("rusefui");
const { togglePanel } = useProtocolLog();

useProtocolLogLifecycle();

onMounted(() => {
  void initOutputChannels();
  void initConfig();
});
</script>

<template>
  <div class="app-shell">
    <header class="app-header">
      <div class="brand-mark" aria-hidden="true" />
      <div>
        <h1 class="app-title">{{ appTitle }}</h1>
        <span class="app-subtitle">rusEFI · декларативный UI</span>
      </div>
      <button
        v-if="dataCtx.connection.value.connected"
        type="button"
        class="conn-badge"
        title="Протокол ECU — команды и ответы"
        @click="togglePanel"
      >
        ECU
      </button>
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

.conn-badge {
  margin-left: auto;
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
