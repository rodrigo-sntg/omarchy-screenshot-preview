use std::cell::Cell;
use std::env;
use std::fs;
use std::process::Command;
use std::rc::Rc;

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{self as gtk, CssProvider};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

const PREVIEW_WIDTH: i32 = 220;
const MARGIN_BOTTOM: i32 = 80;
const MARGIN_RIGHT: i32 = 20;
const DISMISS_MS: u64 = 5000;
const FADE_STEP_MS: u64 = 16;
const FADE_DURATION_MS: f64 = 300.0;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <filepath> [editor_args...]", args[0]);
        std::process::exit(1);
    }

    let filepath = args[1].clone();
    let editor_args: Vec<String> = if args.len() > 2 {
        args[2..].to_vec()
    } else {
        vec![
            "satty".into(),
            "--filename".into(),
            filepath.clone(),
            "--output-filename".into(),
            filepath.clone(),
            "--actions-on-enter".into(),
            "save-to-clipboard".into(),
            "--save-after-copy".into(),
            "--copy-command".into(),
            "wl-copy".into(),
        ]
    };

    let app = gtk::Application::builder()
        .application_id("com.omarchy.screenshot.preview")
        .build();

    app.connect_activate(move |app| {
        build_ui(app, &filepath, &editor_args);
    });

    app.run_with_args::<String>(&[]);
}

fn build_ui(app: &gtk::Application, filepath: &str, editor_args: &[String]) {
    let win = gtk::ApplicationWindow::builder()
        .application(app)
        .decorated(false)
        .resizable(false)
        .build();
    win.set_widget_name("screenshot-preview");

    // Layer shell setup
    win.init_layer_shell();
    win.set_layer(Layer::Overlay);
    win.set_anchor(Edge::Bottom, true);
    win.set_anchor(Edge::Right, true);
    win.set_margin(Edge::Bottom, MARGIN_BOTTOM);
    win.set_margin(Edge::Right, MARGIN_RIGHT);
    win.set_exclusive_zone(0);
    win.set_namespace(Some("screenshot-preview"));
    win.set_keyboard_mode(KeyboardMode::None);

    // Load and scale screenshot
    let pixbuf =
        gtk4::gdk_pixbuf::Pixbuf::from_file(filepath).expect("Failed to load screenshot");
    let orig_w = pixbuf.width() as f64;
    let orig_h = pixbuf.height() as f64;
    let aspect = orig_h / orig_w;
    let preview_h = (PREVIEW_WIDTH as f64 * aspect) as i32;

    let scaled = pixbuf
        .scale_simple(
            PREVIEW_WIDTH,
            preview_h,
            gtk4::gdk_pixbuf::InterpType::Bilinear,
        )
        .expect("Failed to scale pixbuf");
    let texture = gdk::Texture::for_pixbuf(&scaled);

    let image = gtk::Picture::for_paintable(&texture);
    image.set_size_request(PREVIEW_WIDTH, preview_h);

    // Drag icon texture
    let drag_w = 120;
    let drag_h = (drag_w as f64 * aspect) as i32;
    let drag_pixbuf = pixbuf
        .scale_simple(drag_w, drag_h, gtk4::gdk_pixbuf::InterpType::Bilinear)
        .expect("Failed to scale drag pixbuf");
    let drag_texture = gdk::Texture::for_pixbuf(&drag_pixbuf);

    // Frame container
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("preview-frame");
    frame.append(&image);
    frame.set_cursor_from_name(Some("pointer"));
    win.set_child(Some(&frame));

    // CSS
    let css = CssProvider::new();
    css.load_from_data(
        "window#screenshot-preview {
            background: transparent;
        }
        .preview-frame {
            background: #1e1e2e;
            border-radius: 12px;
            border: 2px solid rgba(255, 255, 255, 0.15);
            padding: 8px;
        }
        .preview-frame:hover {
            border-color: rgba(255, 255, 255, 0.4);
        }
        .preview-frame picture {
            border-radius: 6px;
        }",
    );
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Shared state for timers
    let dismiss_source: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
    let fade_source: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));

    // --- Drag source ---
    let drag_source = gtk::DragSource::new();
    drag_source.set_actions(gdk::DragAction::COPY);

    let filepath_for_drag = filepath.to_string();
    drag_source.connect_prepare(move |_source, _x, _y| {
        let file = gtk4::gio::File::for_path(&filepath_for_drag);
        let uri = file.uri();
        let uri_data = format!("{}\r\n", uri);

        let uri_provider = gdk::ContentProvider::for_bytes(
            "text/uri-list",
            &glib::Bytes::from(uri_data.as_bytes()),
        );

        if let Ok(img_data) = fs::read(&filepath_for_drag) {
            let png_provider =
                gdk::ContentProvider::for_bytes("image/png", &glib::Bytes::from(&img_data));
            Some(gdk::ContentProvider::new_union(&[uri_provider, png_provider]))
        } else {
            Some(uri_provider)
        }
    });

    let dismiss_for_drag = dismiss_source.clone();
    drag_source.connect_drag_begin(move |source, _drag| {
        source.set_icon(Some(&drag_texture), 0, 0);
        cancel_timer(&dismiss_for_drag);
    });

    let win_for_drag = win.clone();
    let fade_for_drag = fade_source.clone();
    drag_source.connect_drag_end(move |_source, _drag, _delete| {
        start_fade_out(&win_for_drag, &fade_for_drag);
    });

    frame.add_controller(drag_source);

    // --- Click gesture ---
    let click = gtk::GestureClick::new();
    let editor_args_owned = editor_args.to_vec();
    let dismiss_for_click = dismiss_source.clone();
    let win_for_click = win.clone();
    let fade_for_click = fade_source.clone();
    click.connect_released(move |_gesture, _n, _x, _y| {
        cancel_timer(&dismiss_for_click);
        if let Some((cmd, args)) = editor_args_owned.split_first() {
            let _ = Command::new(cmd).args(args).spawn();
        }
        start_fade_out(&win_for_click, &fade_for_click);
    });
    frame.add_controller(click);

    // --- Hover: pause/resume dismiss ---
    let motion = gtk::EventControllerMotion::new();
    let dismiss_for_enter = dismiss_source.clone();
    motion.connect_enter(move |_ctrl, _x, _y| {
        cancel_timer(&dismiss_for_enter);
    });

    let dismiss_for_leave = dismiss_source.clone();
    let win_for_leave = win.clone();
    let fade_for_leave = fade_source.clone();
    motion.connect_leave(move |_ctrl| {
        start_dismiss_timer(&win_for_leave, &dismiss_for_leave, &fade_for_leave);
    });
    win.add_controller(motion);

    // Present with fade-in, then start dismiss timer
    win.set_opacity(0.0);
    win.present();
    start_fade_in(&win, &fade_source);
    start_dismiss_timer(&win, &dismiss_source, &fade_source);
}

