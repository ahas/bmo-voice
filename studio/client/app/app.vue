<template>
  <!-- The studio says everything it has to say on the one island at the top of the screen (bmo-ui's island), so
       the component library's own toaster is not stood up at all: a second road for news is a road nobody
       watches. -->
  <UApp :locale="libraryLocale" :tooltip="TOOLTIP_PROVIDER" :toaster="null">
    <NuxtPage />

    <MessageIsland />
  </UApp>
</template>

<script setup lang="ts">
import * as libraryLocales from "@nuxt/ui/locale";
import { TOOLTIP_PROVIDER } from "bmo-ui/app/domain/tooltip";
// The Korean names of the days and months the library writes a date with, loaded for the pack it speaks.
import "dayjs/locale/ko";

// The component library's own words, in the language the screen speaks, English where it has none.
const { locale } = useI18n();
const libraryLocale = computed(() => (libraryLocales as Record<string, typeof libraryLocales.en>)[locale.value] ?? libraryLocales.en);
</script>
