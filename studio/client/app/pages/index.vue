<template>
  <ScreenFrame class="bg-default text-default">
    <header class="sticky top-0 z-20 shrink-0 border-b border-default bg-default/95 backdrop-blur">
      <div class="mx-auto flex w-full items-center justify-between gap-3 px-4 py-3 lg:px-6">
        <div class="flex min-w-0 items-center gap-3"><div class="hidden size-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 sm:flex"><UIcon name="i-material-symbols-smart-toy-outline" class="size-6 text-primary" /></div><div><h1 class="whitespace-nowrap text-base font-semibold sm:text-lg">{{ $t("script.title.studio") }}</h1><p class="hidden text-sm text-muted sm:block">{{ $t("script.label.subtitle") }}</p></div></div>
        <div v-if="logged" class="flex items-center gap-2"><UBadge v-if="dirty" color="warning" variant="subtle" class="hidden sm:flex">{{ $t("script.status.unsaved") }}</UBadge><UTooltip :text="$t('script.action.export_csv')"><UButton icon="i-material-symbols-download" color="neutral" variant="ghost" :to="dirty?undefined:base+'/api/export.csv'" external :aria-label="$t('script.action.export_csv')" :disabled="dirty" /></UTooltip><UTooltip :text="$t('script.action.logout')"><UButton icon="i-material-symbols-logout" color="neutral" variant="ghost" :aria-label="$t('script.action.logout')" @click="logout"/></UTooltip><UButton v-if="selected" icon="i-material-symbols-check" :label="$t('common.action.save')" :loading="saving" :disabled="!dirty" @click="save" /></div>
      </div>
    </header>
    <ContentSkeleton v-if="loading" class="p-6" :rows="6" listClass="flex flex-col gap-1" :label="$t('script.status.loading')">
      <div class="rounded-lg border border-transparent p-3">
        <SkeletonLine width="w-3/4" />
        <SkeletonLine class="mt-1" width="w-1/2" />
      </div>
    </ContentSkeleton>
    <div v-else-if="!authResolved" class="p-6"><UAlert color="error" :description="notice || $t('script.error.load_failed')" /><UButton class="mt-3" @click="loadInitial">{{ $t("script.action.refresh") }}</UButton></div>
    <UCard v-else-if="!logged" class="mx-auto mt-16 w-[calc(100%-2rem)] max-w-sm">
      <template #header><h2 class="font-semibold">{{ $t("script.title.login") }}</h2></template>
      <form class="space-y-4" @submit.prevent="login"><UFormField :label="$t('script.label.user_id')"><UInput v-model="credentials.name" autocomplete="username" class="w-full" required /></UFormField><UFormField :label="$t('script.label.password')"><UInput v-model="credentials.password" type="password" autocomplete="current-password" class="w-full" required /></UFormField><UAlert v-if="notice" color="error" :description="notice"/><UButton type="submit" :label="$t('script.action.login')" block :loading="loginBusy"/></form>
    </UCard>
    <main v-else class="flex min-h-0 flex-1 flex-col">
      <div class="flex flex-wrap items-center justify-between gap-2 border-b border-default px-4 py-2 text-sm text-muted" :aria-label="$t('script.aria.downloads')"><span>{{ $t("script.label.downloads", { progress: progressText(completed, episodes.length) }) }}</span><span>{{ gb(received) }} / {{ gb(total) }}</span></div>
      <div class="flex gap-2 border-b border-default p-3 lg:hidden"><ChoiceSelect :model-value="selected?.id" :items="mobileItems" value-key="value" :placeholder="$t('script.placeholder.episode_pick')" :aria-label="$t('script.aria.episode_pick')" class="min-w-0 flex-1" :disabled="saving" @update:model-value="choose"/><UButton icon="i-material-symbols-right-panel-open-outline" :label="$t('script.label.lines')" :aria-label="$t('script.aria.open_lines')" :disabled="!review" @click="mobileLines=true"/></div>
      <div class="studio-workspace min-h-0 flex-1 overflow-x-auto">
        <div class="studio-grid h-full" :style="{gridTemplateColumns:`${leftWidth}px 1px minmax(620px,1fr) 1px ${rightWidth}px`}">
