# Voice Input Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Click a mic button in the composer, speak, click again, and the words land as text in the draft, ready to edit and send.

**Architecture:** A new `aether-voice` crate with no GPUI dependency records the mic with `cpal`, shapes the audio to 16 kHz mono, and transcribes it on this machine with whisper.cpp through `whisper-rs`. A small state machine on `ChatApp` drives it and inserts the result at the composer cursor. The speech model is a 141 MiB file downloaded once, after the user agrees.

**Tech Stack:** Rust, GPUI (`gpui-kit` 0.7.3), `cpal` 0.18, `whisper-rs` 0.16 (`metal` feature on macOS), `reqwest` and `sha2` for the model download.

**Spec:** None. The user skipped the design questions, so every decision below is the planner's call, listed in "Decisions" for them to overrule.

## Decisions

| Decision | Choice | Why, and what it costs |
|---|---|---|
| Engine | Local Whisper, `ggml-base.en` | Works on macOS and Linux (an Arch package ships now), audio never leaves the machine, one permission prompt, and it can be tested end to end from a file. Costs a C++ build and a download. |
| Rejected: Apple `SFSpeechRecognizer` | Not used | Shortest diff and shows words live, but macOS only, needs a second permission prompt, fails when Siri and Dictation are off, and can't run in tests. One third-party benchmark puts its error rate at 9%, unverified. `SpeechAnalyzer` is macOS 26+ and its samples are all Swift. |
| Rejected: cloud API | Not used | Needs a new key and sends audio away. |
| Live text | None in v1 | Text appears when you stop, with a "Transcribing" state between. |
| Language | English only in v1 | `base.en` is smaller and more accurate for English. Other languages need a settings choice and a second model. |
| Start and stop | Toggle: click or `secondary-shift-m` | No hold-to-talk, and no voice-triggered send. |

## Global Constraints

- No audio or transcript leaves the machine. The only network call is the model download, and only after the user clicks Download.
- Model URL: `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin`
- Model SHA-256: `a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002`
- Model size: `147_964_211` bytes. Model path: `aether_core::config_dir()` + `models/ggml-base.en.bin`.
- Recording cap: 600 seconds, then it stops and transcribes on its own.
- Test layout: `crates/<c>/tests/unit/...`, wired with `#[cfg(test)] #[path = "..."] mod tests;` at the bottom of the source file.
- Linux has no compile evidence from CI, because `cargo fmt --check` fails before the build step there. Treat the Linux path as untested until someone builds it.
- Baselines: `cargo fmt --all -- --check` stays at 33 hunks with none in touched code, and `make test` passes. It skips the `hackernews::client` flake. An intermittent `Detected activity on thread Some("tokio-rt-worker")` failure already appears in the `aether` binary, see the `test-and-ci-baseline` memory.
- Comments only for a genuinely hard why, in terse fragments. Run Python through `uv run`.
- **Ask the user before every commit.** No `Co-Authored-By` lines. Commit steps below give the message to use once approved.

## Review Focus

1. **Microphone access denied on macOS.** The stream can open and deliver pure silence. Expect a message pointing at System Settings, not an empty insert. Pinned in Task 1 (`heard`) and Task 4 (`no_speech_copy`).
2. **Silence or room noise.** Whisper invents text like `[BLANK_AUDIO]` or `Thank you.` on silence. Expect nothing inserted. Pinned in Task 1 (`heard`, `clean_transcript`).
3. **Switching chats during recording or transcribing.** Expect the text in the chat where dictation began, with the visible composer untouched. Pinned in Task 4.
4. **Typing during transcription, with emoji or non-ASCII before the cursor.** Expect typed text kept and the transcript at the current cursor. Pinned in Task 4.
5. **Download cut off or corrupted.** Expect no half-written model to be used, and a retry to work. Pinned in Task 2.

---

### Task 1: Audio shaping and transcription

**Files:**
- Create: `crates/voice/Cargo.toml`, `crates/voice/src/lib.rs`, `crates/voice/src/audio.rs`, `crates/voice/src/transcribe.rs`
- Modify: `Cargo.toml` (add `aether-voice = { path = "crates/voice" }` and `cpal`, `whisper-rs`, `sha2` to workspace dependencies)
- Test: `crates/voice/tests/unit/audio.rs`, `crates/voice/tests/unit/transcribe.rs`

