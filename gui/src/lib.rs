use std::sync::{Arc, Mutex};

use tpt_audio_core::backend::AudioBackend;
use tpt_audio_core::diagnostics::Diagnostics;
use tpt_audio_core::graph::{
    AudioDevice, AudioDirection, AudioSink, AudioSource, Route, RouterConfig,
};
use tpt_audio_core::i18n::{I18n, Language};
use tpt_audio_core::plugin::{ActivityCounter, PluginRegistry};
use tpt_audio_core::preset::PresetManager;
use tpt_audio_core::volume::VolumeController;

pub enum Tab {
    Routing,
    Volume,
    Presets,
    Settings,
    Diagnostics,
}

pub struct App {
    backend: Box<dyn AudioBackend>,
    devices: Vec<AudioDevice>,
    sources: Vec<AudioSource>,
    sinks: Vec<AudioSink>,
    routes: Vec<Route>,
    volume_controller: VolumeController,
    preset_manager: PresetManager,
    current_tab: Tab,
    show_confirm_remove: Option<u64>,
    new_preset_name: String,
    settings_message: String,
    preset_message: String,
    export_path: String,
    import_path: String,
    refresh_counter: u32,
    status_text: String,
    diagnostics: Arc<Diagnostics>,
    update_status: String,
    pending_update: Arc<Mutex<Option<String>>>,
    i18n: I18n,
    plugins: PluginRegistry,
}

impl App {
    pub fn new(backend: Box<dyn AudioBackend>, diagnostics: Arc<Diagnostics>) -> Self {
        let devices = backend.enumerate_devices();
        let app_sources = backend.enumerate_applications();

        let (mut sources, sinks) = partition_devices(&devices);
        for source in app_sources {
            if !sources.iter().any(|s| s.device_id == source.device_id) {
                sources.push(source);
            }
        }

        let mut plugins = PluginRegistry::new();
        plugins.register(Box::new(ActivityCounter::new()));

        Self {
            refresh_counter: 0,
            status_text: String::new(),
            backend,
            devices,
            sources,
            sinks,
            routes: Vec::new(),
            volume_controller: VolumeController::new(),
            preset_manager: PresetManager::new(),
            current_tab: Tab::Routing,
            show_confirm_remove: None,
            new_preset_name: String::new(),
            settings_message: String::new(),
            preset_message: String::new(),
            export_path: "tpt-audio-presets.json".to_string(),
            import_path: "tpt-audio-presets.json".to_string(),
            diagnostics,
            update_status: String::new(),
            pending_update: Arc::new(Mutex::new(None)),
            i18n: I18n::new(),
            plugins,
        }
    }

    pub fn refresh_devices(&mut self) -> bool {
        let old_count = self.devices.len();
        self.devices = self.backend.enumerate_devices();
        let app_sources = self.backend.enumerate_applications();

        let (mut sources, sinks) = partition_devices(&self.devices);
        for source in app_sources {
            if !sources.iter().any(|s| s.device_id == source.device_id) {
                sources.push(source);
            }
        }

        self.sources = sources;
        self.sinks = sinks;

        let changed = self.devices.len() != old_count;
        if changed {
            self.status_text = format!("Devices changed: {} → {}", old_count, self.devices.len());
        }
        changed
    }

    /// Apply the current language selection (no-op beyond recording the choice).
    pub fn set_language(&mut self, language: Language) {
        self.i18n.set_language(language);
    }
}

