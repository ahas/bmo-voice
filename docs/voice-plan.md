# The voice

How a text becomes sound: the synthesiser, the road it takes to the voice nodes, how a written line is made a
script for speaking, the trained voices and their table, where the voices are trained, and the voice servers on
the voice nodes. What a caller does with the sound is the caller's.

## The synthesiser

- **Fish Audio S2-Pro is the one synthesiser.** Every line is spoken in a trained voice, a LoRA fine-tune of
  S2-Pro's slow autoregressive model on an agent's own recorded lines (「The voices」), at 48 kHz, with no
  reference clip: the voice is carried by the adapter alone.
- **Where no trained voice can speak a line, nothing is said.** There is no other synthesiser to fall back on.
- **VoxCPM2 is retired.** Its voices stay under the voices folder as data, known as its by holding no `voice.toml`
  (「The voices」), and none of them is served: a synthesis or a briefing naming one is refused 422 `retired-voice`
  (`docs/boundary-api.md`, 「Errors」). The engine a voice must name to be spoken is the policy's `engine`
  (`policy/voice.toml`).
- **What it costs:** the server's process peaks at about 11,100 MiB while it speaks a line, and up to 11,800 MiB for a
  piece of about a thousand frames, about 10.3 GiB of it resident once the model is loaded, so a voice node lends a 12 GB card whole for as long as the model is up; a smaller card
  does not take it. On an RTX 4070 it makes about 25 frames a second, a little faster than real time: about 1.1
  seconds of sound for every second of speaking, once the frame step is compiled (「How the model is loaded」).
- **What fits it in 12 GB** is the mouth's own way of loading the model (「How the model is loaded」).
- **The licence:** S2-Pro's weights, and every adapter trained on them, are under the Fish Audio Research
  License: research and non-commercial use only. A voice spoken for a paying service needs Fish Audio's own
  agreement first.

## The environments

- **Two interpreters, outside the repository,** each under `bmo-voice/` in the account's home:
  - **the voice environment,** `bmo-voice/.venv` on the operations machine: the OGG/Opus encoder every road ends
    in, and Whisper, for the transcriber and for measuring a voice. Its package list is
    `synth/voice-node-requirements.txt`, a full freeze, installed there as it stands without resolving dependencies,
    the CUDA build of torch from its own index;
  - **the engine environment,** `bmo-voice/fish-speech/` on each voice node: a checkout of Fish Audio's
    `fish-speech` at the commit `synth/engine.toml` pins, and its virtual environment `.venv` inside it, built by
    uv from the checkout's own lockfile with the CUDA 12.9 extra, the microphone package left out since nothing
    here records, and the machine's certificate store trusted for the downloads. The checkout is used as it
    stands: what the voice needs of the model beyond it is the mouth's own code (「How the model is loaded」).
- **`BMO_VOICE_PYTHON` and `BMO_ASR_PYTHON`** name the encoder's and the recogniser's interpreter, and
  **`BMO_VOX_PYTHON`** the mouth's, for a process that runs the tool entry itself, a run by hand or a test; a call
  through the tool host carries none of its caller's variables, and each runs on its environment under the call's
  home.
- **No interpreter at all** is a failure.

## Which road speaks a line

- **Every line is spoken in a trained voice,** one row of the voices table, named by the synthesis itself. Where
  it is spoken is the arbiter's master's to say (「Which node speaks a line」):
  - `voice` placed on a voice node: spoken on the voice server of the node the master grants the synthesis a card
    of `voice` on, and encoded on the operations machine by the `voice/encoder` entry;
  - `voice` placed on no node: the synthesis fails. The engine keeps no fallback onto the operations machine's
    card.
- **A voice the table does not hold, a node out of reach and a voice server that does not answer** leave the
  synthesis `failed`; nothing is spoken and the failure is the member's state.
- **A session kept off the engine's port, the chat agent among them,** reads which nodes are listed and whether
  their servers answer through the hosted tool `voice/servers` (`docs/boundary-api.md`, 「The hosted tool」).
