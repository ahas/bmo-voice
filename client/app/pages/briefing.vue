<template>
  <!-- The voice briefing's page in the console (`docs/layout.md`, 「The console plugin bundle」): the player's settings
       section, under the page's title. -->
  <div class="mx-auto flex w-full max-w-(--ui-container) flex-col gap-6 p-4 sm:p-6">
    <h1 class="text-lg font-semibold">{{ $t("briefing.label.title") }}</h1>

    <SettingsBriefing />
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";

const { t } = useI18n();

useHead({ title: t("briefing.label.title") });
</script>
