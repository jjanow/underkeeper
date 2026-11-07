use crate::classchange;
use crate::dft::{Doc, Entry, Key, Value};
use crate::gamedata::{GameData, ItemReq, prettify};
use crate::paths::{self, SaveDir};
use crate::save::{self, SaveFile, SaveSummary};
use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, ThemePreference, Vec2};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const ACCENT: Color32 = Color32::from_rgb(0xE2, 0x8A, 0x3B); // ember
const ACCENT_DARK: Color32 = Color32::from_rgb(0xB8, 0x62, 0x1B);
const OK_GREEN: Color32 = Color32::from_rgb(0x4C, 0xAF, 0x50);
const BAD_RED: Color32 = Color32::from_rgb(0xE5, 0x48, 0x4D);

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Stats,
    Skills,
    Gear,
    Advanced,
}

/// A class change waiting for the user to confirm it.
struct PendingClass {
    hero: usize,
    class: String,
    starter: String,
    give_kit: bool,
}

struct Status {
    text: String,
    error: bool,
    at: Instant,
}

pub struct App {
    gd: GameData,
    game_dir: Option<PathBuf>,
    save_dirs: Vec<SaveDir>,
    dir_idx: usize,
    saves: Vec<SaveSummary>,
    save: Option<SaveFile>,
    hero: usize,
    tab: Tab,
    sync_exp: bool,
    gear_filter: String,
    gear_unmet_only: bool,
    gear_class_only: bool,
    trait_pick: String,
    class_change: Option<PendingClass>,
    status: Option<Status>,
    pending_open: Option<PathBuf>,
    confirm_discard: bool,
    sys: Option<sysinfo::System>,
    game_running: bool,
    last_proc_check: Instant,
    theme: ThemePreference,
    backup_dir: PathBuf,
}

fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("underkeeper").join("settings.txt"))
}

fn load_setting(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(settings_path()?).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")).map(|v| v.to_string()))
}

fn store_setting(key: &str, value: &str) {
    let Some(p) = settings_path() else { return };
    let mut lines: Vec<String> = std::fs::read_to_string(&p)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with(&format!("{key}=")))
        .map(String::from)
        .collect();
    lines.push(format!("{key}={value}"));
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(p, lines.join("\n") + "\n");
}

fn open_in_file_manager(p: &std::path::Path) {
    let _ = std::fs::create_dir_all(p);
    #[cfg(windows)]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd).arg(p).spawn();
}

pub fn setup_style(ctx: &egui::Context) {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| {
            use egui::{FontId, TextStyle};
            style.text_styles = [
                (TextStyle::Small, FontId::proportional(12.0)),
                (TextStyle::Body, FontId::proportional(15.0)),
                (TextStyle::Button, FontId::proportional(15.0)),
                (TextStyle::Monospace, FontId::monospace(14.0)),
                (TextStyle::Heading, FontId::proportional(22.0)),
            ]
            .into();
            style.spacing.item_spacing = Vec2::new(8.0, 6.0);
            style.spacing.button_padding = Vec2::new(10.0, 5.0);
            style.spacing.interact_size.y = 26.0;
            let v = &mut style.visuals;
            let r = CornerRadius::same(6);
            v.widgets.noninteractive.corner_radius = r;
            v.widgets.inactive.corner_radius = r;
            v.widgets.hovered.corner_radius = r;
            v.widgets.active.corner_radius = r;
            v.widgets.open.corner_radius = r;
            v.window_corner_radius = CornerRadius::same(10);
            v.selection.bg_fill = if theme == egui::Theme::Dark {
                ACCENT_DARK
            } else {
                ACCENT
            };
            v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
            v.hyperlink_color = ACCENT;
            if theme == egui::Theme::Dark {
                v.panel_fill = Color32::from_rgb(0x17, 0x18, 0x1C);
                v.window_fill = Color32::from_rgb(0x1E, 0x20, 0x25);
                v.extreme_bg_color = Color32::from_rgb(0x10, 0x11, 0x14);
                v.faint_bg_color = Color32::from_rgb(0x22, 0x24, 0x2A);
            } else {
                v.panel_fill = Color32::from_rgb(0xF4, 0xF2, 0xEF);
                v.window_fill = Color32::from_rgb(0xFB, 0xFA, 0xF8);
                v.faint_bg_color = Color32::from_rgb(0xEC, 0xE9, 0xE4);
            }
        });
    }
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let fill = if ui.visuals().dark_mode {
        Color32::from_rgb(0x20, 0x22, 0x28)
    } else {
        Color32::from_rgb(0xFF, 0xFF, 0xFF)
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong().size(15.0));
    ui.add_space(2.0);
}

