//! AlwaysEnglish: keeps selected Windows apps on the English (US) keyboard layout.
//!
//! A Rust + Slint rewrite of AlwaysEnglish by ChaXxl
//! (<https://github.com/ChaXxl/AlwaysEnglish>, Apache-2.0).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod engine;
mod icons;
mod legacy;
mod platform;
mod single_instance;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};

use config::{AppEntry, CloseAction, Config, Language, Mode, Source, Theme as ThemeSetting};
use engine::Engine;

slint::include_modules!();

const ICON_SIZE: u32 = 64;

/// Work scheduled from a winit event handler.
type Deferred = Box<dyn FnOnce(&Rc<App>)>;

/// Builds a callback that runs `body` with a strong `Rc<App>` while the app is alive,
/// holding only a weak reference so that no `Rc` cycle is created.
macro_rules! app_cb {
    ($app:expr, |$a:ident $(, $arg:ident)*| $body:expr) => {{
        let weak = Rc::downgrade(&$app);
        move |$($arg),*| {
            if let Some($a) = weak.upgrade() {
                $body;
            }
        }
    }};
}

struct App {
    ui: AppWindow,
    tray: Tray,
    config: RefCell<Config>,
    engine: RefCell<Engine>,
    mode: Cell<Mode>,
    apps: Rc<VecModel<AppItem>>,
    toast_timer: Timer,
}

