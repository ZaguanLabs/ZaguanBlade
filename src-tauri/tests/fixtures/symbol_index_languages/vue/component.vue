<template>
  <section id="vue-fixture" class="vue-layout">
    <button class="saveButton">Save</button>
  </section>
</template>

<script setup lang="ts">
const label = "Save";

function saveProfile() {
  return label;
}
</script>

<style scoped>
.vue-panel {
  --vue-accent: #42b883;
}
</style>