fn fmt_age(t: std::time::SystemTime) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86399 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86400),
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        setup_style(&cc.egui_ctx);
        let theme = match load_setting("theme").as_deref() {
            Some("dark") => ThemePreference::Dark,
            Some("light") => ThemePreference::Light,
            _ => ThemePreference::System,
        };
        cc.egui_ctx.set_theme(theme);

        let game_dir = load_setting("game_dir")
            .map(PathBuf::from)
            .filter(|p| p.join("base").is_dir())
            .or_else(paths::find_game_dir);
        let gd = game_dir
            .as_deref()
            .and_then(GameData::load)
            .unwrap_or_default();

        let mut app = App {
            gd,
            game_dir,
            save_dirs: Vec::new(),
            dir_idx: 0,
            saves: Vec::new(),
            save: None,
            hero: 0,
            tab: Tab::Stats,
            sync_exp: true,
            gear_filter: String::new(),
            gear_unmet_only: false,
            gear_class_only: false,
            trait_pick: String::new(),
            class_change: None,
            status: None,
            pending_open: None,
            confirm_discard: false,
            sys: sysinfo::System::new().ok(),
            game_running: false,
            last_proc_check: Instant::now() - Duration::from_secs(10),
            theme,
            backup_dir: dirs::data_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("underkeeper")
                .join("backups"),
        };
        app.rescan_dirs();
        // For screenshots/testing: open a save and tab directly.
        if let Ok(p) = std::env::var("UNDERKEEPER_OPEN") {
            app.open(PathBuf::from(p));
            app.tab = match std::env::var("UNDERKEEPER_TAB").as_deref() {
                Ok("skills") => Tab::Skills,
                Ok("gear") => Tab::Gear,
                Ok("advanced") => Tab::Advanced,
                _ => Tab::Stats,
            };
            app.hero = std::env::var("UNDERKEEPER_HERO")
                .ok()
                .and_then(|h| h.parse().ok())
                .unwrap_or(0);
            if let Ok(class) = std::env::var("UNDERKEEPER_CLASS") {
                let starter = app.gd.starter_choices(&class).first().map(|k| k.id.clone());
                app.class_change = Some(PendingClass {
                    hero: app.hero,
                    starter: starter.unwrap_or_default(),
                    class,
                    give_kit: true,
                });
            }
        }
        if let Some(custom) = load_setting("save_dir").map(PathBuf::from)
            && custom.is_dir()
        {
            app.add_custom_dir(custom);
        }
        app
    }

    fn set_status(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some(Status {
            text: text.into(),
            error,
            at: Instant::now(),
        });
    }

    fn rescan_dirs(&mut self) {
        let current = self.save_dirs.get(self.dir_idx).map(|d| d.path.clone());
        let customs: Vec<SaveDir> = self
            .save_dirs
            .iter()
            .filter(|d| d.label == "Chosen folder")
            .cloned()
            .collect();
        self.save_dirs = paths::find_save_dirs();
        for c in customs {
            if !self.save_dirs.iter().any(|d| d.path == c.path) {
                self.save_dirs.push(c);
            }
        }
        self.dir_idx = current
            .and_then(|c| self.save_dirs.iter().position(|d| d.path == c))
            .unwrap_or(0);
        self.refresh_saves();
    }

    fn add_custom_dir(&mut self, path: PathBuf) {
        if let Some(i) = self.save_dirs.iter().position(|d| d.path == path) {
            self.dir_idx = i;
        } else {
            self.save_dirs.push(SaveDir {
                path,
                label: "Chosen folder".into(),
            });
            self.dir_idx = self.save_dirs.len() - 1;
        }
        self.refresh_saves();
    }

    fn refresh_saves(&mut self) {
        self.saves = self
            .save_dirs
            .get(self.dir_idx)
            .map(|d| save::list_saves(&d.path))
            .unwrap_or_default();
    }

    fn open(&mut self, path: PathBuf) {
        self.class_change = None;
        match SaveFile::open(&path) {
            Ok(f) => {
                if f.hero_count() == 0 {
                    self.set_status("That file has no party in it.", true);
                    return;
                }
                self.hero = self.hero.min(f.hero_count() - 1);
                self.save = Some(f);
                self.set_status(
                    format!(
                        "Opened {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    false,
                );
            }
            Err(e) => self.set_status(e, true),
        }
    }

    fn request_open(&mut self, path: PathBuf) {
        if self.save.as_ref().is_some_and(|s| s.is_dirty()) {
            self.pending_open = Some(path);
            self.confirm_discard = true;
        } else {
            self.open(path);
        }
    }

    fn check_game_running(&mut self) {
        if self.last_proc_check.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_proc_check = Instant::now();
        let Some(sys) = &mut self.sys else { return };
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        self.game_running = sys.processes().values().any(|p| {
            let name = p.name().to_string_lossy().to_lowercase();
            let exe = p
                .exe()
                .and_then(|e| e.file_name())
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let cmd = p
                .cmd()
                .first()
                .map(|c| c.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            name == "underkeep.exe"
                || exe == "underkeep.exe"
                || cmd.ends_with("\\underkeep.exe")
                || cmd.ends_with("/underkeep.exe")
        });
    }

    fn do_save(&mut self) {
        if self.game_running {
            self.set_status(
                "Underkeep is running. Quit the game to desktop first, then save.",
                true,
            );
            return;
        }
        let Some(s) = &mut self.save else { return };
        // Has the game written the file since we opened it?
        let on_disk = std::fs::metadata(&s.path).and_then(|m| m.modified()).ok();
        if on_disk.is_some_and(|m| m != s.modified) {
            self.set_status(
                "The save changed on disk since it was opened (the game saved again?). Reopen it and redo your edits.",
                true,
            );
            return;
        }
        let bytes = s.to_bytes();
        match save::write_with_backup(&s.path, &bytes, &self.backup_dir) {
            Ok(backup) => {
                let path = s.path.clone();
                self.open(path);
                self.refresh_saves();
                self.set_status(
                    format!("Saved. Backup of the old file: {}", backup.display()),
                    false,
                );
            }
            Err(e) => self.set_status(e, true),
        }
    }

    // ---------------- panels ----------------

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Underkeeper")
                    .size(20.0)
                    .strong()
                    .color(ACCENT),
            );
            ui.label(RichText::new("character editor for Underkeep").weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let before = self.theme;
                egui::ComboBox::from_id_salt("theme")
                    .selected_text(match self.theme {
                        ThemePreference::Dark => "Dark",
                        ThemePreference::Light => "Light",
                        ThemePreference::System => "Match system",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.theme,
                            ThemePreference::System,
                            "Match system",
                        );
                        ui.selectable_value(&mut self.theme, ThemePreference::Dark, "Dark");
                        ui.selectable_value(&mut self.theme, ThemePreference::Light, "Light");
                    });
                if before != self.theme {
                    ui.ctx().set_theme(self.theme);
                    store_setting(
                        "theme",
                        match self.theme {
                            ThemePreference::Dark => "dark",
                            ThemePreference::Light => "light",
                            ThemePreference::System => "system",
                        },
                    );
                }
            });
        });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        section_title(ui, "Save folder");
        let label = self
            .save_dirs
            .get(self.dir_idx)
            .map(|d| d.label.clone())
            .unwrap_or_else(|| "No save folder found".into());
        let mut changed = false;
        egui::ComboBox::from_id_salt("savedir")
            .width(ui.available_width())
            .selected_text(label)
            .show_ui(ui, |ui| {
                for (i, d) in self.save_dirs.iter().enumerate() {
                    if ui
                        .selectable_label(i == self.dir_idx, &d.label)
                        .on_hover_text(d.path.display().to_string())
                        .clicked()
                    {
                        self.dir_idx = i;
                        changed = true;
                    }
                }
            });
        if changed {
            self.refresh_saves();
        }
        if let Some(d) = self.save_dirs.get(self.dir_idx) {
            ui.label(RichText::new(d.path.display().to_string()).small().weak());
        }
        ui.horizontal(|ui| {
            if ui
                .button("Choose folder…")
                .on_hover_text("Pick the folder that holds save_1, save_auto, ...")
                .clicked()
                && let Some(p) = rfd::FileDialog::new()
                    .set_title("Underkeep save folder")
                    .pick_folder()
            {
                store_setting("save_dir", &p.display().to_string());
                self.add_custom_dir(p);
            }
            if ui.button("Rescan").clicked() {
                self.rescan_dirs();
            }
        });

        ui.add_space(10.0);
        section_title(ui, "Saves");
        if self.saves.is_empty() {
            ui.label(
                RichText::new("No saves here yet. Play until the game saves, then rescan.").weak(),
            );
        }
        let open_path = self.save.as_ref().map(|s| s.path.clone());
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for s in &self.saves {
                    let selected = open_path.as_ref() == Some(&s.path);
                    let fill = if selected {
                        ui.visuals().selection.bg_fill.gamma_multiply(0.35)
                    } else {
                        ui.visuals().faint_bg_color
                    };
                    let resp = egui::Frame::new()
                        .fill(fill)
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::same(10))
                        .stroke(if selected {
                            Stroke::new(1.5, ACCENT)
                        } else {
                            Stroke::NONE
                        })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let title = match s.file_name.as_str() {
                                "save_auto" => "Autosave".to_string(),
                                "save_quick" => "Quicksave".to_string(),
                                n => n
                                    .strip_prefix("save_")
                                    .map(|x| format!("Slot {x}"))
                                    .unwrap_or(n.to_string()),
                            };
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(title).strong());
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(RichText::new(fmt_age(s.modified)).small().weak());
                                    },
                                );
                            });
                            if let Some(e) = &s.error {
                                ui.label(RichText::new(e).small().color(BAD_RED));
                            } else {
                                ui.label(
                                    RichText::new(format!(
                                        "{} · {}h {:02}m",
                                        s.area,
                                        s.play_time_min / 60,
                                        s.play_time_min % 60
                                    ))
                                    .small(),
                                );
                                let party: Vec<String> = s
                                    .heroes
                                    .iter()
                                    .map(|(n, c, l)| format!("{n} ({} {l})", self.gd.display(c)))
                                    .collect();
                                ui.label(RichText::new(party.join(", ")).small().weak());
                            }
                        })
                        .response
                        .interact(egui::Sense::click());
                    if resp.clicked() {
                        clicked = Some(s.path.clone());
                    }
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    ui.add_space(4.0);
                }
            });
        if let Some(p) = clicked {
            self.request_open(p);
        }
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if self.game_running {
                ui.label(
                    RichText::new("Underkeep is running: quit to desktop before saving")
                        .color(BAD_RED),
                );
            } else if let Some(st) = &self.status {
                let c = if st.error {
                    BAD_RED
                } else {
                    ui.visuals().text_color()
                };
                ui.label(RichText::new(&st.text).color(c));
            } else {
                ui.label(
                    RichText::new(
                        "Close the game before saving. A backup is made every time you save.",
                    )
                    .weak(),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let dirty = self.save.as_ref().is_some_and(|s| s.is_dirty());
                let save_btn =
                    egui::Button::new(RichText::new("Save changes").strong().color(Color32::WHITE))
                        .fill(if dirty && !self.game_running {
                            ACCENT_DARK
                        } else {
                            Color32::from_gray(90)
                        })
                        .min_size(Vec2::new(140.0, 30.0));
                if ui
                    .add_enabled(dirty && !self.game_running, save_btn)
                    .clicked()
                {
                    self.do_save();
                }
                if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked()
                    && let Some(p) = self.save.as_ref().map(|s| s.path.clone())
                {
                    self.open(p);
                    self.set_status("Changes discarded.", false);
                }
                if ui
                    .button("Backups")
                    .on_hover_text(self.backup_dir.display().to_string())
                    .clicked()
                {
                    open_in_file_manager(&self.backup_dir);
                }
                if dirty {
                    ui.label(RichText::new("Unsaved changes").color(ACCENT));
                }
            });
        });
        ui.add_space(4.0);
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(RichText::new("Pick a save on the left").size(22.0).strong());
            ui.add_space(8.0);
            ui.label("Edit your heroes' attributes, level, health, skills and traits.");
            ui.label("Quit Underkeep to the desktop before saving your changes.");
            ui.add_space(16.0);
            match &self.game_dir {
                Some(d) => ui.label(RichText::new(format!("Game data: {}", d.display())).small().weak()),
                None => ui.label(
                    RichText::new(
                        "Game install not found: names, skill limits and item requirements are unavailable.",
                    )
                    .small()
                    .color(ACCENT),
                ),
            };
            if ui.small_button("Set game folder…").clicked()
                && let Some(p) = rfd::FileDialog::new().set_title("Underkeep install folder").pick_folder() {
                    if let Some(gd) = GameData::load(&p) {
                        store_setting("game_dir", &p.display().to_string());
                        self.gd = gd;
                        self.game_dir = Some(p);
                    } else {
                        self.set_status("That folder doesn't look like an Underkeep install (no base folder).", true);
                    }
                }
        });
    }

    fn class_change_dialog(&mut self, ctx: &egui::Context) {
        let (Some(pc), Some(s)) = (&mut self.class_change, &mut self.save) else {
            self.class_change = None;
            return;
        };
        let gd = &self.gd;
        let h = pc.hero;
        if h >= s.hero_count() {
            self.class_change = None;
            return;
        }
        let name = s.hero_str(h, "name").unwrap_or_default();
        let old = gd.display(&s.hero_str(h, "class").unwrap_or_default());
        let new = gd.display(&pc.class);
        let mut close = false;
        let mut applied = false;
        egui::Window::new(format!("Make {name} a {new}?"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_max_width(460.0);
                let choices = gd.starter_choices(&pc.class);
                if !choices.is_empty() {
                    let label = if pc.class == "voidcaller" {
                        "Active aura"
                    } else {
                        "Starting skill"
                    };
                    ui.horizontal(|ui| {
                        ui.label(label);
                        egui::ComboBox::from_id_salt("starter")
                            .selected_text(gd.display(&pc.starter))
                            .show_ui(ui, |ui| {
                                for k in &choices {
                                    ui.selectable_value(
                                        &mut pc.starter,
                                        k.id.clone(),
                                        gd.display(&k.id),
                                    );
                                }
                            });
                    });
                    ui.checkbox(&mut pc.give_kit, format!("Add the {new} starting items"));
                    ui.add_space(6.0);
                }

                let plan = classchange::plan(s, gd, h, &pc.class, &pc.starter, pc.give_kit);
                let mut lines: Vec<String> = Vec::new();
                if plan.class_only {
                    lines.push(
                        "Game data wasn't found, so only the class is changed. \
                         Skills, gear and items are left as they are."
                            .into(),
                    );
                } else {
                    let free: Vec<String> = gd
                        .free_skills(&pc.class)
                        .iter()
                        .filter(|id| **id != pc.starter)
                        .map(|id| gd.display(id))
                        .collect();
                    let mut sk = format!(
                        "{old} skills are replaced by {new} skills: {} at level 1",
                        gd.display(&pc.starter)
                    );
                    if !free.is_empty() {
                        sk += &format!(", plus {} (free for every {new})", free.join(", "));
                    }
                    lines.push(sk + ".");
                    lines.push(format!(
                        "Skill points: {} -> {} (levels earned in {old} skills are refunded).",
                        plan.skill_points_before, plan.skill_points_after
                    ));
                    if let Some((field, from, to)) = &plan.attr_raise {
                        let short = ATTRS
                            .iter()
                            .find(|a| a.0 == field)
                            .map_or(field.as_str(), |a| a.2);
                        lines.push(format!(
                            "{short} {from} -> {to}, the lowest a new {new} starts with."
                        ));
                    }
                    for (_, id) in &plan.unequip {
                        lines.push(format!(
                            "{} can't be used by a {new}; it goes to the party inventory.",
                            gd.display(id)
                        ));
                    }
                    if !plan.kit.is_empty() || plan.kit_coins > 0 {
                        let mut items: Vec<String> = plan
                            .kit
                            .iter()
                            .map(|(id, n)| {
                                if *n > 1 {
                                    format!("{n}× {}", gd.display(id))
                                } else {
                                    gd.display(id)
                                }
                            })
                            .collect();
                        if plan.kit_coins > 0 {
                            items.push(format!("{} coins", plan.kit_coins));
                        }
                        lines.push(format!("Added to the inventory: {}.", items.join(", ")));
                    } else if pc.give_kit {
                        lines.push(format!("The game has no starting items for a {new}."));
                    }
                    if s.hero_str(h, "class").as_deref() == Some("paladin") {
                        lines.push("Zeal is reset.".into());
                    }
                }
                for l in lines {
                    ui.label(format!("• {l}"));
                }
                ui.label(
                    RichText::new(
                        "Health, mana and stamina maximums are recalculated by the game.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(format!("Change to {new}")).color(Color32::WHITE),
                            )
                            .fill(ACCENT_DARK),
                        )
                        .clicked()
                    {
                        classchange::apply(s, gd, h, &plan);
                        applied = true;
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        if close {
            self.class_change = None;
        }
        if applied {
            self.set_status(format!("{name} is now a {new}. Save to keep it."), false);
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let Some(s) = &mut self.save else { return };
        let gd = &self.gd;

        // Header
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(s.path.file_name().unwrap_or_default().to_string_lossy())
                    .size(18.0)
                    .strong(),
            );
            let area = s
                .root_str("current_level")
                .unwrap_or_default()
                .replace('_', " ");
            let diff = s.root_str("difficulty").unwrap_or_default();
            ui.label(RichText::new(format!("{area} · {diff}")).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut coins = s.party_int("coins").unwrap_or(0);
                if ui
                    .add(
                        egui::DragValue::new(&mut coins)
                            .range(0..=9_999_999)
                            .speed(5),
                    )
                    .changed()
                {
                    s.set_party_int("coins", coins);
                }
                ui.label("Coins");
            });
        });
        ui.add_space(6.0);

        // Hero picker
        let n = s.hero_count();
        ui.horizontal_wrapped(|ui| {
            for h in 0..n {
                let name = s.hero_str(h, "name").unwrap_or_default();
                let class = gd.display(&s.hero_str(h, "class").unwrap_or_default());
                let race = gd.display(&s.hero_str(h, "race").unwrap_or_default());
                let lvl = s.hero_int(h, "level").unwrap_or(0);
                let sel = self.hero == h;
                let fill = if sel {
                    ACCENT_DARK
                } else {
                    ui.visuals().faint_bg_color
                };
                let txt = if sel {
                    Color32::WHITE
                } else {
                    ui.visuals().text_color()
                };
                let resp = egui::Frame::new()
                    .fill(fill)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(name).strong().color(txt));
                            ui.label(
                                RichText::new(format!("Lv {lvl} {race} {class}"))
                                    .small()
                                    .color(txt),
                            );
                        });
                    })
                    .response
                    .interact(egui::Sense::click());
                if resp.clicked() {
                    self.hero = h;
                }
            }
        });
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            for (t, label) in [
                (Tab::Stats, "Stats"),
                (Tab::Skills, "Skills & traits"),
                (Tab::Gear, "Gear requirements"),
                (Tab::Advanced, "Advanced"),
            ] {
                ui.selectable_value(&mut self.tab, t, RichText::new(label).size(14.0));
            }
        });
        ui.separator();

        let h = self.hero.min(n.saturating_sub(1));
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match self.tab {
                Tab::Stats => stats_tab(ui, s, gd, h, &mut self.sync_exp, &mut self.class_change),
                Tab::Skills => skills_tab(ui, s, gd, h, &mut self.trait_pick),
                Tab::Gear => gear_tab(
                    ui,
                    s,
                    gd,
                    h,
                    &mut self.gear_filter,
                    &mut self.gear_unmet_only,
                    &mut self.gear_class_only,
                ),
                Tab::Advanced => advanced_tab(ui, s, h),
            });
    }
}