- **`synth/voice-fish.py`** is the trained voice's mouth, both on a card of the machine it runs on and as the voice
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
- **A caller's stop reaches the synthesiser:** a caller that stops a call closes it, and the call's process group
  ends, the synthesiser and its hold on the lock with it.
- **A voice server queues for itself:** a line sent to it takes no lock here, and the server speaks one line at a
  time behind a lock of its own.

## The voice as a tool

- **`synth/tool.toml` declares the voice's entries,** four of the `voice` namespace, each for the system alone:
  never shown to a session, called through the tool host's control socket.
  - **`voice/mouth`** speaks one line on this machine's card, where the engine environment stands: the words or a
    file holding them, where the sound goes and where its wav is kept where asked, and the voice row's checkpoint,
    seed, delivery and sentence switch. It stands in the card's line itself, waiting for the lock
    however long the lines ahead of it take.
  - **`voice/encoder`** writes a wav as OGG/Opus, in no line, since the card is not touched.
  - **`voice/transcriber`** takes spoken files down in one language, each answered with what was heard, in the
    order given.
- **Its executable is `synth/voice-tool.py`,** on the system's `python3` and the standard library alone, placed
  with the mouth, the encoder and the transcriber it runs and the tool host's Python helper; each file it runs runs
  on the voice environment's interpreter.
- **No clock bounds a call:** the manifest declares no span and the tool host keeps none, so a call ends when its
  work ends, fails, or its caller stops it.

## Encoding

- **A voice message is OGG/Opus.** libsndfile writes it directly, so the mouth writes that format with
  `soundfile`, and neither it nor a voice node needs ffmpeg or sox.

## A line is spoken as a script

- **A line written for a room is markdown, and a voice reads its marks.** The engine makes the line a script for
  speaking first, mechanically; no model rewrites its words here.
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
- **The script is a briefing's written words and what `POST /script-readings` answers.** The synthesiser is handed
  a line's spoken words, which a model makes of the written words with judgement (`docs/boundary-api.md`, 「The
  spoken words」), never the script itself.

### Which sentences are English

- **One rule, read where the script is made:** it decides which stretches a line's names and figures are said in
  the Korean way. The mouth speaks a line whole, whatever languages it holds; S2-Pro reads both.
- **A sentence** ends at a full stop, a question or exclamation mark or an ellipsis before a space, and at every
  line break.
- **Its language:** Korean where it holds a Hangul syllable; English where it holds none and has at least two words
  with a lower-case letter; otherwise it takes the language of the next sentence that says one, or the last before
  it. A line where no sentence says is English if it has a roman letter and Korean otherwise.
- **Neighbouring sentences of one language are one stretch.** English is spoken as English.

## How names are said

- **One table says it for both languages:** `voice_names` (`docs/store.md`), one row per spelling, each with what a
  Korean stretch and an English stretch say in its place. An empty reading leaves the spelling as written.
- **Its rows are the agreed readings every line's spoken words are derived with** (`docs/boundary-api.md`, 「How the
  engine derives the spoken words」): the rows with a reading in the voice's language are handed to the model as
  readings to apply as they stand, and the model reads whatever else the line needs with its own judgement. No
  row is applied to a line by the engine itself.
- **A reading the model chose is written in as a row,** for a spelling the table does not hold, so the same name is
  read the same way next time. A spelling the table holds is never written over.
- **A row is the owner's to correct,** and a correction wins from then on: `scripts/voice-names.ts` lists the table,
  `--set <spelling> <korean> <english>` writes a row, `--remove <spelling>` takes one off. A Korean reading carries
  no roman letter and an English one no Hangul.
- **A table first made is given the seed names** of `data/ko/voice-names.toml`; a table that stands is left as it is.
  The names a deployment's own agents, units and owner go by are its own rows.

## How figures are said

- **No line is spoken by these rules.** A line's figures are said as its spoken words say them, which a model
  writes with judgement, and the check refuses spoken words that still hold a digit (`docs/boundary-api.md`, 「The
  spoken words」). The rules are the reading table's, kept with it.
- **A figure's reading the model writes wrongly is settled the way a name's is:** a row of `voice_names` holds the
  spelling and its Korean reading, and every derivation applies it as it stands (`docs/store.md`). The months and
  days of a date are seeded there, so a month and its counter are read as one word every time.
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

