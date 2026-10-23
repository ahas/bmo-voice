# The voice

How a text becomes sound: the synthesiser, the road it takes to the voice nodes, how a written line is made a
script for speaking, the trained voices and their table, where the voices are trained, and the voice servers on
the voice nodes. What a caller does with the sound is the caller's.

## The synthesiser

- **VoxCPM2 is the one synthesiser.** Every line is spoken in a trained voice, a LoRA fine-tune of VoxCPM2 on an
  agent's own recorded lines (「The voices」), at 48 kHz, with its delivery instructed in bracketed natural language.
- **Where no trained voice can speak a line, nothing is said.** There is no other synthesiser to fall back on.
- **What it costs:** the model needs about 7 GiB of a card, so it speaks on a voice node's card where the voice
  node list holds any (「The voice nodes」), and one line at a time on the operations machine's card where it holds
  none.

## One voice environment

- **One interpreter, outside the repository:** the virtual environment `bmo-voice/.venv` under the account's home,
  on the operations machine and on each voice node. Everything the voice runs runs on it: VoxCPM2, for the voice
  server, for a line spoken on the operations machine's card and for training; the OGG/Opus encoder every road
  ends in; and Whisper, for measuring a voice and checking a prompt before it is installed.
- **The package list** is `synth/voice-node-requirements.txt`, a full freeze of that environment and the one file
  read when rebuilding it. It is installed as it stands, without resolving dependencies: a freeze names every
  package, and the CUDA build of torch comes from its own index.
- **`BMO_VOICE_PYTHON`, `BMO_VOX_PYTHON` and `BMO_ASR_PYTHON`** name the encoder's, the mouth's and the
  recogniser's interpreter for a process that runs the tool entry itself, a run by hand or a test; a call through
  the tool host carries none of its caller's variables, and each runs on the voice environment under the call's
  home.
- **No interpreter at all** is a failure.

## Which road speaks a line

- **Every line is spoken in a trained voice,** one row of the voices table, named by the synthesis itself. Where
  it is spoken is settled by the voice node list:
  - a voice node listed: spoken on the voice server of one free voice node, and encoded on the operations machine
    by the `voice/encoder` entry;
  - no voice node listed: the synthesis fails. The engine keeps no fallback onto the operations machine's card.
- **A voice the table does not hold, a node out of reach and a voice server that does not answer** leave the
  synthesis `failed`; nothing is spoken and the failure is the member's state.
- **`synth/voice-vox.py`** is the trained voice's mouth, both on the operations machine's card and as the voice
  server.
- **`synth/voice-encode.py`** is the encoder: it loads no model, takes a wav a mouth spoke and writes it as
  OGG/Opus, with nothing laid over the voice.
- **No formatting gate:** sound has no markup. The line is made a script for speaking instead (「A line is spoken
  as a script」).

## One at a time on the operations machine's card

- **A line spoken there loads a model per call:** no daemon holds one, so each line pays the load.
- **Callers queue** in front of one lock file, `voice/locks/voice-synth.lock`, so only one goes onto the card at a
  time. The lock stands in the `voice/mouth` entry, in front of the synthesiser and nowhere else, so no caller
  reaches the card without it.
- **A caller's cap and stop reach the synthesiser:** a caller that gives up a call closes it, and the tool host ends
  the call's process group, the synthesiser and its hold on the lock with it.
- **A voice server queues for itself:** a line sent to it takes no lock here, and the server speaks one line at a
  time behind a lock of its own.

## The voice as a tool

- **`synth/tool.toml` declares the voice's entries,** four of the `voice` namespace, each for the system alone:
  never shown to a session, called through the tool host's control socket.
  - **`voice/mouth`** speaks one line on this machine's card: the words or a file holding them, where the sound
    goes and where its wav is kept where asked, and the voice row's checkpoint, reference, guidance, steps, seed,
    delivery, sentence switch and continuation prompts. It stands in the card's line itself, waiting five minutes
    for the lock; a wait that runs out is answered as a turn not had, apart from a synthesiser that failed.
  - **`voice/encoder`** writes a wav as OGG/Opus, in no line, since the card is not touched.
  - **`voice/transcriber`** takes spoken files down in one language, each answered with what was heard, in the
    order given.
- **Its executable is `synth/voice-tool.py`,** on the system's `python3` and the standard library alone, placed
  with the mouth, the encoder and the transcriber it runs and the tool host's Python helper; each file it runs runs
  on the voice environment's interpreter.
- **Timeouts:** a minute for a call, ten for the mouth and the transcriber, and the run cap for one synthesis.

