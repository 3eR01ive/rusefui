<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import ConnectionPanel from "../components/builtins/ConnectionPanel.vue";
import { ecuModalOpen } from "../composables/useAppFooter";
import { useEcuConnection } from "../composables/useEcuConnection";
import { useDataContext } from "../core/data-context";
import type { ComponentInstance } from "../core/types";

const dataCtx = useDataContext();
const { offlineMode, setOfflineMode } = useEcuConnection(dataCtx);

const connectionInstance: ComponentInstance = {
  id: "ecu-connection",
  type: "connection",
  bind: { source: "connection" },
  props: {},
  children: [],
};

function close() {
  ecuModalOpen.value = false;
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") close();
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <Teleport to="body">
    <div
      v-if="ecuModalOpen"
      class="modal-overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby="ecu-modal-title"
      @click.self="close"
    >
      <div class="modal-panel">
        <div class="modal-header">
          <h2 id="ecu-modal-title" class="modal-title">Подключение к ECU</h2>
          <button type="button" class="modal-close" aria-label="Закрыть" @click="close">✕</button>
        </div>

        <div class="modal-body">
          <ConnectionPanel
            :instance="connectionInstance"
            path="modal/ecu-connection"
            :props="{}"
            :binding="connectionInstance.bind"
            :meta="{ type: 'connection', label: 'Подключение', mode: 'edit', isContainer: false }"
          />

          <div class="offline-row">
            <label class="offline-toggle">
              <input
                type="checkbox"
                :checked="offlineMode"
                @change="setOfflineMode(($event.target as HTMLInputElement).checked)"
              />
              <span class="offline-label">Offline mode</span>
            </label>
            <p class="offline-hint">Не подключаться к ECU автоматически при запуске</p>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  z-index: 10000;
  background: color-mix(in srgb, var(--color-bg) 55%, transparent);
  backdrop-filter: blur(3px);
  display: flex;
  align-items: flex-end;
  justify-content: flex-start;
  padding: 0 var(--app-padding-x) calc(var(--footer-height) + 0.5rem);
  box-sizing: border-box;
}

.modal-panel {
  width: min(28rem, 100%);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card), 0 8px 32px rgba(58,53,48,0.14);
  overflow: hidden;
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.85rem 1rem 0.85rem 1.25rem;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-bg-muted);
}

.modal-title {
  margin: 0;
  font-size: 0.95rem;
  font-weight: 600;
  color: var(--color-text);
}

.modal-close {
  width: 1.75rem;
  height: 1.75rem;
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  font-size: 0.9rem;
  color: var(--color-text-muted);
  border-radius: var(--radius-sm);
  cursor: pointer;
  line-height: 1;
}

.modal-close:hover {
  background: var(--color-border);
  color: var(--color-text);
}

.modal-body {
  padding: 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}

.offline-row {
  padding-top: 1rem;
  border-top: 1px solid var(--color-border);
}

.offline-toggle {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  cursor: pointer;
  user-select: none;
  font-size: 0.88rem;
  font-weight: 500;
  color: var(--color-text);
}

.offline-toggle input {
  accent-color: var(--color-accent);
  width: 1rem;
  height: 1rem;
}

.offline-hint {
  margin: 0.3rem 0 0 1.5rem;
  font-size: 0.78rem;
  color: var(--color-text-subtle);
}
</style>