- **A trained voice is a LoRA fine-tune of S2-Pro** on an agent's recorded lines: an adapter on each attention and
  feed-forward linear of the slow autoregressive model, the fast one and the codec left as they are. Its folder
  under the voices folder holds two files:
  - **`voice.toml`,** which says what the voice is: its engine, the run and step its adapter was trained at, that it
    speaks with no reference clip, and the temperature and top-p every line of it is sampled with;
  - **`lora.pt`,** the adapter: its two matrices for each linear it names, and the scale they are added at.
- **A voice speaks with no reference clip.** Its lines are asked of the model as a bare conversation, the text and
  nothing more, and the adapter carries the voice; a voice trained that way says so in its `voice.toml`, and the
  row holds no reference.
- **The voices table says which one speaks** (`docs/store.md`): one row per voice, at most one current. The current
  voice is the chat agent's where its registration names none.
- **The instruction is the row's to change:** `scripts/voice-install.ts --style <name> '<words>'`. The mouth puts it
  in brackets before the words, where S2-Pro reads a tag it is given; it is never spoken aloud.
- **The seed is the row's too:** `--seed <name> <n>`. A seed names one delivery of one line; every piece of the line
  is spoken on it, so the same line, seed and settings give back the same take every time on one server; a
  server compiled apart may give another (「How the model is loaded」).
- **Whether a line is spoken a sentence at a time is the row's too:** `--sentences <name> on|off`. On, the line is
  cut at its sentences, each spoken on the row's seed with 0.3 seconds of silence between two; off, in passes of
  whole sentences. Either way a sentence too long for one piece is cut at its clauses (「What a long line takes of
  the card」).
- **How a piece ends and meets the next.** A piece is what one call to the model speaks. It ends where the model
  closes its turn, is levelled to the line's loudness (「What the sound carries」), and both its edges are faded over
  10 ms on a raised-cosine curve.
- **A line is one voice from its first piece to its last.** Every piece after the first is spoken on from the line's
  first piece (「What a long line takes of the card」), so the timbre, pitch and pace the first piece took carry through
  the line rather than being drawn afresh at every sentence.
- **No served voice takes a continuation prompt.** `--prompt` and `--prompt-en` are refused; the prompt columns
  stand only on the retired VoxCPM2 rows.
- **A trained adapter joins by name:** `--add <name> --checkpoint <dir>` copies its folder's `voice.toml` and
  `lora.pt` under the voices folder and adds a row, the current one where the table held none and not current
  otherwise. A folder whose `voice.toml` names another engine than the policy's is refused. Listed with no flag,
  the script shows every voice, marks the current one and says which are retired. **Switching is one command:**
  `--use <name>` makes that row the one current, and a retired row is refused.

### How the model is loaded

- **On the first line,** not at start, and again on the first after the server gave way (「How it holds the card」),
  from the base weights on the node (「Where the weights live」), in bf16.
- **The adapter is folded into the weights on the host,** before anything goes on the card: each linear it names
  gets its two matrices' product at the adapter's scale added to its weight, in fp32, written back in bf16. A line
  in another voice loads the base again.
- **The context is 3,072 tokens:** 2,048 of them are what one piece may generate, about 95 seconds of sound, and
  the 1,024 left take its prompt, which with no reference clip is the text and a few markers.
- **The codec's attention masks are made at each of its transformers' own block length,** not the 32,768 its code
  builds them at, which would take 4 GiB of the card for lengths no piece reaches.
- **The codec is cast to bf16 on the host and put on the card once,** so the card never holds a float32 copy beside
  it.
- **The card's memory is mapped in segments that grow and shrink** (PyTorch's expandable segments), set by the mouth
  before torch is first imported: what one pass frees is used again whole by the next rather than left in pieces
  too small for it. A line peaks about 450 MiB lower than in fixed blocks, and a piece of about a thousand frames
  still has its codec pass on the card.
