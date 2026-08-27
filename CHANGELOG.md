# Changelog

All notable changes to OpenVoice are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Entries are written for the person deciding whether to upgrade, not for the
person who wrote the commit. "Fixed a race in the supervisor" is a commit
message; "dictating twice in quick succession no longer drops the second
utterance" is a changelog entry.

Commit prefixes map to sections: `feat:` → Added, `fix:` → Fixed,
`refactor:`/`perf:` → Changed, and anything that removes or breaks behaviour →
Removed or the **Breaking** note at the top of that release.

<!--
Add to `## Unreleased` in the same PR that changes behaviour. A changelog
assembled at release time from a month of commits is written by whoever has the
least context, at the moment they have the least time.
-->

## [Unreleased]

## [0.4.0] - 2026-08-27

This release closes out the Flow Bar's geometry problems and its z-order
problem in the same pass, then rebuilds it as a combination of Wispr Flow's and
superwhisper's floating bars: a compact indicator, edge docking, and a menu
that is a control surface rather than two links to the Hub.

### Added

- **The Flow Bar can be shrunk instead of hidden.** "Compact bar" in its
  right-click menu reduces it to a small indicator that still shows the status
  light, and the waveform while you are speaking. Hover it and the full bar comes
  back. The choice is remembered between restarts. Hiding a bar you cannot see is
  the one thing that stops it telling you your microphone is open; making it
  smaller does not.

- **Drag the bar to the left or right edge of the screen and it stands up.** It
  reorients into a vertical column so it takes a strip of the margin rather than a
  bite out of whatever is maximised behind it. If something goes wrong while it is
  docked it unfurls back to horizontal to show you the message, because a sentence
  you have to tilt your head to read is not one you will read.

- **Click the bar to start dictating.** The shortcut is still the fast way; this
  is the discoverable one. Clicking to dismiss the menu does not start a
  recording, and a click that turns into a drag is a drag.

- **The Flow Bar's menu is now a real menu.** Start or stop dictating, paste the
  last transcript, jump to your transcript history, microphone or settings,
  switch between the compact and full bar, hide for an hour, or only show it
  while dictating. Each destination opens the screen it names.

- **The tray icon has a Flow Bar submenu:** Show Flow Bar, Hide until I dictate,
  and Reset Flow Bar position. Every control for the bar used to live in a menu on
  the bar itself, which is a dead end the moment the bar is not somewhere you can
  reach it — snoozed, switched off, dragged into a corner, or behind another
  window. The tray cannot be hidden.

### Fixed

- **The Flow Bar no longer disappears behind other windows after a few minutes.**
  Nothing was hiding it. Windows silently strips the always-on-top flag from
  other windows when an application takes the screen fullscreen — a video, a game,
  a screen share — and never puts it back. No event is raised, and the bar still
  reports itself as visible, so it sat at the bottom of the stacking order with
  everything painted over it until the app was restarted. It now notices within
  two seconds and puts itself back, and records each time it had to, so the next
  person to report this has a number rather than a feeling.

- **The Flow Bar no longer claims to be ready while the speech engine is still
  starting.** During the first-run model download, the several seconds of loading
  after it, and permanently after the engine failed to start, the bar displayed
  "Hold Right Ctrl" — an invitation to press a key that was going to do nothing.
  It now shows the download's progress, says when it is starting, and says so in
  red when the engine is not going to come up.

- **The Flow Bar no longer jumps up and to the left when you press the hotkey.**
  The bar was held still while it changed size by moving its window in the
  opposite direction and letting the pill re-centre itself inside it. That only
  works if the move and the re-centring are drawn in the same frame, and they are
  produced by two processes on two schedules — so for the gap between them the
  pill was drawn at its old size inside the window's new position, displaced by
  exactly half the size change. Idle to listening, that is 67px left and 22px up.

  The window is now a fixed size and never moves when the bar changes state, so
  there is nothing left to be out of step. It is clipped to the pill, which means
  the parts of it you cannot see still do not swallow clicks meant for whatever is
  underneath. Your saved bar position is migrated automatically.

  Four earlier releases fixed real defects in this mechanism without fixing the
  symptom; this removes the mechanism. See [ADR 0007](docs/adr/0007-flow-bar-fixed-window.md).

