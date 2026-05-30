<script setup lang="ts">
import { provideTabActivity } from "../composables/useTabActivity";

const props = defineProps<{
  tabId: string;
}>();

provideTabActivity(props.tabId);
</script>

<template>
  <slot />
</template>