<aside class="studio-side studio-left p-3" :aria-label="$t('script.aria.episode_panel')"><h2 class="font-semibold">{{ $t("script.title.episodes") }} <span class="text-sm font-normal text-muted">{{ countText(shown.length) }}</span></h2><UInput v-model="search" icon="i-material-symbols-search" :placeholder="$t('script.placeholder.title_search')" :aria-label="$t('script.aria.episode_search')"/><ChoiceSelect value-key="value" v-model="group" :items="groups" :aria-label="$t('script.aria.season')"/><ChoiceSelect value-key="value" v-model="filter" :items="filters" :aria-label="$t('script.aria.check_state')"/><div class="min-h-0 space-y-1 overflow-y-auto"><button v-for="e in shown" :key="e.id" class="w-full rounded-lg border p-3 text-left transition-colors" :class="selected?.id===e.id?'border-primary/50 bg-primary/10 hover:bg-primary/15 active:bg-primary/20':'border-transparent hover:bg-elevated active:bg-accented'" :disabled="saving" @click="choose(e.id)"><span class="line-clamp-2 text-sm font-medium">{{ e.title }}</span><span class="mt-1 block text-sm text-muted">{{ e.group }} · {{ labels[e.status] }} · {{ $t("script.label.line_count", { n: countText(e.lineCount) }) }}</span></button></div></aside>
          <div class="studio-divider" role="separator" aria-orientation="vertical" :aria-label="$t('script.aria.episode_panel_width')" :aria-valuenow="leftWidth" :aria-valuemin="180" tabindex="0" @pointerdown="resizeStart('left',$event)" @pointermove="resizeMove" @pointerup="resizeEnd" @pointercancel="resizeEnd" @lostpointercapture="resizeEnd" @keydown.left.prevent="resizeBy('left',-16)" @keydown.right.prevent="resizeBy('left',16)"/>
          <section class="studio-center min-w-0" :style="{'--studio-player-height':`${playerHeight}px`}" :aria-label="$t('script.aria.video_and_edit')">
            <ContentSkeleton v-if="episodeLoading && !selected" class="p-4" :label="$t('script.status.episodes_loading')">
              <div class="flex flex-col gap-3">
                <SkeletonBar class="aspect-video w-full rounded-xl" />
                <SkeletonBar class="h-5 w-1/2" />
                <SkeletonText :lines="3" size="sm" lastWidth="w-2/5" />
              </div>
            </ContentSkeleton>
            <p v-if="episodeLoading && selected" role="status" class="px-4 text-sm text-muted">{{ $t("script.status.episodes_loading") }}</p>
            <div v-if="!selected && !episodeLoading" class="p-10 text-center text-muted">{{ $t("script.message.pick_episode") }}</div>
            <div v-else-if="review && selected" class="studio-content-grid" :style="{gridTemplateColumns:videoWidth===null?'minmax(300px,1fr) 1px minmax(300px,1fr)':`${videoWidth}px 1px minmax(300px,1fr)`,minWidth:videoWidth===null?'620px':`${videoWidth+301}px`}">
              <section ref="videoPane" class="studio-media-pane" :aria-label="$t('script.aria.video_info')">
              <div ref="playerFrame" class="studio-video sticky top-0 z-10 space-y-2 border-b border-default bg-default p-3">
              <div class="relative overflow-hidden rounded-xl border border-default bg-black"><SkeletonStatus v-if="media && !duration && !mediaError" as="div" class="absolute inset-0 flex" :label="$t('common.status.video_loading')"><SkeletonBar class="size-full rounded-none" /></SkeletonStatus><video v-if="selected.video" :key="selected.id" ref="video" :src="media" class="aspect-video max-h-[32dvh] lg:max-h-[38dvh] w-full" controls playsinline preload="metadata" tabindex="0" :aria-label="$t('script.aria.video')" @loadedmetadata="duration=video?.duration??0" @error="mediaError=true" @timeupdate="tick" @keydown.capture="arrow"><track v-for="track in tracks" :key="track.index" :src="base+'/subtitle/'+selected.id+'?track='+track.index" kind="subtitles" :srclang="track.lang" :label="track.label"></video><p v-if="mediaError" role="alert" class="p-3 text-sm text-error">{{ $t("common.error.video_failed") }}</p><p v-if="!media" class="p-6 text-center text-sm text-white/70">{{ selected.video?$t('script.message.wait_download'):$t('script.message.check_download') }}</p></div>
              <div class="flex flex-wrap items-center justify-between gap-2"><div class="flex items-center gap-1"><UButton :label="$t('script.action.step_back')" color="neutral" variant="soft" :disabled="!duration" @click="seek(position-0.1)"/><UButton :label="$t('script.action.step_forward')" color="neutral" variant="soft" :disabled="!duration" @click="seek(position+0.1)"/></div><span class="font-mono text-sm tabular-nums">{{ fmt(position) }}</span><ChoiceSelect v-model="speed" :items="['0.5','0.75','1','1.25']" :aria-label="$t('script.aria.playback_rate')" @update:model-value="video&&(video.playbackRate=Number(speed))"/></div>

              </div>
                <div class="space-y-4 p-3 lg:p-5">

          <div><p class="mb-1 text-sm text-primary">{{ selected.group }}</p><h2 class="break-words text-lg font-semibold sm:text-xl">{{ selected.title }}</h2><div class="mt-2 flex flex-wrap items-center gap-2"><UBadge color="neutral" variant="subtle">{{ labels[selected.status] }}</UBadge><UButton v-if="selected.status==='complete'" :to="base+'/media/'+selected.id" external :download="selected.title" :label="$t('script.action.download_original')" icon="i-material-symbols-download" size="xs" variant="link"/><span v-if="dirty" class="text-sm text-warning">{{ $t("script.status.dirty") }}</span></div></div>
          <div v-if="selected.researchSources?.length" class="rounded-lg border border-default p-3"><p class="mb-2 text-sm text-muted">{{ $t("script.message.source_note") }}</p><div class="flex flex-wrap gap-2"><UButton v-for="source in selected.researchSources" :key="source.url" :to="source.url" target="_blank" external icon="i-material-symbols-open-in-new" :label="source.title" size="xs" variant="link"/></div></div>
