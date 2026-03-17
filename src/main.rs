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
const DRAG_ICON_WIDTH: i32 = 120;
const MARGIN_BOTTOM: i32 = 80;
const MARGIN_RIGHT: i32 = 20;
const DISMISS_MS: u64 = 5000;
const FADE_STEP_MS: u64 = 16;
const FADE_DURATION_MS: f64 = 300.0;

type TimerHandle = Rc<Cell<Option<glib::SourceId>>>;

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

fn scale_to_width(pixbuf: &gtk4::gdk_pixbuf::Pixbuf, width: i32, aspect: f64) -> gdk::Texture {
    let height = (width as f64 * aspect) as i32;
    let scaled = pixbuf
        .scale_simple(width, height, gtk4::gdk_pixbuf::InterpType::Bilinear)
        .expect("Failed to scale pixbuf");
    gdk::Texture::for_pixbuf(&scaled)
}

fn build_ui(app: &gtk::Application, filepath: &str, editor_args: &[String]) {
    let win = gtk::ApplicationWindow::builder()
        .application(app)
        .decorated(false)
        .resizable(false)
        .build();
    win.set_widget_name("screenshot-preview");

    // Layer shell: overlay in bottom-right, no keyboard grab
    win.init_layer_shell();
    win.set_layer(Layer::Overlay);
    win.set_anchor(Edge::Bottom, true);
    win.set_anchor(Edge::Right, true);
    win.set_margin(Edge::Bottom, MARGIN_BOTTOM);
    win.set_margin(Edge::Right, MARGIN_RIGHT);
    win.set_exclusive_zone(0);
    win.set_namespace(Some("screenshot-preview"));
    win.set_keyboard_mode(KeyboardMode::None);

    // Load screenshot and create scaled textures
    let pixbuf =
        gtk4::gdk_pixbuf::Pixbuf::from_file(filepath).expect("Failed to load screenshot");
    let aspect = pixbuf.height() as f64 / pixbuf.width() as f64;
    let preview_h = (PREVIEW_WIDTH as f64 * aspect) as i32;

    let texture = scale_to_width(&pixbuf, PREVIEW_WIDTH, aspect);
    let drag_texture = scale_to_width(&pixbuf, DRAG_ICON_WIDTH, aspect);

    let image = gtk::Picture::for_paintable(&texture);
    image.set_size_request(PREVIEW_WIDTH, preview_h);

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

    // Timer state
    let dismiss_handle: TimerHandle = Rc::new(Cell::new(None));
    let fade_handle: TimerHandle = Rc::new(Cell::new(None));

    // Pre-cache drag data to avoid re-reading the file on every drag gesture
    let uri = gtk4::gio::File::for_path(filepath).uri();
    let uri_bytes = Rc::new(glib::Bytes::from(format!("{}\r\n", uri).as_bytes()));
    let png_bytes = fs::read(filepath).ok().map(|v| Rc::new(glib::Bytes::from_owned(v)));

    // Drag source
    let drag_source = gtk::DragSource::new();
    drag_source.set_actions(gdk::DragAction::COPY);

    drag_source.connect_prepare({
        let uri_bytes = uri_bytes.clone();
        let png_bytes = png_bytes.clone();
        move |_source, _x, _y| {
            let uri_provider = gdk::ContentProvider::for_bytes("text/uri-list", &uri_bytes);
            if let Some(ref bytes) = png_bytes {
                let png_provider = gdk::ContentProvider::for_bytes("image/png", bytes);
                Some(gdk::ContentProvider::new_union(&[uri_provider, png_provider]))
            } else {
                Some(uri_provider)
            }
        }
    });

    drag_source.connect_drag_begin({
        let dismiss_handle = dismiss_handle.clone();
        move |source, _drag| {
            source.set_icon(Some(&drag_texture), 0, 0);
            cancel_timer(&dismiss_handle);
        }
    });

    drag_source.connect_drag_end({
        let win = win.clone();
        let fade_handle = fade_handle.clone();
        move |_source, _drag, _delete| {
            start_fade(&win, &fade_handle, 1.0, 0.0, true);
        }
    });

    frame.add_controller(drag_source);

    // Click: open editor and dismiss
    let click = gtk::GestureClick::new();
    click.connect_released({
        let editor_args = editor_args.to_vec();
        let dismiss_handle = dismiss_handle.clone();
        let win = win.clone();
        let fade_handle = fade_handle.clone();
        move |_gesture, _n, _x, _y| {
            cancel_timer(&dismiss_handle);
            if let Some((cmd, args)) = editor_args.split_first() {
                let _ = Command::new(cmd).args(args).spawn();
            }
            start_fade(&win, &fade_handle, 1.0, 0.0, true);
        }
    });
    frame.add_controller(click);

    // Hover: pause/resume dismiss timer
    let motion = gtk::EventControllerMotion::new();
    motion.connect_enter({
        let dismiss_handle = dismiss_handle.clone();
        move |_ctrl, _x, _y| {
            cancel_timer(&dismiss_handle);
        }
    });
    motion.connect_leave({
        let win = win.clone();
        let dismiss_handle = dismiss_handle.clone();
        let fade_handle = fade_handle.clone();
        move |_ctrl| {
            start_dismiss_timer(&win, &dismiss_handle, &fade_handle);
        }
    });
    win.add_controller(motion);

    // Present with fade-in, then start dismiss timer
    win.set_opacity(0.0);
    win.present();
    start_fade(&win, &fade_handle, 0.0, 1.0, false);
    start_dismiss_timer(&win, &dismiss_handle, &fade_handle);
}

// --- Timer helpers ---

fn cancel_timer(handle: &TimerHandle) {
    if let Some(id) = handle.take() {
        id.remove();
    }
}

fn start_dismiss_timer(
    win: &gtk::ApplicationWindow,
    dismiss_handle: &TimerHandle,
    fade_handle: &TimerHandle,
) {
    cancel_timer(dismiss_handle);
    let win = win.clone();
    let fade_handle = fade_handle.clone();
    let id = glib::timeout_add_local_once(
        std::time::Duration::from_millis(DISMISS_MS),
        move || {
            start_fade(&win, &fade_handle, 1.0, 0.0, true);
        },
    );
    dismiss_handle.set(Some(id));
}

fn start_fade(
    win: &gtk::ApplicationWindow,
    fade_handle: &TimerHandle,
    from: f64,
    to: f64,
    close_on_done: bool,
) {
    cancel_timer(fade_handle);
    let win = win.clone();
    let start = std::time::Instant::now();
    let id = glib::timeout_add_local(
        std::time::Duration::from_millis(FADE_STEP_MS),
        move || {
            let progress = (start.elapsed().as_secs_f64() * 1000.0 / FADE_DURATION_MS).min(1.0);
            win.set_opacity(from + (to - from) * progress);
            if progress >= 1.0 {
                if close_on_done {
                    win.close();
                }
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        },
    );
    fade_handle.set(Some(id));
}
