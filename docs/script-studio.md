# The script studio

A studio for checking the provided source material and recording the line spans a voice is trained on.

## Access and service

- **Address:** the path `/bmo-script/` of the host the proxy locations in `deploy/` stand in. The path goes to the
  service, which serves both the screen and its API; the path without the slash is redirected to it.
- **Service:** `bmo-script.service` runs the `script-studio` binary on loopback, at the port `BMO_SCRIPT_PORT`
  names, 3214 where it names none. The data root comes from `BMO_SCRIPT_DATA`, by default `voice/studio/` under the
  data root, and the generated screen from `BMO_SCRIPT_STATIC`, by default `studio/client/.output/public` in the
  release. A port of 0 is one the system chooses, named in the line the service prints as it starts listening.
- **What is the deployment's own** stands in the settings' `[studio]` section (`docs/boundary-api.md`, 「Settings」),
  each part optional: `source_root`, the folder the source manifest's paths and the download records' keys stand
  under, which an asset's path is read without; `source_url`, the address an export names as its source, null where
  none is named; `speakers`, the spellings of the speaker label a subtitle cue may open with, which matching drops,
  matched whatever their case, the first of them the speaker an automatic line's id and notes name; `cloud`, the
  address the media tools download the material from; and `cloud_home`, the folder of the manifest's paths the
  cloud's player names its videos below.
- **The media tools under `studio/`** read the same settings, through `BMO_VOICE_SETTINGS` or the release's
  `settings.toml`, and the same `BMO_SCRIPT_DATA`, so the server, the TypeScript tools and the Python tools stand on
  one data root. The Korean they write into records stands in `data/ko/`: the tags, their defaults and the group of
  a file at the top of the material in `studio-tags.json`, and the notes and the import's evidence sentence in
  `studio-notes.json`.
- **A refusal of the API** answers `error`, an English sentence, and `code`, which the screen words for its reader:
  `signed_out`, `sign_in_throttled`, `bad_sign_in`, `wrong_sign_in`, `sign_in_unavailable`, `foreign_origin`,
  `not_found`, `no_file`, `unsupported`, `bad_track`, `no_subtitle`, `not_downloaded`, `no_local_file`, and for a
  review that does not read the review's own codes: `bad_time`, `time_form`, `time_sixty`, `time_range`,
  `text_length`, `duplicate_line`, `review_status`, `span`, `record_form` and, answered 409, `edited_elsewhere`.
  The import script's reading of a record refuses with the same codes and sentences.
- **A request body is at most 2,000,000 bytes;** a larger one is answered `413` before it is read.
- **One reading of a record, in two languages.** The server reads and writes the review records in Rust, and the
  import script with the studio's TypeScript reading; both are held to one set of stored records in
  `studio/fixtures/reviews.json`, each with what the TypeScript reading makes of it or the sentence it refuses it
  with, so neither reading drifts from the other unseen. `studio/scripts/fixtures.ts` writes it again after the
  reading changes.
- **Screen:** the Nuxt app in `studio/client/`, a host of bmo-ui at the version packed beside it, generated into
  static files. The server hands them
  out as they are and answers every other GET under the prefix, a deep link included, with the app's `index.html`.
  Paths under `api/`, `media/` and `subtitle/` are the server's own.
- **The screen's words** are keys: the studio's own under `script.*`, and every key bmo-ui reads. The repository
  carries both packs, English and Korean, `studio/client/i18n/locales/en.yaml` and
  `studio/client/i18n/locales/ko.yaml`; every key bmo-ui reads is answered in each, the keys its share-link
  dialog adds among them. The deployment generates the screen in Korean from the packed Korean file; a
  deployment that speaks another language hands the generate step its own pack, the file `BMO_SCRIPT_PACK`
  names, in the language `BMO_SCRIPT_LANGUAGE` names, answering the same keys, and English stands where none is
  named. The deployment names the zone every time the screen shows in `BMO_SCRIPT_ZONE`, `Asia/Seoul` where it
  names none. A refusal of the server is shown as the pack's `script.error.<code>` where it has one, and as the
  server's own sentence otherwise.
- **The tags a line is stored with** are data, not words: the emotion, delivery, music, overlap and non-verbal sound
  choices and a new line's defaults stand in `data/ko/studio-tags.json`, which the screen reads.
- **Data:** files and records stand in the studio's data root and are never committed.
- **Review database:** SQLite in write-ahead-log mode. An edit compares versions, so one tab cannot overwrite
  another tab's save. JSON export is offered.

## Sign-in and sessions

- **Accounts are the account service's.** The server asks the web unit's sign-in route with the name and the
  password (`docs/boundary-api.md`, 「bmo-web」); a sign-in it takes is a login, and the server lets the session it
  was given go at once. It never writes an account; an account is made or changed on the web unit's side.
- **Session:** the studio's own, kept in its review database for seven days and carried by the `bmo_script` cookie
  on `Path=/bmo-script` (HttpOnly, SameSite=Strict, Secure behind https).
- **Throttle:** twenty logins per address in a five-minute window. Every login drops the counts whose window has
  passed.