<UCard ><div class="grid grid-cols-2 gap-3"><UFormField :label="$t('script.label.agent_appears')"><ChoiceSelect value-key="value" v-model="review.appearance" :items="choices" class="w-full" @update:model-value="mark"/></UFormField><UFormField :label="$t('script.label.agent_speaks')"><ChoiceSelect value-key="value" v-model="review.speech" :items="choices" class="w-full" @update:model-value="mark"/></UFormField><UFormField :label="$t('script.label.review_note')" class="col-span-2"><UTextarea v-model="review.evidence" :rows="2" :placeholder="$t('script.placeholder.review_note')" @update:model-value="mark"/></UFormField></div></UCard>
                </div>
              </section>
              <div class="studio-divider" role="separator" aria-orientation="vertical" :aria-label="$t('script.aria.video_width')" :aria-valuenow="panelWidth('video')" :aria-valuemin="300" tabindex="0" @pointerdown="resizeStart('video',$event)" @pointermove="resizeMove" @pointerup="resizeEnd" @pointercancel="resizeEnd" @lostpointercapture="resizeEnd" @keydown.left.prevent="resizeBy('video',-16)" @keydown.right.prevent="resizeBy('video',16)"/>
              <section class="studio-editor-pane p-3" :aria-label="$t('script.aria.edit_panel')">
            <UCard class="min-w-0"><template #header><div class="flex items-center justify-between"><h3 class="font-semibold">{{ $t("script.title.edit_line") }}</h3><UTooltip v-if="active" :text="$t('script.action.delete')"><UButton icon="i-material-symbols-delete-outline" color="error" variant="ghost" :aria-label="$t('script.action.delete')" @click="remove"/></UTooltip></div></template>
              <fieldset v-if="active" :key="active.id" :disabled="saving" class="space-y-5">
                <UFormField :label="$t('script.label.review_state')"><ChoiceSelect value-key="value" :model-value="active.reviewStatus??'pending'" :items="[{label:$t('script.status.needs_review'),value:'pending'},{label:$t('script.label.reviewed'),value:'done'}]" :aria-label="$t('script.label.review_state')" class="w-full" @update:model-value="setReviewStatus"/><p class="mt-1 text-sm text-muted">{{ $t("script.message.review_hint") }}</p></UFormField><div><div class="mb-3 flex items-center justify-between"><span class="text-sm font-medium">{{ $t("script.title.range") }}</span><UButton icon="i-material-symbols-play-arrow" :label="$t('script.action.play_range')" size="xs" variant="soft" :disabled="!duration" @click="play"/></div><div class="flex items-center justify-between text-sm text-muted"><span class="font-mono">{{ fmt(sliderMin) }} – {{ fmt(sliderMax) }}</span><UButton :label="zoom?$t('script.label.whole_video'):$t('script.label.zoom_range')" size="xs" color="neutral" variant="ghost" @click="zoom=!zoom;focusRange()"/></div><USlider :model-value="range" :min="sliderMin" :max="sliderMax" :step="0.1" :min-steps-between-thumbs="1" :disabled="!duration" :tooltip="true" :aria-label="$t('script.aria.line_range')" class="my-5" @update:model-value="setRange"/><div class="grid grid-cols-2 gap-3"><UFormField :label="$t('script.label.start')"><UInput :model-value="fmt(active.start)" placeholder="00:00.000" class="w-full font-mono" @change="setTime('start',($event.target as HTMLInputElement).value)"/><UButton :label="$t('script.action.set_now')" size="xs" variant="link" :disabled="!duration" @click="now('start')"/></UFormField><UFormField :label="$t('script.label.end')"><UInput :model-value="fmt(active.end)" placeholder="00:00.000" class="w-full font-mono" @change="setTime('end',($event.target as HTMLInputElement).value)"/><UButton :label="$t('script.action.set_now')" size="xs" variant="link" :disabled="!duration" @click="now('end')"/></UFormField></div><p class="mt-1 text-sm text-muted">{{ $t("script.message.range_hint") }}</p></div>
                <p class="text-sm text-muted">{{ tracks.some(t=>t.lang==='en')?$t('script.message.subtitle_match'):$t('script.message.no_subtitles') }}</p><UFormField :label="$t('script.label.english_line')"><UTextarea v-model="active.text" :rows="3" :placeholder="$t('script.placeholder.english_line')" @update:model-value="mark"/></UFormField><UFormField :label="$t('script.label.annotated_line')"><UTextarea v-model="active.annotated" :rows="2" :placeholder="$t('script.placeholder.annotated_line')" @update:model-value="mark"/></UFormField><div class="grid grid-cols-2 gap-3"><UFormField :label="$t('script.label.emotion')"><ChoiceSelect value-key="value" :model-value="active.emotion" :items="options('emotion',active.emotion)" :aria-label="$t('script.label.emotion')" class="w-full" @update:model-value="setChoice('emotion',$event)"/></UFormField><UFormField :label="$t('script.label.delivery')"><ChoiceSelect value-key="value" :model-value="active.delivery" :items="options('delivery',active.delivery)" :aria-label="$t('script.label.delivery')" class="w-full" @update:model-value="setChoice('delivery',$event)"/></UFormField></div><div class="grid grid-cols-2 gap-3"><UFormField :label="$t('script.label.music')"><ChoiceSelect value-key="value" :model-value="active.music||'__none__'" :items="options('music',active.music)" :aria-label="$t('script.label.music')" class="w-full" @update:model-value="setChoice('music',$event)"/></UFormField><UFormField :label="$t('script.label.overlap')"><ChoiceSelect value-key="value" :model-value="active.overlap||'__none__'" :items="options('overlap',active.overlap)" :aria-label="$t('script.label.overlap')" class="w-full" @update:model-value="setChoice('overlap',$event)"/></UFormField><UFormField :label="$t('script.label.events')" class="col-span-2"><ChoiceSelect :model-value="(active.events||'').split(';').filter(Boolean)" :items="eventOptions" multiple :aria-label="$t('script.label.events')" :placeholder="$t('script.placeholder.events')" class="w-full" @update:model-value="setEvents"/></UFormField></div><UFormField :label="$t('script.label.clip_id')"><UInput :model-value="active.id" readonly class="w-full font-mono"/></UFormField><UFormField :label="$t('script.label.file_path')"><UInput v-model="active.file" placeholder="clips/line_0001.wav" class="w-full" @update:model-value="mark"/><p class="mt-1 text-sm text-muted">{{ $t("script.message.file_path_hint") }}</p></UFormField><UFormField :label="$t('script.label.source')"><UInput v-model="active.source" :placeholder="selected?.path" class="w-full" @update:model-value="mark"/></UFormField><UFormField :label="$t('script.label.notes')"><UTextarea v-model="active.notes" :rows="2" :placeholder="$t('script.placeholder.notes')" @update:model-value="mark"/></UFormField><UButton :label="$t('common.action.save')" icon="i-material-symbols-check" block :loading="saving" :disabled="!dirty" @click="save"/>
              </fieldset><p v-else class="py-8 text-center text-sm text-muted">{{ $t("script.message.pick_line") }}</p>
            </UCard>
              </section>
            </div>
          </section>
          <div class="studio-divider" role="separator" aria-orientation="vertical" :aria-label="$t('script.aria.lines_panel_width')" :aria-valuenow="rightWidth" :aria-valuemin="180" tabindex="0" @pointerdown="resizeStart('right',$event)" @pointermove="resizeMove" @pointerup="resizeEnd" @pointercancel="resizeEnd" @lostpointercapture="resizeEnd" @keydown.left.prevent="resizeBy('right',16)" @keydown.right.prevent="resizeBy('right',-16)"/>
          <aside class="studio-side studio-right" :aria-label="$t('script.aria.lines_panel')"><ScriptDialogueList v-if="review" :lines="review.lines" :active-id="activeId" :duration="duration" @select="selectLine" @play="playId" @add="add"/><p v-else class="p-4 text-sm text-muted">{{ $t("script.message.lines_after_pick") }}</p></aside>
        </div>
      </div>
      <USlideover v-model:open="mobileLines" :title="$t('script.title.lines')" side="right"><template #close="{ ui }"><ShellClose :class="ui.close()" @close="mobileLines = false" /></template><template #body><ScriptDialogueList v-if="review" :lines="review.lines" :active-id="activeId" :duration="duration" @select="selectLine" @play="playId" @add="add();mobileLines=false"/></template></USlideover>
    </main>
  </ScreenFrame>