- **Dictated sentences now capitalize "I" and the days and months.** Whisper
  transcribes the pronoun and every weekday and month name in lowercase, and
  nothing downstream lifted them, so "so i think we should ship on friday" landed
  exactly like that in your document. Sentence-initial words were already handled;
  these are the words English capitalizes wherever they fall. Names of people,
  places and products are deliberately still left alone — guessing those needs
  knowledge the formatter does not have, and a wrong guess is worse than the
  lowercase it replaced.
- **"fifty percent" is no longer written as "fifty%".** The spoken word `percent`
  was substituted for `%` wherever it appeared, which is right after a number and
  nonsense after a word. It now converts only when a digit precedes it, so
  "up 50 percent" gives you `up 50%` and "up fifty percent" is left as spoken.
- **The Flow Bar no longer flashes a cropped black rectangle when you press the
  hotkey.** For a moment as the bar switched into listening, it could appear as a
  square black box with the pill clipped inside it — one rounded corner showing,
  the green edge running into a hard cut, the timer sliced through. Two separate
  races caused it, and both are closed: the pill was being painted at its new
  width before the window had been resized to hold it, and the window was being
  resized before the surface it is drawn on had caught up. The pill's size is now
  read off the window rather than told to it, so the two cannot disagree.
- **The Flow Bar no longer drifts after you drag it near a screen edge.** The
  animation that slides the bar the last few pixels into a snap was being denied
  permission to move the window, silently, on every frame — while the bar went on
  recording the position it believed it had reached. The next time it resized, it
  measured from somewhere it had never been.
- **The bar is now sized correctly at startup with a non-default shortcut.** The
  first resize after launch was dropped, leaving the window at the size declared
  in configuration rather than the size the pill actually needed.
- **The bar no longer jumps up and to the left when you press the hotkey
  repeatedly.** Once you had clicked the bar even once, every automatic resize
  after that was mistaken for you dragging it, and the bar's remembered position
  was rebuilt from a size it had not reached yet. It moved about 57px left and
  22px up on release and back again on the next press.
- **The bar no longer stays stretched after you cancel a dictation.** Cancelling
  while the bar was still growing into its listening size left the window at that
  larger size with the idle content inside it.
- **A click on the bar that does not move it no longer counts as a drag.** The
  next time the bar was placed automatically, that placement was saved as a
  position you had chosen.
- **The bar no longer slides up and to the left when you use the hotkey.** The
  bar can tell its own movements apart from yours now, instead of guessing. When
  it guessed wrong, the ordinary resize that follows every press was treated as
  you having dragged the bar: it was snapped to a screen edge measured for the
  wrong size, saved to disk as your chosen position, and then slid there over a
  fifth of a second. It also left the bar's idea of where it belongs permanently
  wrong, so the next press started from the wrong place.
- **The slide that follows a drag can now be interrupted.** It ran to completion
  no matter what else happened, writing a position sixty times a second against
  everything else that moves the bar — so a dictation started mid-slide fought it,
  and the slide won.
- **The Flow Bar can no longer steal focus from what you are typing into.** The
  flag that prevents it was applied once at startup and then quietly cleared by
  the window toolkit, so on a running copy the bar was able to take focus — and
  losing the caret is how a dictation lands in the wrong place. It is now
  re-applied whenever the bar is shown.

## [0.3.0] - 2026-08-11

This release is about the Flow Bar — the small pill that floats while you
dictate, and the only part of OpenVoice most people ever look at. It can now
tell you when something went wrong, let you throw away a dictation you did not
mean to start, and it moves like something that was designed rather than
resized.

### Added

- **Discard a dictation in progress.** Press Escape, or use the new × on the
  bar. Previously, releasing the key always transcribed and injected: an
  accidental trigger had to land in your document before you could undo it.
  Escape has quietly worked for a while; nothing ever said so.
- **The bar tells you when a dictation did not land.** A failure and a
  clipboard fallback each get their own state on the bar, with the reason.
  Before this, both looked exactly like nothing having happened — you found out
  when you went looking for text that was not there.
- **A confirmation when your words arrive.** A single ring leaves the status
  dot as the text lands, so you can tell success from silence without turning
  your head.
- **The bar glows while the microphone is open**, and the glow tracks your
  voice. It is meant to answer "is it hearing me?" from the corner of your eye.
