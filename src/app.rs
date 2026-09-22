// SPDX-License-Identifier: MIT

use crate::config::{Config, PanelLayout};
use crate::fl;
use crate::prayer::{self, DailyTimes, PrayerKind};
use chrono::Utc;
use chrono_tz::Tz;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::{Alignment, Color, Length, Limits, Subscription, window::Id};
use cosmic::prelude::*;
use cosmic::theme;
use cosmic::widget;
use futures::SinkExt;
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
const PANEL_LAYOUTS: [(PanelLayout, &str); 2] = [
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

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// The popup id.
    popup: Option<Id>,
    /// Currently displayed view in the popup.
    view_mode: ViewMode,
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
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    Tick,
    UpdateConfig(Config),
    SetViewMode(ViewMode),
    SetCity(usize),
    SetMethod(usize),
    SetHanafi(bool),
    Set24h(bool),
    SetLayout(usize),
    SetShowTime(bool),
    // Advanced settings messages
    SetUseCustomAngles(bool),
    SetFajrAngle(String),
    SetIshaAngle(String),
    SetIshaInterval(String),
    SetFajrAdjustment(String),
    SetSunriseAdjustment(String),
    SetDhuhrAdjustment(String),
    SetAsrAdjustment(String),
    SetMaghribAdjustment(String),
    SetIshaAdjustment(String),
    // Notification settings
    SetNotificationFajr(bool),
    SetNotificationDhuhr(bool),
    SetNotificationAsr(bool),
    SetNotificationMaghrib(bool),
    SetNotificationIsha(bool),
    SetSoundEnabled(bool),
    SetVolume(String),
}

fn resolve_tz(name: &str) -> Tz {
    name.parse::<Tz>().unwrap_or(chrono_tz::UTC)
}

impl AppModel {
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
    fn panel_label(&self) -> String {
        if let Some((kind, at, _)) = self.active {
            let detail = if self.config.show_prayer_time {
                prayer::format_time(at, self.config.use_24h)
            } else {
                fl!("now")
            };
            return prayer::panel_label(kind, &detail, self.config.panel_layout.is_stacked());
        }
        match &self.next {
            Some((kind, at, secs)) => {
                let detail = prayer::panel_detail(
                    *secs,
                    *at,
                    self.config.show_prayer_time,
                    self.config.use_24h,
                );
                prayer::panel_label(*kind, &detail, self.config.panel_layout.is_stacked())
            }
            None => fl!("app-title"),
        }
    }

    fn gregorian_label(&self) -> String {
        let now = Utc::now().with_timezone(&self.tz);
        now.format("%A, %-d %B %Y").to_string()
    }

    fn view_header(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        let hijri_text = match &self.hijri {
            Some((en, ar)) => format!("{en} · {ar}"),
            None => String::new(),
        };

        let settings_btn = match self.view_mode {
            ViewMode::Schedule => widget::button::icon(widget::icon::from_name("preferences-system-symbolic"))
                .on_press(Message::SetViewMode(ViewMode::Settings))
                .tooltip(fl!("settings")),
            ViewMode::Settings => widget::button::icon(widget::icon::from_name("go-previous-symbolic"))
                .on_press(Message::SetViewMode(ViewMode::Schedule))
                .tooltip(fl!("back-to-schedule")),
        };

        widget::row(vec![
            widget::column(vec![
                widget::text::title4(self.config.location_label.clone()).into(),
                widget::text::caption(self.gregorian_label()).into(),
                widget::text::caption(hijri_text).into(),
            ])
            .spacing(spacing.space_xxxs)
            .into(),
            widget::space::horizontal().width(Length::Fill).into(),
            settings_btn.into(),
        ])
        .align_y(Alignment::Start)
        .into()
    }

    fn view_hero_card(&self) -> Option<Element<'_, Message>> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        if let Some((active_kind, active_at, _)) = self.active {
            let card_header = widget::row(vec![
                widget::text::caption(fl!("current-prayer")).class(theme::Text::Accent).into(),
                widget::space::horizontal().width(Length::Fill).into(),
                widget::text::caption(fl!("ongoing")).class(theme::Text::Accent).into(),
            ])
            .align_y(Alignment::Center);