- **The frame step is compiled and replayed as one CUDA graph.** What makes one frame of a piece once its prompt is
  read, the slow model's token and the fast model's ten codebooks with the sampling among them, is fish-speech's own
  decode step, compiled by inductor at its fixed shapes the first time a piece is spoken after a start, then
  captured as one CUDA graph and replayed frame after frame. The prompt's own pass is not compiled: its length is
  the piece's.
  - **Its shapes never change:** its attention reads the whole context, masked to what the piece has written.
  - **Each kernel runs in the fastest configuration the node measured for it,** the measurements kept beside the
    compiled kernels. Kernels measured apart do their arithmetic in another order, so a seed may give another take
    of a line on another node or after a start: the voice and its intelligibility are the same, measured against
    the step uncompiled (an ECAPA likeness of 0.94 to 0.98 and a character error rate within the uncompiled draws),
    the take is not.
  - **What it buys,** measured on a 4070 against the same step uncompiled: about 25 frames a second against 12.2, a
    1,664-character line in 181 seconds against 352.
  - **What it costs:** compiling takes two to three minutes the first time on a node and about half a minute
    after that, which reads the compiled kernels back from `bmo-voice/compiled/` on the node's home disk (the
    unit's `TORCHINDUCTOR_CACHE_DIR`). The first line of every load pays it: after a start, after another voice is
    asked for, and after the server gave way. The graph takes about 50 MiB of the card beside the model.
  - **The graph reads the weights and the key-value caches where they stood when it was captured,** so it is let go
    before either is let go, before a long piece's codec pass and as the model is let go, and captured again by the
    next frame, in under a second. What was compiled stays across the lines of every thread for as long as the model
    stands, and is let go with it.
  - **A frame step that fails to compile is taken uncompiled** for the rest of the process, said once in the
    journal, its attention on the card's fused kernels rather than the plain one fish-speech's generator asks for,
    which uncompiled is about 3% faster.

## Where the weights live

- **The voices stand under the data root's `voice/voices/`** (`weights/checkpoints.toml`), never in an experiment's
  directory, which another job may prune; the data root is outside every checkout and carried by the deployment's
  backup. Only what speaking needs is copied: a voice's `voice.toml` and `lora.pt`. The retired VoxCPM2 voices keep
  what they were installed with.
- **The base weights** are `fishaudio/s2-pro` at the revision `synth/engine.toml` pins, each of its files there with
  its size and SHA-256: the model's two shards and their index, the codec, the configuration and the tokenizer's
  files. The operations machine holds them in its Hugging Face cache, as that revision's snapshot; the repository is
  gated, so the snapshot is fetched there once with the owner's own Hugging Face sign-in, or copied from a machine
  that holds it.
- **On a voice node** they stand under `bmo-voice/s2-pro/` in the account's home, on the disk the home is on and
  never another; `setup` carries each file only where the node's copy is missing or its digest differs, and checks
  every copy it made by its digest.

## Where the voice is trained

- **On the nodes an experiment's pipeline names,** in the environment the experiment brings with it. An experiment stands on its home, the node named with `--home`,
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
  leaves a line in the record's `steps.log`. The runner looks at the step every half minute until it has an exit or
  its unit has gone; a node that does not answer a look is looked at again, however long it stays out of reach.

## The voice nodes

### Where they run

- **On every node of the voice node list,** `voice_nodes` (`docs/store.md`), as the user unit
  `bmo-voice-server.service` with the same voices directory, so any of them speaks a line exactly as another would.
  Lingering is on for the account, so the unit starts with the machine. The unit is named apart from the service's
  own `bmo-voice.service`, so the operations machine may be a voice node as well. A stop of the unit waits for the
  server to end, which it does on the signal itself, and never kills it after a span.
- **`voice` stands on as many nodes as its placement names:** its declaration, `weights/voice.card.toml`, spreads it
  over several (`spread` `many`). It does not stand alone, and its priority, 40, is below every service that does on
  the lab's GPU nodes, so it joins the reservation of whatever node it is placed on, named there after the services
  that reserve it, and shares the card with them by its tier (「Which node speaks a line」). The nodes themselves are
  named only in the arbiter's placement, never here.
- **The list is the card arbiter's to change,** through the voice's adapter: `bmo-arbiter place voice --to <node>`
  and `bmo-arbiter remove voice --from <node>`. A node taken off keeps its server; putting it back is all it takes to
  speak there again. An empty list places `voice` nowhere, and a synthesis fails.
