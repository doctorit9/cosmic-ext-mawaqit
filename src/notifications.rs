// SPDX-License-Identifier: MIT

//! Desktop notifications, customizable reminder templates, and sound alerts.

use crate::config::{AdhanVoice, NotificationSettings};
use crate::prayer::PrayerKind;
use chrono::{DateTime, NaiveDate};
use chrono_tz::Tz;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Dynamic values available inside custom notification message templates.
#[derive(Debug, Clone, Copy)]
pub struct TemplateContext<'a> {
    pub prayer_name: &'a str,
    pub minutes: u8,
    pub time_str: &'a str,
    pub city: &'a str,
}

/// Render a user-defined template (or `fallback` when empty) by substituting
/// `{prayer}`, `{minutes}`, `{time}`, and `{city}` placeholders.
#[must_use]
pub fn render_template(template: &str, fallback: &str, ctx: &TemplateContext<'_>) -> String {
    let source = if template.trim().is_empty() {
        fallback
    } else {
        template
    };
    source
        .replace("{prayer}", ctx.prayer_name)
        .replace("{minutes}", &ctx.minutes.to_string())
        .replace("{time}", ctx.time_str)
        .replace("{city}", ctx.city)
}

/// An alert event ready to be dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertEvent {
    PreReminder {
        kind: PrayerKind,
        at: DateTime<Tz>,
        remaining_minutes: u8,
    },
    Adhan {
        kind: PrayerKind,
        at: DateTime<Tz>,
    },
}

/// Evaluate whether a pre-prayer reminder or Adhan notification should fire right now,
/// avoiding duplicate notifications for the same `(date, prayer)` pair.
#[must_use]
pub fn evaluate_alert(
    settings: &NotificationSettings,
    active: Option<(PrayerKind, DateTime<Tz>, i64)>,
    next: Option<(PrayerKind, DateTime<Tz>, i64)>,
    last_adhan_notified: Option<(NaiveDate, PrayerKind)>,
    last_pre_notified: Option<(NaiveDate, PrayerKind)>,
) -> Option<AlertEvent> {
    if settings.muted {
        return None;
    }

    // 1. Check Adhan alert (prayer just started within the last 60 seconds).
    if settings.adhan_alert_enabled
        && let Some((kind, at, elapsed)) = active
        && kind.is_prayer()
        && settings.is_prayer_enabled(kind)
        && (0..60).contains(&elapsed)
    {
        let key = (at.date_naive(), kind);
        if last_adhan_notified != Some(key) {
            return Some(AlertEvent::Adhan { kind, at });
        }
    }

    // 2. Check Pre-prayer reminder (within a 60-second window starting at `pre_reminder_minutes`).
    if settings.pre_reminder_enabled
        && settings.pre_reminder_minutes > 0
        && let Some((kind, at, secs)) = next
        && kind.is_prayer()
        && settings.is_prayer_enabled(kind)
    {
        let lead_mins = settings.pre_reminder_minutes.clamp(1, 60);
        let lead_secs = i64::from(lead_mins) * 60;
        let window_floor = (lead_secs - 60).max(0);
        if secs > window_floor && secs <= lead_secs {
            let key = (at.date_naive(), kind);
            if last_pre_notified != Some(key) {
                let remaining_minutes = ((secs + 59) / 60).clamp(1, 60) as u8;
                return Some(AlertEvent::PreReminder {
                    kind,
                    at,
                    remaining_minutes,
                });
            }
        }
    }

    None
}

static PLAYING_PID: AtomicU32 = AtomicU32::new(0);
static IS_PLAYING: AtomicBool = AtomicBool::new(false);

/// Check if an Adhan or notification sound is currently playing.
#[must_use]
pub fn is_audio_playing() -> bool {
    IS_PLAYING.load(Ordering::SeqCst)
}