fn partition_devices(devices: &[AudioDevice]) -> (Vec<AudioSource>, Vec<AudioSink>) {
    let mut sources = Vec::new();
    let mut sinks = Vec::new();
    for d in devices {
        match d.direction {
            AudioDirection::Input => {
                sources.push(AudioSource {
                    device_id: d.id.clone(),
                    app_name: None,
                    app_pid: None,
                });
            }
            AudioDirection::Output => {
                sinks.push(AudioSink {
                    device_id: d.id.clone(),
                });
            }
        }
    }
    (sources, sinks)
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_shortcuts(ctx);

        self.refresh_counter += 1;
        if let Some(result) = self.pending_update.lock().ok().and_then(|mut g| g.take()) {
            self.update_status = result;
        }
        if self.refresh_counter.is_multiple_of(30) {
            self.refresh_devices();
        }

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(self.i18n.tr("app.title"));
                ui.separator();

                if ui
                    .selectable_label(matches!(self.current_tab, Tab::Routing), self.i18n.tr("tab.routing"))
                    .clicked()
                {
                    self.current_tab = Tab::Routing;
                }
                if ui
                    .selectable_label(matches!(self.current_tab, Tab::Volume), self.i18n.tr("tab.volume"))
                    .clicked()
                {
                    self.current_tab = Tab::Volume;
                }
                if ui
                    .selectable_label(matches!(self.current_tab, Tab::Presets), self.i18n.tr("tab.presets"))
                    .clicked()
                {
                    self.current_tab = Tab::Presets;
                }
                if ui
                    .selectable_label(matches!(self.current_tab, Tab::Settings), self.i18n.tr("tab.settings"))
                    .clicked()
                {
                    self.current_tab = Tab::Settings;
                }
                if ui
                    .selectable_label(matches!(self.current_tab, Tab::Diagnostics), self.i18n.tr("tab.diagnostics"))
                    .clicked()
                {
                    self.current_tab = Tab::Diagnostics;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(self.i18n.tr("button.refresh")).clicked() {
                        self.refresh_devices();
                    }
                    if !self.status_text.is_empty() {
                        ui.label(&self.status_text);
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.current_tab {
            Tab::Routing => self.show_routing_tab(ui),
            Tab::Volume => self.show_volume_tab(ui),
            Tab::Presets => self.show_presets_tab(ui),
            Tab::Settings => self.show_settings_tab(ui),
            Tab::Diagnostics => self.show_diagnostics_tab(ui),
        });

        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}

impl App {
    /// In-app keyboard shortcuts (cross-platform via egui input).
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if i.key_pressed(egui::Key::F5) {
                self.refresh_devices();
            }
            if i.modifiers.ctrl {
                let tab = if i.key_pressed(egui::Key::Num1) {
                    Some(Tab::Routing)
                } else if i.key_pressed(egui::Key::Num2) {
                    Some(Tab::Volume)
                } else if i.key_pressed(egui::Key::Num3) {
                    Some(Tab::Presets)
                } else if i.key_pressed(egui::Key::Num4) {
                    Some(Tab::Settings)
                } else if i.key_pressed(egui::Key::Num5) {
                    Some(Tab::Diagnostics)
                } else {
                    None
                };
                if let Some(tab) = tab {
                    self.current_tab = tab;
                }
            }
        });
    }

    fn show_routing_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(self.i18n.tr("routing.title"));
        ui.separator();

        if self.sources.is_empty() || self.sinks.is_empty() {
            ui.label(self.i18n.tr("routing.no_devices"));
            return;
        }

        let mut route_gain_changes: Vec<(u64, f32)> = Vec::new();
        let mut route_mute_toggles: Vec<u64> = Vec::new();
        let mut route_removals: Vec<u64> = Vec::new();
        let mut route_adds: Vec<(usize, usize)> = Vec::new();
        let mut graph_changed = false;

        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("routing_matrix")
                .striped(true)
                .min_col_width(120.0)
                .show(ui, |ui| {
                    ui.strong(self.i18n.tr("routing.source_header"))
                        .on_hover_text("Rows are audio sources; columns are audio sinks");
                    for sink in &self.sinks {
                        ui.strong(truncate_label(&sink.device_id))
                            .on_hover_text(&sink.device_id);
                    }
                    ui.end_row();

                    for (si, source) in self.sources.iter().enumerate() {
                        let source_label = source
                            .app_name
                            .as_deref()
                            .unwrap_or(&source.device_id)
                            .to_string();
                        ui.label(truncate_label(&source_label))
                            .on_hover_text(&source_label);

                        for (ski, sink) in self.sinks.iter().enumerate() {
                            let sink_label = sink.device_id.clone();
                            let route_id = self.find_route(&source.device_id, &sink.device_id);

                            if let Some(rid) = route_id {
                                let route = self.routes.iter().find(|r| r.id == rid).unwrap();
                                let mut active = !route.muted;
                                let mut gain = route.gain;

                                let route_desc =
                                    format!("route from {source_label} to {sink_label}");

                                ui.horizontal(|ui| {
                                    let mute_label = format!("Mute {route_desc}");
                                    if ui
                                        .checkbox(&mut active, "")
                                        .on_hover_text(&mute_label)
                                        .changed()
                                    {
                                        route_mute_toggles.push(rid);
                                    }
                                    let gain_label = format!("Volume for {route_desc}");
                                    if ui
                                        .add(
                                            egui::Slider::new(&mut gain, 0.0..=1.0)
                                                .text("")
                                                .smallest_positive(60.0),
                                        )
                                        .on_hover_text(&gain_label)
                                        .changed()
                                    {
                                        route_gain_changes.push((rid, gain));
                                    }
                                    let remove_label = format!("Remove {route_desc}");
                                    if ui.button("x").on_hover_text(&remove_label).clicked() {
                                        route_removals.push(rid);
                                    }
                                });
                            } else {
                                let add_label = format!("Add route from {source_label} to {sink_label}");
                                if ui.button("+").on_hover_text(&add_label).clicked() {
                                    route_adds.push((si, ski));
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
        });

        for (rid, gain) in route_gain_changes {
            if let Some(route) = self.routes.iter_mut().find(|r| r.id == rid) {
                route.gain = gain;
                self.backend.set_route_gain(rid, gain);
                graph_changed = true;
            }
        }

        for rid in route_mute_toggles {
            if let Some(route) = self.routes.iter_mut().find(|r| r.id == rid) {
                route.muted = !route.muted;
                self.backend.set_route_mute(rid, route.muted);
                graph_changed = true;
            }
        }

        for rid in route_removals {
            self.show_confirm_remove = Some(rid);
        }

        for (si, ski) in route_adds {
            self.add_route(si, ski);
            graph_changed = true;
        }

        if graph_changed {
            self.plugins.notify_route_change(&self.build_config());
        }

        if let Some(rid) = self.show_confirm_remove {
            egui::Window::new(self.i18n.tr("routing.remove_title"))
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(self.i18n.tr("routing.remove_confirm"));
                    ui.horizontal(|ui| {
                        if ui.button("Yes").clicked() {
                            self.remove_route(rid);
                            self.plugins.notify_route_change(&self.build_config());
                            self.show_confirm_remove = None;
                        }
                        if ui.button("No").clicked() {
                            self.show_confirm_remove = None;
                        }
                    });
                });
        }
    }

    fn show_volume_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(self.i18n.tr("volume.title"));
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(self.i18n.tr("volume.master"));
            let mut master = self.volume_controller.master_volume();
            ui.add(egui::Slider::new(&mut master, 0.0..=1.0).text(""));
            self.volume_controller.set_master_volume(master);
        });

        ui.separator();
        ui.label(self.i18n.tr("volume.per_app"));
        egui::ScrollArea::vertical().show(ui, |ui| {
            let apps = self.volume_controller.app_volumes().clone();
            for (app_name, volume) in &apps {
                ui.horizontal(|ui| {
                    ui.label(truncate_label(app_name));
                    let mut vol = *volume;
                    if ui
                        .add(egui::Slider::new(&mut vol, 0.0..=1.0).text(""))
                        .changed()
                    {
                        self.volume_controller.set_app_volume(app_name, vol);
                        self.backend.set_app_volume(app_name, vol);
                        self.plugins.notify_app_volume(app_name, vol);
                    }
                });
            }
        });

        ui.separator();
        ui.horizontal(|ui| {
            ui.label(self.i18n.tr("volume.active_apps"));
            for source in &self.sources {
                if let Some(ref name) = source.app_name {
                    let vol = self.volume_controller.app_volume(name);
                    ui.label(format!("{}: {:.0}%", name, vol * 100.0));
                }
            }
        });
    }

    fn show_presets_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(self.i18n.tr("presets.title"));
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(self.i18n.tr("presets.name"));
            ui.text_edit_singleline(&mut self.new_preset_name);
            if ui.button(self.i18n.tr("presets.save_current")).clicked() {
                let config = self.build_config();
                self.preset_manager
                    .add_preset(&self.new_preset_name, config);
                self.new_preset_name.clear();
            }
        });

        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for preset in self.preset_manager.presets().to_vec() {
                ui.horizontal(|ui| {
                    ui.label(&preset.name);
                    if ui.button(self.i18n.tr("presets.load")).clicked() {
                        self.load_config(preset.config.clone());
                    }
                    if ui.button(self.i18n.tr("presets.delete")).clicked() {
                        self.preset_manager.remove_preset(&preset.name);
                    }
                });
            }
        });

        ui.separator();
        ui.label(self.i18n.tr("presets.share_header"));
        ui.label(self.i18n.tr("presets.share_desc"));
        ui.horizontal(|ui| {
            ui.label(self.i18n.tr("presets.export_to"));
            ui.text_edit_singleline(&mut self.export_path);
            if ui.button(self.i18n.tr("presets.export_all")).clicked() {
                match self
                    .preset_manager
                    .save_to_json(std::path::Path::new(&self.export_path))
                {
                    Ok(()) => {
                        self.preset_message =
                            format!("Exported presets to {}", self.export_path);
                    }
                    Err(e) => {
                        self.preset_message = format!("Export failed: {e}");
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(self.i18n.tr("presets.import_from"));
            ui.text_edit_singleline(&mut self.import_path);
            if ui.button(self.i18n.tr("presets.import")).clicked() {
                match self
                    .preset_manager
                    .load_from_json(std::path::Path::new(&self.import_path))
                {
                    Ok(()) => {
                        self.preset_message =
                            format!("Imported presets from {}", self.import_path);
                    }
                    Err(e) => {
                        self.preset_message = format!("Import failed: {e}");
                    }
                }
            }
        });
        if !self.preset_message.is_empty() {
            ui.label(&self.preset_message);
        }
    }

    fn show_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(self.i18n.tr("settings.title"));
        ui.separator();
        ui.label(self.i18n.tr("settings.subtitle"));
        ui.label(self.i18n.tr("settings.sources_desc"));
        ui.label(self.i18n.tr("settings.sinks_desc"));
        ui.label(self.i18n.tr("settings.autodetect_desc"));
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(self.i18n.tr("settings.language"));
            egui::ComboBox::from_id_salt("language_selector")
                .selected_text(self.i18n.language().name())
                .show_ui(ui, |ui| {
                    for lang in Language::all() {
                        if ui
                            .selectable_label(self.i18n.language() == *lang, lang.name())
                            .clicked()
                        {
                            self.i18n.set_language(*lang);
                        }
                    }
                });
        });

        if ui.button(self.i18n.tr("settings.refresh_now")).clicked() {
            self.refresh_devices();
            self.settings_message = "Devices refreshed.".to_string();
        }
        if !self.settings_message.is_empty() {
            ui.label(&self.settings_message);
        }

        ui.separator();
        ui.label(self.i18n.tr("settings.shortcuts"));
        ui.label(self.i18n.tr("settings.shortcut.routing"));
        ui.label(self.i18n.tr("settings.shortcut.volume"));
        ui.label(self.i18n.tr("settings.shortcut.presets"));
        ui.label(self.i18n.tr("settings.shortcut.settings"));
        ui.label(self.i18n.tr("settings.shortcut.diagnostics"));
        ui.label(self.i18n.tr("settings.shortcut.refresh"));

        ui.separator();
        ui.label(self.i18n.tr("settings.updates"));
        let current = env!("CARGO_PKG_VERSION");
        ui.label(format!("{} {}", self.i18n.tr("settings.installed_version"), current));
        let pending = self.pending_update.lock().ok().map(|g| g.is_some()).unwrap_or(false);
        ui.horizontal(|ui| {
            if ui.button(self.i18n.tr("settings.check_updates")).clicked() && !pending {
                let result_slot = self.pending_update.clone();
                let ver = current.to_string();
                std::thread::spawn(move || {
                    let status = tpt_audio_core::update::update_status(
                        &ver,
                        "https://api.github.com/repos/tpt-solutions/tpt-audio/releases/latest",
                    );
                    if let Ok(mut slot) = result_slot.lock() {
                        *slot = Some(status);
                    }
                });
            }
            if ui
                .button(self.i18n.tr("settings.open_releases"))
                .on_hover_text("Open the tpt-audio releases page in your browser")
                .clicked()
            {
                let _ = open_url("https://github.com/tpt-solutions/tpt-audio/releases");
            }
        });
        if pending {
            ui.label(self.i18n.tr("settings.checking_updates"));
        } else if !self.update_status.is_empty() {
            ui.label(&self.update_status);
        }

        ui.separator();
        ui.label(self.i18n.tr("plugins.title"));
        if self.plugins.is_empty() {
            ui.label(self.i18n.tr("plugins.none"));
        } else {
            for name in self.plugins.names() {
                ui.label(format!("• {name}"));
            }
        }
    }

    fn show_diagnostics_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(self.i18n.tr("diagnostics.title"));
        ui.separator();

        let report = self.diagnostics.report();
        let entries = self.diagnostics.entries();

        ui.horizontal(|ui| {
            ui.label(format!("{} {}", self.i18n.tr("diagnostics.underruns"), report.stream_underruns));
            ui.label(format!("{} {}", self.i18n.tr("diagnostics.overruns"), report.stream_overruns));
        });
        ui.horizontal(|ui| {
            ui.label(format!("{} {}", self.i18n.tr("diagnostics.routes_created"), report.routes_created));
            ui.label(format!("{} {}", self.i18n.tr("diagnostics.routes_removed"), report.routes_removed));
        });
        ui.horizontal(|ui| {
            ui.label(format!("{} {}", self.i18n.tr("diagnostics.devices_found"), report.devices_found));
            ui.label(format!("{} {}", self.i18n.tr("diagnostics.devices_lost"), report.devices_lost));
        });
        ui.horizontal(|ui| {
            ui.label(format!("{} {}", self.i18n.tr("diagnostics.reconnects"), report.device_reconnects));
        });
        if let Some(last) = report.last_refresh {
            let elapsed = last.elapsed().as_secs();
            ui.label(format!("{} {}s ago", self.i18n.tr("diagnostics.last_refresh"), elapsed));
        }
        ui.separator();
        ui.label(format!("{} ({} entries):", self.i18n.tr("diagnostics.event_log"), entries.len()));
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for entry in entries.iter().rev() {
                    let elapsed = entry.timestamp.elapsed().as_secs();
                    ui.label(format!(
                        "[{:>4}s] [{}] {}",
                        elapsed, entry.level, entry.message
                    ));
                }
            });
    }
}