const ATTRS: [(&str, &str, &str); 4] = [
    ("base_str", "Strength", "STR"),
    ("base_dex", "Dexterity", "DEX"),
    ("base_end", "Endurance", "END"),
    ("base_wis", "Wisdom", "WIS"),
];

fn int_row(
    ui: &mut egui::Ui,
    s: &mut SaveFile,
    h: usize,
    field: &str,
    label: &str,
    range: std::ops::RangeInclusive<i32>,
    hint: &str,
) {
    ui.label(label).on_hover_text(hint);
    let mut v = s.hero_int(h, field).unwrap_or(0);
    if ui
        .add(egui::DragValue::new(&mut v).range(range).speed(0.2))
        .on_hover_text(hint)
        .changed()
    {
        s.set_hero_int(h, field, v);
    }
    ui.end_row();
}

fn stats_tab(
    ui: &mut egui::Ui,
    s: &mut SaveFile,
    gd: &GameData,
    h: usize,
    sync_exp: &mut bool,
    class_change: &mut Option<PendingClass>,
) {
    ui.add_space(4.0);
    ui.columns(2, |cols| {
        // ---- Left column: identity + attributes ----
        let ui = &mut cols[0];
        card(ui, |ui| {
            section_title(ui, "Identity");
            egui::Grid::new("identity")
                .num_columns(2)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Name");
                    let mut name = s.hero_str(h, "name").unwrap_or_default();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut name)
                                .desired_width(180.0)
                                .char_limit(24),
                        )
                        .changed()
                    {
                        s.set_hero_str(h, "name", &name);
                    }
                    ui.end_row();

                    ui.label("Class");
                    let class = s.hero_str(h, "class").unwrap_or_default();
                    let mut new_class = class.clone();
                    egui::ComboBox::from_id_salt("class")
                        .selected_text(gd.display(&class))
                        .show_ui(ui, |ui| {
                            for c in &gd.classes {
                                ui.selectable_value(&mut new_class, c.clone(), gd.display(c));
                            }
                        })
                        .response
                        .on_hover_text(
                            "Change class: new skills (spent skill points are refunded), \
                             class gear moved to the inventory, starting items added. \
                             You'll see what changes before anything is applied.",
                        );
                    if new_class != class {
                        let starter = gd
                            .starter_choices(&new_class)
                            .first()
                            .map(|k| k.id.clone())
                            .unwrap_or_default();
                        *class_change = Some(PendingClass {
                            hero: h,
                            class: new_class,
                            starter,
                            give_kit: true,
                        });
                    }
                    ui.end_row();

                    ui.label("Race");
                    let race = s.hero_str(h, "race").unwrap_or_default();
                    let mut new_race = race.clone();
                    egui::ComboBox::from_id_salt("race")
                        .selected_text(gd.display(&race))
                        .show_ui(ui, |ui| {
                            for r in &gd.races {
                                ui.selectable_value(&mut new_race, r.clone(), gd.display(r));
                            }
                        });
                    if new_race != race {
                        s.set_hero_str(h, "race", &new_race);
                    }
                    ui.end_row();
                });
        });
        ui.add_space(8.0);
        card(ui, |ui| {
            section_title(ui, "Attributes");
            ui.label(
                RichText::new("Base values. Gear bonuses (like +WIS rings) add on top in game.")
                    .small()
                    .weak(),
            );
            ui.add_space(4.0);
            egui::Grid::new("attrs")
                .num_columns(3)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    for (field, label, short) in ATTRS {
                        ui.label(format!("{label} ({short})"));
                        let mut v = s.hero_int(h, field).unwrap_or(0);
                        let r = ui.add(
                            egui::Slider::new(&mut v, 0..=30).clamping(egui::SliderClamping::Never),
                        );
                        if r.changed() {
                            s.set_hero_int(h, field, v.clamp(0, 999));
                        }
                        let max_req = gd
                            .items
                            .iter()
                            .map(|i| match short {
                                "STR" => i.req_str,
                                "DEX" => i.req_dex,
                                "END" => i.req_end,
                                _ => i.req_wis,
                            })
                            .max()
                            .unwrap_or(0);
                        if max_req > 0 {
                            let ok = v >= max_req;
                            ui.label(
                                RichText::new(if ok {
                                    "meets all gear".to_string()
                                } else {
                                    format!("gear needs up to {max_req}")
                                })
                                .small()
                                .color(if ok {
                                    OK_GREEN
                                } else {
                                    ui.visuals().weak_text_color()
                                }),
                            );
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                });
        });

        // ---- Right column: progression + vitals ----
        let ui = &mut cols[1];
        card(ui, |ui| {
            section_title(ui, "Progression");
            egui::Grid::new("prog")
                .num_columns(2)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Level");
                    let mut lvl = s.hero_int(h, "level").unwrap_or(1);
                    if ui
                        .add(
                            egui::DragValue::new(&mut lvl)
                                .range(1..=gd.max_level())
                                .speed(0.1),
                        )
                        .changed()
                    {
                        s.set_hero_int(h, "level", lvl);
                        if *sync_exp && let Some(e) = gd.exp_for_level(lvl) {
                            s.set_hero_int(h, "exp", e);
                        }
                    }
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(sync_exp, "Set experience to match level");
                    ui.end_row();

                    ui.label("Experience");
                    let mut exp = s.hero_int(h, "exp").unwrap_or(0);
                    if ui
                        .add(
                            egui::DragValue::new(&mut exp)
                                .range(0..=10_000_000)
                                .speed(10),
                        )
                        .changed()
                    {
                        s.set_hero_int(h, "exp", exp);
                    }
                    ui.end_row();
                    if let Some(next) = gd.exp_for_level(s.hero_int(h, "level").unwrap_or(1) + 1) {
                        ui.label("");
                        ui.label(
                            RichText::new(format!("Next level at {next}"))
                                .small()
                                .weak(),
                        );
                        ui.end_row();
                    }

                    int_row(
                        ui,
                        s,
                        h,
                        "skill_points",
                        "Skill points",
                        0..=999,
                        "Unspent skill points",
                    );
                });
        });
        ui.add_space(8.0);
        card(ui, |ui| {
            section_title(ui, "Current resources");
            ui.label(
                RichText::new("Current values. Maximums come from class, level and attributes.")
                    .small()
                    .weak(),
            );
            ui.add_space(4.0);
            egui::Grid::new("vitals")
                .num_columns(2)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    int_row(ui, s, h, "hp", "Health", 0..=9999, "Current hit points");
                    int_row(ui, s, h, "mana", "Mana", 0..=9999, "Current mana");
                    int_row(ui, s, h, "stamina", "Stamina", 0..=9999, "Current stamina");
                    int_row(ui, s, h, "zeals", "Zeal", 0..=99, "Paladin zeal charges");
                });
        });
    });
}