</template>

<script setup lang="ts">
import { useIsland } from "bmo-ui/app/domain/island";
import { askQuestion } from "bmo-ui/app/domain/question";
import { countText, progressText } from "bmo-ui/app/domain/numbers";
// The tags a line is stored and exported with, and a new line's: data, read as the language material it is.
import tags from "../../../../data/ko/studio-tags.json";
definePageMeta({ layout: false });
const { t, te } = useI18n();
useHead({ title: t("script.title.studio") });
// Everything this screen says while it is being worked in goes to the studio's island; the
// two alerts it keeps are the page that would not load and the login that was refused, which are
// states of the screen rather than news.
const island=useIsland(t);
// A refusal of the studio's server: its code, and its English sentence for a pack that has no words for the code.
class Refusal extends Error { constructor(readonly code:string,said:string){super(said);} }
// What a failure says: the pack's words for a refusal's code where it has them, the server's sentence otherwise.
function said(e:unknown,fallback:string){if(e instanceof Refusal&&te(`script.error.${e.code}`))return t(`script.error.${e.code}`);return e instanceof Error&&e.message!==''?e.message:fallback;}
function failed(e:unknown,fallback:string){island.say({title:said(e,fallback),tone:'error'});}
interface Line { id:string; text:string; annotated:string; start:number|null; end:number|null; emotion:string; delivery:string; notes:string; file?:string; source?:string; events?:string; music?:string; overlap?:string; reviewStatus?:'pending'|'done' }
interface Review { version:number; appearance:string; speech:string; evidence:string; lines:Line[] }
interface Episode { id:string; title:string; group:string; path:string; status:string; size:number; received:number; lineCount:number; speech:string; video:boolean; tracks:number; error?:string; researchSources?:{title:string;url:string}[] }
const playerFrame=ref<HTMLElement|null>(null),playerHeight=ref(0);
watch(playerFrame,(element,_previous,onCleanup)=>{if(!element){playerHeight.value=0;return;}const observer=new ResizeObserver(()=>playerHeight.value=element.getBoundingClientRect().height);observer.observe(element);playerHeight.value=element.getBoundingClientRect().height;onCleanup(()=>observer.disconnect());},{flush:'post'});
const mobileLines=ref(false),leftWidth=ref(240),rightWidth=ref(340),videoWidth=ref<number|null>(null),videoPane=ref<HTMLElement|null>(null);
type PanelSide='left'|'right'|'video';
let resizeHeld:{side:PanelSide;x:number;width:number;pointer:number;handle:HTMLElement}|null=null;
function panelWidth(side:PanelSide){return side==='left'?leftWidth.value:side==='right'?rightWidth.value:videoWidth.value??videoPane.value?.getBoundingClientRect().width??320;}
function persistPanels(){try{localStorage.setItem('bmo-script-panels',JSON.stringify([leftWidth.value,rightWidth.value,videoWidth.value]));}catch{}}
function setPanelWidth(side:PanelSide,value:number){const width=Math.max(side==='video'?300:180,value);if(side==='left')leftWidth.value=width;else if(side==='right')rightWidth.value=width;else videoWidth.value=width;}
function resizeBy(side:PanelSide,delta:number){setPanelWidth(side,panelWidth(side)+delta);persistPanels();}
function resizeStart(side:PanelSide,event:PointerEvent){if(event.button!==0)return;event.preventDefault();const handle=event.currentTarget as HTMLElement;resizeHeld={side,x:event.clientX,width:panelWidth(side),pointer:event.pointerId,handle};handle.setPointerCapture(event.pointerId);}
function resizeMove(event:PointerEvent){if(!resizeHeld||event.pointerId!==resizeHeld.pointer)return;const h=resizeHeld;setPanelWidth(h.side,h.width+(event.clientX-h.x)*(h.side==='right'?-1:1));}
function resizeEnd(){const h=resizeHeld;resizeHeld=null;if(h?.handle.hasPointerCapture(h.pointer))h.handle.releasePointerCapture(h.pointer);persistPanels();}
function selectLine(id:string){activeId.value=id;mobileLines.value=false;}
async function playId(id:string){const line=review.value?.lines.find(l=>l.id===id);if(line){mobileLines.value=false;await nextTick();await playLine(line);}}
// The studio's own address, with no trailing slash: the API, media and subtitles stand under it.
const base=useRuntimeConfig().app.baseURL.replace(/\/$/,'');
const tracks=ref<{index:number;label:string;lang:string}[]>([]);
const logged=ref(false),loading=ref(true),saving=ref(false),dirty=ref(false),notice=ref(''),loginBusy=ref(false);
const episodeLoading=ref(false),authResolved=ref(false);
const mediaError=ref(false);
const credentials=reactive({name:'',password:''});
const episodes=ref<Episode[]>([]),selected=ref<Episode|null>(null),review=ref<Review|null>(null),activeId=ref('');
const search=ref(''),group=ref('__all__'),filter=ref('all'),video=ref<HTMLVideoElement|null>(null),duration=ref(0),position=ref(0),speed=ref('1');
let disposed=false;
let interval:ReturnType<typeof setInterval>|undefined,playEnd:number|null=null,selection=0;
const labels:Record<string,string>={complete:t("script.status.download_complete"),downloading:t("script.status.downloading"),queued:t("script.status.download_queued"),failed:t("script.status.download_failed")};
const choices=[{label:t("script.label.unknown"),value:'unknown'},{label:t("script.label.yes"),value:'yes'},{label:t("script.label.none"),value:'no'}];
const filters=[{label:t("script.label.all_files"),value:'all'},{label:t("script.label.playable"),value:'complete'},{label:t("script.label.has_lines"),value:'lines'},{label:t("script.label.speaks"),value:'yes'},{label:t("script.label.speaks_unknown"),value:'unknown'},{label:t("script.status.download_failed"),value:'failed'}];
const groups=computed(()=>[{label:t("script.label.all_seasons"),value:'__all__'},...[...new Set(episodes.value.map(e=>e.group))].map(g=>({label:g,value:g}))]);
const shown=computed(()=>episodes.value.filter(e=>(group.value==='__all__'||e.group===group.value)&&e.path.toLowerCase().includes(search.value.toLowerCase())&&(filter.value==='all'||filter.value==='lines'&&e.lineCount>0||e.status===filter.value||e.speech===filter.value)));
const mobileItems=computed(()=>episodes.value.map(e=>({label:e.title,value:e.id,description:e.group})));
const active=computed(()=>review.value?.lines.find(l=>l.id===activeId.value));
const completed=computed(()=>episodes.value.filter(e=>e.status==='complete').length);
const received=computed(()=>episodes.value.reduce((n,e)=>n+e.received,0)),total=computed(()=>episodes.value.reduce((n,e)=>n+e.size,0));
const zoom=ref(true),windowRange=ref([0,20]);
const sliderMin=computed(()=>zoom.value?windowRange.value[0]??0:0),sliderMax=computed(()=>zoom.value?Math.min(duration.value||1,windowRange.value[1]??20):duration.value||1);
const range=computed(()=>[active.value?.start??sliderMin.value,active.value?.end??sliderMax.value]);
function focusRange(){const start=active.value?.start??position.value,end=active.value?.end??start+5;windowRange.value=[Math.max(0,start-5),Math.max(start+15,end+5)];}
watch([activeId,duration],focusRange);
const media=computed(()=>selected.value?.video&&selected.value.status==='complete'?`${base}/media/${selected.value.id}`:undefined);
const fmt=(n:number|null)=>{if(n===null)return '';const ms=Math.round(n*1000);return `${Math.floor(ms/60000).toString().padStart(2,'0')}:${((ms%60000)/1000).toFixed(3).padStart(6,'0')}`;};
const gb=(n:number)=>(n/1e9).toFixed(2)+' GB';
const presets:Record<string,string[]>={emotion:tags.emotion,delivery:tags.delivery,music:tags.music,overlap:tags.overlap};
function options(key:string,current:string|undefined){return [...(key==='emotion'||key==='delivery'?[]:[{label:t("script.label.unclassified"),value:'__none__'}]),...[...new Set([...(presets[key]??[]),...(current?[current]:[])])].map(value=>({label:value,value}))];}
const eventOptions=computed(()=>[...new Set([...tags.events,...(active.value?.events||'').split(';').filter(Boolean)])]);
function setChoice(key:'emotion'|'delivery'|'music'|'overlap',value:unknown){if(active.value&&typeof value==='string'){active.value[key]=value==='__none__'?'':value;mark();}}
function setEvents(values:string[]){if(active.value){active.value.events=values.join(';');mark();}}
function setReviewStatus(value:unknown){if(active.value&&(value==='pending'||value==='done')){active.value.reviewStatus=value;mark();}}
function mark(){dirty.value=true;}
async function api<T>(path:string,init:RequestInit={}):Promise<T>{const r=await fetch(base+path,{...init,headers:{'Content-Type':'application/json',...init.headers}});if(r.status===401){logged.value=false;authResolved.value=true;}const b=r.status===204?null:await r.json();if(!r.ok)throw new Refusal(typeof b?.code==='string'?b.code:'',b?.error||t("script.error.request_failed"));return b as T;}
async function refresh(){const b=await api<{assets:Episode[]}>('/api/assets');episodes.value=b.assets;logged.value=true;authResolved.value=true;if(selected.value)selected.value=b.assets.find(e=>e.id===selected.value?.id)??selected.value;}
async function login(){loginBusy.value=true;try{await api('/api/login',{method:'POST',body:JSON.stringify(credentials)});credentials.password='';await refresh();notice.value='';}catch(e){notice.value=said(e,t("script.error.request_failed"));}finally{loginBusy.value=false;}}
async function choose(id:unknown){if(typeof id!=='string'||saving.value)return;if(dirty.value&&!await askQuestion({title:t("script.message.discard_confirm"),confirm:t("script.action.discard")}))return;const e=episodes.value.find(e=>e.id===id);if(!e)return;const generation=++selection;episodeLoading.value=true;try{const b=await api<{review:Review;subtitleTracks:{index:number;label:string;lang:string}[]}>('/api/episode/'+id);if(generation!==selection)return;video.value?.pause();playEnd=null;duration.value=0;mediaError.value=false;position.value=0;selected.value=e;review.value=b.review;tracks.value=b.subtitleTracks??[];activeId.value=b.review.lines[0]?.id??'';dirty.value=false;notice.value='';history.replaceState(null,'','#'+id);}catch(e){if(generation===selection)failed(e,t("script.error.episode_failed"));}finally{if(generation===selection)episodeLoading.value=false;}}
function add(){if(!review.value||saving.value)return;const l:Line={id:crypto.randomUUID(),text:'',annotated:'',start:null,end:null,emotion:tags.defaults.emotion,delivery:tags.defaults.delivery,notes:'',file:'',source:selected.value?.path??'',events:'',music:'',overlap:''};review.value.lines.push(l);activeId.value=l.id;mark();}
async function remove(){if(!active.value||!review.value)return;if(!await askQuestion({title:t("script.message.delete_confirm"),effect:t("script.message.save_to_apply"),confirm:t("script.action.delete")}))return;review.value.lines=review.value.lines.filter(l=>l.id!==activeId.value);activeId.value=review.value.lines[0]?.id??'';mark();}
function seek(n:number){if(!video.value||!duration.value)return;video.value.pause();playEnd=null;video.value.currentTime=Math.max(0,Math.min(duration.value,Math.round(n*1000)/1000));}
function arrow(e:KeyboardEvent){if(e.key==='ArrowLeft'||e.key==='ArrowRight'){e.preventDefault();e.stopPropagation();seek((video.value?.currentTime??0)+(e.key==='ArrowRight'?0.1:-0.1));}}
function setRange(value:number[]|undefined){if(!active.value||!value||value.length!==2)return;const [a,b]=value;if(a===undefined||b===undefined||b<=a)return;const previous=active.value.start;active.value.start=a;active.value.end=b;mark();seek(previous===a?b:a);}
function setTime(field:'start'|'end',value:string|number){if(!active.value)return;const s=String(value).trim();if(!s){active.value[field]=null;mark();return;}const p=s.split(':');if(p.length>3||p.some(x=>!/^\d+(\.\d{1,3})?$/.test(x))||p.slice(1).some(x=>Number(x)>=60)){island.say({title:t("script.message.time_format"),tone:'warning'});return;}active.value[field]=p.reduce((n,x)=>n*60+Number(x),0);focusRange();mark();}
function now(field:'start'|'end'){if(active.value&&duration.value){active.value[field]=Math.round((video.value?.currentTime??0)*1000)/1000;focusRange();mark();}}
async function play(){if(!active.value||!video.value)return;const {start,end}=active.value;if(start===null||end===null||end<=start||end>duration.value){island.say({title:t("script.message.range_in_video"),tone:'warning'});return;}video.value.currentTime=start;playEnd=end;try{await video.value.play();}catch{island.say({title:t("script.error.video_unplayable"),tone:'error'});}}
async function playLine(line:Line){activeId.value=line.id;await nextTick();await play();video.value?.scrollIntoView({behavior:'smooth',block:'center'});}
function tick(){if(!video.value)return;position.value=video.value.currentTime;if(playEnd!==null&&position.value>=playEnd){video.value.pause();playEnd=null;}}
async function save(){if(!selected.value||!review.value||saving.value)return;saving.value=true;try{for(const l of review.value.lines){if((l.start===null)!==(l.end===null)||l.start!==null&&l.end!==null&&(l.end<=l.start||duration.value>0&&l.end>duration.value))throw Error(t("script.message.check_ranges"));}review.value=await api<Review>('/api/episode/'+selected.value.id,{method:'PUT',body:JSON.stringify(review.value)});dirty.value=false;focusRange();island.say({title:t("script.message.saved"),tone:'success'});await refresh();}catch(e){failed(e,t("script.error.save_failed"));}finally{saving.value=false;}}
async function logout(){if(dirty.value&&!await askQuestion({title:t("script.message.logout_confirm"),confirm:t("script.action.logout")}))return;await api('/api/logout',{method:'POST'});video.value?.pause();logged.value=false;review.value=null;selected.value=null;dirty.value=false;}
function beforeUnload(e:BeforeUnloadEvent){if(dirty.value){e.preventDefault();e.returnValue='';}}
async function loadInitial(){loading.value=true;try{await refresh();if(location.hash)await choose(location.hash.slice(1));}catch(e){notice.value=said(e,t("script.error.load_failed"));}finally{loading.value=false;}}
onMounted(async()=>{try{const sizes=JSON.parse(localStorage.getItem('bmo-script-panels')??'null');if(Array.isArray(sizes)&&sizes.length>=2&&sizes.slice(0,2).every(n=>typeof n==='number'&&Number.isFinite(n)&&n>=180)){leftWidth.value=sizes[0];rightWidth.value=sizes[1];if(typeof sizes[2]==='number'&&Number.isFinite(sizes[2])&&sizes[2]>=300)videoWidth.value=sizes[2];}}catch{}await loadInitial();if(disposed)return;interval=setInterval(()=>{if(logged.value)refresh().catch((e:unknown)=>failed(e,t("script.error.reload_failed")));},10000);window.addEventListener('beforeunload',beforeUnload);});
onBeforeUnmount(()=>{disposed=true;selection++;resizeEnd();clearInterval(interval);window.removeEventListener('beforeunload',beforeUnload);});
// The question is holding the navigation that asked it, so it takes no history entry of its own.
onBeforeRouteLeave(async()=>!dirty.value||await askQuestion({title:t("script.message.discard_confirm"),confirm:t("script.action.discard"),leaving:true}));
</script>