- **A node whose reservation does not name `voice` is never a voice node:** named on the list all the same, it is
  out of reach.
- **Port 7432 on each node's loopback, and nowhere else.** Every call reaches it through the node's own shell, `ssh`
  for an ssh node and the hub's exec and file transfer for a hub node, as the node service's target gives it.
- **What a node takes:** a card of 12 GB or more (「The synthesiser」); a smaller card cannot be a voice node.
- **What lands on each node,** under `bmo-voice/` in the account's home: the server file, the engine environment
  (`fish-speech/`), the base weights (`s2-pro/`), a copy of the voices directory, and `spoken/` (`0700`), where a
  call's request and answer stand while it is under way. The token is `.config/bmo-voice/token`, `0600`.

### Which node speaks a line

- **The arbiter's master says which node speaks a line** (`docs/boundary-api.md`, 「The voice servers and the
  card」): every synthesis asks it for one card of `voice`, naming no node, and is spoken on the node of the card
  granted. The master grants a card where `voice` stands with no demand on it and holds one demand on a card at a
  time, so syntheses set off together are spoken side by side where `voice` stands on several nodes, and one after
  another where it stands on one.
- **The card is held for the whole of the line** and given back as the line ends, spoken or failed.
- **Waiting for a card has no end but its grant:** in the line behind the demands asked before it, while every card
  of `voice` is in use or held by a claim or a lower tier, or while the master is not reached. The synthesis stays
  `waiting`, and its `waitingOn` says why.
- **No clock bounds the speaking:** it ends when the node answers with the line's sound or with a failure.
- **On the node** the server waits for its turn however long that takes, in the card arbiter's hold queue through
  the turn client, and once the turn has come, for the card's memory to take the model. Where the node's arbiter
  gives the voice no place on the card it answers 503, and the run reads its demand again until the card is granted
  again and asks the line of that card's node once more.
- **A caller that gives up its call** ends the speaking the call had started; the synthesis ends `failed`.
- **A granted node out of reach fails the run at once,** naming the node and what it met. It is not spoken on the
  operations machine's card instead: a line quietly spoken elsewhere would hide the fault.

### Building and updating them

- **Each verb of `scripts/voice-node.ts` acts on every voice node, or the one `--node` names,** side by side, each
  output line opening with the node's name.
- **`setup`** builds the engine environment and places the base weights (「The environments」, 「Where the weights
  live」): it installs uv where missing, refuses a node whose driver runs no CUDA 12.9 or whose card holds less than
  12 GB, checks out `fish-speech` at its pinned commit and builds its environment apart on the node, its output in
  `bmo-voice/setup.log`, and carries the weights the node lacks. The build is waited for until it writes its exit,
  or until its process has gone without one, which fails the setup; a second setup on the node waits for the first.
- **`push`** copies the server file, the voices directory and the token, writes the unit, restarts it, waits until it
  answers its health or its process has ended (its unit failed or stopped, or restarting it), and leaves a stamp of
  what it brought, `bmo-voice/pushed`.
- **`sync`** is push where it is due: only where a node's stamp differs from what a push would bring now. Every
  deployment runs it once the service answers (`docs/deploy.md`), so a change to the server or a newly installed
  voice reaches every node; a sync that pushes restarts that node's server.
- **`health`** asks each server what it holds; **`speak --text '<words>' --wav <path>`** speaks one line in the
  current voice or the one `--voice` names, on a free node where `--node` names none, asking the nodes again every two
  seconds until one is free.
- **`adapter`** answers the card arbiter (`docs/boundary-api.md`, 「The node scripts and the card adapter」):
  `install` puts the node last on the list, runs `setup` and `push` and speaks there the check line of
  `data/ko/voice-node.toml`. On a node whose card lock another occupant holds as the check line would be asked, the
  arbiter never makes that occupant yield for it: there the install stands on the server answering its health on
  the card's device with its weights checked, says that the check line waited, and the first line the master grants
  on the node is its first spoken one. `uninstall` takes
  it off the list and stops and disables its unit, keeping its installation; `start`, `stop` and `health` act on its
  unit and server; `placed` reads the list.