impl App {
    fn strings(&self) -> Strings<'_> {
        self.ui.global::<Strings>()
    }

    fn save(&self) {
        let result = self.config.borrow().save();
        if let Err(e) = result {
            let msg = self.strings().invoke_save_failed(e.to_string().into());
            self.toast(msg);
        }
    }

    fn toast(&self, text: impl Into<SharedString>) {
        self.ui.set_toast(text.into());
        let weak = self.ui.as_weak();
        self.toast_timer.start(TimerMode::SingleShot, Duration::from_millis(2600), move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_toast(SharedString::new());
            }
        });
    }

    fn set_mode(&self, mode: Mode, announce: bool) {
        self.mode.set(mode);
        self.engine.borrow_mut().set_mode(mode);
        self.config.borrow_mut().last_mode = mode;
        self.ui.set_mode(mode.to_index());
        self.tray.set_mode(mode.to_index());
        self.save();
        if announce {
            let s = self.strings();
            let msg = match mode {
                Mode::PerApp => s.get_started_per_app(),
                Mode::Always => s.get_started_always(),
                Mode::Stopped => s.get_stopped(),
            };
            self.toast(msg);
        }
    }

    /// Starts `target` mode, or stops if it is already running.
    fn toggle_mode(&self, target: Mode) {
        let next = if self.mode.get() == target { Mode::Stopped } else { target };
        self.set_mode(next, true);
    }

    fn refresh_rules(&self) {
        self.engine.borrow_mut().set_apps(&self.config.borrow().apps);
    }

    fn item_for(entry: &AppEntry) -> AppItem {
        AppItem {
            icon: icons::file_icon(&entry.path, ICON_SIZE).unwrap_or_default(),
            name: entry.name.as_str().into(),
            path: entry.path.as_str().into(),
            enabled: entry.enabled,
            caps_lock: entry.caps_lock,
            missing: !Path::new(&entry.path).exists(),
        }
    }

    /// Adds executables or shortcuts (`.lnk`, resolved to their target).
    fn add_paths(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut message: Option<SharedString> = None;
        let mut changed = false;
        {
            let s = self.strings();
            for path in paths {
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let target = match ext.as_str() {
                    "exe" => path.to_string_lossy().into_owned(),
                    "lnk" => match platform::resolve_shortcut(&path.to_string_lossy()) {
                        Some(t) => t,
                        None => {
                            message = Some(s.invoke_shortcut_failed(file_name.into()));
                            continue;
                        }
                    },
                    _ => {
                        message = Some(s.invoke_not_exe(file_name.into()));
                        continue;
                    }
                };
                if !target.to_lowercase().ends_with(".exe") {
                    message = Some(s.invoke_not_exe(file_name.into()));
                    continue;
                }

                let mut entry = AppEntry::new(target);
                // Like the original: show the name of what was added (e.g. the shortcut's name).
                if let Some(stem) = path.file_stem() {
                    entry.name = stem.to_string_lossy().into_owned();
                }
                let key = entry.path.to_lowercase();
                if self.config.borrow().apps.iter().any(|a| a.path.to_lowercase() == key) {
                    message = Some(s.invoke_already_added(entry.name.as_str().into()));
                    continue;
                }
                self.apps.push(Self::item_for(&entry));
                message = Some(s.invoke_added(entry.name.as_str().into()));
                self.config.borrow_mut().apps.push(entry);
                changed = true;
            }
        }
        if changed {
            self.refresh_rules();
            self.save();
        }
        if let Some(m) = message {
            self.toast(m);
        }
    }

    fn update_app(&self, index: i32, f: impl FnOnce(&mut AppEntry)) {
        let Ok(i) = usize::try_from(index) else { return };
        {
            let mut cfg = self.config.borrow_mut();
            let Some(entry) = cfg.apps.get_mut(i) else { return };
            f(entry);
            if let Some(mut item) = self.apps.row_data(i) {
                item.enabled = entry.enabled;
                item.caps_lock = entry.caps_lock;
                self.apps.set_row_data(i, item);
            }
        }
        self.refresh_rules();
        self.save();
    }

    fn remove_app(&self, index: i32) {
        let Ok(i) = usize::try_from(index) else { return };
        {
            let mut cfg = self.config.borrow_mut();
            if i >= cfg.apps.len() {
                return;
            }
            cfg.apps.remove(i);
        }
        if i < self.apps.row_count() {
            self.apps.remove(i);
        }
        self.refresh_rules();
        self.save();
    }

    fn effective_dark(&self) -> bool {
        match self.config.borrow().theme {
            ThemeSetting::System => platform::system_uses_dark_theme(),
            ThemeSetting::Light => false,
            ThemeSetting::Dark => true,
        }
    }

    fn apply_theme(&self) {
        let dark = self.effective_dark();
        self.ui.invoke_set_dark(dark);
        // Title bar / window frame.
        let theme = if dark { winit::window::Theme::Dark } else { winit::window::Theme::Light };
        self.ui.window().with_winit_window(|w| {
            if w.theme() != Some(theme) {
                w.set_theme(Some(theme));
            }
        });
    }

    fn apply_language(&self) {
        let language = match self.config.borrow().language {
            Language::System if platform::system_language_is_chinese() => "zh_CN",
            Language::System | Language::English => "",
            Language::Chinese => "zh_CN",
        };
        let _ = slint::select_bundled_translation(language);
    }

    fn show_window(self: &Rc<Self>) {
        self.ui.set_english_installed(platform::is_english_installed());
        let _ = self.ui.show();
        // The native window may only exist once the event loop has processed the show request.
        let weak = Rc::downgrade(self);
        Timer::single_shot(Duration::from_millis(30), move || {
            if let Some(app) = weak.upgrade() {
                app.apply_theme();
                if let Some(hwnd) = platform::hwnd_of(app.ui.window()) {
                    platform::bring_to_front(hwnd);
                }
            }
        });
    }

    fn quit(&self) {
        self.save();
        let _ = self.tray.hide();
        let _ = self.ui.hide();
        let _ = slint::quit_event_loop();
    }

    fn on_close_requested(&self) -> CloseRequestResponse {
        let action = self.config.borrow().close_action;
        match action {
            CloseAction::Ask => {
                self.ui.set_show_close_dialog(true);
                CloseRequestResponse::KeepWindowShown
            }
            CloseAction::Minimize => CloseRequestResponse::HideWindow,
            CloseAction::Quit => {
                self.save();
                let _ = self.tray.hide();
                // Quit once the close request has been handled.
                Timer::single_shot(Duration::ZERO, || {
                    let _ = slint::quit_event_loop();
                });
                CloseRequestResponse::HideWindow
            }
        }
    }

    fn on_close_dialog_choice(&self, action: i32, remember: bool) {
        self.ui.set_show_close_dialog(false);
        match action {
            0 => {
                if remember {
                    self.config.borrow_mut().close_action = CloseAction::Minimize;
                    self.ui.set_close_action(CloseAction::Minimize.to_index());
                    self.save();
                }
                let _ = self.ui.hide();
            }
            2 => {
                if remember {
                    self.config.borrow_mut().close_action = CloseAction::Quit;
                    self.ui.set_close_action(CloseAction::Quit.to_index());
                }
                self.quit();
            }
            _ => {}
        }
    }

    fn pick_apps(&self) {
        let (title, filter) = {
            let s = self.strings();
            (s.get_pick_title(), s.get_pick_filter())
        };
        let dialog = rfd::FileDialog::new()
            .set_title(title.as_str())
            .add_filter(filter.as_str(), &["exe", "lnk"])
            .set_parent(&self.ui.window().window_handle());
        if let Some(files) = dialog.pick_files() {
            self.add_paths(files);
        }
    }
}

