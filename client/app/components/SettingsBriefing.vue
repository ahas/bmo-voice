<template>
  <!-- The voice briefing section: the console's voice briefing player, tried and placed (the voice's
       docs/voice-briefing.md, 「The console's player」). What is changed here is kept in this browser and moves the
       player at once. -->
  <div class="flex flex-col gap-6">
    <SettingsGroup>
      <SettingsRow :label="$t('briefing.label.side')">
        <SettingsSegment v-model="side" :items="sideItems" :label="$t('briefing.label.side')" />
      </SettingsRow>

      <SettingsRow :label="$t('briefing.label.top')">
        <NumberInput v-model="top" :min="0" :step="8" :digits="4" :unit="$t('briefing.label.pixels')" :aria-label="$t('briefing.label.top')" />
      </SettingsRow>
    </SettingsGroup>

    <div class="flex flex-wrap items-center justify-end gap-2">
      <UButton
        color="neutral"
        variant="ghost"
        icon="i-material-symbols-restart-alt"
        :label="$t('briefing.action.reset_position')"
        @click="keep(BRIEFING_POSITION_DEFAULT)"
      />
      <UButton
        color="neutral"
        variant="subtle"
        :icon="trialStanding ? 'i-material-symbols-stop-rounded' : 'i-material-symbols-play-arrow-rounded'"
        :label="trialStanding ? $t('briefing.action.stop_trial') : $t('briefing.action.play_trial')"
        @click="toggleTrial"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";

import { BRIEFING_TRIAL_KEY, briefingState, playTrialBriefing } from "../domain/briefing";
import { BRIEFING_POSITION_DEFAULT, useBriefingPosition } from "../domain/briefing-position";

import type { BriefingSide } from "../domain/briefing-position";

const { t } = useI18n();
const route = useRoute();

// Where the briefing player stands, as the player itself keeps it: each change is kept at once.
const { position, keep } = useBriefingPosition();
const sideItems = computed(() => [
  { label: t("briefing.label.side_left"), value: "left" },
  { label: t("briefing.label.side_right"), value: "right" }
]);
const side = computed({
  get: () => position.value.side,
  set: (next: string) => keep({ ...position.value, side: next as BriefingSide })
});
const top = computed({
  get: () => position.value.top,
  set: (next: number | null | undefined) => keep({ ...position.value, top: Math.max(0, Math.round(next ?? 0)) })
});

// The trial voice through the player: played at a press, and stopped by the same button while it stands. Its own
// press leads back to the screen it was tried on.
const shown = briefingState();
const trialStanding = computed(() => shown.value?.key === BRIEFING_TRIAL_KEY);

function toggleTrial(): void {
  if (trialStanding.value) {
    shown.value?.stop();

    return;
  }

  playTrialBriefing(route.fullPath);
}
</script>
