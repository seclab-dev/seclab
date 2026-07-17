<script setup lang="ts">
import OperationLogView from '@/apps/views/OperationLogView.vue'

defineProps<{ nodeId?: string }>()
</script>

<template>
  <OperationLogView module="docker" :node-id="nodeId" embedded data-page="docker-operation-logs" />
</template>
