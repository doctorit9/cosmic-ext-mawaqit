# Mawaqit (مواقيت) — Prayer Times Applet for COSMIC

A COSMIC panel applet showing Islamic prayer times: the next prayer + live
countdown in the panel, and a popup with the full day's schedule, Hijri date,
and quick settings. Inspired by
[salemsayed/omaprayers](https://github.com/salemsayed/omaprayers) (Omarchy bar
widget), rebuilt as a native COSMIC applet in Rust.

- **Offline calculation** via the [`salah`](https://crates.io/crates/salah)
  crate (a Rust port of Batoul Apps' Adhan library — the same astronomical
  lineage as `omaprayers`' `Engine.js`, which ports `adhan-js`).
- **AlAdhan-compatible method IDs**, so times can be compared 1:1 with
  `omaprayers` and [AlAdhan](https://aladhan.com/prayer-times-api).
- **Bilingual labels** (English + Arabic prayer names out of the box; full
  Arabic RTL UI via fluent i18n).
- **Notifications & Adhan audio**: per-prayer toggles, customizable pre-prayer
  reminder (minutes before, message template with `{prayer}`/`{minutes}`/
  `{time}`/`{city}` placeholders), authentic Adhan audio with 7 famous
  reciters (Makkah, Madinah, Al-Aqsa, Mishary Alafasy, Abdul Basit, Muhammad Refaat,
  Mustafa Ismail), custom sound file selection (`.mp3`, `.ogg`, `.wav`),
  volume slider, instant mute/stop banner, and automatic offline caching.
- **No telemetry, no accounts.**

## Validation

Against the `omaprayers` AlAdhan snapshot corpus (`cairo-egypt-2026-10`,
2026-10-01, Egyptian method, Shafi):

| Prayer | Mawaqit (`salah`) | AlAdhan | Δ |
|--------|-------------------|---------|---|
| Fajr | 05:22 | 05:22 | 0 |
| Sunrise | 06:48 | 06:48 | 0 |
| Dhuhr | 12:46 | 12:45 | +1 (official params include Dhuhr+1; AlAdhan omits it) |
| Asr | 16:08 | 16:09 | −1 (PrayTimes-vs-Adhan astronomy band, documented −5…+3) |
| Maghrib | 18:41 | 18:41 | 0 |
| Isha | 19:58 | 19:58 | 0 |

All within the ±1 minute success criterion. Run the engine tests without
building the GUI:

```sh
cargo test prayer
```

## Calculation methods

IDs match AlAdhan/`omaprayers`. Core methods map directly to `salah`
presets; regional methods without a `salah` preset (Jafari, Gulf, France,
Russia, …) use equivalent custom angles. Methods with built-in minute
offsets in `omaprayers` that `salah` does not model (Morocco Dhuhr/Maghrib
+5, Diyanet's full offset set, Jordan Maghrib +5, Lisbon Isha 77′) fall back
to the closest angle match — residual is a few minutes at most. Per-prayer
tuning is on the roadmap.

## Build & run

Requires a COSMIC development environment (Wayland libs etc. — this builds on
Fedora COSMIC out of the box). `just` recipes mirror other COSMIC applets
(`casey/just`):

```sh
just build-release   # cargo build --release
just run             # build + run the applet (needs a COSMIC session)
just check           # clippy, pedantic
just install         # install binary + desktop/metainfo + icon to $prefix
just test            # cargo test (prayer engine)
```

Without `just`, plain `cargo build` / `cargo test` work.

`libcosmic` is pinned to `master` via git. If you hit API errors after a
dependency update (e.g. missing `theme::Svg` variants), run
`cargo update -p libcosmic` to re-resolve against current master before
reporting a build failure.

## Status

- [x] Milestone 1 — scaffold, offline times, popup schedule (Cairo default)
- [x] Milestone 2 — live 1s countdown, next-prayer panel label (horizontal or
  stacked/vertical, countdown or full clock time), method/madhab/12-24h
  settings persisted via `cosmic-config`
- [x] In-app city & location presets (Cairo, Makkah, Madinah, Jerusalem, Istanbul, Dubai, London, NYC, etc.)
- [x] Hero Next/Current prayer card with live countdown pill & status indicators
- [x] Milestone 3 — per-prayer minute adjustments + custom calculation angles
  (Fajr/Isha angle, Isha interval after Maghrib)
- [ ] Milestone 4 — GeoClue2 automatic location + manual fallback
- [x] Milestone 5 — notifications & authentic Adhan audio: 7 authentic Adhan
  reciters (Makkah, Madinah, Al-Aqsa, Alafasy, Abdul Basit, Refaat, Mustafa Ismail),
  custom audio file support, volume slider, quick mute & live Stop button,
  automatic CDN download and offline caching.
- [x] Milestone 6 — full Arabic/English i18n + RTL
- [x] Milestone 7 — icon set (`resources/icon.svg`), packaging

## License

MIT. Prayer astronomy lineage: Batoul Apps Adhan (MIT) → adhan-js (MIT) →
`salah` (MIT) / `omaprayers` Engine.js (MIT).
