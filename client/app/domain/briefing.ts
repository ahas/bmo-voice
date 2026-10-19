import { useIsland } from "bmo-ui/app/domain/island";
import { playAlone } from "bmo-ui/app/domain/player";
import { TAP_FFT, tapOf } from "bmo-ui/app/domain/sound-tap";

import type { Playing } from "bmo-ui/app/domain/player";
import type { Tap } from "bmo-ui/app/domain/sound-tap";
import type { Translate } from "bmo-ui/shared/wording";
import type { Ref } from "vue";

// A voice briefing heard on the voice's listener stream and played in the console (the voice's
// docs/voice-briefing.md, 「The console's player」). It stands as a small player of its own while it plays, one tab
// of the browser plays it, and stop-listening pressed here or on any other listener stops it everywhere.

// How late a listener may hear of a briefing and still play it on its own. Past this it is news that has already
// gone by, shown at most.
export const BRIEFING_FRESH_MS = 2 * 60_000;

export interface HeardBriefing {
  chatId: number;
  messageId: number;
  // When it went out, in epoch milliseconds.
  sentAt: number;
  // Whether it plays by itself, as the owner's switch stood when it was filed. One that does not is left to the line
  // it speaks.
  autoplay: boolean;
}

// A `briefing` frame of the listener stream, as the voice files it (the voice's `docs/boundary-api.md`, 「The listener
// stream」), with the fields the player reads.
export interface BriefingFrame {
  chat: number;
  message: number;
  sent_at: number;
  autoplay: boolean;
}

// A `briefing-stopped` frame: the voice message a listener stopped.
export interface BriefingStopped {
  chatId: number;
  messageId: number;
}

export function briefingOfFrame(frame: BriefingFrame): HeardBriefing | null {
  if (!Number.isSafeInteger(frame.chat) || !Number.isSafeInteger(frame.message) || typeof frame.sent_at !== "number") {
    return null;
  }

  return { chatId: frame.chat, messageId: frame.message, sentAt: frame.sent_at, autoplay: frame.autoplay !== false };
}

// The data of one frame as the voice wrote it, or null where it does not read.
function frameData<T>(event: Event): T | null {
  try {
    return JSON.parse((event as MessageEvent<string>).data) as T;
  } catch {
    return null;
  }
}

// Whether it is still news. The stream hands back everything its buffer holds when it opens, and a
// briefing heard that late — a tab opened afterwards, a laptop waking — is left to the room screen
// rather than spoken over whatever the reader is doing now.
export function fresh(heard: HeardBriefing, now: number): boolean {
  return now - heard.sentAt <= BRIEFING_FRESH_MS;
}

// The key a briefing is known by, one per voice message, so the same briefing heard twice is one.
export function briefingKey(chatId: number, messageId: number): string {
  return `briefing:${chatId}:${messageId}`;
}

// Where a briefing's sound stands. Loading is the moment before the browser has said anything;
// blocked is a browser that refused to start a sound nobody pressed for, which waits for a press.
export type BriefingPhase = "loading" | "playing" | "paused" | "blocked";

// The briefing the player stands for while it plays. It carries no words: what it shows is the
// sound itself, and its two presses.
export interface BriefingShown {
  key: string;
  // Where pressing the visualiser goes: the voice message, on its line in its room.
  to: string;
  phase: BriefingPhase;
  // Starting or carrying on the sound, and stop-listening, which stops it here and on every other listener.
  play: () => void;
  stop: () => void;
  // The sound's spectrum this moment, written into the array; false where the browser gives none.
  levels: (into: Uint8Array<ArrayBuffer>) => boolean;
}

export const BRIEFING_STATE = "bmo-briefing";

export function briefingState(): Ref<BriefingShown | null> {
  return useState<BriefingShown | null>(BRIEFING_STATE, () => null);
}