<style scoped>
.studio-grid { display: grid; }
.studio-center { overflow-x:auto; overflow-y:hidden; }
.studio-content-grid { display:grid; height:100%; }
.studio-media-pane,.studio-editor-pane { min-width:0; min-height:0; overflow-y:auto; }
.studio-media-pane { scroll-padding-top:calc(var(--studio-player-height) + 12px); }
.studio-side { display:flex; flex-direction:column; min-height:0; overflow:hidden; gap:.75rem; }
.studio-divider { position:relative; z-index:15; width:9px; margin-inline-start:-4px; touch-action:none; cursor:col-resize; outline:none; }
.studio-divider::after { content:""; position:absolute; inset-block:0; inset-inline-start:4px; width:1px; background:var(--ui-border); pointer-events:none; }
.studio-divider:hover::after,.studio-divider:focus-visible::after { background:var(--ui-primary); }
@media (max-width:1023px) {
 .studio-grid { display:block; grid-template-columns:none !important; }
 .studio-side,.studio-divider { display:none; }
 .studio-center { height:100%; overflow-y:auto; scroll-padding-top:calc(var(--studio-player-height) + 12px); }
 .studio-content-grid { display:block; height:auto; min-height:100%; min-width:0 !important; grid-template-columns:none !important; }
 .studio-media-pane { display:contents; }
 .studio-editor-pane { overflow:visible; }
}
</style>
