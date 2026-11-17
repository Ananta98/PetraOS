# PetraOS Desktop Production-Readiness Spec

> Purpose: turn the desktop/GUI analysis into an actionable backlog that an AI agent can
> execute one task at a time. Each task has a stable ID, evidence in the current source,
> proposed fix, acceptance criteria, likely files, and verification steps.
>
> How to use: give an agent one task ID (e.g. "implement T-01 and T-02 from
> `spec/DESKTOP_PRODUCTION_READINESS.md`") and require the mandatory post-change workflow
> from `AGENTS.md` (git diff review, `code-review` skill, `cargo osdk check`,
> `cargo osdk build`, commit with the human maintainer identity).

---

## 0. Scope, baseline and non-goals

### 0.1 What exists today (verified against source, 2026-11-17)

| Layer | Implementation | State |
| --- | --- | --- |
| Kernel / ABI | Linux ABI, Void Linux glibc rootfs, xbps; ext4/exfat/procfs/sysfs/devfs/tmpfs; AF_INET/AF_UNIX/AF_NETLINK; modern syscalls (namespaces, seccomp, bpf, pidfd, rseq, membarrier, keyctl) | Broad |
| Drivers | VirtIO GPU + Intel DRM/KMS, evdev + VirtIO input, VirtIO audio, XHCI/USB HID, AHCI/NVMe/VirtIO block, e1000/VirtIO net | QEMU-focused |
| Compositor | `userspace/space-compositor` (~2k LoC): damage-tracked **software** compositing, zero-copy SHM surfaces, double-buffered dumb buffers, 4 workspaces, float/tile, taskbar, launcher, Alt-Tab switcher, decorations, snap-to-edge | Functional, protocol incomplete |
| UI toolkit | `userspace/space-ui` (~1.4k LoC widgets): Button, CheckBox, Menu/MenuBar, TableView, TextInput, ScrollBar, TabWidget, Splitter, ToolBar, StatusBar, BoxLayout, themes | Thin but real |
| Core / HAL | `ab_glyph` TTF rasterizer, SIMD blit, atomic SPSC rings, DRM/KMS backend, raw evdev backend | Solid |
| Apps (7) | `space-files` (dual-pane + copy/mkdir/delete), `space-tasks` (process list + SIGTERM/KILL/STOP/CONT), `space-term` (real PTY + bash/sh + ANSI parser), `space-calc`, `space-pad`, `space-control`, `space-apps` (xbps install/remove) | Useful |
| Style | "Petra Slate", ReactOS/XP-inspired (migrated from XP Luna in commit `74b623f`) | Coherent |

### 0.2 Non-goals (do not implement without explicit user approval)

- **Win32/NT binary compatibility** (PE loader, registry, USER32/GDI32). PetraOS is Linux-ABI;
  this is a separate multi-year project, not a prerequisite for production readiness.
- **Windows Registry clone.** Use key-value config files or a settings service instead.
- Changing the kernel ABI or replacing the Void Linux userland.

### 0.3 Priority legend

| Tag | Meaning |
| --- | --- |
| P0 | Correctness/functional gap; blocks "usable desktop" |
| P1 | Major UX/perf gap; blocks "daily driver" |
| P2 | Parity feature; blocks "production ready" |
| P3 | Nice-to-have / large strategic work |

---

## 1. P0 tasks — finish the window protocol and session basics

### T-01 — Emit the declared-but-unused window protocol messages

**Priority:** P0 · **Estimated size:** S

**Problem.** `MsgKind::SetTitleHint`, `ResizeEvent`, `FocusEvent` are declared in
`userspace/space-core/src/ipc.rs:22-43` and handled client-side in
`userspace/space-ui/src/client.rs:167-174`, but the compositor never sends them.

**Evidence.**
- `grep -rn "MsgKind::SetTitleHint\|MsgKind::ResizeEvent\|MsgKind::FocusEvent" userspace --include='*.rs'`
  returns only definitions and client handlers.
- Compositor sends only `CloseEvent`, `PointerEvent`, `KeyEvent`
  (`space-compositor/src/compositor.rs:302-311`, `:341-418`, `:630-641`).

**Impact.** Apps cannot react to maximize/tile resize; focus-sensitive widgets (`TableView`,
`TextInput`) cannot track cross-window focus; window titles cannot update at runtime.

**Proposed fix.**
1. In `SpaceClient`, add public methods `set_title(&str)`, and ensure resize/focus handling is
   wired to app callbacks (e.g. `UiEvent::Resize`, `UiEvent::Focus`).
2. In the compositor, emit `FocusEvent` whenever `SceneGraph::focus_window` changes the focused
   window (send `1` to the new focus, `0` to the previous one).
3. Emit `ResizeEvent(width, height)` after any damage-driven geometry change (maximize restore,
   tile/retile, snap, edge resize from T-02).
4. Handle `SetTitleHint` in `poll_client_rings` and update `SurfaceNode::title`; repaint the
   taskbar button and titlebar.
5. Add an integration test using `HeadlessBackend` (`space-hal/src/render.rs:446-484`) that
   asserts all four messages are delivered.

**Acceptance criteria.**
- Maximizing/tiling a window results in the client receiving `ResizeEvent` with the new size and
  repainting to that size (no cropped content).
- Switching focus delivers `Focus(true)` to the newly focused client and `Focus(false)` to the
  previously focused client.
- A client can change its title and the taskbar/titlebar reflect it.
- `cargo test` in `userspace/` passes with the new protocol tests.

**Likely files:** `space-core/src/ipc.rs`, `space-ui/src/client.rs`, `space-ui/src/traits.rs`,
`space-compositor/src/{compositor.rs,scene.rs,panel.rs}`.

---

### T-02 — Real window resize (surface resize + edge grips)

**Priority:** P0 · **Estimated size:** M · **Depends on:** T-01

**Problem.** Only the bottom-right 14×14 corner is resizable (`decorator.rs:83`). Maximized and
tiled windows **crop** the client surface rather than resizing it
(`compositor.rs:796-807`, `scene.rs:103-163`).

**Proposed fix.**
1. Extend `WindowDecorator::HitPart` with `ResizeLeft/Right/Top/Bottom` and add hit-test zones
   along all four edges (8 px). Keep corner priority over edges.
2. On `ResizeEvent` receipt, clients recreate/reallocate their `ShmSurface` (new size), update
   `HandshakePacket`-equivalent shared header, and repaint via their layout code.
3. Compositor: when a window's `node.rect.w/h` changes, send `ResizeEvent(new_w, new_h)` and do
   not blit stale pixels beyond the new surface bounds.
4. Ensure tiling (`SceneGraph::retile`) assigns sizes that fit the work area and sends resize
   events for every affected node.

**Acceptance criteria.**
- All four edges and all four corners resize a window with correct damage tracking.
- Tiling and maximizing reflow app content instead of cropping it.
- No flicker or stale pixels during resize.

**Likely files:** `space-compositor/src/{decorator.rs,compositor.rs,scene.rs}`,
`space-ui/src/client.rs`, all 7 `userspace/apps/*/src/main.rs` (layout reflow).

---

### T-03 — Wire up Shut down / Log off and graceful app exit

**Priority:** P0 · **Estimated size:** M

**Problem.** Launcher footer buttons "Log off" and "Shut down" are painted only
(`panel.rs:400-406`); `PanelAction` has no such variants. Compositor death leaves orphan clients.

**Proposed fix.**
1. Add `PanelAction::LogOff` / `ShutDown` and hit regions for both footer entries in
   `DesktopPanel::handle_click`.
2. On `ShutDown`: broadcast `CloseEvent` to all clients, close sockets/SHM, then call the
   kernel reboot/halt path already exposed via `syscall/platform/reboot.rs`
   (e.g. `Command::new("poweroff")` or direct syscall from userspace).
3. On `LogOff`: close all client windows, clear session state, respawn `space-control`
   (or a login shell when T-14 lands).
4. Add `Drop`-based shutdown of clients when the compositor socket closes: clients should treat
   a broken `s2c` ring / socket EOF as a close signal instead of spinning forever.

**Acceptance criteria.**
- Clicking "Shut down" terminates apps cleanly and powers off/reboots the machine in QEMU.
- Clicking "Log off" closes all windows and restores a clean desktop.
- Killing the compositor causes all clients to exit within ~1 s.

**Likely files:** `space-compositor/src/{panel.rs,compositor.rs,main.rs}`,
`space-ui/src/client.rs`, `scripts/qemu_args.sh` (test harness).

---

### T-04 — Persisted user settings instead of compiled-in constants

**Priority:** P0 · **Estimated size:** M

**Problem.** Desktop shortcuts (`compositor.rs:56-63`), pinned/builtin apps and system places
(`panel.rs:25-86`), wallpaper, theme, workspaces are hardcoded.

**Proposed fix.**
1. Define a settings file (e.g. `$HOME/.config/space/settings.toml` or a simple key=value file)
   with: wallpaper, theme, pinned apps, quick launch, desktop shortcuts, workspace count,
   layout mode, display mode, volume. Add serde-free parsing if dependencies are a concern.
2. Load at compositor startup with sane defaults matching today's look; save on change
   (display mode, layout toggle, volume, etc.).
3. Make `panel.rs` / `compositor.rs` read from the loaded config rather than constants.

**Acceptance criteria.**
- Changing resolution, layout mode, or pinned apps survives a compositor restart.
- Fresh user (missing config) gets the current default Petra Slate desktop.
- No compile-time constant still controls user-visible app lists.

**Likely files:** new `space-core/src/config.rs`, `space-core/src/lib.rs`,
`space-compositor/src/{main.rs,panel.rs,compositor.rs,wallpaper.rs}`,
`space-ui/src/theme.rs`.

---

### T-05 — Harden IPC: authentication, validation, cleanup

**Priority:** P0 · **Estimated size:** M

**Problem.** Any local process can connect and claim a window; client damage rects are
unvalidated (`compositor.rs:281-290`); SHM objects are unlinked only by their owner
(`ipc.rs:212-224`, `:363-375`) → orphaned `/dev/shm` segments after crashes; zombie reaping only
every 60 frames (`compositor.rs:167-173`).

**Proposed fix.**
1. Validate `CommitDamage` rects against the surface dimensions before adding to damage.
2. On client disconnect (`poll_client_rings`/socket EOF), unlink all three SHM names
   (`_surf`, `_c2s`, `_s2c`) and close fds; add a compositor-side cleanup helper in `space-core`.
3. Reap children every frame (or on `SIGCHLD`) instead of every 60 frames.
4. Basic authentication: require the connecting pid to be a known/trusted binary path, or at
   minimum record `_pid` and reject duplicate handshakes; document that full sandboxing needs
   a settings/security design (see T-16).

**Acceptance criteria.**
- Killing an app with SIGKILL leaves no `/dev/shm/space_*` files behind.
- A malformed oversized damage rect cannot paint outside the compositor back buffer.
- No zombie processes accumulate after repeated app launches/closes.

**Likely files:** `space-core/src/ipc.rs`, `space-compositor/src/compositor.rs`,
`space-ui/src/client.rs`.

---

## 2. P1 tasks — compositor and graphics

### T-06 — Enable vsync / page-flip events

**Priority:** P1 · **Estimated size:** S

**Problem.** `DRM_IOCTL_MODE_PAGE_FLIP` is issued with `flags = 0` and no flip-event handling
(`render.rs:340-347`) → tearing possible.

**Proposed fix.** Request `DRM_MODE_PAGE_FLIP_EVENT`, add a flip-event handler (read DRM events
from the fd in the main loop), and only commit the next front buffer after the flip completes.

**Acceptance criteria.** No visible tearing during cursor movement or window drag in QEMU;
present rate does not exceed the display refresh rate.

**Likely files:** `space-hal/src/render.rs`, `space-hal/src/drm_uapi.rs`,
`space-compositor/src/compositor.rs`.

---

### T-07 — GPU/GBM rendering path (remove software-only ceiling)

**Priority:** P1 · **Estimated size:** L

**Problem.** All compositing is CPU row blits into dumb buffers (`render.rs:302-338`).

**Proposed fix.** Add an alternative `RenderBackend` using GBM/EGL/OpenGL (or at minimum
hardware cursor planes + overlay planes). Keep the dumb-buffer backend as fallback. Preserve
the `RenderBackend` trait abstraction and damage semantics.

**Acceptance criteria.** At 1920×1080, CPU usage during idle/typing drops measurably;
`PERF.md` updated with before/after numbers using the existing TSC logging convention.

**Likely files:** `space-hal/src/render.rs`, `space-hal/src/drm_uapi.rs`, new
`space-hal/src/gbm_backend.rs`, `docs/PERF.md`.

---

### T-08 — Multi-monitor and connector hotplug

**Priority:** P1 · **Estimated size:** M

**Problem.** The backend picks the first connected connector and `break`s
(`render.rs:82-113`); one CRTC, no hotplug.

**Proposed fix.** Track multiple connector/CRTC pairs; give `SceneGraph` per-output work areas;
handle DRM hotplug uevents; move windows between outputs when a connector disappears.

**Acceptance criteria.** Two virtual displays in QEMU show independent desktop/workspaces;
unplugging one does not crash the compositor.

**Likely files:** `space-hal/src/render.rs`, `space-compositor/src/{scene.rs,panel.rs,compositor.rs}`.

---

### T-09 — Clipboard and drag & drop protocol

**Priority:** P1 · **Estimated size:** M

**Problem.** No clipboard, primary selection, or DnD messages exist in `MsgKind`
(`ipc.rs:22-43`).

**Proposed fix.**
1. Add `ClipboardSet{owner,mime}` / `ClipboardGet` / `ClipboardData` and `DragStart` /
   `DragEnter` / `DragDrop` messages; a compositor-owned clipboard buffer in `/dev/shm`.
2. Expose `space_ui::Clipboard` API; wire `TextInput` copy/paste shortcuts (Ctrl+C/V/X).
3. Add file-uri drag from `space-files` to `space-pad`/`space-term`.

**Acceptance criteria.** Copy text in `space-pad`, paste in `space-term`; drag a file from
`space-files` into a terminal inserts its path.

**Likely files:** `space-core/src/ipc.rs`, new `space-core/src/clipboard.rs`,
`space-compositor/src/compositor.rs`, `space-ui/src/{client.rs,widgets/text_input.rs}`,
apps.

---

### T-10 — Keymaps, key repeat, IME-ready input

**Priority:** P1 · **Estimated size:** M

**Problem.** Hardcoded Linux keycode→char mapping in `input.rs` / `compositor.rs:356-418`;
no layouts, repeat, dead keys, or IME.

**Proposed fix.** Add a keymap table (US QWERTY default, at least one alternative), modifier-aware
translation, key-repeat timers, and pass through raw keycodes so clients can implement IME.

**Acceptance criteria.** Switching layout changes typed characters; holding a key repeats;
non-Latin input is possible at the protocol level (UI rendering depends on T-12).

**Likely files:** `space-hal/src/input.rs`, `space-compositor/src/compositor.rs`,
new `space-core/src/keymap.rs`.

---

### T-11 — App discovery: `.desktop` files, MIME associations, Run dialog

**Priority:** P1 · **Estimated size:** M

**Problem.** `spawn_app(bin)` uses raw binary names (`compositor.rs:145-147`); "Run Command..."
in the launcher opens a terminal instead of a Run dialog.

**Proposed fix.** Parse `.desktop` entries from `/usr/share/applications`, add app search/index,
file-extension → app mapping, "Open with", and a real Run dialog app (`space-run`).

**Acceptance criteria.** Launching by display name works; opening a `.txt` from `space-files`
opens `space-pad`; Run dialog accepts a command and launches it.

**Likely files:** `space-core/src/{void_pkg.rs,new:desktop.rs}`, `space-compositor/src/panel.rs`,
new `userspace/apps/space-run/`, `userspace/Cargo.toml`.

---

### T-12 — Text shaping, font fallback, accessibility basics

**Priority:** P1 · **Estimated size:** M

**Problem.** `font.rs` uses `ab_glyph` with only Ubuntu fonts embedded (`assets/fonts`): no
shaping, no fallback, no bidi. No high-contrast/large-text mode.

**Proposed fix.** Add fallback fonts (at least a CJK-capable one), basic HarfBuzz-like shaping or
document the limitation, bidi ICU-style algorithm if time permits; add theme presets for high
contrast and large text in `space-ui/src/theme.rs` with a settings toggle (T-04).

**Acceptance criteria.** Common non-Latin strings render with no "missing glyph" boxes;
high-contrast mode is selectable in Control Panel and persists.

**Likely files:** `space-core/src/font.rs`, `assets/fonts/`, `space-ui/src/theme.rs`,
`userspace/apps/space-control/src/main.rs`.

---

## 3. P2 tasks — Windows XP / ReactOS desktop parity

### T-13 — Taskbar clock, system tray icons and notifications

**Priority:** P2 · **Estimated size:** S

**Problem.** Tray shows only RAM usage (`panel.rs:259-261`).

**Proposed fix.** Add clock (RTC/time APIs already in kernel), volume/network/battery icons, and
a toast notification area with a compositor-side notification protocol.

**Acceptance criteria.** Time updates every minute; volume icon reflects `space-control`
changes; notifications appear and auto-dismiss.

**Likely files:** `space-compositor/src/panel.rs`, `space-core/src/ipc.rs`,
`space-ui/src/widgets/*`.

---

### T-14 — User accounts, login and lock screen

**Priority:** P2 · **Estimated size:** L

**Problem.** Single-user root; "Log off" is inert (see T-03).

**Proposed fix.** Add an auth service (shadow-style file), a display-manager/login shell drawn
with `space-ui`, per-user config directories, and session ownership of compositor/app processes.

**Acceptance criteria.** Two users can log in with separate wallpapers/settings; lock screen
requires password to restore session.

**Likely files:** `base-files/`, `userspace/apps/space-login/`, `space-compositor/src/main.rs`.

---

### T-15 — Device Manager, Event Viewer, Disk Management UIs

**Priority:** P2 · **Estimated size:** M each

**Problem.** sysfs and block devices exist but no management UI; no structured journal.

**Proposed fix.** Three new apps using `space-ui` widgets: `space-devices` (sysfs tree,
driver info), `space-events` (journal viewer with filters), `space-disks` (mount/unmount/format
via `mount`/`mkfs.ext4`/`exfatprogs`).

**Acceptance criteria.** Device tree reflects hotplug; events from kernel log show in viewer;
a loopback ext4 image can be mounted/unmounted from the GUI.

**Likely files:** new `userspace/apps/space-{devices,events,disks}/`, `userspace/Cargo.toml`,
possibly `petra/src/fs/sysfs/`.

---

### T-16 — Security: GUI sandboxing and permissions

**Priority:** P2 · **Estimated size:** M

**Problem.** GUI apps run with full user privileges; no isolation; compositor accepts any client.

**Proposed fix.** Use existing `seccomp`/`cap` syscalls to sandbox GUI apps; restrict compositor
socket to a session directory with 0600 perms; drop capabilities in apps that don't need them;
document threat model.

**Acceptance criteria.** A sandboxed app cannot open arbitrary `/dev/*` or spawn processes
unless allowed; unauthorized clients cannot register windows.

**Likely files:** `userspace/space-ui/src/client.rs`, new `userspace/space-core/src/sandbox.rs`,
`petra/src/syscall/proc/seccomp.rs`.

---

### T-17 — Update/rollback and System Restore equivalent

**Priority:** P2 · **Estimated size:** M

**Problem.** `space-apps` can install/remove via xbps but has no upgrade-all or rollback.

**Proposed fix.** Add `PackageManager::upgrade_all()` and config/package snapshotting; add
"Check for updates" and "Restore point" UI in Control Panel.

**Acceptance criteria.** Upgrading a package from the GUI succeeds; a restore point can roll back
a config change.

**Likely files:** `space-core/src/void_pkg.rs`, `userspace/apps/space-apps/src/main.rs`,
`space-control/src/main.rs`.

---

### T-18 — Networking GUI and Wi-Fi

**Priority:** P2 · **Estimated size:** L

**Problem.** Kernel has TCP/IP + netlink route sockets but no configuration UI and no Wi-Fi
driver.

**Proposed fix.** `space-network` app for interface list, IP config, DNS, and connection status;
assess adding a Wi-Fi driver (large; may be deferred).

**Acceptance criteria.** Interface up/down, static/DHCP config, and DNS editing work from GUI.

**Likely files:** new `userspace/apps/space-network/`, `petra/src/net/`.

---

### T-19 — Accessibility and localization rollout

**Priority:** P2 · **Estimated size:** M · **Depends on:** T-10, T-12

**Problem.** No magnifier, high contrast, sticky/filter keys, or language packs.

**Proposed fix.** Implement high contrast and text scaling (theme presets), sticky/filter/bounce
keys in input path, screen magnifier as a compositor overlay, and locale selection hooked to T-10.

**Acceptance criteria.** All four features togglable in Control Panel and effective across apps.

**Likely files:** `space-ui/src/theme.rs`, `space-hal/src/input.rs`,
`space-compositor/src/{compositor.rs,panel.rs}`, `space-control/src/main.rs`.

---

## 4. P3 / strategic

- **T-20** — System Restore snapshots (needs a snapshot mechanism on ext4).
- **T-21** — Print/spooler stack (CUPS-like; very large).
- **T-22** — Richer apps: browser (lightweight HTML renderer), mail, office, image viewer,
  media player, editor with tabs/undo/find-replace, screenshot tool.
- **T-23** — Real-hardware bring-up: UEFI boot on physical x86_64, AMD/NVIDIA DRM drivers,
  audio codecs, touchpad drivers; remove "QEMU only" from `README.md` when validated.
- **T-24** — Optional Win32 compatibility layer (only with explicit user approval; see §0.2).

---

## 5. Feature parity matrix (XP / ReactOS → PetraOS)

| Feature | PetraOS today | Target | Task |
| --- | --- | --- | --- |
| Start menu + Run + search | Launcher; "Run" opens terminal | Real Run dialog, app index, recent items | T-11 |
| Taskbar clock, tray icons, notifications | RAM readout only | Clock, volume/network icons, toasts | T-13 |
| Shut down / Log off | Decorative | Working shutdown/logoff | T-03 |
| Explorer: address bar, context menu, DnD, associations | Dual-pane files | Shell model + DnD + Open With | T-09, T-11 |
| Clipboard | Missing | Compositor clipboard + viewer | T-09 |
| User accounts / login / lock | Single-user root | Login shell + per-user config | T-14 |
| Device Manager / PnP UI | sysfs only | Device tree UI | T-15 |
| Event Viewer | ad-hoc prints | Journal + viewer | T-15 |
| Disk Management | Kernel FS support only | GUI mount/format | T-15 |
| System Properties | Control Panel partial | Persisted settings, themes, effects | T-04, T-12 |
| Windows Update | xbps install/remove | upgrade-all + rollback | T-17 |
| System Restore | Missing | Restore points | T-20 |
| Networking GUI | Kernel stack only | Network config UI | T-18 |
| Accessibility | Missing | High contrast, keys, magnifier | T-19 |
| IME / language packs | Missing | Keymap + font fallback + locale | T-10, T-12 |
| Task Manager | Process list + signals | CPU %, I/O, tree, startup apps | T-19 (extend) |
| Notepad / Calculator | Basic | Tabs/undo/find, scientific mode | T-22 |
| Registry / PE loader / Win32 | N/A | **Non-goal** | §0.2 |

---

## 6. Mandatory workflow for every task (from `AGENTS.md`)

1. Keep changes in the correct subsystem directory; register new files in the local `mod.rs`.
2. Prefer explicit traits/minimal APIs; no circular dependencies; no dead code.
3. Review `git diff`; run the `code-review` skill
   (`.agents/skills/code-review/SKILL.md` / `code-review` skill) for systematic audit.
4. Verify build:
   ```bash
   export PATH="$HOME/.cargo/bin:$PATH"
   cargo osdk check
   cargo osdk build
   ```
   Also run `cargo test` in `userspace/` for GUI changes.
5. Commit using the **human maintainer** from `git config user.name` / `user.email`.
   Never use an AI identity in commits, metadata, or `@author` headers.

---

## 7. Recommended execution order

```
T-01 → T-02 → T-03 → T-05        (protocol + session + safety; smallest high-value batch)
T-04 → T-11 → T-09 → T-13        (persistence + app discovery + clipboard + tray)
T-06 → T-08 → T-10 → T-12 → T-19 (graphics/input/i18n/accessibility)
T-07 → T-14 → T-15 → T-16 → T-17 → T-18
T-20 … T-24                       (strategic / optional)
```

Each task should be committed separately with its own `cargo osdk check` / `cargo osdk build`
verification, so a regression is attributable to one change.