- **Origin:** any request other than GET or HEAD must carry an `Origin` whose host is the request's own host.
- **Without a session:** the API, media and subtitles answer 401; the screen loads and shows the login card.

## Layout

- **Mobile:** a searchable episode menu and a vertical editing screen; the line list opens as a closable panel.
- **Desktop, three panels:** episodes on the left, video and editing in the middle, the line list on the right. The
  splitters show as a 1px line with a wide grabbable area around it; width is changed by dragging and by the arrow
  keys and remembered by the browser. A widened panel is reached by scrolling the work area sideways. The video stays
  fixed at the top of its area.
- **The middle splits in two:** video and its information on one side, line editing on the other, with its own
  remembered splitter; each scrolls on its own. On mobile they follow one another vertically.

## Media and downloads

- **A video plays only after a finished download,** its size and SHA-256 recorded. An unfinished file does not play.
- **Full collection:** every file under the source tree is collected recursively, with the subtitles its player
  points at.
- **HTML sizes:** a remote server may inject code into HTML, so list size and response size are told apart.
- **Resuming:** downloads resume through `.part`, and failures are not hidden. `studio/scripts/bmo-script-download.py`
  resumes them; the screen only reads the download records.
- **Where the downloads go:** the data root `--root` names, else `BMO_SCRIPT_DATA`, else the default above.
- **Where they come from:** the settings' `cloud`: a video through the cloud's player page, named by its path below
  `cloud_home`, and its sound and subtitle addresses read off that page; any other file at its path under the
  cloud, the cloud named as the referrer of every request.
- **Expired addresses** are refreshed by opening the original player again. No host or access identifier is ever
  made up.
- **Checking:** the list snapshot is compared with the completed files.

## Recording a line

- **Appearance and speech** are each recorded as unconfirmed, yes or no. A title or a cast listing alone is never
  taken to mean there is a line. The basis and review notes are saved.
- **Where a line comes from:** the provided subtitles, or a short quotation from a source; notes tell apart whether
  the source was checked and whether the video was reviewed.
- **Fields per row:** start and end time, emotion, delivery, transcript, tagged transcript, notes.
- **Time input:** minutes, seconds and milliseconds, or plain seconds; the end must come after the start. Each line
  has a two-handle slider for its span, kept in step with the inputs; with a handle focused the arrow keys move it
  0.1 s, and with the video focused they move playback 0.1 s. The player's time can be put into start or end, and
  the span alone played on repeat.
- **Viewing a span:** a row's span button jumps to its start and stops at its end, disabled where there is no span
  or the video is not ready.

## Emotion and delivery

- **Defaults:** the data's default emotion and default delivery, for new lines, automatic imports and empty values;
  a value already entered is kept.
- **Menus:** emotion, delivery, non-verbal sounds, background music and overlap with another speaker are chosen from
  menus; free-text values already recorded stay among the choices, so saving does not lose them.

## Review state

- **Kept apart from notes:** setting a line's review state to done and saving takes the list's needs-review mark off,
  and it stays after a reload. It can be set back.
- **Old records** with no state read as needing review; source and transcript notes are kept either way.
- **In CSV** the columns stay, and the review state is exported inside the notes column.

## Automatic spans from English subtitles

- **When saving,** a line with an empty span is compared with the provided English VTT subtitles: punctuation, case,
  speaker labels, bracketed directions and contractions normalised; up to six adjacent cues joined; a long sentence
  may differ by one word; only a span that matches uniquely is filled in.
- **Left empty:** repeated utterances, mismatches, a missing English subtitle, and similar candidates in several
  places. A time the user entered is never overwritten.
- **Boundaries** default to the cue's, and the line's notes say the exact voice boundary needs review.
- **Subtitle languages:** the player can select every provided subtitle language. A short web quotation is not a
  full script.

## Automatic transcription

- **For videos with no English subtitles,** `studio/scripts/bmo-script-transcribe.py` makes an automatic English
  transcript from the original audio, on a virtual environment of its own built from
  `studio/scripts/asr-requirements.txt`, the paths handed on `--queue` as a JSON array.
- **Output** goes to `asr/` in the data root: the transcript JSON, word timings and VTT. Provided subtitles are never
  overwritten. The studio offers it apart, marked as an automatic transcript needing review; it does not vouch for
  the speaker.

## Importing speakers from web scripts

- **A web script's speakers matched against the automatic transcript** give a line whose text is the transcript's.
  Short common phrases, several candidates and low agreement are not registered.
- **A registered row carries** the source address, the matching method, and whether the transcript, the speaker and
  the voice boundary need review.
- **`studio/scripts/bmo-script-import.ts`** registers only after checking the original file's id and version, never
  overwrites a manual record, backs up the records first, and writes the result count and the unmatched items to an
  audit file. Having found a source is not having every line of that episode.

## CSV import and export

- **Column order:** the voice fine-tuning template,
  `id,file,source,start,end,text,annotated,emotion,delivery,events,music,overlap,notes`.
- **Identity:** the id identifies the record; the file path and the source can be edited.
- **Encoding:** a UTF-8 BOM and standard quoting preserve Hangul, commas and line breaks.
- **A file path written** does not mean the WAV has been extracted. Save on screen before exporting.
