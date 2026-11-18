use crate::config::{AdhanVoice, Config, FontFamilyChoice, PanelLayout};
use crate::fl;
use crate::notifications::{self, AlertEvent, TemplateContext};
use crate::prayer::{self, DailyTimes, PrayerKind};
use chrono::Utc;
use chrono_tz::Tz;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::{Alignment, Length, Limits, Subscription, window::Id};
use cosmic::prelude::*;
use cosmic::theme;
use cosmic::widget::{self, segmented_button};
use futures::SinkExt;
use std::path::Path;
use std::time::Duration;

/// RDNN application identifier. Shared by the `Application` impl below and
/// anywhere else the config context is opened (trait assoc consts are not
/// in scope in inherent methods).
const APP_ID: &str = "io.github.doctorit.mawaqit";

/// Method ids offered in the settings dropdown (AlAdhan-compatible).
const METHOD_IDS: [u8; 24] = [
    5, 3, 4, 2, 1, 16, 10, 9, 11, 15, 13, 7, 0, 8, 12, 14, 17, 18, 19, 20, 21, 22, 23, 99,
];

/// Panel label arrangements offered in settings.
const PANEL_LAYOUTS: [(PanelLayout, &str); 3] = [
    (PanelLayout::Auto, "Auto (Responsive)"),
    (PanelLayout::Horizontal, "Horizontal"),
    (PanelLayout::Stacked, "Stacked"),
];

/// View mode in the popup window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Schedule,
    Settings,
}

/// Sub-tabs inside the Settings view to avoid overwhelming flat scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Alerts,
    Calculation,
}

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// The popup id.
    popup: Option<Id>,
    /// Currently displayed view in the popup.
    view_mode: ViewMode,
    /// Segmented control model for settings tabs.
    settings_tabs: segmented_button::SingleSelectModel,
    /// Configuration data that persists between application runs.
    config: Config,
    /// Today's schedule in the configured timezone.
    today: Option<DailyTimes>,
    /// Tomorrow's schedule (so the countdown rolls over after Isha).
    tomorrow: Option<DailyTimes>,
    /// Hijri date strings (English, Arabic).
    hijri: Option<(String, String)>,
    /// Next upcoming event, its scheduled time, and seconds remaining.
    next: Option<(PrayerKind, chrono::DateTime<Tz>, i64)>,
    /// Currently ongoing prayer (if within the 15-minute start window).
    active: Option<(PrayerKind, chrono::DateTime<Tz>, i64)>,
    /// Resolved timezone (falls back to UTC on parse failure).
    tz: Tz,
    /// Deduplication marker for pre-prayer reminders.
    last_pre_notified: Option<(chrono::NaiveDate, PrayerKind)>,
    /// Deduplication marker for Adhan-time notifications.
    last_adhan_notified: Option<(chrono::NaiveDate, PrayerKind)>,
    /// Inline status feedback after testing/dispatching an alert: `(is_ok, message)`.
    notification_feedback: Option<(bool, String)>,
    /// Whether Adhan or test audio is actively playing.
    is_playing_audio: bool,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    Tick,
    UpdateConfig(Box<Config>),
    SetViewMode(ViewMode),
    SelectSettingsTab(segmented_button::Entity),
    OpenAlertsSettings,
    SetCity(usize),
    SetMethod(usize),
    SetHanafi(bool),
    Set24h(bool),
    SetLayout(usize),
    SetShowTime(bool),
    SetCompactMode(bool),
    SetShowCelestialHorizon(bool),
    SetShowHeroCard(bool),
    SetShowHijriDate(bool),
    // Typography settings
    SetFontFamily(usize),
    SetCustomFontFamily(String),
    SetPanelFontSize(u8),
    SetPopupFontScale(u8),
    // Advanced settings messages
    SetUseCustomAngles(bool),
    SetFajrAngle(String),
    SetIshaAngle(String),
    SetIshaInterval(String),
    SetFajrAdjustment(i8),
    SetSunriseAdjustment(i8),
    SetDhuhrAdjustment(i8),
    SetAsrAdjustment(i8),
    SetMaghribAdjustment(i8),
    SetIshaAdjustment(i8),
    ResetAllAdjustments,
    // Notification & customizable reminder settings
    ToggleMuted,
    SetMuted(bool),
    TogglePrayerNotification(PrayerKind),
    SetAllPrayersNotification(bool),
    SetPreReminderEnabled(bool),
    SetPreReminderMinutes(u8),
    SetPreReminderTemplate(String),
    ResetPreReminderTemplate,
    SetAdhanAlertEnabled(bool),
    SetAdhanTemplate(String),
    ResetAdhanTemplate,
    #[allow(dead_code)]
    SetNotificationFajr(bool),
    #[allow(dead_code)]
    SetNotificationDhuhr(bool),
    #[allow(dead_code)]
    SetNotificationAsr(bool),
    #[allow(dead_code)]
    SetNotificationMaghrib(bool),
    #[allow(dead_code)]
    SetNotificationIsha(bool),
    SetSoundEnabled(bool),
    SetVolume(u8),
    SetAdhanVoice(usize),
    SetCustomAdhanPath(String),
    PlayTestAdhan,
    StopAudio,
    TestNotification,
    NotificationDispatched(Result<Option<String>, String>),
}

fn resolve_tz(name: &str) -> Tz {
    name.parse::<Tz>().unwrap_or(chrono_tz::UTC)
}

impl AppModel {
    /// Scales base popup font size according to `config.popup_font_scale` (70% - 160%).
    fn scale_size(&self, base: f32) -> f32 {
        let scale = (self.config.popup_font_scale as f32).clamp(70.0, 160.0) / 100.0;
        (base * scale).round().max(8.0)
    }

    /// Recompute today's + tomorrow's schedule and the Hijri date.
    fn refresh_schedule(&mut self) {
        self.tz = resolve_tz(&self.config.timezone);
        let today_date = Utc::now().with_timezone(&self.tz).date_naive();
        let tomorrow_date = today_date.succ_opt().unwrap_or(today_date);

        let custom_angles = if self.config.use_custom_angles {
            Some((
                self.config.custom_angles.fajr_angle,
                self.config.custom_angles.isha_angle,
                self.config.custom_angles.isha_interval,
            ))
        } else {
            None
        };

        let (fajr_angle, isha_angle, isha_interval) = match custom_angles {
            Some((fa, ia, ii)) => (fa, ia, ii),
            None => (None, None, None),
        };

        self.today = prayer::compute_day_with_adjustments(
            today_date,
            self.config.latitude,
            self.config.longitude,
            self.tz,
            self.config.method_id,
            self.config.hanafi,
            fajr_angle,
            isha_angle,
            isha_interval,
            Some(self.config.prayer_adjustments),
        );
        self.tomorrow = prayer::compute_day_with_adjustments(
            tomorrow_date,
            self.config.latitude,
            self.config.longitude,
            self.tz,
            self.config.method_id,
            self.config.hanafi,
            fajr_angle,
            isha_angle,
            isha_interval,
            Some(self.config.prayer_adjustments),
        );
        self.hijri = prayer::hijri_for_gregorian(today_date);
        self.refresh_next(Utc::now().with_timezone(&self.tz));
    }

    /// Recompute next-prayer countdown and active prayer status;
    /// refresh the whole schedule if the civil date rolled over.
    fn refresh_next(&mut self, now: chrono::DateTime<Tz>) {
        let today_date = now.date_naive();
        let stale = self.today.as_ref().is_none_or(|t| t.date != today_date);
        if stale {
            self.refresh_schedule();
            return;
        }
        self.active = self
            .today
            .as_ref()
            .and_then(|today| prayer::active_prayer(today, now));
        self.next = match (&self.today, &self.tomorrow) {
            (Some(today), Some(tomorrow)) => {
                let (kind, at, _) = prayer::next_prayer(today, tomorrow, now);
                let secs = (at - now).num_seconds().max(0);
                Some((kind, at, secs))
            }
            _ => None,
        };
    }

    fn save_config(&self) {
        if let Ok(ctx) = cosmic_config::Config::new(APP_ID, Config::VERSION) {
            let _ = self.config.write_entry(&ctx);
        }
    }

    /// Panel label: name plus countdown ("Asr 42m") or full time ("Asr 16:36").
    /// If an obligatory prayer just started, shows that it is active ("now").
    fn panel_label(&self, is_horizontal_panel: bool) -> String {
        let is_stacked = self.config.panel_layout.is_stacked_in(is_horizontal_panel);
        if let Some((kind, at, _)) = self.active {
            let detail = if self.config.show_prayer_time {
                prayer::format_time(at, self.config.use_24h)
            } else {
                fl!("now")
            };
            return prayer::panel_label(kind, &detail, is_stacked);
        }
        match &self.next {
            Some((kind, at, secs)) => {
                let detail = prayer::panel_detail(
                    *secs,
                    *at,
                    self.config.show_prayer_time,
                    self.config.use_24h,
                );
                prayer::panel_label(*kind, &detail, is_stacked)
            }
            None => fl!("app-title"),
        }
    }

    fn gregorian_label(&self) -> String {
        let now = Utc::now().with_timezone(&self.tz);
        now.format("%A, %-d %B %Y").to_string()
    }