fn main() {
    let start_hidden = std::env::args().any(|a| a == "--minimized");

    // Single instance: a second launch brings the running window to the front and exits.
    let _instance = match single_instance::acquire() {
        single_instance::Instance::Primary(guard) => guard,
        single_instance::Instance::Secondary => {
            single_instance::notify_primary();
            return;
        }
    };

    platform::init_com();
    let (config, source) = Config::load();

    let ui = AppWindow::new().expect("failed to create the main window");
    let tray = Tray::new().expect("failed to create the tray icon");

    let apps = Rc::new(VecModel::from(config.apps.iter().map(App::item_for).collect::<Vec<_>>()));
    ui.set_apps(ModelRc::from(apps.clone()));
    ui.set_version(SharedString::from(format!("v{}", env!("CARGO_PKG_VERSION"))));
    ui.set_theme_index(config.theme.to_index());
    ui.set_language_index(config.language.to_index());
    ui.set_auto_start(config.auto_start);
    ui.set_resume_mode(config.resume_last_mode);
    ui.set_always_caps(config.always_caps_lock);
    ui.set_close_action(config.close_action.to_index());
    ui.set_english_installed(platform::is_english_installed());

    let mut engine = Engine::new();
    engine.set_apps(&config.apps);
    engine.set_always_caps_lock(config.always_caps_lock);

    let initial_mode = if config.resume_last_mode { config.last_mode } else { Mode::Stopped };
    let auto_start = config.auto_start;

    let app = Rc::new(App {
        ui,
        tray,
        config: RefCell::new(config),
        engine: RefCell::new(engine),
        mode: Cell::new(Mode::Stopped),
        apps,
        toast_timer: Timer::default(),
    });

    app.apply_language();
    app.apply_theme();
    if source == Source::MigratedFromQt {
        app.save();
    }
    if auto_start {
        // Keep the Run entry pointing at the current executable.
        let _ = platform::set_auto_start(true);
    }
    app.set_mode(initial_mode, false);

    // --- Home page ---
    let ui = &app.ui;
    ui.on_toggle_per_app(app_cb!(app, |a| a.toggle_mode(Mode::PerApp)));
    ui.on_toggle_always(app_cb!(app, |a| a.toggle_mode(Mode::Always)));
    ui.on_add_app(app_cb!(app, |a| a.pick_apps()));
    ui.on_install_english(platform::open_language_settings);
    ui.on_enabled_toggled(app_cb!(app, |a, i, on| a.update_app(i, |e| e.enabled = on)));
    ui.on_caps_toggled(app_cb!(app, |a, i, on| a.update_app(i, |e| e.caps_lock = on)));
    ui.on_remove_app(app_cb!(app, |a, i| a.remove_app(i)));

    // --- Settings page ---
    ui.on_theme_selected(app_cb!(app, |a, i| {
        a.config.borrow_mut().theme = ThemeSetting::from_index(i);
        a.save();
        a.apply_theme();
    }));
    ui.on_language_selected(app_cb!(app, |a, i| {
        a.config.borrow_mut().language = Language::from_index(i);
        a.save();
        a.apply_language();
    }));
    ui.on_auto_start_toggled(app_cb!(app, |a, on| {
        if platform::set_auto_start(on).is_ok() {
            a.config.borrow_mut().auto_start = on;
            a.save();
        } else {
            a.ui.set_auto_start(!on);
            let msg = a.strings().get_autostart_failed();
            a.toast(msg);
        }
    }));
    ui.on_resume_mode_toggled(app_cb!(app, |a, on| {
        a.config.borrow_mut().resume_last_mode = on;
        a.save();
    }));
    ui.on_always_caps_toggled(app_cb!(app, |a, on| {
        a.config.borrow_mut().always_caps_lock = on;
        a.engine.borrow_mut().set_always_caps_lock(on);
        a.save();
    }));
    ui.on_close_action_selected(app_cb!(app, |a, i| {
        a.config.borrow_mut().close_action = CloseAction::from_index(i);
        a.save();
    }));
    ui.on_open_url(|url| platform::shell_open(&url));

    // --- Window: close button, show requests, drag & drop, system theme changes ---
    ui.on_close_dialog_choice(app_cb!(app, |a, action, remember| a.on_close_dialog_choice(action, remember)));
    ui.on_request_show(app_cb!(app, |a| a.show_window()));
    {
        let weak = Rc::downgrade(&app);
        ui.window().on_close_requested(move || match weak.upgrade() {
            Some(a) => a.on_close_requested(),
            None => CloseRequestResponse::HideWindow,
        });
    }
    {
        let weak = Rc::downgrade(&app);
        ui.window().on_winit_window_event(move |_, event| {
            // Handle the event after winit's dispatch so that the UI can be changed safely.
            let defer = |f: Deferred| {
                let weak = weak.clone();
                Timer::single_shot(Duration::ZERO, move || {
                    if let Some(a) = weak.upgrade() {
                        f(&a);
                    }
                });
            };
            match event {
                winit::event::WindowEvent::HoveredFile(_) => defer(Box::new(|a| a.ui.set_drop_hover(true))),
                winit::event::WindowEvent::HoveredFileCancelled => defer(Box::new(|a| a.ui.set_drop_hover(false))),
                winit::event::WindowEvent::DroppedFile(path) => {
                    let path = path.clone();
                    defer(Box::new(move |a| {
                        a.ui.set_drop_hover(false);
                        a.add_paths([path]);
                    }));
                }
                winit::event::WindowEvent::ThemeChanged(_) => defer(Box::new(|a| a.apply_theme())),
                _ => {}
            }
            EventResult::Propagate
        });
    }

    // --- Tray ---
    let tray = &app.tray;
    tray.on_show_window(app_cb!(app, |a| a.show_window()));
    tray.on_toggle_per_app(app_cb!(app, |a| a.toggle_mode(Mode::PerApp)));
    tray.on_toggle_always(app_cb!(app, |a| a.toggle_mode(Mode::Always)));
    tray.on_quit(app_cb!(app, |a| a.quit()));
    let _ = tray.show();

    // --- Monitoring loop ---
    let tick_timer = Timer::default();
    {
        let weak = Rc::downgrade(&app);
        tick_timer.start(TimerMode::Repeated, engine::TICK, move || {
            if let Some(a) = weak.upgrade() {
                a.engine.borrow_mut().tick();
            }
        });
    }

    // --- Single instance: another launch asks us to show the window ---
    {
        let weak = app.ui.as_weak();
        single_instance::listen(move || {
            let _ = weak.upgrade_in_event_loop(|ui| ui.invoke_request_show());
        });
    }

    if !start_hidden {
        app.show_window();
    }

    slint::run_event_loop_until_quit().expect("event loop failed");
    drop(tick_timer);
}