## Encoding

- **A voice message is OGG/Opus.** libsndfile writes it directly, so the mouth writes that format with
  `soundfile`, and neither it nor a voice node needs ffmpeg or sox.

## A line is spoken as a script

- **A line written for a room is markdown, and a voice reads its marks.** The engine makes the line a script for
  speaking first, mechanically, before the names and the figures; no model rewrites its words here.
  - A list item whose head is a bold title, or a title closed by a colon, is joined to what follows it as one
    sentence, the title taking its topic particle by its last syllable.
  - An item's other lines are joined to it with a space, so one item is one sentence.
  - A numbered item opens with its ordinal said as a word, as the language's reading table gives it, and a comma.
  - A paragraph opening with a line wholly in bold is joined to the lines under it the same way.
  - A table row is its cells joined by commas, its separator row dropped.
  - Bullets, heading marks, quote marks, rules, bold, italics, strike-through and link targets are dropped and their
    words kept; a picture is its description.
  - A fenced code block is not spoken at all; inline code is kept as code.
- **A web address is said to be attached,** never read out:
  - an address is found by the web address rule of bmo-text: it runs to the next space or Korean letter, and the
    punctuation after it and a closing bracket it did not open are the sentence's;
  - a bare address, one in angle brackets and inline code holding only an address are taken out; a link's words
    are said and its address is not; an address in brackets goes with its brackets;
  - an address a Korean particle or word is written onto is said as the reading table's word for an address, the
    particle fitted to it;
  - a colon left at the end of a line by the address after it is dropped, and a line left with no words is not
    said;
  - one sentence closes the script, however many addresses were taken out, in the line's language and politeness,
    as the reading table words it.
- **The particle after a title** is settled last, once the title's reading stands.
- **The script is what the synthesiser is handed and what `POST /script-readings` answers.**

### Which sentences are English

- **One rule, read by the mouth:** it decides by it which prompt and which language a piece is spoken in.
- **A sentence** ends at a full stop, a question or exclamation mark or an ellipsis before a space, and at every
  line break.
- **Its language:** Korean where it holds a Hangul syllable; English where it holds none and has at least two words
  with a lower-case letter; otherwise it takes the language of the next sentence that says one, or the last before
  it. A line where no sentence says is English if it has a roman letter and Korean otherwise.
- **Neighbouring sentences of one language are one stretch.** English is spoken as English.

## How names are said

- **One table says it for both languages:** `voice_names` (`docs/store.md`), one row per spelling, each with what a
  Korean stretch and an English stretch say in its place. An empty reading leaves the spelling as written.
- **A row is read without a model,** so a name is said the same every time. A name the voice gets wrong is a row,
  not a deployment: `scripts/voice-names.ts` lists the table, `--set <spelling> <korean> <english>` writes a row,
  `--remove <spelling>` takes one off. A Korean reading carries no roman letter and an English one no Hangul.
- **A table first made is given the seed names** of `data/ko/voice-names.toml`; a table that stands is left as it is.
- **Where it applies:** to each stretch once the line is cut into its languages, before the figures.
- **How a spelling is matched:** whatever its case, and only as a word of its own: no letter or figure before it, no
  roman letter or figure after it, a Korean particle allowed. The longest spelling is tried first.

## How figures are said

- **A figure is said by rule,** never by a model, in each Korean stretch once its names are said. An English
  stretch keeps its figures.
- **What it reads:** a figure with the marks it is written with (`#`, a thousands comma, a decimal point, a colon, a
  slash, a tilde, `%`, a currency sign), not a figure in or only spaces away from a run holding a roman letter
  unless a counter follows it, and not a figure inside code.
- **How each is said** is the reading table's (`data/ko/voice-script.yaml`): whole numbers in Sino-Korean grouped by
  ten-thousands; an item number with its counter; a count before a native counter in native Korean up to 99; a
  month by its own words; a decimal as its whole part and its figures one by one, three parts or more as a version;
  a fraction; a date year first; an hour native from 1 to 12 and Sino-Korean otherwise; a clock time; a ratio; a
  range; percent, degrees, currency and a leading minus. A counter is a word of its own, and the particle after a
  figure follows its reading.

## The voices

- **A trained voice is a LoRA fine-tune of VoxCPM2** on an agent's recorded lines, spoken with a reference clip, a
  guidance and step count, a seed, a delivery instruction or none, and a continuation prompt for each language it
  speaks.
- **The voices table says which one speaks** (`docs/store.md`): one row per voice, at most one current. The current
  voice is the chat agent's where its registration names none.