    fn localized_prayer_name(kind: PrayerKind) -> String {
        match kind {
            PrayerKind::Fajr => fl!("fajr"),
            PrayerKind::Sunrise => fl!("sunrise"),
            PrayerKind::Dhuhr => fl!("dhuhr"),
            PrayerKind::Asr => fl!("asr"),
            PrayerKind::Maghrib => fl!("maghrib"),
            PrayerKind::Isha => fl!("isha"),
        }
    }

    fn render_pre_reminder_body(&self, kind: PrayerKind, minutes: u8, time_str: &str) -> String {
        let prayer_name = Self::localized_prayer_name(kind);
        let fallback = fl!(
            "pre-reminder-default-body",
            prayer = prayer_name.as_str(),
            minutes = minutes,
            time = time_str
        );
        let ctx = TemplateContext {
            prayer_name: &prayer_name,
            minutes,
            time_str,
            city: &self.config.location_label,
        };
        notifications::render_template(
            &self.config.notification_settings.pre_reminder_template,
            &fallback,
            &ctx,
        )
    }

    fn render_adhan_body(&self, kind: PrayerKind, time_str: &str) -> String {
        let prayer_name = Self::localized_prayer_name(kind);
        let fallback = fl!(
            "adhan-default-body",
            prayer = prayer_name.as_str(),
            city = self.config.location_label.as_str(),
            time = time_str
        );
        let ctx = TemplateContext {
            prayer_name: &prayer_name,
            minutes: 0,
            time_str,
            city: &self.config.location_label,
        };
        notifications::render_template(
            &self.config.notification_settings.adhan_template,
            &fallback,
            &ctx,
        )
    }

    #[must_use]
    fn localized_voice_name(voice: AdhanVoice) -> String {
        match voice {
            AdhanVoice::Makkah => fl!("adhan-voice-makkah"),
            AdhanVoice::Madinah => fl!("adhan-voice-madinah"),
            AdhanVoice::AlAqsa => fl!("adhan-voice-alaqsa"),
            AdhanVoice::Alafasy => fl!("adhan-voice-alafasy"),
            AdhanVoice::AbdulBasit => fl!("adhan-voice-abdulbasit"),
            AdhanVoice::MuhammadRefaat => fl!("adhan-voice-refaat"),
            AdhanVoice::MustafaIsmail => fl!("adhan-voice-ismail"),
            AdhanVoice::SystemBell => fl!("adhan-voice-bell"),
            AdhanVoice::CustomFile => fl!("adhan-voice-custom"),
        }
    }

    fn spawn_notification_task(
        &self,
        summary: String,
        body: String,
        urgent: bool,
        is_adhan: bool,
    ) -> Task<cosmic::Action<Message>> {
        let sound_enabled = self.config.notification_settings.sound_enabled;
        let volume = self.config.notification_settings.volume;
        let voice = self.config.notification_settings.adhan_voice;
        let custom_path = self.config.notification_settings.custom_adhan_path.clone();
        cosmic::task::future(async move {
            let result = notifications::dispatch_notification(
                APP_ID,
                summary,
                body,
                urgent,
                sound_enabled,
                volume,
                voice,
                custom_path,
                is_adhan,
            )
            .await;
            cosmic::Action::App(Message::NotificationDispatched(result))
        })
    }

    fn check_and_dispatch_alerts(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(event) = notifications::evaluate_alert(
            &self.config.notification_settings,
            self.active,
            self.next,
            self.last_adhan_notified,
            self.last_pre_notified,
        ) else {
            return Task::none();
        };

        match event {
            AlertEvent::Adhan { kind, at } => {
                self.last_adhan_notified = Some((at.date_naive(), kind));
                let prayer_name = Self::localized_prayer_name(kind);
                let time_str = prayer::format_time(at, self.config.use_24h);
                let summary = fl!("adhan-summary", prayer = prayer_name.as_str());
                let body = self.render_adhan_body(kind, &time_str);
                self.spawn_notification_task(summary, body, true, true)
            }
            AlertEvent::PreReminder {
                kind,
                at,
                remaining_minutes,
            } => {
                self.last_pre_notified = Some((at.date_naive(), kind));
                let prayer_name = Self::localized_prayer_name(kind);
                let time_str = prayer::format_time(at, self.config.use_24h);
                let summary = fl!("pre-reminder-summary", prayer = prayer_name.as_str());
                let body = self.render_pre_reminder_body(kind, remaining_minutes, &time_str);
                self.spawn_notification_task(summary, body, false, false)
            }
        }
    }

    fn view_header(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        let hijri_text = match &self.hijri {
            Some((en, ar)) => format!("{en} • {ar}"),
            None => String::new(),
        };

        let muted = self.config.notification_settings.muted;
        let mute_icon = if muted {
            "audio-volume-muted-symbolic"
        } else {
            "audio-volume-high-symbolic"
        };
        let mute_btn = widget::button::icon(widget::icon::from_name(mute_icon))
            .on_press(Message::ToggleMuted)
            .tooltip(if muted {
                fl!("unmute-all")
            } else {
                fl!("mute-all")
            });

        let settings_btn = match self.view_mode {
            ViewMode::Schedule => widget::button::icon(widget::icon::from_name("preferences-system-symbolic"))
                .on_press(Message::SetViewMode(ViewMode::Settings))
                .tooltip(fl!("settings")),
            ViewMode::Settings => widget::button::icon(widget::icon::from_name("go-previous-symbolic"))
                .on_press(Message::SetViewMode(ViewMode::Schedule))
                .tooltip(fl!("back-to-schedule")),
        };

        let brand_emblem = widget::icon::from_name("night-light-symbolic")
            .size(20)
            .icon()
            .class(theme::Svg::custom(|t| widget::svg::Style {
                color: Some(t.cosmic().accent.base.into()),
            }));

        let brand_title = widget::row(vec![
            brand_emblem.into(),
            widget::text::title3("Mawaqit")
                .font(self.config.font_bold())
                .size(self.scale_size(18.0))
                .into(),
            widget::text::title4("مواقيت")
                .font(self.config.font_bold())
                .size(self.scale_size(16.0))
                .class(theme::Text::Accent)
                .into(),
        ])
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center);

        let top_row = widget::row(vec![
            brand_title.into(),
            widget::space::horizontal().width(Length::Fill).into(),
            widget::row(vec![mute_btn.into(), settings_btn.into()])
                .spacing(spacing.space_xxs)
                .align_y(Alignment::Center)
                .into(),
        ])
        .align_y(Alignment::Center);