- **More vocabulary out of the box**: Vercel, Tauri, GitHub, pnpm, Claude,
  Anthropic, MCP, SDK, UX, CSS, HTML and a few more now come out spelled the way
  you meant. Only spoken forms that are not also ordinary English are claimed —
  a dictionary that confidently rewrites real words is worse than none.

### Changed

- **The bar moves properly.** Releasing a drag near a screen edge now slides
  into place instead of jumping, and the bar shows you when it is about to
  snap. Its width changes ease between sizes rather than snapping between them.
- **The waveform behaves like a meter**, rising fast and falling slowly with a
  held peak, instead of rising and falling at one flat rate. It also uses fewer,
  clearer bars — at the size the bar is actually seen, thirty-two of them read
  as a green smear rather than as your voice.

### Fixed

- **Dictating twice in quick succession no longer loses the second utterance.**
  Releasing the key left the recording slot occupied for a few hundredths of a
  second while the audio was handed back. A press inside that gap was discarded
  outright — no recording, no error, nothing on screen — so you could speak a
  whole sentence into nothing and only discover it afterwards. The press is now
  held and honoured the moment the slot frees.
- **Three dictations in quick succession no longer produce the wrong text.**
  Only one utterance's audio was kept at a time, while the queue behind it could
  be several deep. The third recording overwrote the second's audio, so the
  second dictation was transcribed from the third one's sound — inserting words
  you said at a different moment — and the third then failed outright. Each
  recording now keeps its own audio until its turn.
- **A dictation that fails now says so.** Transcription and microphone failures
  reached the Flow Bar in the same instant it was told to go back to idle, so
  the failure was never drawn. Failing silently is the one thing a tool that
  types for you must not do.
- **"I mean", "kind of", "sort of", "actually" and "literally" are no longer
  deleted from what you said.** Filler removal treated these as noise, and the
  profile used for browsers, chat and mail removes them by default — so in most
  applications they silently vanished. They are not noise: "I mean it" became
  "it" and "kind of blue" became "blue", which is the opposite of what was
  said. Only tone words are removed now; anything that can change meaning
  stays. "you know", "like" and "basically" are still removed at the aggressive
  setting.
- **"Literally" is no longer swallowed before the next word.** It is the escape
  word that stops a voice command being interpreted — "literally new line"
  types the words instead of breaking the line — but it was consumed before
  *anything*, so "it is literally true" arrived as "it is true". It is now only
  treated as an escape when a real command follows it.
- **A custom shortcut no longer breaks the idle bar.** The bar was sized for
  exactly one shortcut — the default. Anything longer collided with the word
  "Hold" and was cut off. It is now measured from whatever your shortcut is.
- **Error messages are no longer truncated mid-sentence.** The bar was fixed at
  a width that cut most messages off after about thirty characters, which is
  reliably before the part telling you what to do about it.

## [0.2.0] - 2026-08-09

OpenVoice can now tell you when a new version exists, manage its own speech
models, and learn a word it got wrong. Several settings that had never done
anything now do what they say — and one bug meant no fresh install could
download a model at all.

### Added

- **Update checks.** OpenVoice looks once per launch for a newer release and
  offers it; you press a button to install. It never installs on its own. Every
  update is verified against a signing key built into the app, so a tampered or
  substituted download is discarded without running — worth having while the
  installer itself is still unsigned. Switch it off in Settings → Updates and no
  request is made at all. This is the one request OpenVoice makes that you did not
  individually ask for, and the reasoning is in
  [ADR 0005](docs/adr/0005-in-app-updates.md).
- **Secrets are kept out of your history.** API keys and tokens matching
  `privacy.redact_patterns` are replaced with `[redacted]` before a transcript is
  saved or logged. Defaults cover OpenAI, GitHub and AWS credentials. The text
  delivered to your application is never altered — only the stored copy.

- **Press once to start, press again to stop.** Holding a key is fine for a
  sentence and unpleasant for a paragraph, so there is now a choice under
  Settings → Dictation. Hold-to-talk is still the default, because it is the only
  mode where the microphone cannot be left open by accident. A session you forget
  to stop still ends at the maximum recording length.
- **The dictation key can be changed.** Eighteen keys to choose from, including
  F13–F24 for anyone with a macro key or a remapper. The Hub and the tray now name
  the key you actually bound instead of always saying "Right Ctrl".