fn skills_tab(
    ui: &mut egui::Ui,
    s: &mut SaveFile,
    gd: &GameData,
    h: usize,
    trait_pick: &mut String,
) {
    ui.add_space(4.0);
    card(ui, |ui| {
        section_title(ui, "Skills");
        let class = s.hero_str(h, "class").unwrap_or_default();
        let skills = s.skills(h);
        egui::Grid::new("skills")
            .num_columns(3)
            .striped(true)
            .spacing([20.0, 6.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Skill").strong());
                ui.label(RichText::new("Level").strong());
                ui.label(RichText::new("").strong());
                ui.end_row();
                for (id, lvl) in &skills {
                    let info = gd.skill(id);
                    let max = info.map(|i| i.max_level).unwrap_or(10);
                    ui.label(gd.display(id)).on_hover_text(id);
                    let mut v = *lvl;
                    if ui.add(egui::Slider::new(&mut v, 0..=max)).changed() {
                        s.set_skill_level(h, id, v);
                    }
                    if let Some(i) = info {
                        ui.label(
                            RichText::new(format!(
                                "max {max}, unlocks at class level {}",
                                i.req_class_level
                            ))
                            .small()
                            .weak(),
                        );
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        let missing: Vec<&str> = gd
            .skills
            .iter()
            .filter(|k| k.class == class && !skills.iter().any(|(id, _)| *id == k.id))
            .map(|k| k.id.as_str())
            .collect();
        if !missing.is_empty() {
            ui.add_space(6.0);
            if ui
                .button(format!("Add {} missing class skill(s)", missing.len()))
                .clicked()
            {
                for m in missing {
                    s.set_skill_level(h, m, 0);
                }
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Max all skills").clicked() {
                for (id, _) in &skills {
                    let max = gd.skill(id).map(|i| i.max_level).unwrap_or(5);
                    s.set_skill_level(h, id, max);
                }
            }
        });
    });
    ui.add_space(8.0);
    card(ui, |ui| {
        section_title(ui, "Traits");
        ui.label(
            RichText::new("Racial traits and mutations. Click x to remove one.")
                .small()
                .weak(),
        );
        ui.add_space(4.0);
        let traits = s.traits(h);
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for t in &traits {
                egui::Frame::new()
                    .fill(ui.visuals().faint_bg_color)
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(gd.display(t)).on_hover_text(t.as_str());
                            if ui.small_button("x").clicked() {
                                remove = Some(t.clone());
                            }
                        });
                    });
            }
            if traits.is_empty() {
                ui.label(RichText::new("No traits").weak());
            }
        });
        if let Some(t) = remove {
            s.remove_trait(h, &t);
        }
        if !gd.traits.is_empty() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("addtrait")
                    .width(240.0)
                    .selected_text(if trait_pick.is_empty() {
                        "Choose a trait…".to_string()
                    } else {
                        gd.display(trait_pick)
                    })
                    .show_ui(ui, |ui| {
                        for t in &gd.traits {
                            if !traits.contains(t) {
                                ui.selectable_value(trait_pick, t.clone(), gd.display(t));
                            }
                        }
                    });
                if ui
                    .add_enabled(!trait_pick.is_empty(), egui::Button::new("Add trait"))
                    .clicked()
                {
                    s.add_trait(h, trait_pick);
                    trait_pick.clear();
                }
            });
        }
    });
}

