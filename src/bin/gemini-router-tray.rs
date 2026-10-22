#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use claude_codex_gemini_router::ctl;
use claude_codex_gemini_router::ui::{
    format_tray_tooltip, generate_tray_icon_rgba, TrayVisualState, CONTROL_PANEL_HTML,
};
use serde_json::{Map, Value};
use std::borrow::Cow;
use tao::{
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::{Window, WindowBuilder},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, MouseButtonState, Rect, TrayIconBuilder, TrayIconEvent,
};
use wry::{
    http::{header::CONTENT_TYPE, Request, Response, StatusCode},
    WebViewBuilder,
};

#[cfg(target_os = "macos")]
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};

#[cfg(target_os = "windows")]
use tao::platform::windows::WindowBuilderExtWindows;

#[cfg(target_os = "linux")]
use tao::platform::unix::WindowExtUnix;
#[cfg(target_os = "linux")]
use wry::WebViewBuilderExtUnix;

#[derive(Debug, Clone)]
enum UserEvent {
    TrayIcon(TrayIconEvent),
    Menu(MenuEvent),
    StatusUpdated(Map<String, Value>),
    HideWindow,
}

fn load_tray_icon(state: TrayVisualState) -> Icon {
    let (rgba, w, h) = generate_tray_icon_rgba(state);
    Icon::from_rgba(rgba, w, h).expect("Failed to create 32x32 RGBA tray icon")
}

fn build_http_response(
    status: StatusCode,
    content_type: &'static str,
    body: Vec<u8>,
) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, content_type)
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store")
        .body(Cow::Owned(body))
        .unwrap_or_else(|_| Response::new(Cow::Borrowed(b"{}" as &[u8])))
}