// How many bars the visualiser stands, and which bins of the tapped spectrum each one reads: a
// voice lives in the first twenty or so, and each bar takes a band of them, wider as they climb, the
// way an ear hears pitch.
export const BRIEFING_FFT = TAP_FFT;
const BAR_BANDS: Array<[number, number]> = [
  [1, 3],
  [3, 5],
  [5, 8],
  [8, 12],
  [12, 20]
];

export const BRIEFING_BARS = BAR_BANDS.length;

// Each bar's height, from nought to one, out of the spectrum the sound gave.
export function barsOf(bins: Uint8Array): number[] {
  return BAR_BANDS.map(([from, to]) => {
    let sum = 0;

    for (let bin = from; bin < to; bin += 1) {
      sum += bins[bin] ?? 0;
    }

    return sum / (to - from) / 255;
  });
}

// The bars moving on their own, for a sound whose spectrum the browser does not give: each sways at
// a pace of its own, so the row reads as a voice rather than a wave passing along it.
export function swayingBars(seconds: number): number[] {
  return Array.from({ length: BRIEFING_BARS }, (_, bar) => {
    const pace = 5 + bar * 1.7;

    return 0.45 + 0.3 * Math.sin(seconds * pace + bar * 1.3) + 0.15 * Math.sin(seconds * pace * 2.3 + bar);
  });
}

// How much larger the whole player stands at the loudest the bars can read, and the size it stands
// at for one row of bars. It follows the square of their mean: a voice reads loud on every bar at
// once, so the mean moves little between syllables, and the square is what makes those moves show,
// while a pause still brings it back to its own size.
export const BRIEFING_SWELL = 0.1;

export function swellOf(bars: number[]): number {
  if (bars.length === 0) {
    return 1;
  }

  const loudness = bars.reduce((sum, bar) => sum + Math.min(1, Math.max(0, bar)), 0) / bars.length;

  return 1 + BRIEFING_SWELL * loudness * loudness;
}

// What a sound tells the desk as it goes.
export interface SoundHandlers {
  playing: () => void;
  paused: () => void;
  ended: () => void;
  failed: () => void;
}

// The sound a briefing plays through: an element of the page in a browser, a stand-in in a test.
export interface BriefingSound extends Playing {
  play: () => Promise<void>;
  release: () => void;
  levels: (into: Uint8Array<ArrayBuffer>) => boolean;
}

// What the tabs of one browser say to each other to agree which of them plays. Every tab hears the
// same briefing on its own stream at nearly the same moment, so each says it would play it and
// waits a moment; the one every tab ranks first plays and says it has, and the rest stand down.
export type ClaimWord = { kind: "claim"; key: string; tab: string; rank: number } | { kind: "taken"; key: string; tab: string };

export interface Claim {
  tab: string;
  // Lower plays first: the tab being looked at, then one merely shown, then one hidden.
  rank: number;
}

export interface ClaimChannel {
  post: (word: ClaimWord) => void;
  close: () => void;
}

// How long a tab waits for the others' claims before it decides.
export const CLAIM_MS = 300;

// How many briefings' early claims a tab keeps.
const EARLY_KEPT = 8;

// The breath between one briefing ending and the next one waiting behind it starting.
export const BRIEFING_PAUSE_MS = 600;

// The tab every tab ranks first, decided the same way in each of them.
export function claimWinner(claims: Claim[]): string | null {
  const [first] = [...claims].sort((one, two) => one.rank - two.rank || (one.tab < two.tab ? -1 : one.tab > two.tab ? 1 : 0));

  return first?.tab ?? null;
}

export function claimWordOf(data: unknown): ClaimWord | null {
  if (typeof data !== "object" || data === null) {
    return null;
  }

  const said = data as Record<string, unknown>;

  if (typeof said.key !== "string" || typeof said.tab !== "string") {
    return null;
  }

  if (said.kind === "taken") {
    return { kind: "taken", key: said.key, tab: said.tab };
  }

  return said.kind === "claim" && typeof said.rank === "number" ? { kind: "claim", key: said.key, tab: said.tab, rank: said.rank } : null;
}