- **Teach it from a dictation that came out wrong.** Every entry in Recent now
  has a **Fix a word** button. It shows what OpenVoice actually *heard* — which
  you have never been able to see before — and you click the words it got wrong,
  type what you meant, and save. It applies to the very next thing you dictate.
  Knowing that `kubectl` arrived as "cube control" is most of the work; guessing
  it from the tidied-up output is not.
- **Models can be downloaded and removed from the app.** The Speech model screen
  now shows which models are on your disk, how much room each takes, and the
  total. Download fetches a model without switching to it or restarting, so you
  can get the weights first and decide later; Delete gives the space back.
  Deleting the model in use is refused rather than left to fail later.
- **Recordings clean themselves up.** If you turn "Keep recordings" on, they are
  now deleted after a week by default, adjustable to a day, a month, or never.
  Audio is roughly 2 MB a minute and previously nothing ever removed it. Your
  history is separate and unaffected — turning recordings off, or having them
  swept, never touches your transcripts.

### Fixed

- **Downloading a model works at all.** No model could ever be fetched by an
  installed copy — including the very first download on a fresh machine, which
  made a new install unusable unless it happened to find weights already cached.
  Offline mode is read once by the download library when it starts and could not
  be lifted afterwards, so every fetch failed claiming outgoing traffic was
  disabled. This was present before this release and was invisible on any
  machine that already had the models.
- **The Speech model screen looks in the right place.** It reported "using
  nothing on this computer" and offered to download models already present,
  because it always looked under `%APPDATA%\OpenVoice\models`. That is only where
  weights live for a standard install; if `OPENVOICE_PYTHON` or `HF_HOME` is set
  the app uses a different cache, and the screen now asks the running engine
  which one it is actually using.
- **A model you have chosen but not downloaded no longer claims to be "In use".**
  It reads "Selected — not downloaded", next to the Download button that will
  fetch it.
- **A failed update check reads like information rather than a fault.** Nothing
  is wrong if no release has been published yet, or if you are offline, and the
  message now says so while keeping the underlying detail.
- **"Maximum recording" now has an effect.** Choosing 5 minutes still cut the
  recording off at 2: the whole `[limits]` section was written to disk and then
  ignored, because the engine started the session machine with built-in defaults
  instead of your settings.
- **"Keep recordings" now keeps recordings.** The toggle had nothing behind it.
  Turned on, captured audio is kept under `%APPDATA%\OpenVoice\audio` from the
  next restart instead of being deleted after each decode. It remains off by
  default, and it is still the only way audio ever persists.
- The Speech model screen lists what the app can actually load. The model list was
  written down twice — once in the speech engine, once in the interface — and the
  two could disagree about which models existed and how large they were.

### Removed

- `model` under `[config]` in `settings.toml`, which was read by nothing. The
  model has always been the top-level `model` key, and that is unchanged. Your
  existing settings file loads as-is; the stale key is ignored, not deleted.

## [0.1.1] - 2026-08-08

A fix release. The visible complaint was that the Flow Bar could multiply into
several copies of itself, each drawn inside an opaque rectangle. The bar was
innocent: OpenVoice could be running several times over, and each copy was
showing its own.

Worth upgrading to even if you never saw duplicate bars — the same fault meant
several copies could be holding the microphone, decoding the same audio, and
writing the same history database at once, none of which is visible from the
interface.

### Fixed

- **Launching OpenVoice while it is already running no longer starts a second
  copy of it.** Closing the Hub hides it rather than quitting, which is
  deliberate — a dictation tool that stops working when you close its window is
  not available when you need it. But nothing stopped the *next* launch from
  starting a complete second app: another global keyboard hook on the same
  chord, another Flow Bar, another 1.6 GB of weights, and two processes writing
  the same history database and overlay placement. Every hotkey hold then opened
  the microphone once per copy, transcribed the same audio that many times, and
  raced to inject the results into each other. Launching again now raises the
  window of the copy already running, which is what it was always meant to mean.
  The duplicates were easiest to notice after "Hide for an hour", because that
  kept each new copy's bar hidden until a dictation forced it on screen — so
  they arrived all at once, on one keypress, looking like one window gone wrong.