fn position_popover_near_tray(
    window: &Window,
    tray_rect: Option<Rect>,
    click_pos: Option<PhysicalPosition<f64>>,
) {
    let win_size: PhysicalSize<u32> = window.outer_size();
    let win_w = win_size.width as i32;
    let win_h = win_size.height as i32;

    let monitor = window
        .current_monitor()
        .or_else(|| window.primary_monitor());

    let (mon_x, mon_y, mon_w, mon_h) = if let Some(m) = monitor {
        let pos = m.position();
        let size = m.size();
        (pos.x, pos.y, size.width as i32, size.height as i32)
    } else {
        (0, 0, 1920, 1080)
    };

    let (anchor_x, anchor_top_y, anchor_bottom_y) = if let Some(rect) = tray_rect {
        let rx = rect.position.x as i32;
        let ry = rect.position.y as i32;
        let rw = rect.size.width as i32;
        let rh = rect.size.height as i32;
        if rw > 0 || rh > 0 {
            (rx + rw / 2, ry, ry + rh)
        } else if let Some(pos) = click_pos {
            (pos.x as i32, pos.y as i32, pos.y as i32)
        } else {
            (mon_x + mon_w - win_w / 2 - 24, mon_y + 32, mon_y + 32)
        }
    } else if let Some(pos) = click_pos {
        (pos.x as i32, pos.y as i32, pos.y as i32)
    } else {
        #[cfg(target_os = "windows")]
        {
            (
                mon_x + mon_w - win_w / 2 - 24,
                mon_y + mon_h - 48,
                mon_y + mon_h - 48,
            )
        }
        #[cfg(not(target_os = "windows"))]
        {
            (mon_x + mon_w - win_w / 2 - 24, mon_y + 32, mon_y + 32)
        }
    };

    let mut target_x = anchor_x - (win_w / 2);
    let min_x = mon_x + 8;
    let max_x = (mon_x + mon_w - win_w - 8).max(min_x);
    target_x = target_x.clamp(min_x, max_x);

    let is_bottom_bar = anchor_top_y > (mon_y + mon_h / 2);
    let mut target_y = if is_bottom_bar {
        anchor_top_y - win_h - 8
    } else {
        anchor_bottom_y + 8
    };

    let min_y = mon_y + 8;
    let max_y = (mon_y + mon_h - win_h - 8).max(min_y);
    target_y = target_y.clamp(min_y, max_y);

    window.set_outer_position(PhysicalPosition::new(target_x, target_y));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut show_on_launch = false;
    let mut autostart_router = false;

    for arg in &args[1..] {
        match arg.as_str() {
            "--show" => show_on_launch = true,
            "--autostart-router" => autostart_router = true,
            "--help" | "-h" => {
                println!(
                    "Usage: gemini-router-tray [--show] [--autostart-router]\n\n\
                     Cross-platform notification bar / system tray application for Gemini ADC Router.\n\
                     Options:\n  \
                       --show              Open the control panel popover immediately on launch\n  \
                       --autostart-router  Start claude-gemini-router daemon on launch if stopped"
                );
                return;
            }
            _ => {}
        }
    }

    if autostart_router && !ctl::is_router_daemon_running() {
        let _ = ctl::cmd_start_router();
    }

    #[cfg(target_os = "linux")]
    {
        if let Err(e) = gtk::init() {
            eprintln!("Failed to initialize GTK for notification bar app: {}", e);
            std::process::exit(1);
        }
    }

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    // On macOS, run as a Menu Bar / Notification Bar Accessory app (no Dock icon)
    #[cfg(target_os = "macos")]
    {
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }

    let proxy = event_loop.create_proxy();

    // Forward tray icon click events into the main event loop
    let proxy_tray = proxy.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = proxy_tray.send_event(UserEvent::TrayIcon(event));
    }));

    // Forward tray context menu events into the main event loop
    let proxy_menu = proxy.clone();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy_menu.send_event(UserEvent::Menu(event));
    }));

    let id_toggle_panel = MenuId::new("toggle_panel");
    let id_toggle_claude = MenuId::new("toggle_claude");
    let id_toggle_codex = MenuId::new("toggle_codex");
    let id_model_flash = MenuId::new("model_flash");
    let id_model_argon = MenuId::new("model_argon");
    let id_model_flip = MenuId::new("model_flip");
    let id_restart_router = MenuId::new("restart_router");
    let id_adc_login = MenuId::new("adc_login");
    let id_quit = MenuId::new("quit");

    let tray_menu = Menu::new();
    let item_open = MenuItem::with_id(
        id_toggle_panel.clone(),
        "Open Gemini ADC Control Panel",
        true,
        None,
    );
    let item_claude = MenuItem::with_id(
        id_toggle_claude.clone(),
        "Toggle Claude Code -> Gemini",
        true,
        None,
    );
    let item_codex = MenuItem::with_id(
        id_toggle_codex.clone(),
        "Toggle OpenAI Codex -> Gemini",
        true,
        None,
    );
    let item_flash = MenuItem::with_id(
        id_model_flash.clone(),
        "Model: Gemini 3.8 Flash",
        true,
        None,
    );
    let item_argon = MenuItem::with_id(
        id_model_argon.clone(),
        "Model: Gemini 4.0 Argon",
        true,
        None,
    );
    let item_flip = MenuItem::with_id(
        id_model_flip.clone(),
        "Cycle Next Gemini Model (--flip)",
        true,
        None,
    );
    let item_restart = MenuItem::with_id(
        id_restart_router.clone(),
        "Start / Restart Router Daemon",
        true,
        None,
    );
    let item_adc_login = MenuItem::with_id(
        id_adc_login.clone(),
        "Re-authenticate ADC (gcloud)...",
        true,
        None,
    );
    let item_quit = MenuItem::with_id(id_quit.clone(), "Quit Gemini Router Tray", true, None);

    let _ = tray_menu.append(&item_open);
    let _ = tray_menu.append(&PredefinedMenuItem::separator());
    let _ = tray_menu.append(&item_claude);
    let _ = tray_menu.append(&item_codex);
    let _ = tray_menu.append(&PredefinedMenuItem::separator());
    let _ = tray_menu.append(&item_flash);
    let _ = tray_menu.append(&item_argon);
    let _ = tray_menu.append(&item_flip);
    let _ = tray_menu.append(&PredefinedMenuItem::separator());
    let _ = tray_menu.append(&item_restart);
    let _ = tray_menu.append(&item_adc_login);
    let _ = tray_menu.append(&PredefinedMenuItem::separator());
    let _ = tray_menu.append(&item_quit);

    let initial_status = ctl::get_status_map();
    let initial_visual = TrayVisualState::from_status(&initial_status);
    let initial_tooltip = format_tray_tooltip(&initial_status);

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip(&initial_tooltip)
        .with_icon(load_tray_icon(initial_visual))
        .with_menu_on_left_click(false)
        .build()
        .expect("Failed to create system notification bar icon");

    tray_icon.set_show_menu_on_right_click(true);

    // Build frameless popover window matching GeminiRouter.qml (width: 380, height: 620)
    #[allow(unused_mut)]
    let mut window_builder = WindowBuilder::new()
        .with_title("Gemini ADC Router")
        .with_inner_size(LogicalSize::new(380.0, 620.0))
        .with_resizable(false)
        .with_decorations(false)
        .with_always_on_top(true)
        .with_visible(show_on_launch);

    #[cfg(target_os = "windows")]
    {
        window_builder = window_builder.with_skip_taskbar(true);
    }

    let window = window_builder
        .build(&event_loop)
        .expect("Failed to create popover window");

    // Register asynchronous custom protocol `wry://localhost/...` so UI actions never block the main thread
    let proxy_protocol = proxy.clone();
    let webview_builder = WebViewBuilder::new()
        .with_background_color((30, 30, 46, 255)) // Catppuccin Mocha #1e1e2e
        .with_asynchronous_custom_protocol(
            "wry".into(),
            move |_webview_id, request: Request<Vec<u8>>, responder| {
                let path = request.uri().path().to_string();
                let method = request.method().as_str().to_uppercase();
                let body_bytes = request.into_body();
                let proxy_inner = proxy_protocol.clone();

                if method == "OPTIONS" {
                    responder.respond(build_http_response(
                        StatusCode::NO_CONTENT,
                        "text/plain",
                        Vec::new(),
                    ));
                    return;
                }

                if method == "GET" && (path == "/" || path == "/ui" || path == "/index.html") {
                    responder.respond(build_http_response(
                        StatusCode::OK,
                        "text/html; charset=utf-8",
                        CONTROL_PANEL_HTML.as_bytes().to_vec(),
                    ));
                    return;
                }

                if method == "POST" && path == "/api/window/hide" {
                    let _ = proxy_inner.send_event(UserEvent::HideWindow);
                    responder.respond(build_http_response(
                        StatusCode::OK,
                        "application/json",
                        b"{\"ok\":true}".to_vec(),
                    ));
                    return;
                }

                std::thread::spawn(move || {
                    if method == "GET" && path == "/api/status" {
                        let status = ctl::get_status_map();
                        let _ = proxy_inner.send_event(UserEvent::StatusUpdated(status.clone()));
                        let bytes = serde_json::to_vec(&Value::Object(status))
                            .unwrap_or_else(|_| b"{}".to_vec());
                        responder.respond(build_http_response(
                            StatusCode::OK,
                            "application/json",
                            bytes,
                        ));
                    } else if method == "POST" && path == "/api/action" {
                        let payload: Value =
                            serde_json::from_slice(&body_bytes).unwrap_or_default();
                        let response = match ctl::dispatch_json_action(&payload) {
                            Ok(status) => {
                                let _ = proxy_inner
                                    .send_event(UserEvent::StatusUpdated(status.clone()));
                                let bytes = serde_json::to_vec(&Value::Object(status))
                                    .unwrap_or_else(|_| b"{}".to_vec());
                                build_http_response(StatusCode::OK, "application/json", bytes)
                            }
                            Err(e) => {
                                let err_json = serde_json::json!({ "ok": false, "error": e });
                                build_http_response(
                                    StatusCode::BAD_REQUEST,
                                    "application/json",
                                    err_json.to_string().into_bytes(),
                                )
                            }
                        };
                        responder.respond(response);
                    } else {
                        responder.respond(build_http_response(
                            StatusCode::NOT_FOUND,
                            "application/json",
                            b"{\"error\":\"not_found\"}".to_vec(),
                        ));
                    }
                });
            },
        )
        .with_url("wry://localhost/index.html");

    #[cfg(target_os = "linux")]
    let _webview = {
        let vbox = window
            .default_vbox()
            .expect("Failed to get default GTK vbox from window");
        webview_builder
            .build_gtk(vbox)
            .expect("Failed to build GTK WebView")
    };

    #[cfg(not(target_os = "linux"))]
    let _webview = webview_builder
        .build(&window)
        .expect("Failed to build native WebView");

    if show_on_launch {
        position_popover_near_tray(&window, tray_icon.rect(), None);
        window.set_visible(true);
        window.set_focus();
    }

    // Background status polling thread to keep tray icon color & tooltip in sync
    let proxy_bg = proxy.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(4));
        let status = ctl::get_status_map();
        if proxy_bg
            .send_event(UserEvent::StatusUpdated(status))
            .is_err()
        {
            break;
        }
    });

    let mut current_visual = initial_visual;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {}

            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                window.set_visible(false);
            }

            Event::WindowEvent {
                event: WindowEvent::Focused(false),
                ..
            } => {
                window.set_visible(false);
            }

            Event::UserEvent(UserEvent::HideWindow) => {
                window.set_visible(false);
            }

            Event::UserEvent(UserEvent::StatusUpdated(status)) => {
                let next_visual = TrayVisualState::from_status(&status);
                if next_visual != current_visual {
                    current_visual = next_visual;
                    let _ = tray_icon.set_icon(Some(load_tray_icon(current_visual)));
                }
                let tooltip = format_tray_tooltip(&status);
                let _ = tray_icon.set_tooltip(Some(tooltip));
            }

            Event::UserEvent(UserEvent::TrayIcon(TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                position,
                ..
            })) => {
                if window.is_visible() {
                    window.set_visible(false);
                } else {
                    position_popover_near_tray(&window, Some(rect), Some(position));
                    window.set_visible(true);
                    window.set_focus();
                }
            }
            Event::UserEvent(UserEvent::TrayIcon(_)) => {}

            Event::UserEvent(UserEvent::Menu(menu_event)) => {
                if menu_event.id == id_toggle_panel {
                    if window.is_visible() {
                        window.set_visible(false);
                    } else {
                        position_popover_near_tray(&window, tray_icon.rect(), None);
                        window.set_visible(true);
                        window.set_focus();
                    }
                } else if menu_event.id == id_quit {
                    *control_flow = ControlFlow::Exit;
                } else {
                    let menu_id = menu_event.id.clone();
                    let proxy_action = proxy.clone();
                    let id_claude = id_toggle_claude.clone();
                    let id_codex = id_toggle_codex.clone();
                    let id_flash = id_model_flash.clone();
                    let id_argon = id_model_argon.clone();
                    let id_flip = id_model_flip.clone();
                    let id_restart = id_restart_router.clone();
                    let id_login = id_adc_login.clone();

                    std::thread::spawn(move || {
                        if menu_id == id_claude {
                            let _ = ctl::cmd_toggle("claude");
                        } else if menu_id == id_codex {
                            let _ = ctl::cmd_toggle("codex");
                        } else if menu_id == id_flash {
                            let _ = ctl::cmd_set_model(
                                Some("gemini-3.8-flash"),
                                None,
                                false,
                                None,
                                None,
                                None,
                                None,
                                None,
                            );
                        } else if menu_id == id_argon {
                            let _ = ctl::cmd_set_model(
                                Some("gemini-4-argon"),
                                None,
                                false,
                                None,
                                None,
                                None,
                                None,
                                None,
                            );
                        } else if menu_id == id_flip {
                            let _ =
                                ctl::cmd_set_model(None, None, true, None, None, None, None, None);
                        } else if menu_id == id_restart {
                            let _ = ctl::cmd_restart_router();
                        } else if menu_id == id_login {
                            let _ = ctl::cmd_adc_login();
                        }
                        let status = ctl::get_status_map();
                        let _ = proxy_action.send_event(UserEvent::StatusUpdated(status));
                    });
                }
            }

            _ => {}
        }
    });
}
