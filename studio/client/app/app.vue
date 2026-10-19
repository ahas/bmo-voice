<template>
  <!-- The studio says everything it has to say on the one island at the top of the screen (bmo-ui's island), so
       the component library's own toaster is not stood up at all: a second road for news is a road nobody
       watches. -->
  <UApp :locale="libraryLocale" :tooltip="TOOLTIPS" :toaster="null">
    <NuxtPage />

    <MessageIsland />
  </UApp>
</template>

<script setup lang="ts">
import * as libraryLocales from "@nuxt/ui/locale";

// The component library's own words, in the language the screen speaks, English where it has none.
const { locale } = useI18n();
const libraryLocale = computed(() => (libraryLocales as Record<string, typeof libraryLocales.en>)[locale.value] ?? libraryLocales.en);

// A tooltip answers hover and the keyboard. Focus handed back by a closing sheet to the button that opened it is
// neither, and on a phone it would leave the button's name standing over the screen.
const TOOLTIPS = { ignoreNonKeyboardFocus: true };
</script>