- **The instruction is the row's to change:** `scripts/voice-install.ts --style <name> '<words>'`. The mouth knows
  how VoxCPM2 reads it: in brackets before the first words the model is given, which on from a continuation prompt
  are the prompt's own words, so an instruction never stands between the prompt and the piece.
- **The seed is the row's too:** `--seed <name> <n>`. A seed names one delivery of one line; the same line, seed and
  settings give back the same samples every time. It does not carry a tone from one line to the next.
- **Whether a line is spoken a sentence at a time is the row's too:** `--sentences <name> on|off`. On, the line is
  cut at its sentences, each spoken on the row's seed and prompt with 0.3 seconds of silence between two.
- **A freshly loaded model speaks one throwaway line first,** since its first line does not follow its seed.
- **How a piece ends and meets the next.** A piece is what one call to the model speaks. Nothing is trimmed off it;
  the model's stop is held for one step past its vote, and both edges are faded over 10 ms on a raised-cosine curve.
- **A continuation prompt** is a clip of the voice speaking and the words it says, per language, in the row; the clip
  stands in the voice's own directory under a name made of its content, `prompt-<digest>.wav` or
  `prompt-en-<digest>.wav`, the digest the first twelve hex digits of its SHA-256, so a node's copy can never pair
  new words with an old clip. A line is spoken as its stretches, each on from its own language's prompt where the
  voice has one; a clip without its words, words without their clip or a clip that finds no file is refused before
  any model loads.
- **Installing a prompt checks it first** (「How a prompt is checked」): `--prompt <name> <clip.wav> '<words>'` and
  `--prompt-en`, `none` in place of the clip to empty one, `--dry-run` to check and write nothing.
- **A trained checkpoint joins by name:** `--add <name> --checkpoint <dir> --reference <clip.wav>` copies its two
  weight files and the clip it is conditioned on and adds a row, the current one where the table held none and not
  current otherwise. Listed with no flag, the script shows every voice and marks the current one. **Switching is
  one command:** `--use <name>` makes that row the one current.

## Where the weights live

- **Under the data root's `voice/voices/`** (`weights/checkpoints.toml`), never in an experiment's directory, which
  another job may prune; the data root is outside every checkout and carried by the deployment's backup.
- **Only what speaking needs is copied:** `lora_config.json` and `lora_weights.safetensors` per checkpoint, the clip
  of each continuation prompt, and the reference clips under `reference/`. Optimiser state stays where it was made.
- **The base model** is the Hugging Face snapshot each checkpoint's config names, in the account's own Hugging Face
  cache on whichever machine speaks; a node whose home differs finds the same repository and revision in its own.

## Where the voice is trained

- **On the nodes an experiment's pipeline names,** once `scripts/voice-node.ts setup --node <name>` has built the
  voice environment there and fetched the base model. An experiment stands on its home, the node named with `--home`,
  under `bmo-voice-train/` in the account's home: the training code a run imports and `experiments/<run>/`.
- **The copy under the script studio's data root is the record,** `experiments/<run>/` there, and the one place an
  experiment's scripts are written; what a run executes is carried out to the node it runs on.

### Placing a step on a card

- **`scripts/voice-train.ts` runs one step of an experiment on a free card and brings its results home.** It works
  from the operations machine, where every node is reached.
  - `nodes` says, for every node the node service lists or each one `--node` names, whether it carries the voice
    environment and a card arbiter, whether its card is free, and whether the card is its own or shared with the
    voice server.
  - `run --experiment <name> --step <label> --node <name>… --home <name> [--carry <path>]… [--bring <path>]… --
    <command…>` does one step from start to end on one of the nodes it names.
- **A card is the node's card arbiter's to tell about** (bmo-arbiter's `docs/api.md`, 「The node arbiter's units」):
  a node whose arbiter does not answer, or answers for no card, takes no step.
- **A card is free** when the arbiter names no claim on it, nobody but the voice server holds its lock, nobody is in
  line for it but to rest, and no training step is running on the node; a step waits, asking once a minute, while
  every card it may use is taken. Choosing a card is done by one runner at a time, under `voice/locks/voice-train.lock`,
  so steps launched together never pick the same card.
- **The step takes its turn on the card through the arbiter's turn client** (「The turn client」), as the voice: it
  waits for the turn, holds the card lock for as long as it runs, and lets go as it ends. A voice server on the same
  card gives way to it as it gives way to any other occupant that asks.
- **A card of its own is chosen before the voice server's.** The step is told in `BMO_VOICE_CARD`: `own` or
  `shared`.