// --- Timer helpers ---

fn cancel_timer(source: &Rc<Cell<Option<glib::SourceId>>>) {
    if let Some(id) = source.take() {
        id.remove();
    }
}

fn start_dismiss_timer(
    win: &gtk::ApplicationWindow,
    dismiss_source: &Rc<Cell<Option<glib::SourceId>>>,
    fade_source: &Rc<Cell<Option<glib::SourceId>>>,
) {
    cancel_timer(dismiss_source);
    let win = win.clone();
    let fade = fade_source.clone();
    let id = glib::timeout_add_local_once(std::time::Duration::from_millis(DISMISS_MS), move || {
        start_fade_out(&win, &fade);
    });
    dismiss_source.set(Some(id));
}

fn start_fade_in(win: &gtk::ApplicationWindow, fade_source: &Rc<Cell<Option<glib::SourceId>>>) {
    cancel_timer(fade_source);
    let win = win.clone();
    let start = std::time::Instant::now();
    let id = glib::timeout_add_local(
        std::time::Duration::from_millis(FADE_STEP_MS),
        move || {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let progress = (elapsed / FADE_DURATION_MS).min(1.0);
            win.set_opacity(progress);
            if progress >= 1.0 {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        },
    );
    fade_source.set(Some(id));
}

fn start_fade_out(win: &gtk::ApplicationWindow, fade_source: &Rc<Cell<Option<glib::SourceId>>>) {
    cancel_timer(fade_source);
    let win = win.clone();
    let start_opacity = win.opacity();
    let start = std::time::Instant::now();
    let id = glib::timeout_add_local(
        std::time::Duration::from_millis(FADE_STEP_MS),
        move || {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let progress = (elapsed / FADE_DURATION_MS).min(1.0);
            win.set_opacity(start_opacity * (1.0 - progress));
            if progress >= 1.0 {
                win.close();
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        },
    );
    fade_source.set(Some(id));
}
