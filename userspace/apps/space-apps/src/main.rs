//! PetraOS Applications Manager (ReactOS RAPPS-inspired software catalog & manager
//! built with SerenityOS-style MenuBar, ToolBar, TableView, and StatusBar).

use space_core::font;
use space_core::icons::{self, IconId};
use space_core::void_pkg::{PackageInfo, PackageManager};
use space_core::{Canvas, Color, Rect};
use space_ui::{
    AppLogic, BoxLayout, Button, EventListener, Insets, Layoutable, Lifecycle, Menu, MenuBar,
    MenuItem, Paintable, SizePolicy, SpaceClient, StatusBar, StylePainter, TableColumn, TableView,
    TextInput, Theme, ToolBar, ToolButton, UiEvent,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppCategory {
    All,
    Installed,
    Available,
    Utilities,
    Development,
    System,
}

impl AppCategory {
    const ALL: [Self; 6] = [
        Self::All,
        Self::Installed,
        Self::Available,
        Self::Utilities,
        Self::Development,
        Self::System,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All Applications",
            Self::Installed => "Installed",
            Self::Available => "Available",
            Self::Utilities => "Utilities",
            Self::Development => "Development",
            Self::System => "System & Libraries",
        }
    }

    fn matches(self, pkg: &PackageInfo) -> bool {
        let lower_name = pkg.name.to_ascii_lowercase();
        let lower_desc = pkg.description.to_ascii_lowercase();
        match self {
            Self::All => true,
            Self::Installed => pkg.installed,
            Self::Available => !pkg.installed,
            Self::Utilities => {
                lower_desc.contains("utility")
                    || lower_desc.contains("tool")
                    || lower_desc.contains("editor")
                    || lower_desc.contains("archive")
                    || lower_name.contains("nano")
                    || lower_name.contains("vim")
                    || lower_name.contains("curl")
            }
            Self::Development => {
                lower_desc.contains("compiler")
                    || lower_desc.contains("develop")
                    || lower_desc.contains("debug")
                    || lower_name.contains("gcc")
                    || lower_name.contains("git")
                    || lower_name.contains("python")
                    || lower_name.contains("rust")
            }
            Self::System => {
                lower_desc.contains("library")
                    || lower_desc.contains("kernel")
                    || lower_desc.contains("system")
                    || lower_desc.contains("init")
                    || lower_name.starts_with("lib")
            }
        }
    }
}

struct AppsManager {
    width: u32,
    height: u32,
    menubar: MenuBar,
    toolbar: ToolBar,
    search: TextInput,
    category: AppCategory,
    cat_buttons: Vec<Button>,
    table: TableView,
    statusbar: StatusBar,
    sidebar_rect: Rect,
    details_rect: Rect,
    catalog: Vec<PackageInfo>,
    filtered_indices: Vec<usize>,
}

impl AppsManager {
    fn new(width: u32, height: u32) -> Self {
        let menubar = MenuBar::new(
            Rect::new(0, 0, 0, 0),
            vec![
                Menu::new(
                    "File",
                    vec![
                        MenuItem::action("refresh", "Refresh Catalog", "F5"),
                        MenuItem::separator(),
                        MenuItem::action("exit", "Exit", "Alt+F4"),
                    ],
                ),
                Menu::new(
                    "Programs",
                    vec![
                        MenuItem::action("install", "Install Selected", ""),
                        MenuItem::action("remove", "Uninstall Selected", ""),
                    ],
                ),
                Menu::new(
                    "View",
                    vec![
                        MenuItem::action("cat_all", "All Applications", ""),
                        MenuItem::action("cat_installed", "Installed Only", ""),
                        MenuItem::action("cat_available", "Available Only", ""),
                    ],
                ),
            ],
        );
        let toolbar = ToolBar::new(
            Rect::new(0, 0, 0, 0),
            vec![
                ToolButton::new("install", "Install"),
                ToolButton::new("remove", "Uninstall"),
                ToolButton::new("refresh", "Refresh"),
            ],
        );
        let search = TextInput::new(Rect::new(0, 0, 0, 0), "Filter applications...");
        let cat_buttons = AppCategory::ALL
            .iter()
            .map(|c| Button::new(Rect::new(0, 0, 0, 0), c.label()))
            .collect();
        let table = TableView::new(
            Rect::new(0, 0, 0, 0),
            vec![
                TableColumn::new("Application", 160),
                TableColumn::new("Version", 104),
                TableColumn::new("Status", 92),
                TableColumn::new("Description", 200),
            ],
        );
        let statusbar = StatusBar::new(Rect::new(0, 0, 0, 0));

        let mut app = Self {
            width,
            height,
            menubar,
            toolbar,
            search,
            category: AppCategory::All,
            cat_buttons,
            table,
            statusbar,
            sidebar_rect: Rect::new(0, 0, 0, 0),
            details_rect: Rect::new(0, 0, 0, 0),
            catalog: Vec::new(),
            filtered_indices: Vec::new(),
        };
        app.relayout();
        app.reload_catalog();
        app
    }