// The briefings this browser has already played or seen stopped, which a stream opening again
// hands back and which are not played twice. It is shared by the browser's tabs.
export interface HeardLog {
  has: (key: string) => boolean;
  add: (key: string, at: number) => void;
}

// The log as it is kept, read back with what can no longer be played dropped from it.
export function keptHeard(raw: string | null, now: number): Record<string, number> {
  let read: unknown;

  try {
    read = raw === null ? {} : JSON.parse(raw);
  } catch {
    read = {};
  }

  const kept: Record<string, number> = {};

  if (typeof read === "object" && read !== null) {
    for (const [key, at] of Object.entries(read)) {
      if (typeof at === "number" && now - at <= 2 * BRIEFING_FRESH_MS) {
        kept[key] = at;
      }
    }
  }

  return kept;
}

export interface DeskDeps {
  t: Translate;
  // The player's state: what stands while a briefing plays, and nothing otherwise.
  shown: { read: () => BriefingShown | null; write: (shown: BriefingShown | null) => void };
  sound: (src: string, handlers: SoundHandlers) => BriefingSound;
  src: (chatId: number, messageId: number) => string;
  // Where a press on the player leads while a briefing plays: its voice message, on its line in its room.
  line: (chatId: number, messageId: number) => string;
  // The room screen's own players, which a briefing starting pauses.
  players: () => Iterable<Playing>;
  channel: (hear: (word: ClaimWord) => void) => ClaimChannel | null;
  log: HeardLog;
  tab: string;
  rank: () => number;
  now: () => number;
  wait: (ms: number) => Promise<void>;
  // Stop-listening said to every listener.
  stopEverywhere: (chatId: number, messageId: number) => Promise<void>;
  failed: (error: unknown, fallback: string) => void;
}

// One sound the desk plays through the player: where it comes from, where a press on the player
// leads while it plays, and what stop-listening on it tells the other listeners.
interface Playable {
  key: string;
  src: string;
  to: string;
  tell: () => Promise<void>;
}

// The trial voice the account screen plays: its key, which no briefing can have.
export const BRIEFING_TRIAL_KEY = "trial";

interface Current {
  playable: Playable;
  sound: BriefingSound;
  over: boolean;
}

interface Pending {
  claims: Claim[];
  yielded: boolean;
}

// A browser that would not start a sound nobody pressed for. Any other refusal is a failure.
function refusedUnasked(error: unknown): boolean {
  return (error as { name?: unknown } | null)?.name === "NotAllowedError";
}

// A play the page itself cut short — a pause arriving before the sound had started — which is a
// pause rather than a failure.
function cutShort(error: unknown): boolean {
  return (error as { name?: unknown } | null)?.name === "AbortError";
}

// What plays the briefings of one tab. One plays at a time: a new one waits behind the one playing
// and plays once it has ended, after a short pause (docs/voice-briefing.md, 「The console's player」).
export class BriefingDesk {
  private current: Current | null = null;
  // What this tab took while another was playing — briefings, and the trial — in the order it came.
  private waiting: Playable[] = [];
  private readonly pending = new Map<string, Pending>();
  // Claims another tab made before this one heard the briefing itself. The tabs hear it moments
  // apart, and a claim dropped for arriving first would leave this tab deciding on its own claim
  // alone while the other decided on both.
  private readonly early = new Map<string, Claim[]>();
  private readonly channel: ClaimChannel | null;

  constructor(private readonly deps: DeskDeps) {
    this.channel = deps.channel((word) => this.word(word));
  }

  // The key of the briefing this tab is playing, if any.
  playing(): string | null {
    return this.current?.playable.key ?? null;
  }