            let card_main = widget::row(vec![
                widget::column(vec![
                    widget::text::title3(active_kind.name_en()).into(),
                    widget::text::caption(active_kind.name_ar()).into(),
                ])
                .spacing(spacing.space_xxxs)
                .into(),
                widget::space::horizontal().width(Length::Fill).into(),
                widget::column(vec![
                    widget::text::title3(prayer::format_time(active_at, self.config.use_24h)).into(),
                    widget::text::caption(fl!("now")).class(theme::Text::Accent).into(),
                ])
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
        } else if let Some((next_kind, next_at, secs)) = self.next {
            let countdown_str = if secs < 120 && !self.config.show_prayer_time {
                fl!("now")
            } else {
                prayer::format_countdown(secs)
            };

            let card_header = widget::row(vec![
                widget::text::caption(fl!("next-prayer")).into(),
                widget::space::horizontal().width(Length::Fill).into(),
                widget::text::caption(countdown_str).class(theme::Text::Accent).into(),
            ])
            .align_y(Alignment::Center);

            let card_main = widget::row(vec![
                widget::column(vec![
                    widget::text::title3(next_kind.name_en()).into(),
                    widget::text::caption(next_kind.name_ar()).into(),
                ])
                .spacing(spacing.space_xxxs)
                .into(),
                widget::space::horizontal().width(Length::Fill).into(),
                widget::column(vec![
                    widget::text::title3(prayer::format_time(next_at, self.config.use_24h)).into(),
                    widget::text::caption(prayer::format_countdown(secs)).into(),
                ])
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
            None
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

                let indicator: Element<'_, Message> = if is_active {
                    widget::text("●").class(theme::Text::Accent).into()
                } else if is_next {
                    widget::text("▸").class(theme::Text::Accent).into()
                } else if is_passed {
                    widget::text("✓").class(theme::Text::Color(Color::from_rgba(0.5, 0.5, 0.5, 0.6))).into()
                } else {
                    widget::text("·").class(theme::Text::Color(Color::from_rgba(0.5, 0.5, 0.5, 0.6))).into()
                };

                let en_name: Element<'_, Message> = if is_passed {
                    widget::text(kind.name_en()).class(theme::Text::Color(Color::from_rgba(0.55, 0.55, 0.55, 0.7))).into()
                } else if is_next || is_active {
                    widget::text(kind.name_en()).font(cosmic::font::bold()).into()
                } else {
                    widget::text(kind.name_en()).into()
                };

                let ar_name: Element<'_, Message> = if is_passed {
                    widget::text(kind.name_ar()).class(theme::Text::Color(Color::from_rgba(0.45, 0.45, 0.45, 0.5))).into()
                } else {
                    widget::text(kind.name_ar()).class(theme::Text::Color(Color::from_rgba(0.65, 0.65, 0.65, 0.8))).into()
                };

                let left_row = widget::row(vec![
                    indicator,
                    en_name,
                    ar_name,
                ])
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center);

                let mut right_items: Vec<Element<'_, Message>> = Vec::new();
                if !kind.is_prayer() {
                    right_items.push(
                        widget::text::caption(fl!("sunrise"))
                            .class(theme::Text::Color(Color::from_rgba(0.6, 0.6, 0.6, 0.6)))
                            .into(),
                    );
                } else if is_active {
                    right_items.push(
                        widget::text::caption(fl!("ongoing"))
                            .class(theme::Text::Accent)
                            .into(),
                    );
                } else if is_next {
                    let countdown_str = match self.next {
                        Some((_, _, s)) if s < 120 => fl!("now"),
                        Some((_, _, s)) => prayer::format_countdown(s),
                        None => String::new(),
                    };
                    right_items.push(
                        widget::text::caption(countdown_str)
                            .class(theme::Text::Accent)
                            .into(),
                    );
                }

