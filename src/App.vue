<script setup lang="ts">
import { registerBuiltinComponents } from "./components/register";
import AppShell from "./shell/AppShell.vue";

registerBuiltinComponents();
</script>

<template>
  <AppShell />
</template>