fn req_value(i: &ItemReq, short: &str) -> i32 {
    match short {
        "STR" => i.req_str,
        "DEX" => i.req_dex,
        "END" => i.req_end,
        _ => i.req_wis,
    }
}

fn gear_tab(
    ui: &mut egui::Ui,
    s: &mut SaveFile,
    gd: &GameData,
    h: usize,
    filter: &mut String,
    unmet_only: &mut bool,
    class_only: &mut bool,
) {
    ui.add_space(4.0);
    if gd.items.is_empty() {
        ui.label("Item requirements come from the game install, which wasn't found. Use \"Set game folder…\" on the start screen.");
        return;
    }
    let class = s.hero_str(h, "class").unwrap_or_default();
    let stats: Vec<i32> = ATTRS
        .iter()
        .map(|(f, _, _)| s.hero_int(h, f).unwrap_or(0))
        .collect();
    let level = s.hero_int(h, "level").unwrap_or(1);

    card(ui, |ui| {
        section_title(ui, "Hidden gear requirements");
        ui.label(
            "Items the game won't let a hero use until their attributes are high enough. \
             Checked against this hero's base attributes; gear bonuses can make up the difference in game.",
        );
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(RichText::new("Meet every stat requirement").strong())
                .on_hover_text("Raise each base attribute to the highest value any item asks for. Never lowers a stat.")
                .clicked()
            {
                for (k, (field, _, short)) in ATTRS.iter().enumerate() {
                    let need = gd.items.iter().map(|i| req_value(i, short)).max().unwrap_or(0);
                    if need > stats[k] {
                        s.set_hero_int(h, field, need);
                    }
                }
            }
            ui.add_space(12.0);
            ui.label("Search");
            ui.add(egui::TextEdit::singleline(filter).desired_width(160.0).hint_text("item name"));
            ui.checkbox(unmet_only, "Only items this hero can't use");
            ui.checkbox(class_only, "Hide other classes' items");
        });
    });
    ui.add_space(8.0);

    let f = filter.to_lowercase();
    let mut raise: Option<Vec<(usize, i32)>> = None;
    egui::Grid::new("gear")
        .num_columns(8)
        .striped(true)
        .spacing([14.0, 6.0])
        .show(ui, |ui| {
            for t in ["Item", "STR", "DEX", "END", "WIS", "Class", "", ""] {
                ui.label(RichText::new(t).strong());
            }
            ui.end_row();
            for it in &gd.items {
                if !f.is_empty() && !it.name.to_lowercase().contains(&f) && !it.id.contains(&f) {
                    continue;
                }
                let class_ok = it.req_class.is_empty() || it.req_class == class;
                if *class_only && !class_ok {
                    continue;
                }
                let short_ok: Vec<bool> = ATTRS
                    .iter()
                    .enumerate()
                    .map(|(k, (_, _, sh))| stats[k] >= req_value(it, sh))
                    .collect();
                let level_ok = it.req_class_level <= level;
                let all_ok = class_ok && level_ok && short_ok.iter().all(|b| *b);
                if *unmet_only && all_ok {
                    continue;
                }
                ui.label(&it.name).on_hover_text(&it.id);
                for (k, (_, _, sh)) in ATTRS.iter().enumerate() {
                    let r = req_value(it, sh);
                    if r == 0 {
                        ui.label(RichText::new("·").weak());
                    } else {
                        ui.label(
                            RichText::new(r.to_string())
                                .color(if short_ok[k] { OK_GREEN } else { BAD_RED })
                                .strong(),
                        );
                    }
                }
                if it.req_class.is_empty() {
                    ui.label(RichText::new("any").weak());
                } else {
                    ui.label(RichText::new(gd.display(&it.req_class)).color(if class_ok {
                        OK_GREEN
                    } else {
                        BAD_RED
                    }));
                }
                if it.req_class_level > 0 {
                    ui.label(
                        RichText::new(format!("Lv {}", it.req_class_level)).color(if level_ok {
                            OK_GREEN
                        } else {
                            BAD_RED
                        }),
                    );
                } else {
                    ui.label("");
                }
                if short_ok.iter().all(|b| *b) {
                    ui.label(RichText::new("OK").color(OK_GREEN));
                } else if ui
                    .small_button("Raise stats")
                    .on_hover_text("Raise this hero's base attributes just enough for this item")
                    .clicked()
                {
                    raise = Some(
                        ATTRS
                            .iter()
                            .enumerate()
                            .map(|(k, (_, _, sh))| (k, req_value(it, sh).max(stats[k])))
                            .collect(),
                    );
                }
                ui.end_row();
            }
        });
    if let Some(r) = raise {
        for (k, v) in r {
            s.set_hero_int(h, ATTRS[k].0, v);
        }
    }
}

