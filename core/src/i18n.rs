//! User-facing string localization (i18n).
//!
//! The UI pulls every visible label through [`I18n::tr`], keyed by a stable
//! English string. Each supported language supplies a table mapping those
//! keys to translations; missing keys fall back to the English table, and any
//! key absent from all tables falls back to the key itself so the UI never
//! renders a blank label.

/// Supported UI languages. The first entry is always the fallback language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    German,
    Spanish,
}

impl Language {
    /// Stable, serializable language code (e.g. `"en"`, `"de"`).
    pub fn code(&self) -> &'static str {
        match self {
            Language::English => "en",
            Language::German => "de",
            Language::Spanish => "es",
        }
    }

    /// Endonym — the language's name in its own script.
    pub fn name(&self) -> &'static str {
        match self {
            Language::English => "English",
            Language::German => "Deutsch",
            Language::Spanish => "Español",
        }
    }

    /// Every language, in display order.
    pub fn all() -> &'static [Language] {
        &[Language::English, Language::German, Language::Spanish]
    }

    /// Resolve a language from its code, falling back to English.
    pub fn from_code(code: &str) -> Language {
        match code {
            "de" => Language::German,
            "es" => Language::Spanish,
            _ => Language::English,
        }
    }
}

/// Localized string tables. Index 0 is the fallback (English).
const TABLES: &[(Language, &[(&str, &str)])] = &[
    (
        Language::English,
        &[
            ("app.title", "tpt-audio"),
            ("tab.routing", "Routing"),
            ("tab.volume", "Volume"),
            ("tab.presets", "Presets"),
            ("tab.settings", "Settings"),
            ("tab.diagnostics", "Diagnostics"),
            ("button.refresh", "Refresh"),
            ("routing.title", "Routing Matrix"),
            ("routing.no_devices", "No audio devices found. Click Refresh to scan."),
            ("routing.source_header", "Source \\ Sink"),
            ("routing.remove_title", "Remove Route"),
            ("routing.remove_confirm", "Remove this route?"),
            ("volume.title", "Volume Control"),
            ("volume.master", "Master Volume:"),
            ("volume.per_app", "Per-App Volumes:"),
            ("volume.active_apps", "Active apps:"),
            ("presets.title", "Presets"),
            ("presets.name", "Name:"),
            ("presets.save_current", "Save Current"),
            ("presets.load", "Load"),
            ("presets.delete", "Delete"),
            ("presets.share_header", "Share presets as files"),
            ("presets.share_desc", "Export all presets to a JSON file, or import presets from a shared file."),
            ("presets.export_to", "Export to:"),
            ("presets.import_from", "Import from:"),
            ("presets.export_all", "Export All"),
            ("presets.import", "Import"),
            ("settings.title", "Settings"),
            ("settings.subtitle", "tpt-audio — Audio Router & Virtual Mixer"),
            ("settings.sources_desc", "Sources: audio inputs (microphones, app audio)."),
            ("settings.sinks_desc", "Sinks: audio outputs (speakers, headsets)."),
            ("settings.autodetect_desc", "Devices are auto-detected every ~6 seconds."),
            ("settings.refresh_now", "Refresh Devices Now"),
            ("settings.updates", "Updates"),
            ("settings.installed_version", "Installed version:"),
            ("settings.check_updates", "Check for Updates"),
            ("settings.open_releases", "Open Releases Page"),
            ("settings.checking_updates", "Checking for updates…"),
            ("settings.language", "Language"),
            ("settings.shortcuts", "Keyboard Shortcuts"),
            ("settings.shortcut.routing", "Ctrl+1 — Routing tab"),
            ("settings.shortcut.volume", "Ctrl+2 — Volume tab"),
            ("settings.shortcut.presets", "Ctrl+3 — Presets tab"),
            ("settings.shortcut.settings", "Ctrl+4 — Settings tab"),
            ("settings.shortcut.diagnostics", "Ctrl+5 — Diagnostics tab"),
            ("settings.shortcut.refresh", "F5 — Refresh devices"),
            ("diagnostics.title", "Diagnostics"),
            ("diagnostics.underruns", "Stream underruns:"),
            ("diagnostics.overruns", "Stream overruns:"),
            ("diagnostics.routes_created", "Routes created:"),
            ("diagnostics.routes_removed", "Routes removed:"),
            ("diagnostics.devices_found", "Devices found:"),
            ("diagnostics.devices_lost", "Devices lost:"),
            ("diagnostics.reconnects", "Device reconnects:"),
            ("diagnostics.last_refresh", "Last refresh:"),
            ("diagnostics.event_log", "Event Log"),
            ("plugins.title", "Extensions"),
            ("plugins.none", "No extensions loaded."),
        ],
    ),
    (
        Language::German,
        &[
            ("app.title", "tpt-audio"),
            ("tab.routing", "Routing"),
            ("tab.volume", "Lautstärke"),
            ("tab.presets", "Presets"),
            ("tab.settings", "Einstellungen"),
            ("tab.diagnostics", "Diagnose"),
            ("button.refresh", "Aktualisieren"),
            ("routing.title", "Routing-Matrix"),
            ("routing.no_devices", "Keine Audio-Geräte gefunden. Zum Scannen auf Aktualisieren klicken."),
            ("routing.source_header", "Quelle \\ Senke"),
            ("routing.remove_title", "Route entfernen"),
            ("routing.remove_confirm", "Diese Route entfernen?"),
            ("volume.title", "Lautstärkeregelung"),
            ("volume.master", "Gesamtlautstärke:"),
            ("volume.per_app", "Lautstärke pro App:"),
            ("volume.active_apps", "Aktive Apps:"),
            ("presets.title", "Presets"),
            ("presets.name", "Name:"),
            ("presets.save_current", "Aktuelles speichern"),
            ("presets.load", "Laden"),
            ("presets.delete", "Löschen"),
            ("presets.share_header", "Presets als Dateien teilen"),
            ("presets.share_desc", "Alle Presets in eine JSON-Datei exportieren oder Presets aus einer geteilten Datei importieren."),
            ("presets.export_to", "Exportieren nach:"),
            ("presets.import_from", "Importieren von:"),
            ("presets.export_all", "Alle exportieren"),
            ("presets.import", "Importieren"),
            ("settings.title", "Einstellungen"),
            ("settings.subtitle", "tpt-audio — Audio-Router & virtueller Mischer"),
            ("settings.sources_desc", "Quellen: Audio-Eingänge (Mikrofone, App-Audio)."),
            ("settings.sinks_desc", "Senken: Audio-Ausgänge (Lautsprecher, Headsets)."),
            ("settings.autodetect_desc", "Geräte werden alle ~6 Sekunden automatisch erkannt."),
            ("settings.refresh_now", "Geräte jetzt aktualisieren"),
            ("settings.updates", "Updates"),
            ("settings.installed_version", "Installierte Version:"),
            ("settings.check_updates", "Nach Updates suchen"),
            ("settings.open_releases", "Releases-Seite öffnen"),
            ("settings.checking_updates", "Nach Updates wird gesucht…"),
            ("settings.language", "Sprache"),
            ("settings.shortcuts", "Tastenkürzel"),
            ("settings.shortcut.routing", "Strg+1 — Routing-Reiter"),
            ("settings.shortcut.volume", "Strg+2 — Lautstärke-Reiter"),
            ("settings.shortcut.presets", "Strg+3 — Presets-Reiter"),
            ("settings.shortcut.settings", "Strg+4 — Einstellungen-Reiter"),
            ("settings.shortcut.diagnostics", "Strg+5 — Diagnose-Reiter"),
            ("settings.shortcut.refresh", "F5 — Geräte aktualisieren"),
            ("diagnostics.title", "Diagnose"),
            ("diagnostics.underruns", "Stream-Unterläufe:"),
            ("diagnostics.overruns", "Stream-Überläufe:"),
            ("diagnostics.routes_created", "Routen erstellt:"),
            ("diagnostics.routes_removed", "Routen entfernt:"),
            ("diagnostics.devices_found", "Geräte gefunden:"),
            ("diagnostics.devices_lost", "Geräte verloren:"),
            ("diagnostics.reconnects", "Geräte-Neuverbindungen:"),
            ("diagnostics.last_refresh", "Letzte Aktualisierung:"),
            ("diagnostics.event_log", "Ereignisprotokoll"),
            ("plugins.title", "Erweiterungen"),
            ("plugins.none", "Keine Erweiterungen geladen."),
        ],
    ),
    (
        Language::Spanish,
        &[
            ("app.title", "tpt-audio"),
            ("tab.routing", "Enrutamiento"),
            ("tab.volume", "Volumen"),
            ("tab.presets", "Ajustes"),
            ("tab.settings", "Ajustes"),
            ("tab.diagnostics", "Diagnóstico"),
            ("button.refresh", "Actualizar"),
            ("routing.title", "Matriz de enrutamiento"),
            ("routing.no_devices", "No se encontraron dispositivos de audio. Haz clic en Actualizar para buscar."),
            ("routing.source_header", "Origen \\ Destino"),
            ("routing.remove_title", "Eliminar ruta"),
            ("routing.remove_confirm", "¿Eliminar esta ruta?"),
            ("volume.title", "Control de volumen"),
            ("volume.master", "Volumen maestro:"),
            ("volume.per_app", "Volumen por aplicación:"),
            ("volume.active_apps", "Aplicaciones activas:"),
            ("presets.title", "Ajustes"),
            ("presets.name", "Nombre:"),
            ("presets.save_current", "Guardar actual"),
            ("presets.load", "Cargar"),
            ("presets.delete", "Eliminar"),
            ("presets.share_header", "Compartir ajustes como archivos"),
            ("presets.share_desc", "Exporta todos los ajustes a un archivo JSON, o importa ajustes desde un archivo compartido."),
            ("presets.export_to", "Exportar a:"),
            ("presets.import_from", "Importar de:"),
            ("presets.export_all", "Exportar todo"),
            ("presets.import", "Importar"),
            ("settings.title", "Ajustes"),
            ("settings.subtitle", "tpt-audio — Enrutador de audio y mezclador virtual"),
            ("settings.sources_desc", "Fuentes: entradas de audio (micrófonos, audio de aplicaciones)."),
            ("settings.sinks_desc", "Destinos: salidas de audio (altavoces, auriculares)."),
            ("settings.autodetect_desc", "Los dispositivos se detectan automáticamente cada ~6 segundos."),
            ("settings.refresh_now", "Actualizar dispositivos ahora"),
            ("settings.updates", "Actualizaciones"),
            ("settings.installed_version", "Versión instalada:"),
            ("settings.check_updates", "Buscar actualizaciones"),
            ("settings.open_releases", "Abrir página de versiones"),
            ("settings.checking_updates", "Buscando actualizaciones…"),
            ("settings.language", "Idioma"),
            ("settings.shortcuts", "Atajos de teclado"),
            ("settings.shortcut.routing", "Ctrl+1 — Pestaña de enrutamiento"),
            ("settings.shortcut.volume", "Ctrl+2 — Pestaña de volumen"),
            ("settings.shortcut.presets", "Ctrl+3 — Pestaña de ajustes"),
            ("settings.shortcut.settings", "Ctrl+4 — Pestaña de ajustes"),
            ("settings.shortcut.diagnostics", "Ctrl+5 — Pestaña de diagnóstico"),
            ("settings.shortcut.refresh", "F5 — Actualizar dispositivos"),
            ("diagnostics.title", "Diagnóstico"),
            ("diagnostics.underruns", "Faltas de datos del flujo:"),
            ("diagnostics.overruns", "Desbordamientos del flujo:"),
            ("diagnostics.routes_created", "Rutas creadas:"),
            ("diagnostics.routes_removed", "Rutas eliminadas:"),
            ("diagnostics.devices_found", "Dispositivos encontrados:"),
            ("diagnostics.devices_lost", "Dispositivos perdidos:"),
            ("diagnostics.reconnects", "Reconexiones de dispositivos:"),
            ("diagnostics.last_refresh", "Última actualización:"),
            ("diagnostics.event_log", "Registro de eventos"),
            ("plugins.title", "Extensiones"),
            ("plugins.none", "No hay extensiones cargadas."),
        ],
    ),
];

