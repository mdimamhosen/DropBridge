#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! DropBridge desktop (macOS / Windows / Linux): thin Tauri shell over `dropbridge-core`.
use dropbridge::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_notification::NotificationExt;

const HISTORY_MAX: usize = 300;

#[derive(Serialize, Deserialize, Clone)]
struct Settings {
    name: String,
    download_dir: String,
    auto_accept: bool,
}

struct App {
    node: Node,
    data: PathBuf,
    settings: Mutex<Settings>,
    history: Mutex<Vec<Value>>,
}

impl App {
    fn save_settings(&self) {
        let _ = std::fs::write(self.data.join("settings.json"), serde_json::to_vec_pretty(&*self.settings.lock().unwrap()).unwrap());
    }
    fn push_history(&self, v: Value) {
        let mut h = self.history.lock().unwrap();
        h.insert(0, v);
        h.truncate(HISTORY_MAX);
        let _ = std::fs::write(self.data.join("history.json"), serde_json::to_vec(&*h).unwrap());
    }
}

struct Bridge(AppHandle);

impl Delegate for Bridge {
    fn on_event(&self, e: Event) {
        let app = &self.0;
        match &e {
            Event::IncomingOffer { from_name, files, bytes, texts, trusted, .. } => {
                let what = match (*files, *texts) {
                    (0, _) => "a message".to_string(),
                    (f, _) => format!("{f} file(s), {}", human(*bytes)),
                };
                notify(
                    app,
                    &format!("{from_name} wants to send {what}"),
                    if *trusted {
                        "Open DropBridge to accept."
                    } else {
                        "Unknown device — check the verification code before accepting."
                    },
                );
                show(app);
            }
            Event::TextReceived { from_name, text, .. } => {
                notify(app, &format!("Text from {from_name}"), &text.chars().take(120).collect::<String>());
                if let Some(s) = app.try_state::<App>() {
                    s.push_history(json!({ "kind": "text", "time": unix_now(), "peer_name": from_name, "text": text }));
                }
            }
            Event::Finished { outgoing, peer_name, ok, error, files_ok, failed, bytes, secs, avg_bps, peak_bps, transfer_id, .. } => {
                if !outgoing && *ok && *files_ok > 0 {
                    notify(
                        app,
                        &format!("Received {files_ok} item(s) from {peer_name}"),
                        &format!("{} at {}/s", human(*bytes), human(*avg_bps)),
                    );
                }
                if let Some(s) = app.try_state::<App>() {
                    if *files_ok > 0 || error.as_deref() != Some("Declined") {
                        s.push_history(json!({
                            "kind": "transfer", "time": unix_now(), "id": transfer_id, "outgoing": outgoing, "peer_name": peer_name,
                            "ok": ok, "error": error, "files_ok": files_ok, "failed": failed, "bytes": bytes, "secs": secs,
                            "avg_bps": avg_bps, "peak_bps": peak_bps,
                        }));
                    }
                }
            }
            _ => {}
        }
        let _ = app.emit("db", &e);
    }
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let focused = app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
    if !focused {
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn device_name() -> String {
    let run = |c: &str, a: &[&str]| {
        std::process::Command::new(c)
            .args(a)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    if cfg!(target_os = "macos") {
        if let Some(n) = run("scutil", &["--get", "ComputerName"]) {
            return n;
        }
    }
    std::env::var("COMPUTERNAME").ok().or_else(|| run("hostname", &[])).unwrap_or_else(|| "My computer".into())
}

/// Private key lives in the OS credential store (Keychain / Credential Manager / Secret Service).
/// Linux machines without a Secret Service fall back to a 0600 file.
fn load_identity(data: &std::path::Path) -> String {
    if let Ok(e) = keyring::Entry::new("dev.dropbridge.app", "identity") {
        if let Ok(s) = e.get_password() {
            return s;
        }
    }
    std::fs::read_to_string(data.join("identity")).unwrap_or_default()
}

fn store_identity(data: &std::path::Path, secret: &str) {
    if keyring::Entry::new("dev.dropbridge.app", "identity").and_then(|e| e.set_password(secret)).is_ok() {
        return;
    }
    let p = data.join("identity");
    let _ = std::fs::write(&p, secret);
    #[cfg(unix)]
    let _ = std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o600));
}

fn kind() -> &'static str {
    if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

#[derive(Serialize, Deserialize)]
struct TargetIn {
    host: String,
    port: u16,
    fingerprint: Option<String>,
    token: Option<String>,
}

type Res<T> = Result<T, String>;

#[tauri::command]
fn info(app: State<App>) -> Value {
    let s = app.settings.lock().unwrap().clone();
    let devices: Vec<Value> =
        app.node.devices().into_iter().map(|(fp, d)| json!({ "id": fp, "name": d.name, "blocked": d.blocked })).collect();
    json!({ "name": s.name, "fingerprint": app.node.fingerprint(), "port": app.node.port(), "ips": local_ips(), "settings": s, "devices": devices, "kind": kind(), "version": env!("CARGO_PKG_VERSION") })
}

#[tauri::command]
fn send(app: State<App>, target: TargetIn, paths: Vec<String>, text: Option<String>) -> Res<String> {
    let mut items: Vec<SendItem> = paths.into_iter().map(|path| SendItem::Path { path }).collect();
    items.extend(text.filter(|t| !t.is_empty()).map(|text| SendItem::Text { text }));
    if items.is_empty() {
        return Err("Choose files, a folder or some text to send first.".into());
    }
    Ok(app.node.send(Target { host: target.host, port: target.port, fingerprint: target.fingerprint, token: target.token }, items))
}

#[tauri::command]
fn respond(app: State<App>, id: String, accept: bool, trust: bool, conflict: Conflict) {
    app.node.respond(&id, Decision { accept, trust, conflict });
}

#[tauri::command]
fn control(app: State<App>, id: String, action: String) -> Option<String> {
    match action.as_str() {
        "pause" => app.node.pause(&id),
        "resume" => app.node.resume(&id),
        "cancel" => app.node.cancel(&id),
        "retry" => return app.node.retry(&id),
        _ => {}
    }
    None
}

#[tauri::command]
fn pairing(app: State<App>) -> Res<Value> {
    let uri = app.node.pairing_uri();
    let svg = qrcode::QrCode::new(uri.as_bytes())
        .map_err(|e| e.to_string())?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .quiet_zone(true)
        .build();
    Ok(json!({ "uri": uri, "code": app.node.pairing_code(), "svg": svg, "ips": local_ips(), "port": app.node.port() }))
}

/// Accepts a dropbridge:// link or `host:port`.
#[tauri::command]
fn parse_target(input: String) -> Res<TargetIn> {
    let s = input.trim();
    if s.starts_with("dropbridge://") {
        let t = parse_pairing_uri(s).map_err(|e| e.to_string())?;
        return Ok(TargetIn { host: t.host, port: t.port, fingerprint: t.fingerprint, token: t.token });
    }
    let (h, p) = s.rsplit_once(':').ok_or("Enter the address shown on the other device, like 192.168.1.20:52345")?;
    let port = p.parse().map_err(|_| "The port must be a number, like 192.168.1.20:52345")?;
    Ok(TargetIn { host: h.trim_matches(['[', ']']).into(), port, fingerprint: None, token: None })
}

#[tauri::command]
fn device(app: State<App>, id: String, name: String, action: String) -> Res<()> {
    match action.as_str() {
        "trust" => app.node.trust(&id, &name),
        "block" => app.node.block(&id, &name),
        _ => app.node.forget(&id),
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_settings(app: State<App>, settings: Settings) -> Res<()> {
    app.node.set_download_dir(&settings.download_dir).map_err(|e| format!("Can't use that folder: {e}"))?;
    app.node.set_auto_accept(settings.auto_accept);
    *app.settings.lock().unwrap() = settings;
    app.save_settings();
    Ok(())
}

#[tauri::command]
fn history(app: State<App>) -> Vec<Value> {
    app.history.lock().unwrap().clone()
}

#[tauri::command]
fn clear_history(app: State<App>) {
    app.history.lock().unwrap().clear();
    let _ = std::fs::remove_file(app.data.join("history.json"));
}

/// Paths passed on the command line (Windows "Send to", `open -a`, second instance).
fn staged_paths(argv: &[String]) -> Vec<String> {
    argv.iter().skip(1).filter(|a| !a.starts_with('-') && std::path::Path::new(a).exists()).cloned().collect()
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let h = app.handle().clone();
    let data = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data)?;
    let defaults = Settings { name: device_name(), download_dir: app.path().download_dir()?.to_string_lossy().into(), auto_accept: false };
    let settings: Settings =
        std::fs::read(data.join("settings.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(defaults);
    let history = std::fs::read(data.join("history.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let identity = load_identity(&data);
    let cfg = NodeConfig {
        name: settings.name.clone(),
        kind: kind().into(),
        identity: identity.clone(),
        data_dir: data.to_string_lossy().into(),
        download_dir: settings.download_dir.clone(),
        port: 0,
        discovery: true,
        auto_accept_trusted: settings.auto_accept,
    };
    let node = Node::new(cfg, std::sync::Arc::new(Bridge(h.clone()))).map_err(|e| format!("{e:#}"))?;
    if identity.is_empty() {
        store_identity(&data, &node.identity_secret());
    }
    app.manage(App { node, data, settings: Mutex::new(settings), history: Mutex::new(history) });

    let open = MenuItem::with_id(app, "open", "Open DropBridge", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit DropBridge", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    TrayIconBuilder::new()
        .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .icon_as_template(true)
        .tooltip("DropBridge — ready to receive")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "quit" => app.exit(0),
            _ => show(app),
        })
        .on_tray_icon_event(|t, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, .. } = e {
                show(t.app_handle());
            }
        })
        .build(app)?;

    let staged = staged_paths(&std::env::args().collect::<Vec<_>>());
    if !staged.is_empty() {
        let h2 = h.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(800)); // let the page load
            let _ = h2.emit("stage", staged);
        });
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _| {
            show(app);
            let p = staged_paths(&argv);
            if !p.is_empty() {
                let _ = app.emit("stage", p);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .setup(setup)
        .on_window_event(|w, e| {
            // Closing the window keeps DropBridge receiving in the menu bar / tray.
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            info,
            send,
            respond,
            control,
            pairing,
            parse_target,
            device,
            set_settings,
            history,
            clear_history
        ])
        .run(tauri::generate_context!())
        .expect("failed to start DropBridge");
}