- **The unit** declares the server to the node's card arbiter as `BMO_CARD_SERVICE=voice`, and names the arbiter's
  own installation, `~/bmo-card`, on the server's import path: the server waits its turn for the card through the
  turn client the arbiter installs there, and takes the card lock as it comes on a node without it.

### How a line crosses

- **The request goes in on the node's standard input,** never on a command line. The node writes it into `spoken/`
  and starts the asker apart, `voice-fish.py --ask`, which posts it to the server on loopback with the token and writes
  the answer's wav beside it with one line of JSON: the status, the reason, the headers.
- **The operations machine hears of that line** by waiting on the asker's lock on the node, each wait a short piece
  inside what a hub node lets a command run, asked again until the asker lets go, then fetches the wav a slice at a
  time, four MiB to a command, every byte counted back, and removes every file of the call. Each call first sweeps
  away the files a call left behind once its asker has let go of its lock and nothing of it has been written for an
  hour; a call whose asker still holds its lock is never swept, however long it runs.
- **Why apart:** a hub node stops a command after five minutes, and a long line from a cold server can take longer.
- **How a failure is named:** unreachable (the shell, the hub or the server not there), refused (a wrong token, a
  voice the node does not hold), failed (the line not spoken), a bad answer (not a wav).

### What it answers

- **Every call carries the bearer token,** `voice/voice.token` under the data root, made by the first `push` and
  copied to every node; it never goes into a database, a log, a command line or an environment variable.
- **`POST /speak`:** a line and the voice as a name under the voices root, with `seed`, `style` and `sentences`. The
  answer is the spoken line as a 48 kHz 16-bit wav, peak-matched, with its length, load time, speaking time and
  checkpoint in its headers. The line is spoken whole however many characters it holds; only the request's body is
  bounded, at 1 MiB, read no further than that. A path leaving the voices root, a voice the node does not hold, a
  voice whose `voice.toml` names another engine, nothing to say, or a `style` carrying a bracket or longer than 300
  characters is refused before any model loads.