// ---------------- advanced raw editor ----------------

enum RawEdit {
    Int(Vec<usize>, i32),
    Float(Vec<usize>, f32),
    Str(Vec<usize>, String),
}

fn raw_tree(
    ui: &mut egui::Ui,
    doc: &Doc,
    t: &[Entry],
    path: &mut Vec<usize>,
    edits: &mut Vec<RawEdit>,
) {
    for (i, e) in t.iter().enumerate() {
        path.push(i);
        raw_entry(ui, doc, e, path, edits);
        path.pop();
    }
}

/// Render one entry whose position is `path`.
fn raw_entry(
    ui: &mut egui::Ui,
    doc: &Doc,
    e: &Entry,
    path: &mut Vec<usize>,
    edits: &mut Vec<RawEdit>,
) {
    {
        let name = doc.key_name(e.key);
        match &e.value {
            Value::Table(sub) => {
                let label = match e.key {
                    Key::Index(_) => {
                        // Show something recognisable for list items.
                        let hint = sub.iter().find_map(|x| match (&x.key, &x.value) {
                            (Key::Name(_), Value::Str(s)) => Some(doc.str(*s)),
                            _ => None,
                        });
                        format!("{name} {}", hint.map(|h| prettify(&h)).unwrap_or_default())
                    }
                    _ => format!("{name} ({})", sub.len()),
                };
                egui::CollapsingHeader::new(label)
                    .id_salt(path.clone())
                    .show(ui, |ui| raw_tree(ui, doc, sub, path, edits));
            }
            Value::Int(v) => {
                ui.horizontal(|ui| {
                    ui.label(&name);
                    let mut x = *v;
                    if ui.add(egui::DragValue::new(&mut x).speed(0.2)).changed() {
                        edits.push(RawEdit::Int(path.clone(), x));
                    }
                });
            }
            Value::Float(b) => {
                ui.horizontal(|ui| {
                    ui.label(&name);
                    let mut x = f32::from_bits(*b);
                    if ui.add(egui::DragValue::new(&mut x).speed(0.05)).changed() {
                        edits.push(RawEdit::Float(path.clone(), x));
                    }
                });
            }
            Value::Str(s) => {
                ui.horizontal(|ui| {
                    ui.label(&name);
                    let mut x = doc.str(*s);
                    let r = ui.add(egui::TextEdit::singleline(&mut x).desired_width(220.0));
                    if r.changed() {
                        edits.push(RawEdit::Str(path.clone(), x));
                    }
                });
            }
        }
    }
}