- **The Flow Bar no longer flashes a dark rectangle around itself.** The
  overlay window is transparent, but transparency was applied from a script that
  runs after the first paint, while the app's black canvas was painted the
  moment the stylesheet loaded. The window filled itself in for the whole gap
  between the two. The canvas is now something each window opts into, so the
  overlay has nothing to fall back to; the window is also declared at the size
  of the idle pill rather than 280×52, so it no longer appears oversized and
  then shrinks.
- **The Flow Bar's right-click menu now closes when you start dictating.** Its
  only dismissal path was losing window focus — which cannot happen to a window
  built never to take focus. Opening the menu and then dictating left the panel
  standing open behind the bar as an opaque block, with no way to click away
  from it.
- **Auto-placement is no longer recorded as a position you chose.** The Windows
  move loop swallows the mouse-up that ends a drag, so a click on the bar that
  moved nothing could leave it looking permanently dragged; the next time the
  app placed the bar itself, that placement came back as a drag, got snapped to
  an edge, and was saved.

## [0.1.0] - 2026-08-03

The first published build. Hotkey → capture → transcribe → format → inject →
history works end to end on Windows, and it installs from a single `.exe` with
no Python, no Rust and no Node on the target machine.

This is an alpha, and the version number is the promise: there is no upgrade
path from anything, and nothing here is a compatibility commitment yet. Two
things to know before installing, both covered in the README: the installer is
**not code-signed**, so SmartScreen will warn about an unknown publisher (verify
the published SHA-256 instead), and the bundled speech engine is **CPU-only** —
using an NVIDIA GPU still means running from source.

### Added

- **Sound feedback.** A short tone on starting a dictation, and a distinct
  one on a successful finish — both synthesized with the Web Audio API rather
  than shipped as audio assets, so there's nothing to license or bundle.
  Off/on in Settings (`Config.sound_enabled`, default on). The finish chime is
  deliberately withheld on a clipboard-fallback completion, which settles to
  the same "idle" state a clean success does — playing it there would tell the
  user a dictation landed cleanly when it didn't.
- **A language setting.** `ov-asr`/the sidecar have accepted a forced-language
  parameter since day one, but it was hardcoded to English at every call site
  in `ov-app` and `ov-cli` with no way to change it. Now a real setting
  (`Config.language`), exposed as a dropdown in Settings, defaulting to English
  rather than auto-detect — Whisper's auto-detection looks at only the first
  ~30s of audio and is measurably less reliable than being told the language,
  which matters most for exactly the case this app is built around: a short,
  single push-to-talk utterance. Unlike the model, changing it needs no
  restart — it's a per-request decode parameter, not something baked into
  loaded weights, so it reloads the same way the dictionary does. `ov-cli`
  gained a matching `--language` flag (`auto` for auto-detect).
- **An installer.** `OpenVoice_0.1.0_x64-setup.exe`, 47 MB, with the speech
  engine inside it. No Python, no virtualenv, no Rust on the target machine.
  Built by `release.yml` on a tag, with a SHA-256 published beside it.
- **A frozen speech engine.** PyInstaller packages the sidecar into a standalone
  binary (`scripts/build-sidecar.ps1`). The build is not considered finished
  until the frozen binary answers a real request on its protocol — a missed
  hidden import is invisible to PyInstaller and fatal at run time.
- **First-run model download, with progress.** The weights are fetched on first
  launch and reported as a real byte count rather than a spinner. Previously the
  app simply looked frozen for the several minutes this takes.
- **Screenshots that cannot go stale.** `scripts/screenshots.mjs` captures the
  README images from the running UI over the DevTools protocol.

### Changed

- An installed copy keeps its weights under `%APPDATA%\OpenVoice\models` rather
  than in a shared Hugging Face cache, so uninstalling reclaims the space.
- A debug build always prefers the Python sidecar in your checkout; a release
  build prefers the frozen one it shipped with. Edits to `sidecar/` are no longer
  silently shadowed by whatever was last frozen.
- The speech engine ships CPU-only. The CUDA libraries are 1.9 GB against 240 MB
  for everything else and do nothing without an NVIDIA GPU.
- **The installed default model is now `base.en` (~75 MB), not `large-v3-turbo`
  (~1.6 GB).** Pairing the heaviest model with a CPU-only install was the wrong
  default: worst latency, biggest download. Upgrading is a Models-screen action,
  already built, that downloads on demand.