/// A thin translation lookup bound to a chosen [`Language`].
#[derive(Clone, Copy, Debug)]
pub struct I18n {
    language: Language,
}

impl I18n {
    pub fn new() -> Self {
        Self {
            language: Language::English,
        }
    }

    pub fn with_language(language: Language) -> Self {
        Self { language }
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn set_language(&mut self, language: Language) {
        self.language = language;
    }

    pub fn set_language_code(&mut self, code: &str) {
        self.language = Language::from_code(code);
    }

    /// Translate `key`, falling back to the English table, then to the key.
    pub fn tr<'a>(&self, key: &'a str) -> &'a str {
        for (lang, table) in TABLES {
            if *lang == self.language {
                if let Some((_, value)) = table.iter().find(|(k, _)| *k == key) {
                    return value;
                }
            }
        }
        // Fallback to English.
        for (lang, table) in TABLES {
            if *lang == Language::English {
                if let Some((_, value)) = table.iter().find(|(k, _)| *k == key) {
                    return value;
                }
            }
        }
        key
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_fallback() {
        let i18n = I18n::new();
        assert_eq!(i18n.tr("tab.routing"), "Routing");
        assert_eq!(i18n.tr("missing.key"), "missing.key");
    }

    #[test]
    fn german_translation() {
        let i18n = I18n::with_language(Language::German);
        assert_eq!(i18n.tr("tab.volume"), "Lautstärke");
        assert_eq!(i18n.tr("app.title"), "tpt-audio");
    }

    #[test]
    fn unknown_key_falls_through_to_key() {
        let i18n = I18n::with_language(Language::Spanish);
        assert_eq!(i18n.tr("totally.unknown"), "totally.unknown");
    }

    #[test]
    fn language_roundtrip_via_code() {
        assert_eq!(Language::from_code("de"), Language::German);
        assert_eq!(Language::German.code(), "de");
        assert_eq!(Language::from_code("xx"), Language::English);
    }
}