    fn relayout(&mut self) {
        let root = Rect::new(0, 0, self.width as i32, self.height as i32);
        let outer = BoxLayout::vbox(0, Insets::ZERO);
        let rows = outer.compute(
            root,
            &[
                SizePolicy::Fixed(24), // MenuBar
                SizePolicy::Fixed(34), // ToolBar
                SizePolicy::Expand,    // Main split body
                SizePolicy::Fixed(24), // StatusBar
            ],
        );
        self.menubar.set_bounds(rows[0]);
        self.toolbar.set_bounds(rows[1]);
        self.statusbar.set_bounds(rows[3]);

        let body_slots = BoxLayout::hbox(8, Insets::all(8)).compute(
            rows[2],
            &[SizePolicy::Fixed(168), SizePolicy::Expand],
        );
        self.sidebar_rect = body_slots[0];
        let mut cy = self.sidebar_rect.y + 32;
        for btn in &mut self.cat_buttons {
            btn.set_bounds(Rect::new(self.sidebar_rect.x + 8, cy, self.sidebar_rect.w - 16, 28));
            cy += 34;
        }

        let right_slots = BoxLayout::vbox(6, Insets::ZERO).compute(
            body_slots[1],
            &[
                SizePolicy::Fixed(28), // Search box
                SizePolicy::Expand,    // TableView
                SizePolicy::Fixed(58), // Selected app details
            ],
        );
        self.search.set_bounds(right_slots[0]);
        self.table.set_bounds(right_slots[1]);
        self.details_rect = right_slots[2];
    }

    fn reload_catalog(&mut self) {
        self.catalog = PackageManager::list_catalog();
        self.apply_filter();
    }

    fn apply_filter(&mut self) {
        let q = self.search.text.trim().to_ascii_lowercase();
        self.filtered_indices.clear();
        let mut table_rows = Vec::new();
        for (idx, pkg) in self.catalog.iter().enumerate() {
            if !self.category.matches(pkg) {
                continue;
            }
            if !q.is_empty()
                && !pkg.name.to_ascii_lowercase().contains(&q)
                && !pkg.description.to_ascii_lowercase().contains(&q)
            {
                continue;
            }
            self.filtered_indices.push(idx);
            table_rows.push(vec![
                pkg.name.clone(),
                pkg.version.clone(),
                if pkg.installed { "Installed" } else { "Available" }.to_string(),
                pkg.description.clone(),
            ]);
        }
        self.table.set_rows(table_rows);
        self.statusbar.set_text(
            0,
            format!(
                "{} applications ({} total)",
                self.filtered_indices.len(),
                self.catalog.len()
            ),
        );
    }

    fn selected_package(&self) -> Option<&PackageInfo> {
        let row = self.table.selected?;
        let cat_idx = *self.filtered_indices.get(row)?;
        self.catalog.get(cat_idx)
    }

    fn install_selected(&mut self) {
        let Some(pkg) = self.selected_package().cloned() else {
            self.statusbar.set_text(1, "Select an application first");
            return;
        };
        match PackageManager::install(&pkg.name) {
            Ok(_) => {
                self.statusbar.set_text(1, format!("Installed {}", pkg.name));
                self.reload_catalog();
            }
            Err(e) => {
                self.statusbar.set_text(1, format!("Install failed: {}", e.lines().next().unwrap_or("")));
            }
        }
    }

    fn remove_selected(&mut self) {
        let Some(pkg) = self.selected_package().cloned() else {
            self.statusbar.set_text(1, "Select an application first");
            return;
        };
        match PackageManager::remove(&pkg.name) {
            Ok(_) => {
                self.statusbar.set_text(1, format!("Uninstalled {}", pkg.name));
                self.reload_catalog();
            }
            Err(e) => {
                self.statusbar.set_text(1, format!("Remove failed: {}", e.lines().next().unwrap_or("")));
            }
        }
    }

    fn handle_action(&mut self, action: &str) -> bool {
        match action {
            "refresh" => {
                self.reload_catalog();
                self.statusbar.set_text(1, "Catalog refreshed");
                true
            }
            "install" => {
                self.install_selected();
                true
            }
            "remove" => {
                self.remove_selected();
                true
            }
            "cat_all" => {
                self.category = AppCategory::All;
                self.apply_filter();
                true
            }
            "cat_installed" => {
                self.category = AppCategory::Installed;
                self.apply_filter();
                true
            }
            "cat_available" => {
                self.category = AppCategory::Available;
                self.apply_filter();
                true
            }
            _ => false,
        }
    }
}