- **PyAV is no longer in the frozen build**, cutting it from 240 MB to 173 MB.
  `engine.py` reads its own WAV files (it is the only producer of them, and
  always writes 16 kHz mono 16-bit PCM) instead of going through
  faster-whisper's `decode_audio`, which is the only thing that needed PyAV's
  bundled FFmpeg. Verified against the real fixture audio, not just unit tests.

### Fixed

- **A dictated accented character, CJK character, or emoji could crash the
  sidecar or silently lose the transcript.** Python's stdio only defaults to
  UTF-8 inside an interactive console; redirected to a pipe — exactly how the
  Rust host launches this process — it fell back to the Windows ANSI code
  page (`cp1252` on a typical install). Confirmed by actually spawning a
  piped child process: a character `cp1252` can't represent at all (CJK,
  emoji) crashed the sidecar outright, and one it *can* represent as a
  different single byte than UTF-8 uses — which includes ordinary accented
  Latin text ("café", "naïve") and the smart quotes/dashes Whisper's own
  punctuation restoration commonly adds — wrote bytes the Rust side rejected
  as invalid UTF-8. The automatic retry couldn't help, because it decodes the
  exact same audio and hits the exact same failure again. Fixed by forcing
  UTF-8 on stdin/stdout/stderr explicitly, first thing in `main()`, rather
  than trusting the launching environment. Verified with a real piped
  subprocess in both directions (fails without the fix, survives with it).
- **Four idempotency bugs in the formatting pipeline**, found by fuzzing a
  battery of realistic phrases through every profile twice (formatting is
  required to be idempotent on its own output — history replay and any
  "preview this phrase" UI depend on it): the `literally` escape hatch only
  protected one word after itself, so a two-word command like `literally fat
  arrow` still had its second word (`arrow`) reinterpreted as `->`; a
  resolved identifier (`userName`) got re-capitalized into `UserName` if
  already-formatted text was re-parsed; `Doc::parse` used `split_whitespace`,
  which silently discarded the literal newlines that "new line"/"new
  paragraph" produce on a second pass; and force-lowercase profiles
  corrupted a surviving `SCREAMING_SNAKE_CASE` constant into `mAX_SIZE`
  instead of leaving it alone.
- **The Flow Bar overlay had permissions it never used and must never use.**
  It shared the Hub's full capability grant, including
  `core:window:allow-set-focus` — directly contradicting the overlay's one
  architectural invariant, that it can never take focus. Nothing currently
  misuses this, but any future or compromised dependency running in that
  webview could have called `getCurrentWindow().setFocus()` on itself and it
  would have been permitted. Split into its own capability file granting
  only what `Overlay.tsx` actually calls.
- **The clipboard-restore decision after a paste couldn't tell "succeeded" from
  "failed, and the caller is now using the clipboard as a safety net"** — both
  end up with the same text sitting on the clipboard, and the old check only
  asked "is the clipboard still ours," which is true either way. Refactored
  into a single, unit-tested decision (`should_restore`) that also checks
  whether the paste actually sent.
- **A failed clipboard-paste recorded the wrong text.** When injection fell back
  to leaving text on the clipboard, history and the `Outcome::ClipboardFallback`
  payload recorded the *raw* ASR transcript instead of the *formatted* text —
  the thing that was actually placed on the clipboard. History could disagree
  with what `Ctrl+V` actually pastes.
- **A muted/disconnected microphone was recorded as "transcription failed."**
  `AudioFailed` (capture never even reached the transcriber) was mapped to the
  same `Outcome::AsrFailed` as a genuine model failure, so a dead microphone
  and a broken sidecar were indistinguishable in history. Split into its own
  `Outcome::CaptureFailed`, threaded through the Rust↔TypeScript wire contract.
- **The engine could report "idle" while still transcribing a queued session.**
  Holding the hotkey again while a previous utterance was still being
  processed queues a second session; if that second session ended as a
  fat-finger tap, silence, or a capture failure, the engine unconditionally
  announced `Idle` — even though the first session was still actively in
  flight. Now only announced once the whole pipeline is genuinely empty.
- **A NaN or negative silence threshold silently disabled muted-mic
  detection.** TOML happily parses `nan`/`-nan` float literals, and
  `rms < silence_rms` is `false` for every possible `rms` when the threshold
  is NaN (or negative, since RMS is never negative) — completely and silently
  turning off the check with no error anywhere. Config validation now rejects
  both.