  async hear(heard: HeardBriefing): Promise<void> {
    const key = briefingKey(heard.chatId, heard.messageId);

    if (!heard.autoplay || !fresh(heard, this.deps.now()) || this.deps.log.has(key) || this.current?.playable.key === key || this.pending.has(key) || this.waiting.some((one) => one.key === key)) {
      return;
    }

    if (!(await this.claim(key)) || this.deps.log.has(key)) {
      return;
    }

    this.deps.log.add(key, this.deps.now());
    this.channel?.post({ kind: "taken", key, tab: this.deps.tab });
    this.line({
      key,
      src: this.deps.src(heard.chatId, heard.messageId),
      to: this.deps.line(heard.chatId, heard.messageId),
      tell: () => this.deps.stopEverywhere(heard.chatId, heard.messageId)
    });
  }

  // The trial voice, played through the same player at a press: no other tab claims it or hears of
  // it, no log keeps it, and its stop-listening stops it here alone. Pressed again while it stands or waits,
  // it is not played twice.
  trial(src: string, to: string): void {
    if (this.current?.playable.key === BRIEFING_TRIAL_KEY || this.waiting.some((one) => one.key === BRIEFING_TRIAL_KEY)) {
      return;
    }

    this.line({ key: BRIEFING_TRIAL_KEY, src, to, tell: async () => undefined });
  }

  // Played now, or behind the one playing and those already waiting.
  private line(playable: Playable): void {
    if (this.current !== null || this.waiting.length > 0) {
      this.waiting.push(playable);

      return;
    }

    this.start(playable);
  }

  // The keys of the briefings waiting behind the one playing, in the order they will play.
  queued(): string[] {
    return this.waiting.map((one) => one.key);
  }

  // Stop-listening pressed on some other listener, or on this one and come back on the stream.
  stoppedElsewhere(stopped: BriefingStopped): void {
    const key = briefingKey(stopped.chatId, stopped.messageId);
    const pending = this.pending.get(key);

    this.deps.log.add(key, this.deps.now());

    if (pending !== undefined) {
      pending.yielded = true;
    }

    this.waiting = this.waiting.filter((one) => one.key !== key);
    this.finish(key);
  }

  close(): void {
    this.waiting = [];

    if (this.current !== null) {
      this.finish(this.current.playable.key);
    }

    this.channel?.close();
  }

  private async claim(key: string): Promise<boolean> {
    if (this.channel === null) {
      return true;
    }

    const mine: Claim = { tab: this.deps.tab, rank: this.deps.rank() };
    const pending: Pending = { claims: [mine, ...(this.early.get(key) ?? [])], yielded: false };

    this.early.delete(key);
    this.pending.set(key, pending);
    this.channel.post({ kind: "claim", key, ...mine });
    await this.deps.wait(CLAIM_MS);
    this.pending.delete(key);

    return !pending.yielded && claimWinner(pending.claims) === this.deps.tab;
  }

  private word(word: ClaimWord): void {
    const pending = this.pending.get(word.key);

    if (word.kind === "taken") {
      if (pending !== undefined) {
        pending.yielded = true;
      }

      return;
    }

    const claim: Claim = { tab: word.tab, rank: word.rank };

    if (pending !== undefined) {
      pending.claims.push(claim);
    } else if (this.current?.playable.key === word.key) {
      // A tab that heard it after this one had already taken it.
      this.channel?.post({ kind: "taken", key: word.key, tab: this.deps.tab });
    } else {
      this.keepEarly(word.key, claim);
    }
  }

  // Kept for the few that can still be heard: a briefing this tab never hears leaves its claims
  // here, and the oldest go once there are more than a handful.
  private keepEarly(key: string, claim: Claim): void {
    this.early.set(key, [...(this.early.get(key) ?? []), claim]);

    for (const old of this.early.keys()) {
      if (this.early.size <= EARLY_KEPT) {
        break;
      }

      this.early.delete(old);
    }
  }