/// Stop any currently active audio playback process.
pub fn stop_audio() {
    let pid = PLAYING_PID.swap(0, Ordering::SeqCst);
    IS_PLAYING.store(false, Ordering::SeqCst);
    if pid > 0 {
        // Send SIGTERM directly via POSIX kill syscall (avoids spawning /bin/kill)
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
}

/// Path to local audio cache directory: `$XDG_DATA_HOME/mawaqit/sounds` or `~/.local/share/mawaqit/sounds`.
#[must_use]
pub fn sounds_dir() -> PathBuf {
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME")
        && !data_home.is_empty()
    {
        return PathBuf::from(data_home).join("mawaqit").join("sounds");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("mawaqit")
            .join("sounds");
    }
    std::env::temp_dir().join("mawaqit-sounds")
}

/// Check whether the audio file for a given voice is cached locally on disk.
#[must_use]
pub fn is_voice_cached(voice: AdhanVoice) -> bool {
    match voice.filename() {
        Some(file) => sounds_dir().join(file).exists(),
        None => true,
    }
}

/// Download voice MP3 if not already cached.
pub async fn ensure_voice_cached(voice: AdhanVoice) -> Result<PathBuf, String> {
    let filename = voice
        .filename()
        .ok_or_else(|| "Voice has no downloadable file".to_string())?;
    let dir = sounds_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Failed to create sound directory: {e}"))?;
    let dest = dir.join(filename);
    if dest.exists()
        && tokio::fs::metadata(&dest)
            .await
            .map(|m| m.len())
            .unwrap_or(0)
            > 10_000
    {
        return Ok(dest);
    }

    let url = voice
        .cdn_url()
        .ok_or_else(|| "Voice has no download URL".to_string())?;

    let tmp_dest = dir.join(format!("{filename}.tmp.{}", std::process::id()));
    let output = tokio::process::Command::new("curl")
        .args(["-s", "-f", "-L", "--max-time", "30", "-o"])
        .arg(&tmp_dest)
        .arg(url)
        .output()
        .await
        .map_err(|e| format!("curl download failed: {e}"))?;

    if !output.status.success() {
        let _ = tokio::fs::remove_file(&tmp_dest).await;
        return Err(format!(
            "Download failed with status: {:?}",
            output.status.code()
        ));
    }

    tokio::fs::rename(&tmp_dest, &dest)
        .await
        .map_err(|e| format!("Failed to write sound cache: {e}"))?;
    Ok(dest)
}

/// Play audio file or bell sound based on user settings.
pub async fn play_voice_sound(
    voice: AdhanVoice,
    custom_path: &str,
    volume: u8,
) -> Result<(), String> {
    stop_audio();
    if volume == 0 {
        return Ok(());
    }

    match voice {
        AdhanVoice::SystemBell => play_bell_sound(volume).await,
        AdhanVoice::CustomFile => {
            let path = Path::new(custom_path);
            let metadata = tokio::fs::metadata(path)
                .await
                .map_err(|e| format!("Custom sound file cannot be read: {e}"))?;
            if !metadata.is_file() {
                return Err("Custom sound path is not a regular file".to_string());
            }
            let path_str = path
                .to_str()
                .ok_or_else(|| "Invalid sound path".to_string())?
                .to_string();
            tokio::task::spawn_blocking(move || play_file_blocking(&path_str, volume))
                .await
                .map_err(|e| e.to_string())?
        }
        preset => match ensure_voice_cached(preset).await {
            Ok(path) => {
                let path_str = path
                    .to_str()
                    .ok_or_else(|| "Invalid sound path".to_string())?
                    .to_string();
                tokio::task::spawn_blocking(move || play_file_blocking(&path_str, volume))
                    .await
                    .map_err(|e| e.to_string())?
            }
            Err(err) => {
                let _ = play_bell_sound(volume).await;
                Err(format!("Could not download Adhan audio ({err}); played chime fallback."))
            }
        },
    }
}

fn play_file_blocking(path_str: &str, volume: u8) -> Result<(), String> {
    let path = Path::new(path_str);
    let metadata = std::fs::metadata(path).map_err(|e| format!("Cannot read file: {e}"))?;
    if !metadata.is_file() {
        return Err("Sound path is not a regular file".to_string());
    }

    let vol_pct = volume.min(100);
    let vol_str = format!("--volume={vol_pct}");

    // 1. Try mpv (supports mp3, volume flag, quiet, headless)
    // Pass "--" before path_str to prevent option injection
    if let Ok(mut child) = std::process::Command::new("mpv")
        .args(["--no-video", "--really-quiet", &vol_str, "--", path_str])
        .spawn()
    {
        let id = child.id();
        PLAYING_PID.store(id, Ordering::SeqCst);
        IS_PLAYING.store(true, Ordering::SeqCst);
        let _ = child.wait();
        let _ = PLAYING_PID.compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst);
        if PLAYING_PID.load(Ordering::SeqCst) == 0 {
            IS_PLAYING.store(false, Ordering::SeqCst);
        }
        return Ok(());
    }

    // 2. Try pw-play (PipeWire native)
    let pw_vol = format!("{:.2}", f32::from(vol_pct) / 100.0);
    if let Ok(mut child) = std::process::Command::new("pw-play")
        .args(["--volume", &pw_vol, "--", path_str])
        .spawn()
    {
        let id = child.id();
        PLAYING_PID.store(id, Ordering::SeqCst);
        IS_PLAYING.store(true, Ordering::SeqCst);
        let _ = child.wait();
        let _ = PLAYING_PID.compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst);
        if PLAYING_PID.load(Ordering::SeqCst) == 0 {
            IS_PLAYING.store(false, Ordering::SeqCst);
        }
        return Ok(());
    }

    // 3. Try paplay (PulseAudio)
    let pa_vol = u32::from(vol_pct) * 65536 / 100;
    if let Ok(mut child) = std::process::Command::new("paplay")
        .args(["--volume", &pa_vol.to_string(), "--", path_str])
        .spawn()
    {
        let id = child.id();
        PLAYING_PID.store(id, Ordering::SeqCst);
        IS_PLAYING.store(true, Ordering::SeqCst);
        let _ = child.wait();
        let _ = PLAYING_PID.compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst);
        if PLAYING_PID.load(Ordering::SeqCst) == 0 {
            IS_PLAYING.store(false, Ordering::SeqCst);
        }
        return Ok(());
    }

    Err(String::from("No supported audio player (mpv, pw-play, paplay) could be spawned"))
}