**Interfaces:**
- Produces:
  - `pub const SAMPLE_RATE: u32 = 16_000`
  - `pub fn to_speech_rate(samples: &[f32], channels: u16, rate: u32) -> Vec<f32>`
  - `pub enum Heard { Muted, Silence, Speech }` and `pub fn heard(samples: &[f32]) -> Heard`
  - `pub fn clean_transcript<'a>(segments: impl IntoIterator<Item = &'a str>) -> String`
  - `pub fn transcribe(model: &Path, samples: &[f32]) -> Result<String, VoiceError>`
  - `pub enum VoiceError` with `Display`, holding a user-readable message

Dependencies: `whisper-rs = "0.16"`, plus `features = ["metal"]` under `[target.'cfg(target_os = "macos")'.dependencies]`. Dev-dependency `hound = "3"` for the file test.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn stereo_48k_becomes_mono_16k() {
    let stereo: Vec<f32> = [0.5, -0.5].repeat(48_000);
    let out = to_speech_rate(&stereo, 2, 48_000);
    assert_eq!(out.len(), 16_000);
    assert!(out.iter().all(|s| s.abs() < 1e-6), "channels average out");
    assert_eq!(to_speech_rate(&[0.25; 16_000], 1, 16_000), vec![0.25; 16_000]);
    assert!(to_speech_rate(&[], 2, 44_100).is_empty());
}

#[test]
fn heard_tells_a_dead_mic_from_a_quiet_room_from_speech() {
    assert_eq!(heard(&[0.; 16_000]), Heard::Muted);
    assert_eq!(heard(&[]), Heard::Muted);
    assert_eq!(heard(&[0.0005; 16_000]), Heard::Silence);
    assert_eq!(heard(&[0.1; 16_000]), Heard::Speech);
}

#[test]
fn transcripts_drop_whisper_silence_markers() {
    assert_eq!(clean_transcript([" fix the bug", " in login."]), "fix the bug in login.");
    assert_eq!(clean_transcript(["[BLANK_AUDIO]", " (silence)", " [Music]"]), "");
    assert_eq!(clean_transcript([]), "");
}

#[test]
fn a_bad_model_file_is_an_error_not_a_crash() {
    let file = std::env::temp_dir().join("aether-voice-bad-model.bin");
    std::fs::write(&file, b"not a model").unwrap();
    assert!(transcribe(&file, &[0.1; 16_000]).is_err());
    std::fs::remove_file(file).unwrap();
}