        let mut meta_items: Vec<Element<'_, Message>> = vec![
            widget::row(vec![
                widget::icon::from_name("mark-location-symbolic")
                    .size(12)
                    .icon()
                    .class(theme::Svg::custom(|t| widget::svg::Style {
                        color: Some(t.cosmic().accent.base.into()),
                    }))
                    .into(),
                widget::text::caption(&self.config.location_label)
                    .font(self.config.font())
                    .size(self.scale_size(11.0))
                    .into(),
            ])
            .spacing(spacing.space_xxxs)
            .align_y(Alignment::Center)
            .into(),
            widget::text::caption("•")
                .font(self.config.font())
                .size(self.scale_size(11.0))
                .into(),
            widget::text::caption(self.gregorian_label())
                .font(self.config.font())
                .size(self.scale_size(11.0))
                .into(),
        ];
        if self.config.show_hijri_date && !hijri_text.is_empty() {
            meta_items.push(
                widget::text::caption("•")
                    .font(self.config.font())
                    .size(self.scale_size(11.0))
                    .into(),
            );
            meta_items.push(
                widget::row(vec![
                    widget::icon::from_name("weather-clear-night-symbolic")
                        .size(12)
                        .icon()
                        .class(theme::Svg::custom(|t| widget::svg::Style {
                            color: Some(t.cosmic().accent.base.into()),
                        }))
                        .into(),
                    widget::text::caption(hijri_text)
                        .font(self.config.font())
                        .size(self.scale_size(11.0))
                        .class(theme::Text::Accent)
                        .into(),
                ])
                .spacing(spacing.space_xxxs)
                .align_y(Alignment::Center)
                .into(),
            );
        }
        let meta_row = widget::row(meta_items)
            .spacing(spacing.space_xxs)
            .align_y(Alignment::Center);

        widget::column(vec![top_row.into(), meta_row.into()])
            .spacing(spacing.space_xxxs)
            .into()
    }

    fn view_celestial_horizon(&self) -> Option<Element<'_, Message>> {
        let today = self.today.as_ref()?;
        let spacing = cosmic::theme::active().cosmic().spacing;
        let now = Utc::now().with_timezone(&self.tz);
        let next_kind = self.next.as_ref().map(|(k, _, _)| *k);
        let active_kind = self.active.as_ref().map(|(k, _, _)| *k);

        let mut milestone_cols: Vec<Element<'_, Message>> = Vec::new();

        for (i, kind) in PrayerKind::ALL.iter().copied().enumerate() {
            if i > 0 {
                milestone_cols.push(
                    widget::space::horizontal()
                        .width(Length::Fill)
                        .into(),
                );
            }

            let event_time = today.time(kind);
            let is_active = Some(kind) == active_kind;
            let is_next = Some(kind) == next_kind && self.active.is_none();
            let is_passed = prayer::is_passed(event_time, now) && !is_active;

            let icon_widget: Element<'_, Message> = if is_active || is_next {
                widget::icon::from_name(kind.celestial_icon())
                    .size(18)
                    .icon()
                    .class(theme::Svg::custom(|t| widget::svg::Style {
                        color: Some(t.cosmic().accent.base.into()),
                    }))
                    .into()
            } else {
                widget::icon::from_name(kind.celestial_icon())
                    .size(18)
                    .into()
            };

            let label_widget: Element<'_, Message> = if is_active || is_next {
                widget::text(kind.short_ar())
                    .font(cosmic::font::bold())
                    .class(theme::Text::Accent)
                    .into()
            } else {
                widget::text::caption(kind.short_ar()).into()
            };

            let marker: Element<'_, Message> = if is_active {
                widget::text("●").class(theme::Text::Accent).into()
            } else if is_next {
                widget::text("◉").class(theme::Text::Accent).into()
            } else if is_passed {
                widget::icon::from_name("object-select-symbolic")
                    .size(12)
                    .into()
            } else {
                widget::text::caption("·").into()
            };

            let time_str = prayer::format_time(event_time, self.config.use_24h);
            let time_widget: Element<'_, Message> = if is_active || is_next {
                widget::text::caption(time_str)
                    .font(cosmic::font::bold())
                    .class(theme::Text::Accent)
                    .into()
            } else {
                widget::text::caption(time_str).into()
            };

            let node = widget::column(vec![
                icon_widget,
                label_widget,
                marker,
                time_widget,
            ])
            .align_x(Alignment::Center)
            .spacing(2);

            milestone_cols.push(node.into());
        }

        let track_row = widget::row(milestone_cols)
            .align_y(Alignment::Center)
            .width(Length::Fill);

        Some(
            widget::container(track_row)
                .class(theme::Container::Card)
                .padding([spacing.space_xs, spacing.space_s])
                .width(Length::Fill)
                .into(),
        )
    }

    fn view_hero_card(&self) -> Option<Element<'_, Message>> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        if let Some((active_kind, active_at, _)) = self.active {
            let card_header = widget::row(vec![
                widget::row(vec![
                    widget::icon::from_name("media-playback-start-symbolic")
                        .size(14)
                        .icon()
                        .class(theme::Svg::custom(|t| widget::svg::Style {
                            color: Some(t.cosmic().accent.base.into()),
                        }))
                        .into(),
                    widget::text(fl!("active-now"))
                        .font(self.config.font_bold())
                        .size(self.scale_size(13.0))
                        .class(theme::Text::Accent)
                        .into(),
                ])
                .spacing(spacing.space_xxs)
                .align_y(Alignment::Center)
                .into(),
                widget::space::horizontal().width(Length::Fill).into(),
                widget::text::caption(fl!("ongoing"))
                    .font(self.config.font())
                    .size(self.scale_size(12.0))
                    .class(theme::Text::Accent)
                    .into(),
            ])
            .align_y(Alignment::Center);

            let left_info = widget::column(vec![
                widget::row(vec![
                    widget::text::title2(active_kind.name_en())
                        .font(self.config.font_bold())
                        .size(self.scale_size(20.0))
                        .into(),
                    widget::text::title3(active_kind.name_ar())
                        .font(self.config.font_bold())
                        .size(self.scale_size(18.0))
                        .class(theme::Text::Accent)
                        .into(),
                ])
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center)
                .into(),
                widget::text::caption(format!(
                    "{} • {}",
                    active_kind.description_en(),
                    active_kind.description_ar()
                ))
                .font(self.config.font())
                .size(self.scale_size(12.0))
                .into(),
            ])
            .spacing(spacing.space_xxxs);

            let right_info = widget::column(vec![
                widget::text::title2(prayer::format_time(active_at, self.config.use_24h))
                    .size(self.scale_size(22.0))
                    .into(),
                widget::text::caption(fl!("now"))
                    .font(self.config.font())
                    .size(self.scale_size(12.0))
                    .class(theme::Text::Accent)
                    .into(),
            ])
            .align_x(Alignment::End)
            .spacing(spacing.space_xxxs);

            let card_main = widget::row(vec![
                left_info.into(),
                widget::space::horizontal().width(Length::Fill).into(),
                right_info.into(),
            ])
            .align_y(Alignment::Center);

            Some(
                widget::container(
                    widget::column(vec![card_header.into(), card_main.into()])
                        .spacing(spacing.space_xs),
                )
                .class(theme::Container::Card)
                .padding(spacing.space_xs)
                .width(Length::Fill)
                .into(),
            )
        } else if let Some((next_kind, next_at, secs)) = self.next {
            let countdown_str = if secs == 0 {
                fl!("now")
            } else {
                prayer::format_countdown(secs)
            };

            let alert_badge = if self.config.notification_settings.muted {
                fl!("muted-badge")
            } else if next_kind.is_prayer()
                && self.config.notification_settings.is_prayer_enabled(next_kind)
                && self.config.notification_settings.pre_reminder_enabled
            {
                fl!(
                    "pre-reminder-badge",
                    minutes = self.config.notification_settings.pre_reminder_minutes
                )
            } else {
                String::new()
            };

            let card_header = widget::row(vec![
                widget::row(vec![
                    widget::icon::from_name(next_kind.celestial_icon())
                        .size(16)
                        .icon()
                        .class(theme::Svg::custom(|t| widget::svg::Style {
                            color: Some(t.cosmic().accent.base.into()),
                        }))
                        .into(),
                    widget::text(fl!("next-prayer"))
                        .class(theme::Text::Accent)
                        .font(self.config.font_bold())
                        .size(self.scale_size(13.0))
                        .into(),
                ])
                .spacing(spacing.space_xxs)
                .align_y(Alignment::Center)
                .into(),
                widget::space::horizontal().width(Length::Fill).into(),
                widget::text::title4(countdown_str)
                    .font(self.config.font_bold())
                    .size(self.scale_size(16.0))
                    .class(theme::Text::Accent)
                    .into(),
            ])
            .align_y(Alignment::Center);

            let mut right_col: Vec<Element<'_, Message>> = vec![
                widget::text::title2(prayer::format_time(next_at, self.config.use_24h))
                    .size(self.scale_size(22.0))
                    .into(),
            ];
            if !alert_badge.is_empty() {
                right_col.push(
                    widget::button::text(alert_badge)
                        .on_press(Message::OpenAlertsSettings)
                        .padding(0)
                        .into(),
                );
            }

            let left_info = widget::column(vec![
                widget::row(vec![
                    widget::text::title2(next_kind.name_en())
                        .font(self.config.font_bold())
                        .size(self.scale_size(20.0))
                        .into(),
                    widget::text::title3(next_kind.name_ar())
                        .font(self.config.font_bold())
                        .size(self.scale_size(18.0))
                        .class(theme::Text::Accent)
                        .into(),
                ])
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center)
                .into(),
                widget::text::caption(format!(
                    "{} • {}",
                    next_kind.description_en(),
                    next_kind.description_ar()
                ))
                .font(self.config.font())
                .size(self.scale_size(12.0))
                .into(),
            ])
            .spacing(spacing.space_xxxs);

            let card_main = widget::row(vec![
                left_info.into(),
                widget::space::horizontal().width(Length::Fill).into(),
                widget::column(right_col)
                    .align_x(Alignment::End)
                    .spacing(spacing.space_xxxs)
                    .into(),
            ])
            .align_y(Alignment::Center);

            Some(
                widget::container(
                    widget::column(vec![card_header.into(), card_main.into()])
                        .spacing(spacing.space_xs),
                )
                .class(theme::Container::Card)
                .padding(spacing.space_xs)
                .width(Length::Fill)
                .into(),
            )
        } else {
            Some(
                widget::container(
                    widget::column(vec![
                        widget::text::body(fl!("schedule-unavailable")).into(),
                        widget::button::standard(fl!("open-settings"))
                            .on_press(Message::SetViewMode(ViewMode::Settings))
                            .into(),
                    ])
                    .spacing(spacing.space_xs),
                )
                .class(theme::Container::Card)
                .padding(spacing.space_xs)
                .width(Length::Fill)
                .into(),
            )
        }
    }

    fn view_schedule(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        let mut schedule = widget::list_column();
        if let Some(today) = &self.today {
            let now = Utc::now().with_timezone(&self.tz);
            let next_kind = self.next.as_ref().map(|(k, _, _)| *k);
            let active_kind = self.active.as_ref().map(|(k, _, _)| *k);

            for kind in PrayerKind::ALL {
                let event_time = today.time(kind);
                let is_next = Some(kind) == next_kind && self.active.is_none();
                let is_active = Some(kind) == active_kind;
                let is_passed = prayer::is_passed(event_time, now) && !is_active;

                let icon_widget: Element<'_, Message> = if is_active || is_next {
                    widget::icon::from_name(kind.celestial_icon())
                        .size(16)
                        .icon()
                        .class(theme::Svg::custom(|t| widget::svg::Style {
                            color: Some(t.cosmic().accent.base.into()),
                        }))
                        .into()
                } else {
                    widget::icon::from_name(kind.celestial_icon())
                        .size(16)
                        .into()
                };

                let en_name: Element<'_, Message> = if is_next || is_active {
                    widget::text(kind.name_en())
                        .font(self.config.font_bold())
                        .size(self.scale_size(14.0))
                        .into()
                } else {
                    widget::text(kind.name_en())
                        .font(self.config.font())
                        .size(self.scale_size(14.0))
                        .into()
                };

                let ar_name: Element<'_, Message> = if is_next || is_active {
                    widget::text::caption(kind.name_ar())
                        .font(self.config.font_bold())
                        .size(self.scale_size(12.0))
                        .class(theme::Text::Accent)
                        .into()
                } else {
                    widget::text::caption(kind.name_ar())
                        .font(self.config.font())
                        .size(self.scale_size(12.0))
                        .into()
                };

                let left_col: Element<'_, Message> = if self.config.compact_mode {
                    widget::row(vec![icon_widget, en_name, ar_name])
                        .spacing(spacing.space_xs)
                        .align_y(Alignment::Center)
                        .into()
                } else {
                    widget::column(vec![
                        widget::row(vec![icon_widget, en_name, ar_name])
                            .spacing(spacing.space_xs)
                            .align_y(Alignment::Center)
                            .into(),
                        widget::text::caption(kind.description_en())
                            .font(self.config.font())
                            .size(self.scale_size(11.0))
                            .into(),
                    ])
                    .spacing(spacing.space_xxxs)
                    .into()
                };

                let mut right_items: Vec<Element<'_, Message>> = Vec::new();
                if !kind.is_prayer() {
                    right_items.push(widget::text::caption(fl!("sunrise")).into());
                } else if is_active {
                    right_items.push(
                        widget::text::caption(fl!("ongoing"))
                            .class(theme::Text::Accent)
                            .into(),
                    );
                } else if is_next {
                    let countdown_str = match self.next {
                        Some((_, _, 0)) => fl!("now"),
                        Some((_, _, s)) => prayer::format_countdown(s),
                        None => String::new(),
                    };
                    right_items.push(
                        widget::text::caption(countdown_str)
                            .class(theme::Text::Accent)
                            .into(),
                    );
                } else if is_passed {
                    right_items.push(widget::text::caption(fl!("passed-status")).into());
                }

                let time_str = prayer::format_time(event_time, self.config.use_24h);
                let time_widget: Element<'_, Message> = if is_next || is_active {
                    widget::text::monotext(time_str)
                        .font(cosmic::font::bold())
                        .size(self.scale_size(13.0))
                        .into()
                } else {
                    widget::text::monotext(time_str)
                        .size(self.scale_size(13.0))
                        .into()
                };
                right_items.push(time_widget);

                if kind.is_prayer() {
                    let alert_on = !self.config.notification_settings.muted
                        && self.config.notification_settings.is_prayer_enabled(kind);
                    let bell_name = if alert_on {
                        "alarm-symbolic"
                    } else {
                        "notifications-disabled-symbolic"
                    };
                    let bell_icon: Element<'_, Message> = if alert_on {
                        widget::icon::from_name(bell_name)
                            .size(16)
                            .icon()
                            .class(theme::Svg::custom(|t| widget::svg::Style {
                                color: Some(t.cosmic().accent.base.into()),
                            }))
                            .into()
                    } else {
                        widget::icon::from_name(bell_name).size(16).into()
                    };
                    right_items.push(bell_icon);
                }

                let right_row = widget::row(right_items)
                    .spacing(spacing.space_xs)
                    .align_y(Alignment::Center);

                let row_padding = if self.config.compact_mode {
                    [spacing.space_xxxs, spacing.space_xs]
                } else {
                    [spacing.space_xxs, spacing.space_xs]
                };

                let row_content = widget::row(vec![
                    left_col,
                    widget::space::horizontal().width(Length::Fill).into(),
                    right_row.into(),
                ])
                .align_y(Alignment::Center)
                .padding(row_padding);

                let mut button_item =
                    widget::list_column::button(row_content).selected(is_next || is_active);
                if kind.is_prayer() {
                    button_item = button_item.on_press(Message::TogglePrayerNotification(kind));
                }
                schedule = schedule.add(button_item);
            }
        }
        schedule.into()
    }

    fn active_settings_tab(&self) -> SettingsTab {
        self.settings_tabs
            .active_data::<SettingsTab>()
            .copied()
            .unwrap_or_default()
    }

    fn view_settings(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;

        let tabs = widget::segmented_control::horizontal(&self.settings_tabs)
            .on_activate(Message::SelectSettingsTab)
            .width(Length::Fill);

        let body: Element<'_, Message> = match self.active_settings_tab() {
            SettingsTab::General => self.view_general_settings(),
            SettingsTab::Alerts => self.view_alerts_settings(),
            SettingsTab::Calculation => self.view_calculation_settings(),
        };

        let scrollable_body = widget::scrollable::vertical(
            widget::container(body).padding([0, spacing.space_xxs]),
        )
        .height(Length::Fixed(480.0))
        .width(Length::Fill);

        widget::column(vec![tabs.into(), scrollable_body.into()])
            .spacing(spacing.space_s)
            .width(Length::Fill)
            .into()
    }

    fn view_general_settings(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        let city_names: Vec<String> = prayer::CITIES
            .iter()
            .map(|c| format!("{} ({})", c.name, c.name_ar))
            .collect();
        let city_selected = prayer::find_city_preset(&self.config.location_label);

        let layout_names = vec![
            fl!("panel-layout-auto"),
            fl!("panel-layout-horizontal"),
            fl!("panel-layout-stacked"),
        ];
        let layout_selected = PANEL_LAYOUTS
            .iter()
            .position(|(layout, _)| *layout == self.config.panel_layout);

        let coords_str = fl!(
            "coordinates-info",
            lat = format!("{:.4}", self.config.latitude),
            lon = format!("{:.4}", self.config.longitude)
        );
        let tz_str = fl!("timezone-info", tz = self.config.timezone.as_str());

        let location_meta_card = widget::container(
            widget::row(vec![
                widget::text::caption(coords_str).into(),
                widget::space::horizontal().width(Length::Fill).into(),
                widget::text::caption(tz_str)
                    .class(theme::Text::Accent)
                    .into(),
            ])
            .align_y(Alignment::Center),
        )
        .class(theme::Container::Card)
        .padding(spacing.space_xs)
        .width(Length::Fill);

        let is_panel_horizontal = self.core.applet.is_horizontal();
        let is_stacked = self.config.panel_layout.is_stacked_in(is_panel_horizontal);
        let preview_label = self.panel_label(is_panel_horizontal);
        let preview_font = self.config.font_bold();
        let preview_size = if self.config.panel_font_size > 0 {
            self.config.panel_font_size as f32
        } else if is_stacked {
            10.5
        } else {
            13.0
        };
        let preview_text_widget: Element<'_, Message> = if is_stacked {
            widget::text(preview_label)
                .size(preview_size)
                .font(preview_font)
                .line_height(1.1)
                .align_x(Alignment::Center)
                .into()
        } else {
            widget::text(preview_label)
                .size(preview_size)
                .font(preview_font)
                .into()
        };

        let preview_card = widget::container(
            widget::column(vec![
                widget::row(vec![
                    widget::text::caption(fl!("panel-preview")).into(),
                    widget::space::horizontal().width(Length::Fill).into(),
                    widget::text::caption("COSMIC Panel").class(theme::Text::Accent).into(),
                ])
                .align_y(Alignment::Center)
                .into(),
                widget::container(
                    widget::row(vec![
                        widget::icon::from_name("night-light-symbolic").size(14).into(),
                        preview_text_widget,
                    ])
                    .spacing(spacing.space_xxs)
                    .align_y(Alignment::Center),
                )
                .class(theme::Container::Card)
                .padding([spacing.space_xxs, spacing.space_s])
                .into(),
            ])
            .spacing(spacing.space_xxs),
        )
        .class(theme::Container::Card)
        .padding(spacing.space_xs)
        .width(Length::Fill);

        widget::column(vec![
            widget::settings::section()
                .title(fl!("section-location-display"))
                .add(widget::settings::item(
                    fl!("city"),
                    widget::dropdown(city_names, city_selected, Message::SetCity),
                ))
                .add(location_meta_card)
                .into(),
            widget::settings::section()
                .title(fl!("panel-layout"))
                .add(widget::settings::item(
                    fl!("use-24h"),
                    widget::toggler(self.config.use_24h).on_toggle(Message::Set24h),
                ))
                .add(widget::settings::item(
                    fl!("panel-layout"),
                    widget::dropdown(layout_names, layout_selected, Message::SetLayout),
                ))
                .add(widget::settings::item(
                    fl!("show-time"),
                    widget::toggler(self.config.show_prayer_time).on_toggle(Message::SetShowTime),
                ))
                .add(preview_card)
                .into(),
            widget::settings::section()
                .title(fl!("section-main-page-customization"))
                .add(widget::settings::item(
                    fl!("compact-mode"),
                    widget::toggler(self.config.compact_mode).on_toggle(Message::SetCompactMode),
                ))
                .add(widget::settings::item(
                    fl!("show-celestial-horizon"),
                    widget::toggler(self.config.show_celestial_horizon)
                        .on_toggle(Message::SetShowCelestialHorizon),
                ))
                .add(widget::settings::item(
                    fl!("show-hero-card"),
                    widget::toggler(self.config.show_hero_card).on_toggle(Message::SetShowHeroCard),
                ))
                .add(widget::settings::item(
                    fl!("show-hijri-date"),
                    widget::toggler(self.config.show_hijri_date)
                        .on_toggle(Message::SetShowHijriDate),
                ))
                .into(),
            {
                let font_names: Vec<String> = FontFamilyChoice::ALL
                    .iter()
                    .map(|f| f.display_name(&self.config.custom_font_family))
                    .collect();
                let font_selected = FontFamilyChoice::ALL
                    .iter()
                    .position(|f| *f == self.config.font_family);

                let mut typography_section = widget::settings::section()
                    .title(fl!("section-typography"))
                    .add(widget::settings::item(
                        fl!("font-family"),
                        widget::dropdown(font_names, font_selected, Message::SetFontFamily),
                    ));

                if self.config.font_family == FontFamilyChoice::Custom {
                    typography_section = typography_section.add(widget::settings::item(
                        fl!("custom-font-name"),
                        widget::text_input(
                            fl!("custom-font-placeholder"),
                            &self.config.custom_font_family,
                        )
                        .on_input(Message::SetCustomFontFamily),
                    ));
                }

                let panel_size_label = if self.config.panel_font_size == 0 {
                    fl!("panel-font-size-auto")
                } else {
                    format!("{} pt", self.config.panel_font_size)
                };

                let popup_scale_label = format!("{}%", self.config.popup_font_scale);

                let font_preview_card = widget::container(
                    widget::column(vec![
                        widget::row(vec![
                            widget::text::caption(fl!("font-preview")).into(),
                            widget::space::horizontal().width(Length::Fill).into(),
                            widget::text::caption(self.config.font_family.family_name(&self.config.custom_font_family))
                                .class(theme::Text::Accent)
                                .into(),
                        ])
                        .align_y(Alignment::Center)
                        .into(),
                        widget::text(fl!("sample-text-ar"))
                            .size(self.scale_size(15.0))
                            .font(self.config.font())
                            .into(),
                        widget::text(fl!("sample-text-en"))
                            .size(self.scale_size(13.0))
                            .font(self.config.font())
                            .into(),
                    ])
                    .spacing(spacing.space_xxs),
                )
                .class(theme::Container::Card)
                .padding(spacing.space_xs)
                .width(Length::Fill);

                typography_section = typography_section
                    .add(widget::settings::item(
                        fl!("panel-font-size"),
                        widget::spin_button(
                            panel_size_label,
                            fl!("panel-font-size"),
                            self.config.panel_font_size,
                            1u8,
                            0u8,
                            24u8,
                            Message::SetPanelFontSize,
                        ),
                    ))
                    .add(widget::settings::item(
                        fl!("popup-font-scale"),
                        widget::spin_button(
                            popup_scale_label,
                            fl!("popup-font-scale"),
                            self.config.popup_font_scale,
                            5u8,
                            75u8,
                            150u8,
                            Message::SetPopupFontScale,
                        ),
                    ))
                    .add(font_preview_card);

                typography_section.into()
            },
        ])
        .spacing(spacing.space_s)
        .into()
    }

    fn view_alerts_settings(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        let ns = &self.config.notification_settings;

        let sample_prayer = self
            .next
            .map(|(k, _, _)| k)
            .filter(|k| k.is_prayer())
            .unwrap_or(PrayerKind::Asr);
        let sample_time = self.next.map_or_else(
            || String::from("16:09"),
            |(_, at, _)| prayer::format_time(at, self.config.use_24h),
        );

        let mut children: Vec<Element<'_, Message>> = Vec::new();

        // 1. Master mute bar
        children.push(
            widget::settings::section()
                .title(fl!("section-sound"))
                .add(widget::settings::item(
                    fl!("mute-all"),
                    widget::toggler(ns.muted).on_toggle(Message::SetMuted),
                ))
                .into(),
        );

        // 2. Prayer Alert Matrix with quick actions
        let mut prayer_rows: Vec<Element<'_, Message>> = Vec::new();
        for kind in [
            PrayerKind::Fajr,
            PrayerKind::Dhuhr,
            PrayerKind::Asr,
            PrayerKind::Maghrib,
            PrayerKind::Isha,
        ] {
            let is_enabled = ns.is_prayer_enabled(kind);
            let icon: Element<'_, Message> = if is_enabled {
                widget::icon::from_name(kind.celestial_icon())
                    .size(16)
                    .icon()
                    .class(theme::Svg::custom(|t| widget::svg::Style {
                        color: Some(t.cosmic().accent.base.into()),
                    }))
                    .into()
            } else {
                widget::icon::from_name(kind.celestial_icon())
                    .size(16)
                    .into()
            };

            let name: Element<'_, Message> = if is_enabled {
                widget::text(kind.name_en()).font(cosmic::font::bold()).into()
            } else {
                widget::text(kind.name_en()).into()
            };
            let name_ar: Element<'_, Message> = widget::text::caption(kind.name_ar()).into();
            let switch: Element<'_, Message> =
                widget::toggler(is_enabled).on_toggle(move |_| Message::TogglePrayerNotification(kind)).into();

            prayer_rows.push(
                widget::row(vec![
                    icon,
                    name,
                    name_ar,
                    widget::space::horizontal().width(Length::Fill).into(),
                    switch,
                ])
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center)
                .padding([spacing.space_xxs, spacing.space_xs])
                .into(),
            );
        }

        let matrix_header = widget::row(vec![
            widget::text::title4(fl!("section-prayers-enabled")).into(),
            widget::space::horizontal().width(Length::Fill).into(),
            widget::button::text(fl!("all-prayers-on"))
                .on_press(Message::SetAllPrayersNotification(true))
                .padding(0)
                .into(),
            widget::text::caption("·").into(),
            widget::button::text(fl!("all-prayers-off"))
                .on_press(Message::SetAllPrayersNotification(false))
                .padding(0)
                .into(),
        ])
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center);

        let prayer_matrix_card = widget::container(
            widget::column(vec![
                matrix_header.into(),
                widget::column(prayer_rows).spacing(spacing.space_xxxs).into(),
            ])
            .spacing(spacing.space_xs),
        )
        .class(theme::Container::Card)
        .padding(spacing.space_xs)
        .width(Length::Fill);

        children.push(prayer_matrix_card.into());

        // 3. Pre-prayer reminder section
        let mut pre_section = widget::settings::section()
            .title(fl!("section-pre-reminder"))
            .add(widget::settings::item(
                fl!("pre-reminder-enabled"),
                widget::toggler(ns.pre_reminder_enabled).on_toggle(Message::SetPreReminderEnabled),
            ));

        if ns.pre_reminder_enabled {
            let mins = ns.pre_reminder_minutes.clamp(1, 60);
            let mins_label = format!("{mins} {}", fl!("minutes-unit"));
            let spin = widget::spin_button(
                mins_label,
                fl!("pre-reminder-minutes"),
                mins,
                1u8,
                1u8,
                60u8,
                Message::SetPreReminderMinutes,
            );
            pre_section = pre_section.add(widget::settings::item(
                fl!("pre-reminder-minutes"),
                spin,
            ));

            // Quick preset pills: 5m, 10m, 15m, 30m
            let presets = [5u8, 10, 15, 30]
                .into_iter()
                .map(|m| {
                    let label = format!("{m}{}", fl!("minutes-unit"));
                    if m == mins {
                        widget::button::suggested(label)
                            .on_press(Message::SetPreReminderMinutes(m))
                            .into()
                    } else {
                        widget::button::standard(label)
                            .on_press(Message::SetPreReminderMinutes(m))
                            .into()
                    }
                })
                .collect::<Vec<Element<'_, Message>>>();

            pre_section = pre_section.add(
                widget::row(presets)
                    .spacing(spacing.space_xxs)
                    .padding([spacing.space_xxs, spacing.space_xs]),
            );

            // Customizable pre-reminder message template + live preview
            let preview_text = self.render_pre_reminder_body(sample_prayer, mins, &sample_time);
            let template_editor = widget::column(vec![
                widget::row(vec![
                    widget::text::caption(fl!("pre-reminder-message")).into(),
                    widget::space::horizontal().width(Length::Fill).into(),
                    widget::button::text(fl!("reset-default"))
                        .on_press(Message::ResetPreReminderTemplate)
                        .padding(0)
                        .into(),
                ])
                .align_y(Alignment::Center)
                .into(),
                widget::text_input(
                    fl!("pre-reminder-placeholder"),
                    &ns.pre_reminder_template,
                )
                .on_input(Message::SetPreReminderTemplate)
                .into(),
                widget::text::caption(fl!("message-placeholders-hint")).into(),
                widget::container(
                    widget::column(vec![
                        widget::text::caption(fl!("message-preview"))
                            .class(theme::Text::Accent)
                            .into(),
                        widget::text::body(preview_text).into(),
                    ])
                    .spacing(spacing.space_xxxs),
                )
                .class(theme::Container::Card)
                .padding(spacing.space_xxs)
                .width(Length::Fill)
                .into(),
            ])
            .spacing(spacing.space_xxs)
            .padding([spacing.space_xxs, spacing.space_xs]);

            pre_section = pre_section.add(template_editor);
        }
        children.push(pre_section.into());

        // 4. Adhan time alert section
        let mut adhan_section = widget::settings::section()
            .title(fl!("section-adhan-alert"))
            .add(widget::settings::item(
                fl!("adhan-alert-enabled"),
                widget::toggler(ns.adhan_alert_enabled).on_toggle(Message::SetAdhanAlertEnabled),
            ));

        if ns.adhan_alert_enabled {
            let adhan_preview = self.render_adhan_body(sample_prayer, &sample_time);
            let adhan_editor = widget::column(vec![
                widget::row(vec![
                    widget::text::caption(fl!("adhan-message")).into(),
                    widget::space::horizontal().width(Length::Fill).into(),
                    widget::button::text(fl!("reset-default"))
                        .on_press(Message::ResetAdhanTemplate)
                        .padding(0)
                        .into(),
                ])
                .align_y(Alignment::Center)
                .into(),
                widget::text_input(fl!("adhan-placeholder"), &ns.adhan_template)
                    .on_input(Message::SetAdhanTemplate)
                    .into(),
                widget::container(
                    widget::column(vec![
                        widget::text::caption(fl!("message-preview"))
                            .class(theme::Text::Accent)
                            .into(),
                        widget::text::body(adhan_preview).into(),
                    ])
                    .spacing(spacing.space_xxxs),
                )
                .class(theme::Container::Card)
                .padding(spacing.space_xxs)
                .width(Length::Fill)
                .into(),
            ])
            .spacing(spacing.space_xxs)
            .padding([spacing.space_xxs, spacing.space_xs]);

            adhan_section = adhan_section.add(adhan_editor);
        }
        children.push(adhan_section.into());

        // 5. Sound & Adhan Audio Engine
        let mut sound_section = widget::settings::section()
            .title(fl!("section-sound"))
            .add(widget::settings::item(
                fl!("sound-enabled"),
                widget::toggler(ns.sound_enabled).on_toggle(Message::SetSoundEnabled),
            ));

        if ns.sound_enabled {
            let vol = ns.volume.min(100);
            sound_section = sound_section.add(widget::settings::item(
                format!("{} ({vol}%)", fl!("volume")),
                widget::slider(0..=100, vol, Message::SetVolume),
            ));

            let voice_names: Vec<String> = AdhanVoice::ALL
                .iter()
                .map(|&v| Self::localized_voice_name(v))
                .collect();
            let voice_selected = AdhanVoice::ALL
                .iter()
                .position(|&v| v == ns.adhan_voice);

            sound_section = sound_section.add(widget::settings::item(
                fl!("adhan-voice"),
                widget::dropdown(voice_names, voice_selected, Message::SetAdhanVoice),
            ));

            if ns.adhan_voice == AdhanVoice::CustomFile {
                let custom_editor = widget::column(vec![
                    widget::text_input(
                        fl!("custom-adhan-placeholder"),
                        &ns.custom_adhan_path,
                    )
                    .on_input(Message::SetCustomAdhanPath)
                    .into(),
                    widget::text::caption(fl!("custom-adhan-hint")).into(),
                ])
                .spacing(spacing.space_xxs)
                .padding([spacing.space_xxs, spacing.space_xs]);

                sound_section = sound_section.add(custom_editor);
            }

            let status_text = match ns.adhan_voice {
                AdhanVoice::SystemBell => String::new(),
                AdhanVoice::CustomFile => {
                    if !ns.custom_adhan_path.trim().is_empty()
                        && Path::new(&ns.custom_adhan_path).exists()
                    {
                        fl!("adhan-cached")
                    } else {
                        String::new()
                    }
                }
                preset => {
                    if notifications::is_voice_cached(preset) {
                        fl!("adhan-cached")
                    } else {
                        fl!("adhan-online")
                    }
                }
            };

            let play_btn: Element<'_, Message> = if self.is_playing_audio {
                widget::button::destructive(fl!("stop-adhan"))
                    .on_press(Message::StopAudio)
                    .padding([spacing.space_xxs, spacing.space_xs])
                    .into()
            } else {
                widget::button::standard(fl!("play-adhan"))
                    .on_press(Message::PlayTestAdhan)
                    .padding([spacing.space_xxs, spacing.space_xs])
                    .into()
            };

            let audio_control_row = widget::row(vec![
                if !status_text.is_empty() {
                    widget::text::caption(status_text).class(theme::Text::Accent).into()
                } else {
                    widget::space::horizontal().width(0).into()
                },
                widget::space::horizontal().width(Length::Fill).into(),
                play_btn,
            ])
            .align_y(Alignment::Center)
            .padding([spacing.space_xxs, spacing.space_xs]);

            sound_section = sound_section.add(audio_control_row);
        }
        children.push(sound_section.into());

        // 6. Test Notification CTA + status feedback
        children.push(
            widget::button::suggested(fl!("test-alert"))
                .on_press(Message::TestNotification)
                .width(Length::Fill)
                .into(),
        );

        if let Some((is_ok, msg)) = &self.notification_feedback {
            let text_widget = if *is_ok {
                widget::text::caption(msg).class(theme::Text::Accent)
            } else {
                widget::text::caption(msg)
            };
            children.push(
                widget::container(text_widget)
                    .class(theme::Container::Card)
                    .padding(spacing.space_xs)
                    .width(Length::Fill)
                    .into(),
            );
        }

        widget::column(children).spacing(spacing.space_s).into()
    }

    fn view_calculation_settings(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        let method_names: Vec<String> = METHOD_IDS
            .iter()
            .map(|id| prayer::method_label(*id).to_string())
            .collect();
        let selected = METHOD_IDS
            .iter()
            .position(|id| *id == self.config.method_id);

        let mut children: Vec<Element<'_, Message>> = Vec::new();

        // 1. Calculation Method with live specs
        let specs_text = prayer::method_specs(self.config.method_id);
        let specs_card = widget::container(
            widget::column(vec![
                widget::text::caption(fl!("specs-header")).class(theme::Text::Accent).into(),
                widget::text::caption(specs_text).into(),
            ])
            .spacing(spacing.space_xxxs),
        )
        .class(theme::Container::Card)
        .padding(spacing.space_xs)
        .width(Length::Fill);

        children.push(
            widget::settings::section()
                .title(fl!("calculation-method"))
                .add(widget::settings::item(
                    fl!("calculation-method"),
                    widget::dropdown(method_names, selected, Message::SetMethod),
                ))
                .add(specs_card)
                .into(),
        );

        // 2. Asr Juristic Method
        let madhab_desc = if self.config.hanafi {
            fl!("madhab-hanafi")
        } else {
            fl!("madhab-standard")
        };
        children.push(
            widget::settings::section()
                .title(fl!("hanafi-asr"))
                .add(widget::settings::item(
                    madhab_desc,
                    widget::toggler(self.config.hanafi).on_toggle(Message::SetHanafi),
                ))
                .into(),
        );

        // 3. Custom Astronomical Angles
        let mut angles_section = widget::settings::section()
            .title(fl!("section-angles"))
            .add(widget::settings::item(
                fl!("use-custom-angles"),
                widget::toggler(self.config.use_custom_angles).on_toggle(Message::SetUseCustomAngles),
            ));

        if self.config.use_custom_angles {
            let fajr_angle = self
                .config
                .custom_angles
                .fajr_angle
                .map_or(String::new(), |a| a.to_string());
            let isha_angle = self
                .config
                .custom_angles
                .isha_angle
                .map_or(String::new(), |a| a.to_string());
            let isha_interval = self
                .config
                .custom_angles
                .isha_interval
                .map_or(String::new(), |i| i.to_string());

            angles_section = angles_section
                .add(widget::settings::item(
                    fl!("fajr-angle"),
                    widget::text_input("18.0", fajr_angle).on_input(Message::SetFajrAngle),
                ))
                .add(widget::settings::item(
                    fl!("isha-angle"),
                    widget::text_input("18.0", isha_angle).on_input(Message::SetIshaAngle),
                ))
                .add(widget::settings::item(
                    fl!("isha-interval"),
                    widget::text_input("90", isha_interval).on_input(Message::SetIshaInterval),
                ));
        }
        children.push(angles_section.into());

        // 4. Minute Adjustments with Quick Reset Header
        let adj = self.config.prayer_adjustments;
        let fmt_adj = |v: i8| -> String {
            if v > 0 {
                format!("+{v}")
            } else {
                v.to_string()
            }
        };

        let adj_section = widget::settings::section()
            .title(fl!("section-adjustments"))
            .add(widget::settings::item(
                fl!("reset-all-adjustments"),
                widget::button::standard(fl!("reset-default"))
                    .on_press(Message::ResetAllAdjustments),
            ))
            .add(widget::settings::item(
                fl!("fajr-adjustment"),
                widget::spin_button(
                    fmt_adj(adj.fajr),
                    fl!("fajr-adjustment"),
                    adj.fajr,
                    1i8,
                    -60i8,
                    60i8,
                    Message::SetFajrAdjustment,
                ),
            ))
            .add(widget::settings::item(
                fl!("sunrise-adjustment"),
                widget::spin_button(
                    fmt_adj(adj.sunrise),
                    fl!("sunrise-adjustment"),
                    adj.sunrise,
                    1i8,
                    -60i8,
                    60i8,
                    Message::SetSunriseAdjustment,
                ),
            ))
            .add(widget::settings::item(
                fl!("dhuhr-adjustment"),
                widget::spin_button(
                    fmt_adj(adj.dhuhr),
                    fl!("dhuhr-adjustment"),
                    adj.dhuhr,
                    1i8,
                    -60i8,
                    60i8,
                    Message::SetDhuhrAdjustment,
                ),
            ))
            .add(widget::settings::item(
                fl!("asr-adjustment"),
                widget::spin_button(
                    fmt_adj(adj.asr),
                    fl!("asr-adjustment"),
                    adj.asr,
                    1i8,
                    -60i8,
                    60i8,
                    Message::SetAsrAdjustment,
                ),
            ))
            .add(widget::settings::item(
                fl!("maghrib-adjustment"),
                widget::spin_button(
                    fmt_adj(adj.maghrib),
                    fl!("maghrib-adjustment"),
                    adj.maghrib,
                    1i8,
                    -60i8,
                    60i8,
                    Message::SetMaghribAdjustment,
                ),
            ))
            .add(widget::settings::item(
                fl!("isha-adjustment"),
                widget::spin_button(
                    fmt_adj(adj.isha),
                    fl!("isha-adjustment"),
                    adj.isha,
                    1i8,
                    -60i8,
                    60i8,
                    Message::SetIshaAdjustment,
                ),
            ));

        children.push(adj_section.into());

        widget::column(children).spacing(spacing.space_s).into()
    }
}

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = ();

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        // Construct the app model with the runtime's core.
        let config = cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
            .map(|context| match Config::get_entry(&context) {
                Ok(config) => config,
                Err((_errors, config)) => config,
            })
            .unwrap_or_default();
        let settings_tabs = segmented_button::Model::builder()
            .insert(|b| {
                b.text(fl!("tab-general-short"))
                    .data(SettingsTab::General)
                    .activate()
            })
            .insert(|b| b.text(fl!("tab-alerts-audio")).data(SettingsTab::Alerts))
            .insert(|b| {
                b.text(fl!("tab-calc-short"))
                    .data(SettingsTab::Calculation)
            })
            .build();

        let mut app = AppModel {
            core,
            popup: None,
            view_mode: ViewMode::default(),
            settings_tabs,
            config,
            today: None,
            tomorrow: None,
            hijri: None,
            next: None,
            active: None,
            tz: chrono_tz::UTC,
            last_pre_notified: None,
            last_adhan_notified: None,
            notification_feedback: None,
            is_playing_audio: false,
        };
        app.refresh_schedule();
        (app, Task::none())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    /// The applet's button in the panel: next prayer + live countdown.
    /// Text comes from `applet.text()` so its size follows the panel size
    /// and theme instead of overflowing on small panels.
    fn view(&self) -> Element<'_, Self::Message> {
        let is_horizontal_panel = self.core.applet.is_horizontal();
        let is_stacked = self.config.panel_layout.is_stacked_in(is_horizontal_panel);
        let label = self.panel_label(is_horizontal_panel);
        let custom_size = if self.config.panel_font_size > 0 {
            Some(self.config.panel_font_size as f32)
        } else {
            None
        };
        let font = self.config.font_bold();

        if is_stacked {
            let size = custom_size.unwrap_or(10.5);
            let text_widget = widget::text(label)
                .size(size)
                .font(font)
                .line_height(1.1)
                .align_x(Alignment::Center);
            self.core
                .applet
                .text_button(text_widget, Message::TogglePopup)
                .into()
        } else if let Some(sz) = custom_size {
            let text_widget = widget::text(label)
                .size(sz)
                .font(font)
                .align_x(Alignment::Center);
            self.core
                .applet
                .text_button(text_widget, Message::TogglePopup)
                .into()
        } else {
            let text_widget = self.core.applet.text(label).font(font);
            self.core
                .applet
                .text_button(text_widget, Message::TogglePopup)
                .into()
        }
    }

    /// The applet's popup: full day schedule, dates, and quick settings.
    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        let header = self.view_header();

        let content = match self.view_mode {
            ViewMode::Schedule => {
                let mut children = vec![header];
                if self.is_playing_audio {
                    let voice_name =
                        Self::localized_voice_name(self.config.notification_settings.adhan_voice);
                    children.push(
                        widget::container(
                            widget::row(vec![
                                widget::icon::from_name("audio-volume-high-symbolic").into(),
                                widget::text::body(fl!(
                                    "adhan-playing-banner",
                                    reader = voice_name.as_str()
                                ))
                                .into(),
                                widget::space::horizontal().width(Length::Fill).into(),
                                widget::button::destructive(fl!("stop-adhan"))
                                    .on_press(Message::StopAudio)
                                    .padding([spacing.space_xxs, spacing.space_xs])
                                    .into(),
                            ])
                            .spacing(spacing.space_xs)
                            .align_y(Alignment::Center),
                        )
                        .class(theme::Container::Card)
                        .padding(spacing.space_xs)
                        .width(Length::Fill)
                        .into(),
                    );
                }
                if self.config.show_celestial_horizon
                    && let Some(horizon) = self.view_celestial_horizon()
                {
                    children.push(horizon);
                }
                if self.config.show_hero_card
                    && let Some(hero) = self.view_hero_card()
                {
                    children.push(hero);
                }
                children.push(self.view_schedule());
                let column_spacing = if self.config.compact_mode {
                    spacing.space_xxs
                } else {
                    spacing.space_xs
                };
                let column_padding = if self.config.compact_mode {
                    [spacing.space_xxs, spacing.space_xs]
                } else {
                    [spacing.space_xs, spacing.space_xs]
                };
                widget::column(children)
                    .spacing(column_spacing)
                    .padding(column_padding)
            }
            ViewMode::Settings => widget::column(vec![
                header,
                widget::divider::horizontal::default().into(),
                self.view_settings(),
                widget::divider::horizontal::default().into(),
                widget::button::standard(fl!("back-to-schedule"))
                    .on_press(Message::SetViewMode(ViewMode::Schedule))
                    .width(Length::Fill)
                    .into(),
            ])
            .spacing(spacing.space_xs)
            .padding([spacing.space_xs, spacing.space_xs]),
        };

        self.core.applet.popup_container(content).into()
    }

    /// Register subscriptions for this application.
    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch(vec![
            // Tick once per second for the live countdown.
            Subscription::run(|| {
                cosmic::iced::stream::channel(
                    4,
                    move |mut tx: futures::channel::mpsc::Sender<Message>| async move {
                        loop {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            if tx.send(Message::Tick).await.is_err() {
                                break;
                            }
                        }
                    },
                )
            }),
            // Watch for application configuration changes.
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(Box::new(update.config))),
        ])
    }

    /// Handles messages emitted by the application and its widgets.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Tick => {
                self.is_playing_audio = notifications::is_audio_playing();
                self.refresh_next(Utc::now().with_timezone(&self.tz));
                return self.check_and_dispatch_alerts();
            }
            Message::UpdateConfig(config) => {
                self.config = *config;
                self.refresh_schedule();
            }
            Message::SetViewMode(mode) => {
                self.view_mode = mode;
                if mode == ViewMode::Schedule {
                    self.notification_feedback = None;
                }
            }
            Message::SelectSettingsTab(entity) => {
                self.settings_tabs.activate(entity);
            }
            Message::OpenAlertsSettings => {
                self.settings_tabs.activate_position(1);
                self.view_mode = ViewMode::Settings;
            }
            Message::SetCity(index) => {
                if let Some(city) = prayer::CITIES.get(index) {
                    self.config.location_label = city
                        .name
                        .split(',')
                        .next()
                        .unwrap_or(city.name)
                        .to_string();
                    self.config.latitude = city.latitude;
                    self.config.longitude = city.longitude;
                    self.config.timezone = city.timezone.to_string();
                    self.config.method_id = city.method_id;
                    self.save_config();
                    self.refresh_schedule();
                }
            }
            Message::SetMethod(index) => {
                if let Some(id) = METHOD_IDS.get(index) {
                    self.config.method_id = *id;
                    self.save_config();
                    self.refresh_schedule();
                }
            }
            Message::SetHanafi(hanafi) => {
                self.config.hanafi = hanafi;
                self.save_config();
                self.refresh_schedule();
            }
            Message::Set24h(use_24h) => {
                self.config.use_24h = use_24h;
                self.save_config();
            }
            Message::SetLayout(index) => {
                if let Some((layout, _)) = PANEL_LAYOUTS.get(index) {
                    // Display only; no schedule recompute needed.
                    self.config.panel_layout = *layout;
                    self.save_config();
                }
            }
            Message::SetShowTime(show_time) => {
                // Display only; no schedule recompute needed.
                self.config.show_prayer_time = show_time;
                self.save_config();
            }
            Message::SetCompactMode(compact) => {
                self.config.compact_mode = compact;
                self.save_config();
            }
            Message::SetShowCelestialHorizon(show) => {
                self.config.show_celestial_horizon = show;
                self.save_config();
            }
            Message::SetShowHeroCard(show) => {
                self.config.show_hero_card = show;
                self.save_config();
            }
            Message::SetShowHijriDate(show) => {
                self.config.show_hijri_date = show;
                self.save_config();
            }
            Message::SetFontFamily(index) => {
                if let Some(family) = FontFamilyChoice::ALL.get(index) {
                    self.config.font_family = *family;
                    self.save_config();
                }
            }
            Message::SetCustomFontFamily(name) => {
                self.config.custom_font_family = name;
                self.save_config();
            }
            Message::SetPanelFontSize(size) => {
                self.config.panel_font_size = size;
                self.save_config();
            }
            Message::SetPopupFontScale(scale) => {
                self.config.popup_font_scale = scale.clamp(70, 160);
                self.save_config();
            }
            // Advanced settings handlers
            Message::SetUseCustomAngles(use_custom) => {
                self.config.use_custom_angles = use_custom;
                self.save_config();
                self.refresh_schedule();
            }
            Message::SetFajrAngle(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    self.config.custom_angles.fajr_angle = None;
                    self.save_config();
                    self.refresh_schedule();
                } else if let Ok(angle) = trimmed.parse::<f64>()
                    && (0.0..=30.0).contains(&angle)
                {
                    self.config.custom_angles.fajr_angle = Some(angle);
                    self.save_config();
                    self.refresh_schedule();
                }
            }
            Message::SetIshaAngle(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    self.config.custom_angles.isha_angle = None;
                    self.save_config();
                    self.refresh_schedule();
                } else if let Ok(angle) = trimmed.parse::<f64>()
                    && (0.0..=30.0).contains(&angle)
                {
                    self.config.custom_angles.isha_angle = Some(angle);
                    self.save_config();
                    self.refresh_schedule();
                }
            }
            Message::SetIshaInterval(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    self.config.custom_angles.isha_interval = None;
                    self.save_config();
                    self.refresh_schedule();
                } else if let Ok(interval) = trimmed.parse::<u8>() {
                    self.config.custom_angles.isha_interval = Some(interval);
                    self.save_config();
                    self.refresh_schedule();
                }
            }
            Message::SetFajrAdjustment(adj) => {
                self.config.prayer_adjustments.fajr = adj.clamp(-60, 60);
                self.save_config();
                self.refresh_schedule();
            }
            Message::SetSunriseAdjustment(adj) => {
                self.config.prayer_adjustments.sunrise = adj.clamp(-60, 60);
                self.save_config();
                self.refresh_schedule();
            }
            Message::SetDhuhrAdjustment(adj) => {
                self.config.prayer_adjustments.dhuhr = adj.clamp(-60, 60);
                self.save_config();
                self.refresh_schedule();
            }
            Message::SetAsrAdjustment(adj) => {
                self.config.prayer_adjustments.asr = adj.clamp(-60, 60);
                self.save_config();
                self.refresh_schedule();
            }
            Message::SetMaghribAdjustment(adj) => {
                self.config.prayer_adjustments.maghrib = adj.clamp(-60, 60);
                self.save_config();
                self.refresh_schedule();
            }
            Message::SetIshaAdjustment(adj) => {
                self.config.prayer_adjustments.isha = adj.clamp(-60, 60);
                self.save_config();
                self.refresh_schedule();
            }
            Message::ResetAllAdjustments => {
                self.config.prayer_adjustments = Default::default();
                self.save_config();
                self.refresh_schedule();
            }
            // Notification & customizable alert handlers
            Message::ToggleMuted => {
                self.config.notification_settings.muted =
                    !self.config.notification_settings.muted;
                if self.config.notification_settings.muted {
                    notifications::stop_audio();
                    self.is_playing_audio = false;
                }
                self.save_config();
            }
            Message::SetMuted(muted) => {
                self.config.notification_settings.muted = muted;
                if muted {
                    notifications::stop_audio();
                    self.is_playing_audio = false;
                }
                self.save_config();
            }
            Message::SetAllPrayersNotification(enabled) => {
                let ns = &mut self.config.notification_settings;
                ns.fajr = enabled;
                ns.dhuhr = enabled;
                ns.asr = enabled;
                ns.maghrib = enabled;
                ns.isha = enabled;
                self.save_config();
            }
            Message::TogglePrayerNotification(kind) => {
                match kind {
                    PrayerKind::Fajr => {
                        self.config.notification_settings.fajr =
                            !self.config.notification_settings.fajr;
                    }
                    PrayerKind::Dhuhr => {
                        self.config.notification_settings.dhuhr =
                            !self.config.notification_settings.dhuhr;
                    }
                    PrayerKind::Asr => {
                        self.config.notification_settings.asr =
                            !self.config.notification_settings.asr;
                    }
                    PrayerKind::Maghrib => {
                        self.config.notification_settings.maghrib =
                            !self.config.notification_settings.maghrib;
                    }
                    PrayerKind::Isha => {
                        self.config.notification_settings.isha =
                            !self.config.notification_settings.isha;
                    }
                    PrayerKind::Sunrise => {}
                }
                self.save_config();
            }
            Message::SetPreReminderEnabled(enabled) => {
                self.config.notification_settings.pre_reminder_enabled = enabled;
                self.save_config();
            }
            Message::SetPreReminderMinutes(minutes) => {
                self.config.notification_settings.pre_reminder_minutes = minutes.clamp(1, 60);
                self.last_pre_notified = None;
                self.save_config();
            }
            Message::SetPreReminderTemplate(template) => {
                self.config.notification_settings.pre_reminder_template = template;
                self.save_config();
            }
            Message::ResetPreReminderTemplate => {
                self.config.notification_settings.pre_reminder_template.clear();
                self.save_config();
            }
            Message::SetAdhanAlertEnabled(enabled) => {
                self.config.notification_settings.adhan_alert_enabled = enabled;
                self.save_config();
            }
            Message::SetAdhanTemplate(template) => {
                self.config.notification_settings.adhan_template = template;
                self.save_config();
            }
            Message::ResetAdhanTemplate => {
                self.config.notification_settings.adhan_template.clear();
                self.save_config();
            }
            Message::SetNotificationFajr(enabled) => {
                self.config.notification_settings.fajr = enabled;
                self.save_config();
            }
            Message::SetNotificationDhuhr(enabled) => {
                self.config.notification_settings.dhuhr = enabled;
                self.save_config();
            }
            Message::SetNotificationAsr(enabled) => {
                self.config.notification_settings.asr = enabled;
                self.save_config();
            }
            Message::SetNotificationMaghrib(enabled) => {
                self.config.notification_settings.maghrib = enabled;
                self.save_config();
            }
            Message::SetNotificationIsha(enabled) => {
                self.config.notification_settings.isha = enabled;
                self.save_config();
            }
            Message::SetSoundEnabled(enabled) => {
                self.config.notification_settings.sound_enabled = enabled;
                if !enabled {
                    notifications::stop_audio();
                    self.is_playing_audio = false;
                }
                self.save_config();
            }
            Message::SetVolume(volume) => {
                self.config.notification_settings.volume = volume.min(100);
                self.save_config();
            }
            Message::SetAdhanVoice(index) => {
                if let Some(&voice) = AdhanVoice::ALL.get(index) {
                    self.config.notification_settings.adhan_voice = voice;
                    self.save_config();
                }
            }
            Message::SetCustomAdhanPath(path) => {
                self.config.notification_settings.custom_adhan_path = path;
                self.save_config();
            }
            Message::PlayTestAdhan => {
                let voice = self.config.notification_settings.adhan_voice;
                let custom_path = self.config.notification_settings.custom_adhan_path.clone();
                let volume = self.config.notification_settings.volume;
                let voice_name = Self::localized_voice_name(voice);
                self.is_playing_audio = true;
                self.notification_feedback =
                    Some((true, fl!("test-sound-started", voice = voice_name.as_str())));
                return cosmic::task::future(async move {
                    let result =
                        notifications::play_voice_sound(voice, &custom_path, volume).await;
                    cosmic::Action::App(Message::NotificationDispatched(result.map(|()| None)))
                });
            }
            Message::StopAudio => {
                notifications::stop_audio();
                self.is_playing_audio = false;
                self.notification_feedback = Some((true, fl!("audio-stopped")));
            }
            Message::TestNotification => {
                let sample_prayer = self
                    .next
                    .map(|(k, _, _)| k)
                    .filter(|k| k.is_prayer())
                    .unwrap_or(PrayerKind::Asr);
                let sample_time = self.next.map_or_else(
                    || String::from("16:09"),
                    |(_, at, _)| prayer::format_time(at, self.config.use_24h),
                );
                let prayer_name = Self::localized_prayer_name(sample_prayer);
                let summary = fl!("adhan-summary", prayer = prayer_name.as_str());
                let body = self.render_adhan_body(sample_prayer, &sample_time);
                return self.spawn_notification_task(summary, body, true, true);
            }
            Message::NotificationDispatched(result) => {
                self.notification_feedback = Some(match result {
                    Ok(None) => (true, fl!("test-alert-sent")),
                    Ok(Some(reason)) => (false, fl!("test-alert-sound-warn", reason = reason)),
                    Err(reason) => (false, fl!("test-alert-failed", reason = reason)),
                });
            }
            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    destroy_popup(p)
                } else {
                    let Some(main_id) = self.core.main_window_id() else {
                        return Task::none();
                    };
                    let new_id = Id::unique();
                    self.popup.replace(new_id);
                    let mut popup_settings = self.core.applet.get_popup_settings(
                        main_id,
                        new_id,
                        None,
                        None,
                        None,
                    );
                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(420.0)
                        .min_width(360.0)
                        .min_height(200.0)
                        .max_height(1080.0);
                    get_popup(popup_settings)
                };
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                    self.view_mode = ViewMode::Schedule;
                }
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