- A stale, incorrect `tauri_build::build()` assumption meant a fresh checkout
  (a new contributor, or CI, neither of which have ever run the sidecar
  freeze step) would fail the very first `cargo test`/`clippy`/`check` it
  ever ran — `bundle.resources` is validated against the filesystem
  unconditionally, not just for an actual `tauri build`. This had been
  invisible all session because the frozen sidecar already existed locally
  from manual testing. `build.rs` now creates an empty placeholder for
  ordinary dev builds, but still hard-fails a real release build if the
  frozen sidecar is genuinely missing.
- **A failed short dictation left nothing on the clipboard at all.** Only the
  long-text/clipboard-paste path had a fallback that left text behind for
  `Ctrl+V` on failure; the short-text path (`send_unicode`, used for the
  overwhelming majority of dictations, since most utterances are well under
  the paste threshold) returned the error directly with no fallback. A failed
  short dictation was silently gone. Both paths now leave text on the
  clipboard on failure.
- **The clipboard fallback for a failed paste was silently undoing itself.**
  When injection can't type text into the target app directly, the text is
  left on the clipboard so `Ctrl+V` still works — but a background thread was
  restoring the user's *previous* clipboard contents exactly 1 second later,
  regardless of whether the paste actually succeeded. That 1-second window
  existed to solve a different race (giving the target app time to read the
  clipboard) and was never long enough for a *person* to notice a failed paste
  and react to it. Worse, the restore fired even down the known-failure path,
  undoing the exact fallback the error branch had just set up. Fixed two ways:
  the hold is now 15 seconds, and the restore no longer runs at all when the
  paste chord itself failed to send. A failed injection also now shows a
  notice telling the user to press `Ctrl+V` — previously the only way to
  discover an injection had failed was to spot a non-"delivered" badge in
  History afterward.
- **Injection failures were completely invisible in the log.** `Effect::Inject`
  logged nothing on error — the only trace was the in-app notice and a history
  badge, neither of which helps once the moment has passed and someone is
  trying to reproduce an intermittent failure. It now logs the error directly.
  Also added a check for a real, previously undiagnosable class of bug this
  surfaced: transcription and formatting can take several seconds, so the
  window that was foreground when the user started speaking is not
  guaranteed to still be foreground at injection time. That gap was silent;
  it now logs a warning naming both the app the user spoke into and whatever
  is actually foreground at injection, so a report of "it sometimes doesn't
  paste into app X" is diagnosable from one log line instead of a guess.
- History no longer shows a green "delivered" badge on every single row. It
  is the default, expected outcome for nearly every dictation, so repeating it
  down the whole list was decoration, not information — and diluted the one
  place green is supposed to mean something (the microphone is open) into
  noise. A badge now appears only when an outcome needs attention.
- The sidecar no longer flashes a console window on every start and restart.
- `beforeBuildCommand` resolved its path against whichever directory invoked the
  build, so `tauri build` failed depending on how it was called. Build commands
  now live in the root `package.json`, which `npm run` finds by walking up.
- The design-system sheet documented `--faint` as `#6b6b6b` at 3.4:1 — a value
  the tokens had already moved away from, on the one page whose job is to be
  correct about them.
- Excluding PyAV initially broke `import faster_whisper` outright —
  `faster_whisper/__init__.py` imports it unconditionally, whether or not
  `decode_audio` is ever called. Caught by actually running the frozen binary's
  `probe`, not by reading the diff. Fixed with a stub module installed only when
  the real package is absent, scoped to the frozen entry point.
- `ensure_model`'s request nested `"model"` under a literal `"params"` object,
  which the sidecar's own wire format treats as a param named `params` — the
  model name never actually reached the handler, which silently fell back to
  the sidecar's already-configured model instead. Harmless today only because
  `ensure_model` has never been asked for a different model than the one the
  sidecar was launched with. Found by writing a real end-to-end test against
  the frozen binary rather than trusting the unit tests already in place.

[Unreleased]: https://github.com/adityashelke04/OpenVoice/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/adityashelke04/OpenVoice/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/adityashelke04/OpenVoice/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/adityashelke04/OpenVoice/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/adityashelke04/OpenVoice/releases/tag/v0.1.0