fn entry_at<'a>(root: &'a mut [Entry], path: &[usize]) -> Option<&'a mut Entry> {
    let (last, parents) = path.split_last()?;
    let mut cur = root;
    for &p in parents {
        match &mut cur.get_mut(p)?.value {
            Value::Table(t) => cur = t,
            _ => return None,
        }
    }
    cur.get_mut(*last)
}

fn advanced_tab(ui: &mut egui::Ui, s: &mut SaveFile, h: usize) {
    ui.add_space(4.0);
    card(ui, |ui| {
        section_title(ui, "Every field in the save");
        ui.label(
            RichText::new(
                "Raw view of this hero and the party, for anything the other tabs don't cover. \
                 Typing a value the game doesn't expect can break the save; a backup is made when you save.",
            )
            .small()
            .color(ACCENT),
        );
    });
    ui.add_space(6.0);
    let mut edits = Vec::new();
    if let Some(hp) = s.hero_pos(h) {
        ui.label(RichText::new("Hero").strong());
        let mut path = hp.clone();
        if let Some(t) = hero_table_at(&s.doc, &hp) {
            raw_tree(ui, &s.doc, t, &mut path, &mut edits);
        }
        ui.add_space(8.0);
        // Party-level fields (everything except heroes).
        ui.label(RichText::new("Party").strong());
        let party_pos = [hp[0]];
        if let Some(Value::Table(pt)) = s.doc.root.get(hp[0]).map(|e| &e.value) {
            let heroes_idx = hp[1];
            for (i, e) in pt.iter().enumerate() {
                if i == heroes_idx {
                    continue;
                }
                let mut path = vec![party_pos[0], i];
                raw_entry(ui, &s.doc, e, &mut path, &mut edits);
            }
        }
    }
    for ed in edits {
        match ed {
            RawEdit::Int(p, v) => {
                if let Some(e) = entry_at(&mut s.doc.root, &p) {
                    e.value = Value::Int(v);
                }
            }
            RawEdit::Float(p, v) => {
                if let Some(e) = entry_at(&mut s.doc.root, &p) {
                    e.value = Value::Float(v.to_bits());
                }
            }
            RawEdit::Str(p, v) => {
                let idx = s.doc.intern(&v);
                if let Some(e) = entry_at(&mut s.doc.root, &p) {
                    e.value = Value::Str(idx);
                    e.val_w = 1;
                }
            }
        }
    }
}

