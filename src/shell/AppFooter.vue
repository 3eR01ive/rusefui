<script setup lang="ts">
import { useAppFooter } from "../composables/useAppFooter";

const { line, hasError, hasWarn, ledState, ledLabel } = useAppFooter();
</script>

<template>
  <footer
    class="app-footer"
    :class="{ 'app-footer--error': hasError, 'app-footer--warn': hasWarn && !hasError }"
    role="status"
    aria-live="polite"
  >
    <span class="ecu-status" :class="`ecu-status--${ledState}`">
      <span class="ecu-led" aria-hidden="true" />
      <span class="ecu-label">{{ ledLabel || "\u00a0" }}</span>
    </span>
    <span class="footer-sep" aria-hidden="true" />
    <span class="app-footer-text" :title="line || undefined">{{ line || "\u00a0" }}</span>
  </footer>
</template>

<style scoped>
.app-footer {
  position: fixed;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 9000;
  height: var(--footer-height);
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0 var(--app-padding-x);
  box-sizing: border-box;
  border-top: 1px solid var(--color-border);
  background: color-mix(in srgb, var(--color-bg-elevated) 92%, transparent);
  backdrop-filter: blur(6px);
  font-size: 0.72rem;
  line-height: 1.2;
  color: var(--color-text-subtle);
}

.app-footer--warn {
  color: var(--color-accent-hover);
  border-top-color: var(--color-success-border);
}

.app-footer--error {
  color: var(--color-error);
  border-top-color: color-mix(in srgb, var(--color-error) 35%, var(--color-border));
  background: color-mix(in srgb, var(--color-error-bg) 85%, var(--color-bg-elevated));
}

/* ---- ECU status block ---- */
.ecu-status {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  flex-shrink: 0;
  white-space: nowrap;
}

.ecu-led {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
  transition: background 0.25s, box-shadow 0.25s;
}

.ecu-status--off .ecu-led {
  background: var(--color-border-strong);
  opacity: 0.45;
}
.ecu-status--off .ecu-label {
  color: var(--color-text-subtle);
  opacity: 0.55;
}

.ecu-status--connected .ecu-led {
  background: #22c55e;
  box-shadow: 0 0 4px 1px #22c55e, 0 0 10px 2px #16a34a55;
  animation: led-breathe 2.8s ease-in-out infinite;
}
.ecu-status--connected .ecu-label {
  color: #16a34a;
}

.ecu-status--scanning .ecu-led {
  background: #f59e0b;
  box-shadow: 0 0 4px 1px #f59e0b, 0 0 10px 2px #d9770655;
  animation: led-blink 0.9s ease-in-out infinite;
}
.ecu-status--scanning .ecu-label {
  color: #b45309;
}

.ecu-status--error .ecu-led {
  background: #ef4444;
  box-shadow: 0 0 4px 1px #ef4444, 0 0 10px 2px #dc262655;
}
.ecu-status--error .ecu-label {
  color: var(--color-error);
}

.ecu-label {
  font-size: 0.72rem;
  font-weight: 500;
}

@keyframes led-breathe {
  0%, 100% { opacity: 1; }
  50%       { opacity: 0.5; }
}

@keyframes led-blink {
  0%, 100% { opacity: 1; }
  50%       { opacity: 0.15; }
}

/* ---- separator ---- */
.footer-sep {
  width: 1px;
  height: 0.75rem;
  background: var(--color-border);
  flex-shrink: 0;
}

/* ---- text ---- */
.app-footer-text {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1;
  min-width: 0;
}
</style>