  private start(playable: Playable): void {
    const { key } = playable;
    const current: Current = { playable, sound: undefined as unknown as BriefingSound, over: false };

    current.sound = this.deps.sound(playable.src, {
      playing: () => {
        this.move(current, "playing");
        playAlone(current.sound, this.deps.players());
      },
      paused: () => this.move(current, "paused"),
      ended: () => this.finish(key),
      failed: () => this.fail(current, null)
    });
    this.current = current;

    this.deps.shown.write({
      key,
      to: playable.to,
      phase: "loading",
      play: () => void this.play(current),
      stop: () => void this.stop(current),
      levels: (into) => current.sound.levels(into)
    });
    void this.play(current);
  }

  private move(current: Current, phase: BriefingPhase): void {
    const shown = this.deps.shown.read();

    if (current.over || shown?.key !== current.playable.key) {
      return;
    }

    this.deps.shown.write({ ...shown, phase });
  }

  private async play(current: Current): Promise<void> {
    if (current.over) {
      return;
    }

    try {
      await current.sound.play();
    } catch (error) {
      if (current.over || cutShort(error)) {
        return;
      }

      if (refusedUnasked(error)) {
        this.move(current, "blocked");
      } else {
        this.fail(current, error);
      }
    }
  }

  private async stop(current: Current): Promise<void> {
    if (current.over) {
      return;
    }

    // Stop-listening pressed here is a stop to listening: what waits behind it goes too.
    this.waiting = [];
    this.finish(current.playable.key);

    try {
      await current.playable.tell();
    } catch (error) {
      this.deps.failed(error, this.deps.t("briefing.error.stop_failed"));
    }
  }

  private fail(current: Current, error: unknown): void {
    if (current.over) {
      return;
    }

    this.finish(current.playable.key);
    this.deps.failed(error, this.deps.t("briefing.error.play_failed"));
  }

  private finish(key: string): void {
    const current = this.current;

    if (current === null || current.playable.key !== key) {
      return;
    }

    current.over = true;
    this.current = null;
    current.sound.pause();
    current.sound.release();
    this.deps.shown.write(null);
    void this.next();
  }

  // The briefing waiting first, played after a breath, unless something else has started meanwhile.
  private async next(): Promise<void> {
    if (this.waiting.length === 0) {
      return;
    }

    await this.deps.wait(BRIEFING_PAUSE_MS);

    const upcoming = this.current === null ? this.waiting.shift() : undefined;

    if (upcoming !== undefined) {
      this.start(upcoming);
    }
  }
}

// The page's own element for a briefing's sound. It is one of the page's players, so a voice
// message started on the room screen pauses it as it pauses any other; it is marked as the
// briefing's, so the briefing starting does not pause itself. The tap is taken once it plays,
// since the copy of a sound that has not started carries nothing to read.
function elementSound(src: string, on: SoundHandlers): BriefingSound {
  const element = document.createElement("audio");
  const listening = new AbortController();
  const options = { signal: listening.signal };
  let tap: Tap | null = null;

  element.dataset.player = "";
  element.dataset.briefing = "";
  element.hidden = true;
  element.preload = "auto";
  element.src = src;
  element.addEventListener(
    "playing",
    () => {
      tap ??= tapOf(element);
      on.playing();
    },
    options
  );
  element.addEventListener("pause", () => on.paused(), options);
  element.addEventListener("ended", () => on.ended(), options);
  element.addEventListener("error", () => on.failed(), options);
  document.body.append(element);

  return {
    get paused() {
      return element.paused;
    },
    play: () => element.play(),
    pause: () => element.pause(),
    levels: (into) => tap?.levels(into) ?? false,
    release: () => {
      listening.abort();
      tap?.release();
      tap = null;
      element.removeAttribute("src");
      element.load();
      element.remove();
    }
  };
}

const CHANNEL = "bmo-briefing";
const HEARD_KEY = "bmo-briefings-heard";

