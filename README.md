# omarchy-screenshot-preview

macOS-style screenshot preview for Wayland/Hyprland. After taking a screenshot, a draggable thumbnail appears in the bottom-right corner.

- **Drag** it into any app to share the image
- **Click** to open the screenshot editor (satty by default)
- **Hover** to pause the auto-dismiss timer
- Auto-dismisses after 5 seconds with a fade animation

Built with Rust, GTK4, and gtk4-layer-shell. Single 420K binary, no runtime dependencies beyond GTK4.

## Install

### From AUR

```bash
yay -S omarchy-screenshot-preview
```

### From source

```bash
git clone https://github.com/rodrigo-sntg/omarchy-screenshot-preview.git
cd omarchy-screenshot-preview
cargo build --release
sudo install -Dm755 target/release/omarchy-screenshot-preview /usr/bin/omarchy-screenshot-preview
```

**Build dependencies:** `cargo`, `gtk4`, `gtk4-layer-shell`

## Setup on Omarchy

The preview tool needs to be wired into the screenshot workflow. There is a [pending PR](https://github.com/basecamp/omarchy/pull/5054) to add native hook support. Until it's merged, follow these steps:

### Step 1: Override the screenshot command

Copy the original script and patch it:

```bash
cp ~/.local/share/omarchy/bin/omarchy-cmd-screenshot ~/.local/bin/omarchy-cmd-screenshot
```

Edit `~/.local/bin/omarchy-cmd-screenshot` and find this block near the end of the file:

```bash
  (
    ACTION=$(notify-send "Screenshot saved to clipboard and file" "Edit with Super + Alt + , (or click this)" -t 10000 -i "$FILEPATH" -A "default=edit")
    [[ $ACTION == "default" ]] && open_editor "$FILEPATH"
  ) &
```

Replace it with:

```bash
  pkill -f 'omarchy-screenshot-preview' 2>/dev/null
  omarchy-screenshot-preview "$FILEPATH" &
```

### Step 2: Override the Print Screen binding

Hyprland may not have `~/.local/bin` in its PATH. Add this to `~/.config/hypr/bindings.conf`:

```
unbind = , PRINT
bindd = , PRINT, Screenshot, exec, ~/.local/bin/omarchy-cmd-screenshot
```

Hyprland auto-reloads on save. Press Print Screen to test.

### After the PR is merged

Once [PR #5054](https://github.com/basecamp/omarchy/pull/5054) is merged, the setup simplifies to:

```bash
yay -S omarchy-screenshot-preview

# Enable the screenshot hook
cp ~/.config/omarchy/hooks/screenshot.sample ~/.config/omarchy/hooks/screenshot
```

Edit `~/.config/omarchy/hooks/screenshot` to contain:

```bash
#!/bin/bash
omarchy-screenshot-preview "$1"
```

No binding override needed.

## Setup on other Hyprland systems (non-Omarchy)

The preview tool works standalone. Call it after capturing with `grim`:

```bash
grim -g "$(slurp)" /tmp/screenshot.png && wl-copy < /tmp/screenshot.png && omarchy-screenshot-preview /tmp/screenshot.png &
```

## Usage

```
omarchy-screenshot-preview <filepath> [editor_args...]
```

**Examples:**

```bash
# Default: click opens satty editor
omarchy-screenshot-preview ~/Pictures/screenshot.png

# Custom editor
omarchy-screenshot-preview ~/Pictures/screenshot.png gimp ~/Pictures/screenshot.png
```

## Hyprland layer rule (optional)

Add to `~/.config/hypr/looknfeel.conf` for a slide-in animation:

```
layerrule = animation slide bottom, match:namespace screenshot-preview
```

## License

MIT