                let time_str = prayer::format_time(event_time, self.config.use_24h);
                let time_widget: Element<'_, Message> = if is_passed {
                    widget::text::monotext(time_str).class(theme::Text::Color(Color::from_rgba(0.55, 0.55, 0.55, 0.7))).into()
                } else if is_next || is_active {
                    widget::text::monotext(time_str).font(cosmic::font::bold()).into()
                } else {
                    widget::text::monotext(time_str).into()
                };
                right_items.push(time_widget);

                let right_row = widget::row(right_items)
                    .spacing(spacing.space_xs)
                    .align_y(Alignment::Center);

                let row_content = widget::row(vec![
                    left_row.into(),
                    widget::space::horizontal().width(Length::Fill).into(),
                    right_row.into(),
                ])
                .align_y(Alignment::Center)
                .padding([spacing.space_xxs, spacing.space_xs]);

                let button_item = widget::list_column::button(row_content)
                    .selected(is_next || is_active);
                schedule = schedule.add(button_item);
            }
        }
        schedule.into()
    }

    fn view_settings(&self) -> Element<'_, Message> {
        let city_names: Vec<String> = prayer::CITIES
            .iter()
            .map(|c| format!("{} ({})", c.name, c.name_ar))
            .collect();
        let city_selected = prayer::find_city_preset(&self.config.location_label);

        let method_names: Vec<String> = METHOD_IDS
            .iter()
            .map(|id| prayer::method_label(*id).to_string())
            .collect();
        let selected = METHOD_IDS
            .iter()
            .position(|id| *id == self.config.method_id);

        let layout_names = vec![
            fl!("panel-layout-horizontal"),
            fl!("panel-layout-stacked"),
        ];
        let layout_selected = PANEL_LAYOUTS
            .iter()
            .position(|(layout, _)| *layout == self.config.panel_layout);

        let mut settings = widget::list_column()
            .add(widget::settings::item(
                fl!("city"),
                widget::dropdown(city_names, city_selected, Message::SetCity),
            ))
            .add(widget::settings::item(
                fl!("calculation-method"),
                widget::dropdown(method_names, selected, Message::SetMethod),
            ))
            .add(widget::settings::item(
                fl!("hanafi-asr"),
                widget::toggler(self.config.hanafi).on_toggle(Message::SetHanafi),
            ))
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
            ));

        // Advanced settings section
        settings = settings.add(widget::divider::horizontal::default());

        // Custom angles toggle
        settings = settings.add(widget::settings::item(
            fl!("use-custom-angles"),
            widget::toggler(self.config.use_custom_angles).on_toggle(Message::SetUseCustomAngles),
        ));

        if self.config.use_custom_angles {
            let fajr_angle = self.config.custom_angles.fajr_angle.map_or(String::new(), |a| a.to_string());
            let isha_angle = self.config.custom_angles.isha_angle.map_or(String::new(), |a| a.to_string());
            let isha_interval = self.config.custom_angles.isha_interval.map_or(String::new(), |i| i.to_string());

            settings = settings
                .add(widget::settings::item(
                    fl!("fajr-angle"),
                    widget::text_input("18.0", fajr_angle)
                        .on_input(Message::SetFajrAngle),
                ))
                .add(widget::settings::item(
                    fl!("isha-angle"),
                    widget::text_input("18.0", isha_angle)
                        .on_input(Message::SetIshaAngle),
                ))
                .add(widget::settings::item(
                    fl!("isha-interval"),
                    widget::text_input("90", isha_interval)
                        .on_input(Message::SetIshaInterval),
                ));
        }

        // Time adjustments section
        settings = settings.add(widget::divider::horizontal::default());

        let fajr_adj = self.config.prayer_adjustments.fajr.to_string();
        let sunrise_adj = self.config.prayer_adjustments.sunrise.to_string();
        let dhuhr_adj = self.config.prayer_adjustments.dhuhr.to_string();
        let asr_adj = self.config.prayer_adjustments.asr.to_string();
        let maghrib_adj = self.config.prayer_adjustments.maghrib.to_string();
        let isha_adj = self.config.prayer_adjustments.isha.to_string();

        settings = settings
            .add(widget::settings::item(
                fl!("fajr-adjustment"),
                widget::text_input("0", fajr_adj)
                    .on_input(Message::SetFajrAdjustment),
            ))
            .add(widget::settings::item(
                fl!("sunrise-adjustment"),
                widget::text_input("0", sunrise_adj)
                    .on_input(Message::SetSunriseAdjustment),
            ))
            .add(widget::settings::item(
                fl!("dhuhr-adjustment"),
                widget::text_input("0", dhuhr_adj)
                    .on_input(Message::SetDhuhrAdjustment),
            ))
            .add(widget::settings::item(
                fl!("asr-adjustment"),
                widget::text_input("0", asr_adj)
                    .on_input(Message::SetAsrAdjustment),
            ))
            .add(widget::settings::item(
                fl!("maghrib-adjustment"),
                widget::text_input("0", maghrib_adj)
                    .on_input(Message::SetMaghribAdjustment),
            ))
            .add(widget::settings::item(
                fl!("isha-adjustment"),
                widget::text_input("0", isha_adj)
                    .on_input(Message::SetIshaAdjustment),
            ));

        // Notification settings section
        settings = settings.add(widget::divider::horizontal::default());

        settings = settings
            .add(widget::settings::item(
                fl!("fajr-notification"),
                widget::toggler(self.config.notification_settings.fajr).on_toggle(Message::SetNotificationFajr),
            ))
            .add(widget::settings::item(
                fl!("dhuhr-notification"),
                widget::toggler(self.config.notification_settings.dhuhr).on_toggle(Message::SetNotificationDhuhr),
            ))
            .add(widget::settings::item(
                fl!("asr-notification"),
                widget::toggler(self.config.notification_settings.asr).on_toggle(Message::SetNotificationAsr),
            ))
            .add(widget::settings::item(
                fl!("maghrib-notification"),
                widget::toggler(self.config.notification_settings.maghrib).on_toggle(Message::SetNotificationMaghrib),
            ))
            .add(widget::settings::item(
                fl!("isha-notification"),
                widget::toggler(self.config.notification_settings.isha).on_toggle(Message::SetNotificationIsha),
            ))
            .add(widget::settings::item(
                fl!("sound-enabled"),
                widget::toggler(self.config.notification_settings.sound_enabled).on_toggle(Message::SetSoundEnabled),
            ))
            .add(widget::settings::item(
                fl!("volume"),
                widget::text_input("70", self.config.notification_settings.volume.to_string())
                    .on_input(Message::SetVolume),
            ));

        settings.into()
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
        let mut app = AppModel {
            core,
            popup: None,
            view_mode: ViewMode::default(),
            config,
            today: None,
            tomorrow: None,
            hijri: None,
            next: None,
            active: None,
            tz: chrono_tz::UTC,
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
        let label = self.panel_label();
        self.core
            .applet
            .text_button(self.core.applet.text(label), Message::TogglePopup)
            .into()
    }

    /// The applet's popup: full day schedule, dates, and quick settings.
    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let spacing = cosmic::theme::active().cosmic().spacing;
        let header = self.view_header();

        let content = match self.view_mode {
            ViewMode::Schedule => {
                let mut children = vec![header];
                if let Some(hero) = self.view_hero_card() {
                    children.push(hero);
                }
                children.push(self.view_schedule());
                widget::column(children)
                    .spacing(spacing.space_xs)
                    .padding([spacing.space_xs, spacing.space_xs])
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
                .map(|update| Message::UpdateConfig(update.config)),
        ])
    }

    /// Handles messages emitted by the application and its widgets.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Tick => {
                self.refresh_next(Utc::now().with_timezone(&self.tz));
            }
            Message::UpdateConfig(config) => {
                self.config = config;
                self.refresh_schedule();
            }
            Message::SetViewMode(mode) => {
                self.view_mode = mode;
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
                } else if let Ok(angle) = trimmed.parse::<f64>() {
                    if (0.0..=30.0).contains(&angle) {
                        self.config.custom_angles.fajr_angle = Some(angle);
                        self.save_config();
                        self.refresh_schedule();
                    }
                }
            }
            Message::SetIshaAngle(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    self.config.custom_angles.isha_angle = None;
                    self.save_config();
                    self.refresh_schedule();
                } else if let Ok(angle) = trimmed.parse::<f64>() {
                    if (0.0..=30.0).contains(&angle) {
                        self.config.custom_angles.isha_angle = Some(angle);
                        self.save_config();
                        self.refresh_schedule();
                    }
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
            Message::SetFajrAdjustment(value) => {
                if let Ok(adj) = value.parse::<i8>() {
                    self.config.prayer_adjustments.fajr = adj;
                    self.save_config();
                    self.refresh_schedule();
                }
            }
            Message::SetSunriseAdjustment(value) => {
                if let Ok(adj) = value.parse::<i8>() {
                    self.config.prayer_adjustments.sunrise = adj;
                    self.save_config();
                    self.refresh_schedule();
                }
            }
            Message::SetDhuhrAdjustment(value) => {
                if let Ok(adj) = value.parse::<i8>() {
                    self.config.prayer_adjustments.dhuhr = adj;
                    self.save_config();
                    self.refresh_schedule();
                }
            }
            Message::SetAsrAdjustment(value) => {
                if let Ok(adj) = value.parse::<i8>() {
                    self.config.prayer_adjustments.asr = adj;
                    self.save_config();
                    self.refresh_schedule();
                }
            }
            Message::SetMaghribAdjustment(value) => {
                if let Ok(adj) = value.parse::<i8>() {
                    self.config.prayer_adjustments.maghrib = adj;
                    self.save_config();
                    self.refresh_schedule();
                }
            }
            Message::SetIshaAdjustment(value) => {
                if let Ok(adj) = value.parse::<i8>() {
                    self.config.prayer_adjustments.isha = adj;
                    self.save_config();
                    self.refresh_schedule();
                }
            }
            // Notification settings handlers
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
                self.save_config();
            }
            Message::SetVolume(value) => {
                if let Ok(volume) = value.parse::<u8>() {
                    self.config.notification_settings.volume = volume.min(100);
                    self.save_config();
                }
            }
            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    destroy_popup(p)
                } else {
                    let new_id = Id::unique();
                    self.popup.replace(new_id);
                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );
                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(380.0)
                        .min_width(320.0)
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