function storedLog(): HeardLog {
  const read = (): Record<string, number> => {
    try {
      return keptHeard(localStorage.getItem(HEARD_KEY), Date.now());
    } catch {
      return {};
    }
  };

  return {
    has: (key) => key in read(),
    add: (key, at) => {
      try {
        localStorage.setItem(HEARD_KEY, JSON.stringify({ ...read(), [key]: at }));
      } catch {
        // A browser that keeps nothing plays a briefing at most once per tab, which is still once.
      }
    }
  };
}

function browserChannel(hear: (word: ClaimWord) => void): ClaimChannel | null {
  if (typeof BroadcastChannel === "undefined") {
    return null;
  }

  const channel = new BroadcastChannel(CHANNEL);

  channel.onmessage = (message: MessageEvent<unknown>) => {
    const word = claimWordOf(message.data);

    if (word !== null) {
      hear(word);
    }
  };

  return { post: (word) => channel.postMessage(word), close: () => channel.close() };
}

function tabRank(): number {
  if (document.visibilityState !== "visible") {
    return 2;
  }

  return document.hasFocus() ? 0 : 1;
}

// The desk of the screen hearing the briefings, for as long as it stands, so the account screen can
// play the trial voice through the same player.
let openDesk: (() => BriefingDesk) | null = null;

// The trial voice: about a minute of the agent's voice, spoken once and shipped with the bundle in its `public/`.
const TRIAL_FILE = "briefing-trial.ogg";

// An address of the app, under the base the host serves it at.
function appUrl(path: string): string {
  return `${useRuntimeConfig().app.baseURL.replace(/\/$/, "")}/${path}`;
}

export function playTrialBriefing(to: string): void {
  openDesk?.().trial(appUrl(TRIAL_FILE), to);
}

// Where a press on the player leads: the voice message on its line, by the host's room route, the app
// configuration's `briefing.line` with its `{chat}` and `{message}` marks filled.
function lineOf(chatId: number, messageId: number): string {
  const { briefing } = useAppConfig() as unknown as { briefing?: { line?: string } };
  const line = briefing?.line ?? "/rooms/{chat}?at={message}";

  return line.replaceAll("{chat}", String(chatId)).replaceAll("{message}", String(messageId));
}

// The console's briefings, heard on the voice's listener stream for as long as the screen that calls this stands.
export function useBriefings(t: Translate): void {
  if (import.meta.server) {
    return;
  }

  const state = briefingState();
  const island = useIsland(t);
  let desk: BriefingDesk | null = null;

  const made = (): BriefingDesk =>
    (desk ??= new BriefingDesk({
      t,
      shown: { read: () => state.value, write: (next) => (state.value = next) },
      sound: elementSound,
      src: (chatId, messageId) => appUrl(`api/briefing/sound?chat=${chatId}&message=${messageId}`),
      line: lineOf,
      players: () => document.querySelectorAll<HTMLMediaElement>("[data-player]:not([data-briefing])"),
      channel: browserChannel,
      log: storedLog(),
      tab: `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`,
      rank: tabRank,
      now: Date.now,
      wait: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
      stopEverywhere: async (chatId, messageId) => {
        await $fetch(appUrl("api/briefing/stop"), { method: "POST", body: { chat: chatId, message: messageId } });
      },
      failed: (error, fallback) => void island.fail(error, fallback)
    }));

  // The browser's own event source holds the stream, resuming it with the last id it read after a drop.
  const source = new EventSource(appUrl("api/briefing/events"));

  source.addEventListener("briefing", (event) => {
    const frame = frameData<BriefingFrame>(event);
    const heard = frame === null ? null : briefingOfFrame(frame);

    if (heard !== null) {
      void made().hear(heard);
    }
  });
  source.addEventListener("briefing-stopped", (event) => {
    const frame = frameData<{ chat: number; message: number }>(event);

    if (frame !== null) {
      made().stoppedElsewhere({ chatId: frame.chat, messageId: frame.message });
    }
  });
  openDesk = made;
  onScopeDispose(() => {
    if (openDesk === made) {
      openDesk = null;
    }

    source.close();
    desk?.close();
  });
}