- **`GET /health`:** the checkpoint loaded, whether a line is being spoken, whether the model is on the card, the
  device and VRAM held, the card's free memory (asked of `nvidia-smi`, opening no CUDA context), whether
  other work holds the card lock (read from the kernel's lock table, never by taking it), whether the server holds
  the card between lines, and whether it has room: 11.0 GiB free to load the model.
- **`503`:** the node's card arbiter gives the voice no place on the card; nothing was loaded or spoken.
- **`500`:** the line failed while it was spoken. The answer names the failure's kind, its message and the frame of
  the mouth's own code it was raised in, which the engine writes into the synthesis's failure; the node's journal
  holds the whole traceback.
- **A line's words are never logged** on the node.

### How it holds the card

- **The model stays on the card between lines** until another occupant asks for the card, and off the card it stands
  nowhere: the server keeps no copy of it in host memory, pinned or not, so a card given away costs the node's memory
  nothing beside the server's own process. The next line loads it from the disk (「How the model is loaded」). The
  key-value caches are scratch every piece writes before it reads: they stay on the card from piece to piece while
  the model stands, are let go before a long piece's codec pass (「What a long line takes of the card」) and with the
  model, and are made again, on the card, by the next piece.
- **After each line** the server asks the arbiter whether the next line waiting still stands first; while it does,
  that line is spoken with the model still up. **With no line waiting the turn rests,** the model and the lock kept.
- **It gives way the moment another occupant asks** or a claim begins: the model is let go whole, off the card and
  out of host memory, the C heap trimmed, and the lock and the ticket are given back. With no model left
  to rest on the card, the server asks for no ticket until its next line comes.
- **A line being spoken is never given way under:** the model is let go only between lines, under the line's own lock,
  once the line in hand has ended. A line that was waiting behind it when the card went elsewhere asks in line for
  the card and loads the model when its turn comes.
- **The card lock is `~/.gpu.lock`,** held under `flock` by every service that puts work on the card; it goes with
  the process, so a server that falls over never leaves the card locked.
- **The model loads on the first line,** not at start (「How the model is loaded」), on the first line after the server
  gave way, and on a line in another voice. A line comes back the same whichever way the model stood.
- **A line that fails lets the model go whole,** and so does a load that failed half way.

### What a long line takes of the card

- **No count of characters bounds a line.** The mouth speaks a line piece by piece, so what a line takes of the card
  is what its longest piece takes; what grows with the line is only the sound held in host memory until it leaves,
  about 10 MiB a minute, and the time it takes, which no clock bounds. The engine bounds the line itself, at the
  30,000 characters of spoken words a synthesis holds (`docs/boundary-api.md`, 「Syntheses」), and hands it to one
  server whole.
- **A line is cut into pieces:** a row that speaks a sentence at a time speaks each sentence as one piece, and any
  other row speaks a line in passes of whole sentences of up to 240 characters, 0.3 seconds apart.
- **No piece is longer than 340 characters.** A sentence longer than that is cut at its clauses — after a comma, a
  semicolon, a colon or a dash before a space — and its clauses gathered in order into parts each as long as it can
  be within 340, each part one piece; a clause longer than 340 alone is cut at the last space within the limit. A
  cut never leaves a word out and never falls inside one. The bound is what the codec takes of the card: it makes a
  piece's sound from its frames in one pass, its memory growing with the piece's length, beside the 9.4 GiB the
  model and the codec keep. On a 4070 the sound of 1,000 frames fits beside them alone and 1,050 does not; dense
  text, English names and figures among Korean, runs up to 2.7 frames a character, so 340 characters is at most
  about 920 frames, and far inside the 2,048 a piece may generate.
- **The key-value caches stay on the card from piece to piece,** 453 MiB at the context above, so the frame step's
  graph stands for the whole of a card turn. They are let go, with the graph, and the allocator's cache emptied,
  only before the codec pass of a piece of more than 600 frames: on a 4070 the sound of 750 frames fits beside the
  caches and 800 does not, and 600 leaves a fifth of that to spare.
- **Every piece carries the line's first piece, and only that one.** The first piece of a line is asked as a bare
  conversation, its text and nothing more. Every later piece is asked with the first piece as its reference: its
  words and the codes the model generated for them, in the place S2-Pro reads a reference from. The model ends a
  piece the way its reference ends, so a reference cut short makes the piece after it stop short too; a piece asked
  bare ends on the voice's own fall, so only such a piece is ever carried. One sentence's sound is all the prompt
  holds, inside the 1,024 tokens the context leaves it; a piece whose text would not fit beside the first piece is
  asked bare. Every piece is spoken on the line's seed.
- **A piece that stops short is spoken again bare.** A piece whose voice is still loud in its last 30 ms, within
  30 dB of its loudest frame, with under 100 ms after its last loud frame, was ended by the model before its last
  syllable fell away; it is asked once more with no reference, on the same seed, and that one is kept.
- **A piece the card runs out of memory for is spoken once more,** as its sentences one by one on an emptied cache,
  each cut at its clauses into parts of at most 170 characters; only a second shortfall fails the line.

## What the sound carries

- **Nothing is laid over the voice:** no band filter, bit reduction, noise or imitation, on any road. The one change
  is the rate: the codec speaks at 44.1 kHz and OGG/Opus is written at 48 kHz, so the mouth resamples the line once,
  band-limited, before it leaves.
- **Every piece is levelled to one loudness** before the line is joined: its RMS over its voiced frames, those
  within 40 dB of its loudest, is brought to −20 dBFS, so no sentence stands louder or quieter than the next.
- **The mouth then matches the whole line's peak to 0.95,** one gain for all of it, and **the encoder only ever
  lowers:** a sound over the ceiling is scaled down whole rather than clipped, one under it written exactly as it
  arrived. Length is untouched.

## How a voice is measured

- **Intelligibility:** `synth/voice-cer.py` draws a voice's clips from a voice node's server, through
  `scripts/voice-node.ts speak`, of the sentences in `synth/voice-cer-sentences.json`, transcribes them with Whisper
  and compares them with the originals character by character, both sides first matched to their spoken spelling.
