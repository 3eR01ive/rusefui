<script setup lang="ts">
import { registerBuiltinComponents } from "./components/register";
import AppShell from "./shell/AppShell.vue";
import AppFooter from "./shell/AppFooter.vue";

registerBuiltinComponents();
</script>

<template>
  <AppShell />
  <AppFooter />
</template>