/// Send a desktop notification over `org.freedesktop.Notifications` and optionally
/// play an alert sound or Adhan recitation.
///
/// Returns `Ok(None)` when both notification and optional sound succeed,
/// `Ok(Some(warning))` when the desktop notification succeeded but audio playback failed,
/// or `Err(error)` when D-Bus notification delivery failed.
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_notification(
    app_id: &str,
    summary: String,
    body: String,
    urgent: bool,
    sound_enabled: bool,
    volume: u8,
    voice: AdhanVoice,
    custom_path: String,
    is_adhan: bool,
) -> Result<Option<String>, String> {
    let conn = zbus::Connection::session()
        .await
        .map_err(|e| format!("D-Bus session error: {e}"))?;

    let mut hints: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
    hints.insert(
        "urgency",
        zbus::zvariant::Value::U8(if urgent { 2 } else { 1 }),
    );
    hints.insert("category", zbus::zvariant::Value::from("alarm"));
    if sound_enabled && volume > 0 {
        hints.insert("sound-name", zbus::zvariant::Value::from("bell"));
    }

    let expire_timeout: i32 = if urgent { 12_000 } else { 8_000 };
    let actions: Vec<&str> = Vec::new();

    conn.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &(
            "Mawaqit",
            0u32,
            app_id,
            summary.as_str(),
            body.as_str(),
            actions,
            hints,
            expire_timeout,
        ),
    )
    .await
    .map_err(|e| format!("Notification service error: {e}"))?;

    if sound_enabled && volume > 0 {
        let warning = if is_adhan {
            play_voice_sound(voice, &custom_path, volume).await.err()
        } else {
            play_bell_sound(volume).await.err()
        };
        return Ok(warning);
    }

    Ok(None)
}

async fn play_bell_sound(volume: u8) -> Result<(), String> {
    tokio::task::spawn_blocking(move || play_bell_sound_blocking(volume))
        .await
        .map_err(|e| e.to_string())?
}