impl App {
    fn find_route(&self, source_id: &str, sink_id: &str) -> Option<u64> {
        self.routes
            .iter()
            .find(|r| r.source.device_id == source_id && r.sink.device_id == sink_id)
            .map(|r| r.id)
    }

    fn add_route(&mut self, source_idx: usize, sink_idx: usize) {
        if source_idx < self.sources.len() && sink_idx < self.sinks.len() {
            let source = self.sources[source_idx].clone();
            let sink = self.sinks[sink_idx].clone();
            let route = self.backend.create_route(source, sink);
            self.routes.push(route);
        }
    }

    fn remove_route(&mut self, route_id: u64) {
        self.backend.remove_route(route_id);
        self.routes.retain(|r| r.id != route_id);
    }

    fn build_config(&self) -> RouterConfig {
        let mut config = RouterConfig::new();
        config.routes = self.routes.clone();
        config.app_volumes = self.volume_controller.app_volumes().clone();
        config
    }

    fn load_config(&mut self, config: RouterConfig) {
        self.routes = config.routes.clone();
        self.volume_controller = VolumeController::new();
        for (app, vol) in &config.app_volumes {
            self.volume_controller.set_app_volume(app, *vol);
            self.backend.set_app_volume(app, *vol);
        }
        self.backend.apply_config(&config);
        self.plugins.notify_config_applied(&config);
    }
}

fn truncate_label(s: &str) -> String {
    if s.len() > 20 {
        format!("{}...", &s[..17])
    } else {
        s.to_string()
    }
}

/// Open a URL in the system default browser using platform-specific commands.
/// Returns an error if no suitable handler is available.
fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn()?
            .wait()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?.wait()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?.wait()?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = url;
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "open_url not supported on this platform",
        ));
    }
    Ok(())
}
