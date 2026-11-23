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
// The names of the days and months the library writes a date with, loaded for every pack the repository carries.
import "dayjs/locale/ja";
import "dayjs/locale/ko";
import "dayjs/locale/zh-cn";

// The component library's own words, in the language the screen speaks, English where it has none. The library
// names a regional language with an underscore in lower case, `zh_cn` for `zh-CN`.
const { locale } = useI18n();
const libraryLocale = computed(
  () => (libraryLocales as Record<string, typeof libraryLocales.en>)[locale.value.toLowerCase().replace("-", "_")] ?? libraryLocales.en
);
</script>