fn play_bell_sound_blocking(volume: u8) -> Result<(), String> {
    // 1. Try canberra-gtk-play (standard on Fedora / COSMIC / GNOME)
    if let Ok(status) = std::process::Command::new("canberra-gtk-play")
        .args(["-i", "bell", "-d", "Mawaqit prayer alert"])
        .status()
        && status.success()
    {
        return Ok(());
    }

    // 2. Fallback to PipeWire/PulseAudio players if a standard freedesktop bell sound exists
    let sound_candidates = [
        "/usr/share/sounds/freedesktop/stereo/bell.oga",
        "/usr/share/sounds/freedesktop/stereo/complete.oga",
        "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga",
    ];
    let sound_path = sound_candidates.iter().find(|p| Path::new(p).exists());

    if let Some(path) = sound_path {
        let pw_vol = format!("{:.2}", f32::from(volume.min(100)) / 100.0);
        if let Ok(status) = std::process::Command::new("pw-play")
            .args(["--volume", &pw_vol, path])
            .status()
            && status.success()
        {
            return Ok(());
        }

        let pa_vol = u32::from(volume.min(100)) * 65536 / 100;
        if let Ok(status) = std::process::Command::new("paplay")
            .args(["--volume", &pa_vol.to_string(), path])
            .status()
            && status.success()
        {
            return Ok(());
        }
    }

    Err(String::from("No supported sound player or bell file found"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn cairo_tz() -> Tz {
        "Africa/Cairo".parse().unwrap()
    }

    #[test]
    fn template_renders_custom_and_fallback() {
        let ctx = TemplateContext {
            prayer_name: "العصر",
            minutes: 10,
            time_str: "16:09",
            city: "القاهرة",
        };
        assert_eq!(
            render_template(
                "باقي {minutes} دقيقة على {prayer} ({time}) في {city}",
                "fallback",
                &ctx
            ),
            "باقي 10 دقيقة على العصر (16:09) في القاهرة"
        );
        assert_eq!(
            render_template("   ", "10m until Asr", &ctx),
            "10m until Asr"
        );
    }

    #[test]
    fn pre_reminder_triggers_once_within_window() {
        let tz = cairo_tz();
        let at = tz.with_ymd_and_hms(2026, 10, 1, 16, 9, 0).unwrap();
        let settings = NotificationSettings {
            pre_reminder_enabled: true,
            pre_reminder_minutes: 10,
            ..Default::default()
        };

        // 11 minutes left -> too early
        assert_eq!(
            evaluate_alert(&settings, None, Some((PrayerKind::Asr, at, 660)), None, None),
            None
        );

        // Exactly 10 minutes left -> triggers
        let event = evaluate_alert(&settings, None, Some((PrayerKind::Asr, at, 600)), None, None);
        assert_eq!(
            event,
            Some(AlertEvent::PreReminder {
                kind: PrayerKind::Asr,
                at,
                remaining_minutes: 10,
            })
        );

        // Already notified for this date & prayer -> deduplicated
        let dup = evaluate_alert(
            &settings,
            None,
            Some((PrayerKind::Asr, at, 595)),
            None,
            Some((at.date_naive(), PrayerKind::Asr)),
        );
        assert_eq!(dup, None);
    }

    #[test]
    fn sunrise_never_triggers_pre_reminder() {
        let tz = cairo_tz();
        let at = tz.with_ymd_and_hms(2026, 10, 1, 6, 48, 0).unwrap();
        let settings = NotificationSettings::default();
        assert_eq!(
            evaluate_alert(
                &settings,
                None,
                Some((PrayerKind::Sunrise, at, 600)),
                None,
                None
            ),
            None
        );
    }

    #[test]
    fn muted_suppresses_all_alerts() {
        let tz = cairo_tz();
        let at = tz.with_ymd_and_hms(2026, 10, 1, 16, 9, 0).unwrap();
        let settings = NotificationSettings {
            muted: true,
            ..Default::default()
        };
        assert_eq!(
            evaluate_alert(
                &settings,
                Some((PrayerKind::Asr, at, 5)),
                Some((PrayerKind::Maghrib, at, 600)),
                None,
                None
            ),
            None
        );
    }

    #[test]
    fn adhan_alert_triggers_at_prayer_start() {
        let tz = cairo_tz();
        let at = tz.with_ymd_and_hms(2026, 10, 1, 16, 9, 0).unwrap();
        let settings = NotificationSettings::default();
        assert_eq!(
            evaluate_alert(
                &settings,
                Some((PrayerKind::Asr, at, 12)),
                None,
                None,
                None
            ),
            Some(AlertEvent::Adhan {
                kind: PrayerKind::Asr,
                at
            })
        );
    }

    #[test]
    fn adhan_voices_have_valid_metadata() {
        assert_eq!(AdhanVoice::ALL.len(), 9);
        for voice in AdhanVoice::ALL {
            assert!(!voice.id().is_empty());
            match voice {
                AdhanVoice::SystemBell | AdhanVoice::CustomFile => {
                    assert!(voice.filename().is_none());
                    assert!(voice.cdn_url().is_none());
                }
                preset => {
                    assert!(preset.filename().is_some());
                    assert!(preset.cdn_url().unwrap().starts_with("https://"));
                }
            }
        }
    }

    #[test]
    fn stop_audio_resets_is_playing() {
        IS_PLAYING.store(true, Ordering::SeqCst);
        assert!(is_audio_playing());
        stop_audio();
        assert!(!is_audio_playing());
    }
}