- **What the step needs is carried before it starts:** the experiment's folder but for what its runs write, and every
  path `--carry` names, wherever the node lacks a file or holds a different one; then the record's top-level files.
- **The step runs apart,** as the transient user unit `bmo-voice-train-<experiment>-<step>`, its output in
  `steps/<step>.log` and its exit in `steps/<step>.code`; what `--bring` names comes back once it ends, and every step
  leaves a line in the record's `steps.log`.
- **A run is not compiled,** and checkpointed decoder layers are left off on a card of its own: compiling cost
  quality, and checkpointing is only for fitting beside a loaded voice server.

## The voice nodes

### Where they run

- **On every node of the voice node list,** `voice_nodes` (`docs/store.md`), as the user unit `bmo-voice.service`
  with the same voices directory, so any of them speaks a line exactly as another would. Lingering is on for the
  account, so the unit starts with the machine.
- **The list is the card arbiter's to change,** through the voice's adapter: `card-master place voice --to <node>`
  and `card-master remove voice --from <node>`. A node taken off keeps its server; putting it back is all it takes to
  speak there again. An empty list is the trained voice spoken on the operations machine's card.
- **A node reserved for another service is never a voice node:** the arbiter refuses it, and named on the list all
  the same it is out of reach.
- **Port 7432 on each node's loopback, and nowhere else.** Every call reaches it through the node's own shell, `ssh`
  for an ssh node and the hub's exec and file transfer for a hub node, as the node service's target gives it.
- **What lands on each node,** under `bmo-voice/` in the account's home: the server file, the virtual environment, a copy of the voices directory, and `spoken/` (`0700`), where a call's
  request and answer stand while it is under way. The token is `.config/bmo-voice/token`, `0600`.

### Which node speaks a line

- **A synthesis is spoken on one free node,** held for the whole of its line, so syntheses set off together are
  spoken side by side on different nodes.
- **Free means both:** no other run here holds the node (a lock file of its own under `voice/locks/`, taken without
  waiting), and the node's server answers its health, is not speaking and says its card has room.
- **A warm node goes first:** a server that already holds the voice's checkpoint in host memory. Otherwise the list's
  order decides.
- **No node free, but one whose card lock other work holds:** the line is asked of that node and waits its turn in
  the node's own queue, the warm one first.
- **Every node taken or busy:** the run asks again every two seconds for five minutes; past it the synthesis has
  no turn and ends `failed`. The speaking itself is held to what is left of the synthesis's cap.
- **On the node** the server waits up to a minute for its turn, in the card arbiter's hold queue through the turn
  client; past it, or where the card has no room once the turn came, it answers 503 and the run chooses again.
- **A caller that gives up its call** ends the speaking the call had started; the synthesis ends `failed`.
- **Every node out of reach fails the run at once,** naming each node and what it met. It is not spoken on the
  operations machine's card instead: a line quietly spoken elsewhere would hide the fault.

### Building and updating them

- **Each verb of `scripts/voice-node.ts` acts on every voice node, or the one `--node` names,** side by side, each
  output line opening with the node's name.
- **`setup`** builds the virtual environment from the frozen package list and downloads the base model the
  checkpoints name; it installs uv where missing, refuses a node whose driver runs no CUDA 13, and runs apart on the
  node, its output in `bmo-voice/setup.log`.
- **`push`** copies the server file, the voices directory and the token, writes the unit, restarts
  it, waits for it to answer, and leaves a stamp of what it brought, `bmo-voice/pushed`.
- **`sync`** is push where it is due: only where a node's stamp differs from what a push would bring now. Every
  deployment runs it once the service answers (`docs/deploy.md`), so a change to the server or a newly installed
  voice reaches every node; a sync that pushes restarts that node's server.
- **`health`** asks each server what it holds; **`speak --text '<words>' --wav <path>`** speaks one line in the
  current voice or the one `--voice` names, on a free node where `--node` names none.
- **`adapter`** answers the card arbiter (`docs/boundary-api.md`, 「The node scripts and the card adapter」):
  `install` puts the node last on the list, runs `setup` and `push` and speaks there the check line of
  `data/ko/voice-node.toml`; `uninstall` takes
  it off the list and stops and disables its unit, keeping its installation; `start`, `stop` and `health` act on its
  unit and server; `placed` reads the list.
- **The unit** declares the server to the node's card arbiter as `BMO_CARD_SERVICE=voice`, and names the arbiter's
  own installation, `~/bmo-card`, on the server's import path: the server waits its turn for the card through the
  turn client the arbiter installs there, and takes the card lock as it comes on a node without it.