impl Lifecycle for AppsManager {}

impl Paintable for AppsManager {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::PETRA_SLATE;
        let painter = StylePainter::new(&t);

        self.menubar.paint(canvas);
        self.toolbar.paint(canvas);

        // Left Category Sidebar
        canvas.fill_rect(self.sidebar_rect, t.bg_surface);
        canvas.draw_rect_outline(self.sidebar_rect, 1, t.border);
        let side_hdr = Rect::new(self.sidebar_rect.x + 1, self.sidebar_rect.y + 1, self.sidebar_rect.w - 2, 24);
        canvas.fill_gradient_v(side_hdr, Theme::TITLE_ACTIVE_TOP, Theme::TITLE_ACTIVE_BOT);
        font::draw_text_bold(canvas, side_hdr.x + 8, side_hdr.y + 4, "Categories", Color::WHITE, 2);

        for (i, btn) in self.cat_buttons.iter().enumerate() {
            let mut b = btn.clone();
            b.active = self.category == AppCategory::ALL[i];
            b.paint_with(canvas, &t);
        }

        // Right Search + Catalog Table + Details Card
        self.search.paint_with(canvas, &t);
        self.table.paint_with(canvas, &t);

        painter.paint_sunken_field(canvas, self.details_rect, false);
        icons::draw_icon(
            canvas,
            self.details_rect.x + 10,
            self.details_rect.y + 12,
            32,
            IconId::AppManager,
        );
        if let Some(pkg) = self.selected_package() {
            let hdr = format!("{}  ({})", pkg.name, pkg.version);
            font::draw_text_bold(
                canvas,
                self.details_rect.x + 52,
                self.details_rect.y + 8,
                &hdr,
                t.accent,
                2,
            );
            let desc = if pkg.description.is_empty() {
                "PetraOS userland package"
            } else {
                &pkg.description
            };
            font::draw_text_clipped(
                canvas,
                Rect::new(
                    self.details_rect.x + 52,
                    self.details_rect.y + 28,
                    (self.details_rect.w - 64).max(40),
                    24,
                ),
                self.details_rect.x + 52,
                self.details_rect.y + 30,
                desc,
                t.text_primary,
                2,
            );
        } else {
            font::draw_text_bold(
                canvas,
                self.details_rect.x + 52,
                self.details_rect.y + 10,
                "Applications Manager",
                t.text_primary,
                2,
            );
            font::draw_text(
                canvas,
                self.details_rect.x + 52,
                self.details_rect.y + 30,
                "Select an application to view details, install, or uninstall.",
                t.text_secondary,
                2,
            );
        }

        self.statusbar.paint_with(canvas, &t);
        self.menubar.paint_popup(canvas);
    }
}

impl AppLogic for AppsManager {
    fn handle_event(&mut self, event: UiEvent) -> bool {
        if let UiEvent::Resize { width, height } = event {
            self.width = width;
            self.height = height;
            self.relayout();
            return true;
        }

        let menu_was_open = self.menubar.is_open();
        let mut dirty = self.menubar.on_event(event);
        if let Some(act) = self.menubar.take_action() {
            if act == "exit" {
                std::process::exit(0);
            }
            return self.handle_action(act) || dirty;
        }
        if menu_was_open {
            return dirty;
        }

        dirty |= self.toolbar.on_event(event);
        if let Some(act) = self.toolbar.take_action() {
            return self.handle_action(act) || dirty;
        }

        for (i, btn) in self.cat_buttons.iter_mut().enumerate() {
            dirty |= btn.on_event(event);
            if btn.take_click() {
                self.category = AppCategory::ALL[i];
                self.apply_filter();
                return true;
            }
        }

        let prev_search = self.search.text.clone();
        if self.search.on_event(event) {
            dirty = true;
            if self.search.text != prev_search {
                self.apply_filter();
            }
        }

        dirty |= self.table.on_event(event);
        if self.table.take_activated().is_some() {
            if let Some(pkg) = self.selected_package() {
                self.statusbar.set_text(1, format!("Selected {}", pkg.name));
            }
            return true;
        }

        dirty
    }
}

fn main() {
    let mut client =
        SpaceClient::connect("Applications Manager", 760, 500).expect("connect space-apps");
    let mut app = AppsManager::new(760, 500);
    client.run(&mut app);
}