#[test]
#[ignore = "needs the model at AETHER_VOICE_MODEL; macOS for say and afconvert"]
fn real_speech_becomes_text() {
    // say -o a.aiff "fix the login bug"; afconvert -f WAVE -d LEF32@16000 -c 1 a.aiff a.wav
    // read with hound, transcribe, assert lowercase result contains "login"
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p aether-voice`
Expected: fails to compile, functions not defined.

- [ ] **Step 3: Implement**

`to_speech_rate`: average channels, then box-average source samples into each 16 kHz slot, with a `ponytail:` comment naming the ceiling (no proper low-pass, use `rubato` if accuracy suffers). `heard`: peak of exactly 0 is `Muted`, RMS below `SILENCE_RMS = 0.002` is `Silence`. `clean_transcript`: trim, drop segments that are wholly `[...]` or `(...)`, join with one space. `transcribe`: `WhisperContext::new_with_params`, `create_state`, `WhisperState::full` with greedy `best_of: 1`, language `"en"` and printing off, threads `min(8, available_parallelism)`. Call `install_logging_hooks` once so nothing prints to stderr. Load the model per call, with a `ponytail:` comment (memory held only during use, cache if load time shows).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p aether-voice`
Expected: all pass, one ignored. Note that a clean build compiles whisper.cpp, which needs `cmake` and `clang`. Both are already in `scripts/setup-linux.sh` and `PKGBUILD`.

- [ ] **Step 5: Commit** `feat(voice): transcribe audio locally with whisper`, including `Cargo.lock`

### Task 2: Model install

**Files:**
- Create: `crates/voice/src/model.rs`
- Test: `crates/voice/tests/unit/model.rs`

**Interfaces:**
- Produces:
  - `pub const MODEL_BYTES: u64 = 147_964_211`
  - `pub fn model_path() -> Option<PathBuf>`
  - `pub fn installed() -> bool`, true when the file exists at exactly `MODEL_BYTES`
  - `pub fn download(cancel: &AtomicBool, progress: impl FnMut(u64)) -> Result<PathBuf, VoiceError>`, blocking, called from a background task
  - `fn install_verified(part: &Path, dest: &Path, sha256: &str) -> Result<(), VoiceError>`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn only_a_matching_file_gets_installed() {
    let dir = std::env::temp_dir().join(format!("aether-voice-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (part, dest) = (dir.join("m.part"), dir.join("m.bin"));
    std::fs::write(&part, b"hello").unwrap();
    let hello = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
    assert!(install_verified(&part, &dest, "00").is_err());
    assert!(!dest.exists() && !part.exists(), "bad download is removed");
    std::fs::write(&part, b"hello").unwrap();
    install_verified(&part, &dest, hello).unwrap();
    assert!(dest.exists() && !part.exists());
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p aether-voice model`
Expected: fails to compile.

- [ ] **Step 3: Implement**

`download`: create `models/`, stream to `ggml-base.en.bin.part` using `Response::chunk()` on a local tokio runtime (workspace `reqwest` has no `stream` or `blocking` feature; see `crates/app/src/hackernews/client.rs` for the runtime pattern). Check `cancel` each chunk. On any failure or cancel, remove the part file. Finish with `install_verified` against the pinned hash, which renames into place. Do not call `download` from tests.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p aether-voice`
Expected: all pass.

- [ ] **Step 5: Commit** `feat(voice): download and verify the speech model`

### Task 3: Microphone capture and permissions

**Files:**
- Create: `crates/voice/src/capture.rs`, `packaging/macos/Aether.entitlements`
- Modify: `scripts/build.py:43-66`, `crates/app/src/notify.rs:210-225` (`DEV_PLIST`), `PKGBUILD:7` (add `alsa-lib`)
- Test: `scripts/tests/test_packaging.py`, `crates/app/tests/unit/notify.rs`

**Interfaces:**
- Produces:
  - `pub struct Recording`
  - `Recording::start() -> Result<Recording, VoiceError>`
  - `Recording::failure(&self) -> Option<String>`, set when the device is lost mid-recording
  - `Recording::finish(self) -> Vec<f32>`, stops and returns mono 16 kHz samples
  - Dropping a `Recording` cancels it and discards the audio

Usage string for both plists: `Aether turns your speech into text in the composer. Audio stays on this device.`

- [ ] **Step 1: Write the failing tests**

In `test_macos_bundle`, after the existing assertions:
```python
self.assertIn("NSMicrophoneUsageDescription", info)
entitlements = subprocess.run(["codesign", "-d", "--entitlements", "-", app],
                              capture_output=True, text=True).stdout
self.assertIn("com.apple.security.device.audio-input", entitlements)
```
In `crates/app/tests/unit/notify.rs`, under the same `cfg` as `DEV_PLIST`:
```rust
#[test]
fn dev_bundle_can_ask_for_the_microphone() {
    assert!(DEV_PLIST.contains("NSMicrophoneUsageDescription"));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `uv run -m unittest discover -s scripts/tests` and `cargo test -p aether --bin aether dev_bundle_can_ask`
Expected: both fail on the missing key.

- [ ] **Step 3: Implement the permissions**

Add the key to both plists. Create the entitlements file with `com.apple.security.device.audio-input` set true, and pass `--entitlements` to `codesign` in `build.py` on every signing path. Under hardened runtime a missing entitlement means no prompt and a silent mic. A bundled app without the usage string is killed by macOS on mic access.

- [ ] **Step 4: Implement `Recording`**

Own the `cpal` stream on a dedicated thread, because its `Send` status isn't guaranteed across platforms. `start` reports the open result over a channel, then the thread parks on a stop channel. Use `default_input_device`, `default_input_config`, and `build_input_stream` for `F32`, `I16` and `U16`; other formats return `VoiceError`. The callback appends to a shared buffer and stops appending at the 600 second cap. The error callback records `failure`. `finish` joins the thread and returns `to_speech_rate` of the buffer. No device returns the error `No microphone found`.

- [ ] **Step 5: Run the tests and a real capture**

Run: `uv run -m unittest discover -s scripts/tests`, `make test`, `make app` (proves whisper.cpp links in a signed bundle at the deployment target), and a throwaway `#[ignore]` test that records 2 seconds from a terminal and prints `heard`.
Expected: tests pass. The throwaway prints a `Heard` value without crashing; a terminal without mic access may print `Muted`. Delete the throwaway.

- [ ] **Step 6: Commit** `feat(voice): capture the microphone with permissions`

### Task 4: Dictation state and text insertion

**Files:**
- Create: `crates/app/src/app/composer/voice.rs`
- Modify: `crates/app/Cargo.toml`, `crates/app/src/app/mod.rs` (field), `crates/app/src/app/new.rs:606` (init), `crates/app/src/app/chat/send.rs:102` (guard)
- Test: `crates/app/tests/unit/app/composer/voice.rs`, `crates/app/tests/unit/app/watch.rs` (harness tests, it already has `app_with_two_chats` and `as_agents`)

**Interfaces:**
- Consumes: Tasks 1 to 3.
- Produces:
  - `pub(crate) enum VoiceState { Idle, NeedsModel { chat: i64 }, Downloading { chat: i64, done: u64, cancel: Arc<AtomicBool> }, Recording { chat: i64, recording: Recording, started: Instant }, Transcribing { chat: i64 } }`, with `Default` as `Idle`
  - `ChatApp::voice: VoiceState`, `ChatApp::voice_note: Option<(i64, SharedString)>`
  - `pub(crate) fn dictated(draft: &str, cursor: usize, text: &str) -> String`, the text with any spaces it needs to sit at byte offset `cursor`
  - `pub(crate) fn no_speech_copy(heard: Heard) -> Option<&'static str>`
  - `ChatApp::toggle_voice(&mut self, window, cx)`, `download_voice_model(&mut self, window, cx)`, `stop_voice(&mut self, window, cx)`, `cancel_voice(&mut self, cx)`
  - `ChatApp::deliver_transcript(&mut self, chat: i64, text: &str, window, cx)`
  - `ChatApp::voice_active(&self) -> bool`, true outside `Idle` and `NeedsModel`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn dictation_spaces_itself_against_its_neighbours() {
    assert_eq!(dictated("", 0, "hello"), "hello");
    assert_eq!(dictated("fix", 3, "the bug"), " the bug");
    assert_eq!(dictated("fix ", 4, "the bug"), "the bug");
    assert_eq!(dictated("ab", 1, "x"), " x ");
    assert_eq!(dictated("ok.", 2, "x"), " x");
    assert_eq!(dictated("héllo🙂", "héllo🙂".len(), "x"), " x");
}

#[test]
fn only_real_speech_goes_to_the_model() {
    assert!(no_speech_copy(Heard::Muted).unwrap().contains("System Settings"));
    assert!(no_speech_copy(Heard::Silence).is_some());
    assert_eq!(no_speech_copy(Heard::Speech), None);
}
```
In `watch.rs`, using `app_with_two_chats`:
```rust
// a_transcript_lands_in_the_chat_it_started_in
//   active = chat A, composer "typed"; app.deliver_transcript(B.id, "hello", window, cx)
//   assert composer still "typed", B.draft contains "hello"
// a_transcript_joins_the_draft_at_the_cursor
//   composer "🙂 fix", cursor at end; deliver to active chat
//   assert composer value == "🙂 fix hello", chat.draft matches
// sending_waits_for_dictation
//   composer "hi"; app.voice = VoiceState::Transcribing { chat: active id }; app.send(...)
//   assert no new message, composer still "hi"
// a_note_clears_when_dictation_starts_over: voice_note set, toggle from NeedsModel, note gone
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p aether --bin aether voice`
Expected: fails to compile.

- [ ] **Step 3: Implement the pure functions and delivery**

`deliver_transcript` follows `crates/app/src/app/worktree/setup.rs:183-191`: for the active chat insert into the composer with `replace_text_in_range(None, ...)`, which replaces any selection. For another chat, append to `chats[ix].draft` with one space. Call it inside `in_session(chat, ...)`, as `bindings.rs:33` does. A chat deleted meanwhile drops the text and clears the state.

- [ ] **Step 4: Implement the state machine**

Every `toggle_voice` clears `voice_note` first. `toggle_voice`, from `Idle`: if `!installed()` go to `NeedsModel`, else `Recording::start`, with errors going to `voice_note`. From `NeedsModel`: back to `Idle`. From `Recording`: `stop_voice`. From `Downloading` or `Transcribing`: do nothing. `stop_voice`: `finish` on a background task; on `no_speech_copy` set the note and go `Idle`, else go `Transcribing`, run `transcribe` in `cx.background_spawn`, and deliver through `cx.spawn_in`. A failed load removes the model file and notes the error, so the next try offers the download again. While recording, a once-a-second `spawn_in` tick calls `cx.notify()`, stops at 600 seconds, and stops with a note on `Recording::failure`. A finished download goes `Idle` with the note `Speech model ready`, and does not open the mic on its own.

- [ ] **Step 5: Guard send**

Early return in `send` when `voice_active()`, keeping the draft.

- [ ] **Step 6: Run the tests**

Run: `make test`
Expected: all pass. `Recording` is never constructed in tests.

- [ ] **Step 7: Commit** `feat(composer): dictate into the draft`

### Task 5: Composer UI

**Files:**
- Modify: `crates/ui/src/chat/composer.rs`, `crates/app/src/app/composer/mod.rs:200-486`, `crates/app/src/app/composer/voice.rs`, `crates/app/src/actions.rs`
- Test: `crates/app/tests/unit/app/composer/voice.rs`

**Interfaces:**
- Consumes: Task 4.
- Produces:
  - `Composer::recording(mut self, recording: bool) -> Self`, a `danger_text(cx).opacity(0.5)` border that takes precedence over `shell` and `attention`
  - `pub(crate) fn voice_copy(state: &VoiceState, seconds: u64) -> Option<String>`
  - Action `ToggleVoice`, bound to `secondary-shift-m` in `COMPOSER_CONTEXT`

Copy, fixed:
- `NeedsModel`: `Dictation needs a speech model (141 MB). It runs on this device.` with buttons `Download` and `Cancel`
- `Downloading`: `Downloading speech model, {percent}%` with `Cancel`
- `Recording`: `Listening {m:ss}` with `Done` and `Cancel`
- `Transcribing`: `Transcribing…` with a spinner
- `Heard::Muted`: `Aether can't hear your microphone. Check System Settings > Privacy & Security > Microphone.`
- `Heard::Silence`: `Didn't catch anything. Try again a little closer to the mic.`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn the_banner_says_what_dictation_is_doing() {
    let chat = 1;
    assert_eq!(voice_copy(&VoiceState::Idle, 0), None);
    assert!(voice_copy(&VoiceState::NeedsModel { chat }, 0).unwrap().contains("141 MB"));
    let cancel = Arc::new(AtomicBool::new(false));
    let half = VoiceState::Downloading { chat, done: MODEL_BYTES / 2, cancel };
    assert!(voice_copy(&half, 0).unwrap().contains("50%"));
    assert_eq!(voice_copy(&VoiceState::Transcribing { chat }, 0).unwrap(), "Transcribing…");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p aether --bin aether the_banner_says`
Expected: fails to compile.

- [ ] **Step 3: Implement**

Add `voice_copy`. Add a mic `icon_chip("voice", recording, cx)` as the first `.trailing` in `composer_card`, using `Lucide::Mic` (already in `scripts/icon-packs.json`), or `Lucide::Square` while recording, with tooltip `Dictate` or `Stop dictating`, and `disabled(switching)`. Add `voice_banner` to the `header` stack and to the `.when(...)` condition above it, styled like the `editing` banner at `composer/mod.rs:215`, shown only when the state's `chat` or the note's chat is the active chat. Add `.recording(...)`. Add `voice_active()` to the `SendButton` `.disabled`, with the reason `Finish dictating first`. Bind `ToggleVoice` and add its `on_action` beside `EditDraft` in `render_composer`. Command mode hides `trailing`, so the banner's `Done` is the way to stop there.

- [ ] **Step 4: Run the tests and render it**

Run: `make test`, then render the `Recording` copy and `NeedsModel` banner offscreen per the `headless-render-check` memory. `Recording` can't be built without a device there, so render `NeedsModel`, `Downloading` and `Transcribing`.
Expected: tests pass, card height stays steady, banner reads cleanly in Dracula and Light.

- [ ] **Step 5: Commit** `feat(composer): add mic button and dictation banner`

## Desktop checks after Task 5

These need a real mic and can't run here, so the user does them on a rebuilt app:

1. The first click offers the download, and Download finishes with `Speech model ready`.
2. The first recording shows one macOS permission prompt, and a rebuild doesn't ask again.
3. Speaking a sentence with `cargo`, `GPUI` and `refactor` in it. If jargon is poor, try `small.en` (465 MiB) before anything else.
4. Deny the mic in System Settings and confirm the `Muted` message shows.
5. Unplug a USB mic mid-recording.
6. How long the wait after `Done` feels for a 30-second dictation.