- **`setup` downloads the base model the current voice's `lora_config.json` names,** so a voice is installed
  (「The voices」) before the first node is set up.

### How a line crosses

- **The request goes in on the node's standard input,** never on a command line. The node writes it into `spoken/`
  and starts the asker apart, `voice-vox.py --ask`, which posts it to the server on loopback with the token and writes
  the answer's wav beside it with one line of JSON: the status, the reason, the headers.
- **The operations machine hears of that line** by waiting on the asker's lock on the node, each wait bounded inside
  what a hub node lets a command run, then fetches the wav a slice at a time, four MiB to a command, every byte
  counted back, and removes every file of the call. Each call first sweeps away files more than an hour old.
- **Why apart:** a hub node stops a command after five minutes, and a long line from a cold server can take longer.
- **How a failure is named:** unreachable (the shell, the hub or the server not there), a timeout (the cap passed),
  refused (a wrong token, a voice the node does not hold), failed (the line not spoken), a bad answer (not a wav).

### What it answers

- **Every call carries the bearer token,** `voice/voice.token` under the data root, made by the first `push` and
  copied to every node; it never goes into a database, a log, a command line or an environment variable.
- **`POST /speak`:** a line and the voice as names under the voices root, with cfg, steps, seed, `style`, `prompts`
  and `sentences`. The answer is the spoken line as a 48 kHz 16-bit wav, peak-matched, with its length, load time,
  speaking time and checkpoint in its headers. A path leaving the voices root, a voice the node does not hold, a
  prompt missing its clip or words, nothing to say, or a `style` carrying a bracket or longer than 300 characters is
  refused before any model loads.
- **`GET /health`:** the checkpoint loaded, whether a line is being spoken, whether the model is on the card, the
  device, VRAM and host memory held, the card's free memory (asked of `nvidia-smi`, opening no CUDA context), whether
  other work holds the card lock (read from the kernel's lock table, never by taking it), whether the server holds
  the card between lines, and whether it has room: 6.8 GiB free to put a loaded model up, 7.2 GiB for a first load.
- **`503`:** the card lock stayed held by other work for the whole of the line's wait; nothing was loaded or spoken.
- **A line's words are never logged** on the node.

### How it holds the card

- **The model stays on the card between lines** until another occupant asks for the card; off it, it stands in
  pinned host memory and goes back up from there in about half a second.
- **After each line** the server asks the arbiter whether the next line waiting still stands first; while it does,
  that line is spoken with the model still up. **With no line waiting the turn rests,** the model and the lock kept.
- **It gives way the moment another occupant asks** or a claim begins: the model goes down into its host copies and
  the lock is let go, and the server asks to rest on the card again.
- **The card lock is `~/.gpu.lock`,** held under `flock` by every service that puts work on the card; it goes with
  the process, so a server that falls over never leaves the card locked.
- **The model loads on the first line,** not at start, and another checkpoint on the same base swaps only the LoRA
  weights. A line comes back the same whichever way the model stood.
- **A line that fails takes the model down,** and a load that failed half way is let go whole.

### What a long line takes of the card

- **The audio decoder's share grows with a piece's length,** so a stretch longer than the mouth's pass limit is
  spoken in passes of whole sentences, 0.3 seconds apart; a sentence longer than the limit is one pass all the same.
- **A piece the card runs out of memory for is spoken once more,** as its sentences one by one on an emptied cache;
  only a second shortfall fails the line.

## What the sound carries

- **Nothing is laid over the voice:** no band filter, resampling, bit reduction, noise or imitation, on any road.
- **The mouth matches its output's peak to 0.95,** and **the encoder only ever lowers:** a sound over the ceiling is
  scaled down whole rather than clipped, one under it written exactly as it arrived. Length is untouched.

## How a voice is measured

- **Intelligibility:** `synth/voice-cer.py` draws a voice's clips from a voice node's server, through
  `scripts/voice-node.ts speak`, of the sentences in `synth/voice-cer-sentences.json`, transcribes them with Whisper
  and compares them with the originals character by character, both sides first matched to their spoken spelling.

### How a prompt is checked

- **Whole lines are spoken on the clip before any row names it:** the lines of
  `data/<language>/voice-prompt-check-lines.json`, spoken the way a synthesis speaks them, each on a free voice
  node, and taken down by the `voice/transcriber` entry.
- **A line fails** on a character error rate above 0.12 over its bare letters, one unit of one to four letters
  repeated five times or more where the line has no such run, or a pace outside 3 to 10 syllables a second in Korean
  or 2 to 8 in English. One failed line refuses the clip.