fn hero_table_at<'a>(doc: &'a Doc, pos: &[usize]) -> Option<&'a Vec<Entry>> {
    let mut cur: &Vec<Entry> = &doc.root;
    for &p in pos {
        match &cur.get(p)?.value {
            Value::Table(t) => cur = t,
            _ => return None,
        }
    }
    Some(cur)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.check_game_running();
        ui.ctx().request_repaint_after(Duration::from_secs(2));
        if let Some(st) = &self.status
            && !st.error
            && st.at.elapsed() > Duration::from_secs(12)
        {
            self.status = None;
        }

        egui::Panel::top("top")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(Margin::symmetric(14, 10)))
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("bottom")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(Margin::symmetric(14, 6)))
            .show(ui, |ui| self.bottom_bar(ui));
        egui::Panel::left("saves")
            .resizable(true)
            .default_size(290.0)
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(Margin::same(12)))
            .show(ui, |ui| self.side_panel(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(Margin::same(16)))
            .show(ui, |ui| {
                if self.save.is_some() {
                    self.editor(ui);
                } else {
                    self.welcome(ui);
                }
            });

        self.class_change_dialog(ui.ctx());

        if self.confirm_discard {
            egui::Window::new("Discard changes?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ui.ctx(), |ui| {
                    ui.label("You have unsaved changes. Open the other save and lose them?");
                    ui.horizontal(|ui| {
                        if ui.button("Discard and open").clicked() {
                            self.confirm_discard = false;
                            if let Some(p) = self.pending_open.take() {
                                self.save = None;
                                self.open(p);
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_discard = false;
                            self.pending_open = None;
                        }
                    });
                });
        }
    }
}
