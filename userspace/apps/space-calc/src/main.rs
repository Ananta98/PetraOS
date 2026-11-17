use space_core::font;
use space_core::{Canvas, Color, Rect};
use space_ui::{
    AppLogic, Button, EventListener, Lifecycle, Paintable, SpaceClient, StylePainter, Theme,
    UiEvent,
};

struct Parser<'a> {
    chars: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn eval(expr: &str) -> Result<f64, &'static str> {
        let cleaned: String = expr
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| match c {
                '×' => '*',
                '÷' => '/',
                '−' => '-',
                _ => c,
            })
            .collect();
        if cleaned.is_empty() {
            return Ok(0.0);
        }
        let mut p = Parser {
            chars: cleaned.as_bytes(),
            pos: 0,
        };
        let v = p.parse_expr()?;
        if p.pos < p.chars.len() {
            return Err("Syntax error");
        }
        if !v.is_finite() {
            return Err("Overflow");
        }
        Ok(v)
    }

    fn peek(&self) -> Option<u8> {
        self.chars.get(self.pos).copied()
    }

    fn parse_expr(&mut self) -> Result<f64, &'static str> {
        let mut v = self.parse_term()?;
        while let Some(op) = self.peek() {
            if op == b'+' || op == b'-' {
                self.pos += 1;
                let rhs = self.parse_term()?;
                v = if op == b'+' { v + rhs } else { v - rhs };
            } else {
                break;
            }
        }
        Ok(v)
    }

    fn starts_implicit_mul(&self) -> bool {
        matches!(self.peek(), Some(b'(') | Some(b'a'..=b'z') | Some(b'A'..=b'Z'))
    }

    fn parse_term(&mut self) -> Result<f64, &'static str> {
        let mut v = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.pos += 1;
                    let rhs = self.parse_unary()?;
                    v *= rhs;
                }
                Some(b'/') => {
                    self.pos += 1;
                    let rhs = self.parse_unary()?;
                    if rhs == 0.0 {
                        return Err("Cannot divide by zero");
                    }
                    v /= rhs;
                }
                _ if self.starts_implicit_mul() => {
                    let rhs = self.parse_unary()?;
                    v *= rhs;
                }
                _ => break,
            }
        }
        Ok(v)
    }

    fn parse_unary(&mut self) -> Result<f64, &'static str> {
        match self.peek() {
            Some(b'-') => {
                self.pos += 1;
                Ok(-self.parse_unary()?)
            }
            Some(b'+') => {
                self.pos += 1;
                self.parse_unary()
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<f64, &'static str> {
        let base = self.parse_postfix()?;
        if self.peek() == Some(b'^') {
            self.pos += 1;
            let exp = self.parse_unary()?;
            let res = base.powf(exp);
            if !res.is_finite() {
                return Err("Invalid input");
            }
            Ok(res)
        } else {
            Ok(base)
        }
    }

    fn parse_postfix(&mut self) -> Result<f64, &'static str> {
        let mut v = self.parse_primary()?;
        while self.peek() == Some(b'%') {
            self.pos += 1;
            v /= 100.0;
        }
        Ok(v)
    }

    fn parse_primary(&mut self) -> Result<f64, &'static str> {
        if self.peek() == Some(b'(') {
            self.pos += 1;
            let v = self.parse_expr()?;
            if self.peek() != Some(b')') {
                return Err("Missing )");
            }
            self.pos += 1;
            return Ok(v);
        }

        let start = self.pos;
        if self.peek().map(|c| c.is_ascii_alphabetic()).unwrap_or(false) {
            while self.peek().map(|c| c.is_ascii_alphabetic()).unwrap_or(false) {
                self.pos += 1;
            }
            let ident = std::str::from_utf8(&self.chars[start..self.pos])
                .unwrap_or("")
                .to_ascii_lowercase();
            if ident == "pi" {
                return Ok(std::f64::consts::PI);
            }
            if ident == "e" {
                return Ok(std::f64::consts::E);
            }
            if self.peek() != Some(b'(') {
                return Err("Expected (");
            }
            self.pos += 1;
            let arg = self.parse_expr()?;
            if self.peek() != Some(b')') {
                return Err("Missing )");
            }
            self.pos += 1;
            return match ident.as_str() {
                "sin" => Ok(arg.sin()),
                "cos" => Ok(arg.cos()),
                "tan" => Ok(arg.tan()),
                "sqrt" => {
                    if arg < 0.0 {
                        Err("Invalid input")
                    } else {
                        Ok(arg.sqrt())
                    }
                }
                "sqr" => Ok(arg * arg),
                "recip" => {
                    if arg == 0.0 {
                        Err("Cannot divide by zero")
                    } else {
                        Ok(1.0 / arg)
                    }
                }
                "ln" => {
                    if arg <= 0.0 {
                        Err("Invalid input")
                    } else {
                        Ok(arg.ln())
                    }
                }
                "log" => {
                    if arg <= 0.0 {
                        Err("Invalid input")
                    } else {
                        Ok(arg.log10())
                    }
                }
                _ => Err("Unknown fn"),
            };
        }

        while self
            .peek()
            .map(|c| c.is_ascii_digit() || c == b'.')
            .unwrap_or(false)
        {
            self.pos += 1;
        }
        if self.pos == start {
            return Err("Expected number");
        }
        let num_str = std::str::from_utf8(&self.chars[start..self.pos]).unwrap_or("");
        num_str.parse::<f64>().map_err(|_| "Bad number")
    }
}

fn format_number(v: f64) -> String {
    if !v.is_finite() {
        return String::from("Overflow");
    }
    let clean = if v.abs() < 1e-12 { 0.0 } else { v };
    if clean != 0.0 && (clean.abs() >= 1e12 || clean.abs() < 1e-9) {
        return format!("{:.6e}", clean);
    }
    let s = format!("{:.10}", clean);
    if let Some((int_part, frac_part)) = s.split_once('.') {
        let trimmed_frac = frac_part.trim_end_matches('0');
        if trimmed_frac.is_empty() {
            int_part.to_string()
        } else {
            format!("{}.{}", int_part, trimmed_frac)
        }
    } else {
        s
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CalcMode {
    Standard,
    Scientific,
}

struct CalcApp {
    width: u32,
    height: u32,
    mode: CalcMode,
    history_open: bool,
    menu_open: bool,
    expr: String,
    current_input: String,
    just_evaluated: bool,
    error: Option<String>,
    memory: Option<f64>,
    history: Vec<(String, String)>,
    mem_buttons: Vec<(Button, &'static str)>,
    buttons: Vec<(Button, &'static str, bool)>,
    btn_mode_toggle: Button,
    btn_hist_toggle: Button,
}

impl CalcApp {
    fn new(width: u32, height: u32) -> Self {
        let mut app = Self {
            width,
            height,
            mode: CalcMode::Standard,
            history_open: false,
            menu_open: false,
            expr: String::new(),
            current_input: String::from("0"),
            just_evaluated: false,
            error: None,
            memory: None,
            history: Vec::new(),
            mem_buttons: Vec::new(),
            buttons: Vec::new(),
            btn_mode_toggle: Button::new(Rect::new(8, 6, 30, 28), "").with_icon("view-list"),
            btn_hist_toggle: Button::new(Rect::new(width as i32 - 38, 6, 30, 28), "").with_icon("clock"),
        };
        app.rebuild_layout();
        app
    }

    fn rebuild_layout(&mut self) {
        let w = self.width as i32;
        let h = self.height as i32;
        self.btn_mode_toggle = Button::new(Rect::new(8, 6, 30, 28), "").with_icon("view-list");
        self.btn_hist_toggle = Button::new(Rect::new(w - 38, 6, 30, 28), "").with_icon("clock");

        let mem_y = 132;
        let mem_labels = ["MC", "MR", "M+", "M-", "MS", "M"];
        let mem_w = ((w - 16) / 6).max(36);
        self.mem_buttons.clear();
        for (i, &lbl) in mem_labels.iter().enumerate() {
            let mut b = Button::new(Rect::new(8 + i as i32 * mem_w, mem_y, mem_w - 2, 26), lbl);
            b.disabled = (lbl == "MC" || lbl == "MR") && self.memory.is_none();
            self.mem_buttons.push((b, lbl));
        }

        let grid_top = 164;
        let grid_h = (h - grid_top - 8).max(160);
        self.buttons.clear();

        if self.mode == CalcMode::Standard {
            let rows: [[(&str, bool); 4]; 6] = [
                [("%", false), ("CE", false), ("C", false), ("<-", false)],
                [("1/x", false), ("x^2", false), ("sqrt", false), ("/", false)],
                [("7", true), ("8", true), ("9", true), ("*", false)],
                [("4", true), ("5", true), ("6", true), ("-", false)],
                [("1", true), ("2", true), ("3", true), ("+", false)],
                [("+/-", true), ("0", true), (".", true), ("=", false)],
            ];
            let gap = 4;
            let cols = 4;
            let rcount = 6;
            let cell_w = ((w - 16 - gap * (cols - 1)) / cols).max(40);
            let cell_h = ((grid_h - gap * (rcount - 1)) / rcount).max(28);
            for (r, row) in rows.iter().enumerate() {
                for (c, &(lbl, is_digit)) in row.iter().enumerate() {
                    let x = 8 + c as i32 * (cell_w + gap);
                    let y = grid_top + r as i32 * (cell_h + gap);
                    let bw = if c as i32 == cols - 1 { w - 8 - x } else { cell_w };
                    let mut btn = Button::new(Rect::new(x, y, bw, cell_h), lbl);
                    if lbl == "=" {
                        btn.accent = true;
                    }
                    self.buttons.push((btn, lbl, is_digit));
                }
            }
        } else {
            let rows: [[(&str, bool); 5]; 7] = [
                [("sin", false), ("cos", false), ("tan", false), ("(", false), (")", false)],
                [("ln", false), ("log", false), ("pi", false), ("e", false), ("^", false)],
                [("%", false), ("CE", false), ("C", false), ("<-", false), ("/", false)],
                [("1/x", false), ("7", true), ("8", true), ("9", true), ("*", false)],
                [("x^2", false), ("4", true), ("5", true), ("6", true), ("-", false)],
                [("sqrt", false), ("1", true), ("2", true), ("3", true), ("+", false)],
                [("+/-", true), ("0", true), (".", true), ("^2", false), ("=", false)],
            ];
            let gap = 4;
            let cols = 5;
            let rcount = 7;
            let cell_w = ((w - 16 - gap * (cols - 1)) / cols).max(36);
            let cell_h = ((grid_h - gap * (rcount - 1)) / rcount).max(26);
            for (r, row) in rows.iter().enumerate() {
                for (c, &(lbl, is_digit)) in row.iter().enumerate() {
                    let x = 8 + c as i32 * (cell_w + gap);
                    let y = grid_top + r as i32 * (cell_h + gap);
                    let bw = if c as i32 == cols - 1 { w - 8 - x } else { cell_w };
                    let mut btn = Button::new(Rect::new(x, y, bw, cell_h), lbl);
                    if lbl == "=" {
                        btn.accent = true;
                    }
                    self.buttons.push((btn, lbl, is_digit));
                }
            }
        }
    }

    fn current_val(&self) -> Result<f64, &'static str> {
        if self.current_input.is_empty() {
            Ok(0.0)
        } else {
            Parser::eval(&self.current_input)
        }
    }

    fn apply_token(&mut self, tok: &str) {
        if self.error.is_some() && tok != "C" && tok != "CE" {
            self.expr.clear();
            self.current_input = String::from("0");
            self.error = None;
            self.just_evaluated = false;
        }

        match tok {
            "C" => {
                self.expr.clear();
                self.current_input = String::from("0");
                self.just_evaluated = false;
                self.error = None;
            }
            "CE" => {
                if self.just_evaluated {
                    self.expr.clear();
                    self.just_evaluated = false;
                }
                self.current_input = String::from("0");
                self.error = None;
            }
            "<-" => {
                if self.just_evaluated {
                    self.expr.clear();
                    self.just_evaluated = false;
                } else if self.current_input.len() > 1 {
                    self.current_input.pop();
                    if self.current_input == "-" {
                        self.current_input = String::from("0");
                    }
                } else {
                    self.current_input = String::from("0");
                }
            }
            "+/-" => {
                if self.current_input.starts_with('-') {
                    self.current_input.remove(0);
                } else if self.current_input != "0" {
                    self.current_input.insert(0, '-');
                }
            }
            "1/x" => {
                match self.current_val() {
                    Ok(v) if v == 0.0 => self.error = Some("Cannot divide by zero".to_string()),
                    Ok(v) => {
                        self.expr = format!("1/({})", self.current_input);
                        self.current_input = format_number(1.0 / v);
                        self.just_evaluated = true;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "x^2" | "^2" => {
                match self.current_val() {
                    Ok(v) => {
                        self.expr = format!("sqr({})", self.current_input);
                        self.current_input = format_number(v * v);
                        self.just_evaluated = true;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "sqrt" => {
                match self.current_val() {
                    Ok(v) if v < 0.0 => self.error = Some("Invalid input".to_string()),
                    Ok(v) => {
                        self.expr = format!("sqrt({})", self.current_input);
                        self.current_input = format_number(v.sqrt());
                        self.just_evaluated = true;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "sin" | "cos" | "tan" | "ln" | "log" => {
                let fn_expr = format!("{}({})", tok, self.current_input);
                match Parser::eval(&fn_expr) {
                    Ok(v) => {
                        self.expr = fn_expr;
                        self.current_input = format_number(v);
                        self.just_evaluated = true;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "%" => {
                // Windows Calculator semantics: if an LHS exists ("100+"), compute 100 * (cur / 100).
                if let Ok(cur) = self.current_val() {
                    let trimmed = self.expr.trim_end();
                    if let Some(last_op) = trimmed.chars().last() {
                        if matches!(last_op, '+' | '-' | '*' | '/') {
                            let lhs_str = &trimmed[..trimmed.len() - last_op.len_utf8()];
                            if let Ok(lhs) = Parser::eval(lhs_str) {
                                let pct_val = if matches!(last_op, '+' | '-') {
                                    lhs * (cur / 100.0)
                                } else {
                                    cur / 100.0
                                };
                                self.current_input = format_number(pct_val);
                                return;
                            }
                        }
                    }
                    self.current_input = format_number(cur / 100.0);
                }
            }
            "pi" => {
                self.current_input = format_number(std::f64::consts::PI);
                self.just_evaluated = false;
            }
            "e" => {
                self.current_input = format_number(std::f64::consts::E);
                self.just_evaluated = false;
            }
            "+" | "-" | "*" | "/" | "^" => {
                if self.just_evaluated {
                    self.expr = format!("{}{}", self.current_input, tok);
                    self.current_input = String::from("0");
                    self.just_evaluated = false;
                } else if !self.expr.is_empty() && self.current_input == "0"
                    && self.expr.ends_with(['+', '-', '*', '/', '^'])
                {
                    // Allow changing the pending operator cleanly.
                    self.expr.pop();
                    self.expr.push_str(tok);
                } else {
                    self.expr.push_str(&self.current_input);
                    self.expr.push_str(tok);
                    self.current_input = String::from("0");
                }
            }
            "(" | ")" => {
                if self.just_evaluated {
                    self.expr.clear();
                    self.just_evaluated = false;
                }
                if tok == "(" && self.current_input == "0" {
                    self.expr.push('(');
                } else if tok == ")" {
                    self.expr.push_str(&self.current_input);
                    self.expr.push(')');
                    self.current_input = String::from("0");
                } else {
                    self.expr.push_str(&self.current_input);
                    self.expr.push('(');
                    self.current_input = String::from("0");
                }
            }
            "=" => {
                let full_expr = if self.just_evaluated {
                    self.expr.clone()
                } else if self.expr.is_empty() {
                    self.current_input.clone()
                } else {
                    format!("{}{}", self.expr, self.current_input)
                };
                match Parser::eval(&full_expr) {
                    Ok(v) => {
                        let res = format_number(v);
                        self.history.insert(0, (format!("{} =", full_expr), res.clone()));
                        if self.history.len() > 25 {
                            self.history.pop();
                        }
                        self.expr = format!("{} =", full_expr);
                        self.current_input = res;
                        self.just_evaluated = true;
                        self.error = None;
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                        self.just_evaluated = true;
                    }
                }
            }
            "." => {
                if self.just_evaluated {
                    self.expr.clear();
                    self.current_input = String::from("0.");
                    self.just_evaluated = false;
                } else if !self.current_input.contains('.') {
                    self.current_input.push('.');
                }
            }
            "MC" => {
                self.memory = None;
                self.rebuild_layout();
            }
            "MR" => {
                if let Some(m) = self.memory {
                    self.current_input = format_number(m);
                    self.just_evaluated = false;
                }
            }
            "MS" => {
                if let Ok(v) = self.current_val() {
                    self.memory = Some(v);
                    self.just_evaluated = true;
                    self.rebuild_layout();
                }
            }
            "M+" => {
                if let Ok(v) = self.current_val() {
                    self.memory = Some(self.memory.unwrap_or(0.0) + v);
                    self.just_evaluated = true;
                    self.rebuild_layout();
                }
            }
            "M-" => {
                if let Ok(v) = self.current_val() {
                    self.memory = Some(self.memory.unwrap_or(0.0) - v);
                    self.just_evaluated = true;
                    self.rebuild_layout();
                }
            }
            "M" => {
                self.history_open = !self.history_open;
            }
            d if d.len() == 1 && d.chars().all(|c| c.is_ascii_digit()) => {
                if self.just_evaluated {
                    self.expr.clear();
                    self.current_input = d.to_string();
                    self.just_evaluated = false;
                } else if self.current_input == "0" {
                    self.current_input = d.to_string();
                } else if self.current_input.len() < 18 {
                    self.current_input.push_str(d);
                }
            }
            _ => {}
        }
    }
}

impl Lifecycle for CalcApp {}

impl Paintable for CalcApp {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::PETRA_SLATE;
        let w = self.width as i32;
        let h = self.height as i32;

        // Clean Petra Slate Windows-Calculator background
        canvas.fill_rect(Rect::new(0, 0, w, h), t.bg_window);

        // Top mode header: ≡ Standard / Scientific + History toggle
        self.btn_mode_toggle.paint_with(canvas, &t);
        let mode_title = match self.mode {
            CalcMode::Standard => "Standard",
            CalcMode::Scientific => "Scientific",
        };
        font::draw_text_bold(canvas, 46, 11, mode_title, t.text_primary, 2);
        self.btn_hist_toggle.paint_with(canvas, &t);

        // Display area (clean right-aligned expression + large crisp readout)
        let disp = Rect::new(8, 40, w - 16, 86);
        canvas.fill_rect(disp, t.bg_surface);
        canvas.draw_rect_outline(disp, 1, t.border);

        let expr_str = if self.expr.is_empty() { " " } else { &self.expr };
        let ew = font::measure_text_width(expr_str, 2);
        font::draw_text_clipped(
            canvas,
            disp,
            (disp.right() - ew - 10).max(disp.x + 8),
            disp.y + 10,
            expr_str,
            t.text_secondary,
            2,
        );

        let readout = self.error.as_deref().unwrap_or(&self.current_input);
        let scale = if readout.len() > 14 { 2 } else { 3 };
        let rw = font::measure_text_width(readout, scale);
        let readout_col = if self.error.is_some() { t.danger } else { t.text_primary };
        font::draw_text_clipped(
            canvas,
            disp,
            (disp.right() - rw - 10).max(disp.x + 8),
            disp.y + 42,
            readout,
            readout_col,
            scale,
        );

        // Memory bar (MC MR M+ M- MS M)
        for (btn, _) in &self.mem_buttons {
            btn.paint_with(canvas, &t);
        }

        // Main keypad buttons (digits use crisp white bg_surface; operators use soft slate face)
        let painter = StylePainter::new(&t);
        for (btn, _, is_digit) in &self.buttons {
            if btn.accent {
                btn.paint_with(canvas, &t);
            } else if *is_digit {
                let mut digit_theme = t;
                digit_theme.bg_window = Color::rgb(0xFB, 0xFC, 0xFE);
                digit_theme.bg_surface = Color::WHITE;
                btn.paint_with(canvas, &digit_theme);
            } else {
                btn.paint_with(canvas, &t);
            }
        }
        let _ = painter;

        // Mode selector flyout
        if self.menu_open {
            let mr = Rect::new(8, 36, 150, 64);
            canvas.fill_rect(Rect::new(mr.x + 3, mr.y + 3, mr.w, mr.h), Color::argb(70, 0, 0, 0));
            canvas.fill_rect(mr, t.bg_surface);
            canvas.draw_rect_outline(mr, 1, t.border);
            for (idx, lbl) in ["Standard", "Scientific"].iter().enumerate() {
                let ir = Rect::new(mr.x + 3, mr.y + 4 + idx as i32 * 28, mr.w - 6, 26);
                let active = (idx == 0 && self.mode == CalcMode::Standard)
                    || (idx == 1 && self.mode == CalcMode::Scientific);
                if active {
                    canvas.fill_rect(ir, t.accent_soft);
                }
                font::draw_text(canvas, ir.x + 10, ir.y + 5, lbl, t.text_primary, 2);
            }
        }

        // History drawer overlay (Windows Calculator style)
        if self.history_open {
            let hr = Rect::new(8, 164, w - 16, (h - 172).max(120));
            canvas.fill_rect(Rect::new(hr.x + 2, hr.y + 2, hr.w, hr.h), Color::argb(65, 0, 0, 0));
            canvas.fill_rect(hr, t.bg_surface);
            canvas.draw_rect_outline(hr, 1, t.border);
            font::draw_text_bold(canvas, hr.x + 12, hr.y + 10, "History", t.accent, 2);
            if self.history.is_empty() {
                font::draw_text(
                    canvas,
                    hr.x + 12,
                    hr.y + 42,
                    "There's no history yet.",
                    t.text_secondary,
                    2,
                );
            } else {
                for (i, (e_str, res_str)) in self.history.iter().take(7).enumerate() {
                    let ry = hr.y + 36 + i as i32 * 40;
                    let item_r = Rect::new(hr.x + 6, ry, hr.w - 12, 36);
                    canvas.fill_rect(item_r, t.bg_surface_alt);
                    canvas.draw_rect_outline(item_r, 1, t.bevel_shadow);
                    let ew = font::measure_text_width(e_str, 1);
                    font::draw_text_clipped(
                        canvas,
                        item_r,
                        (item_r.right() - ew - 8).max(item_r.x + 6),
                        item_r.y + 4,
                        e_str,
                        t.text_secondary,
                        1,
                    );
                    let rw = font::measure_text_width(res_str, 2);
                    font::draw_text_clipped(
                        canvas,
                        item_r,
                        (item_r.right() - rw - 8).max(item_r.x + 6),
                        item_r.y + 16,
                        res_str,
                        t.text_primary,
                        2,
                    );
                }
            }
        }
    }
}

impl AppLogic for CalcApp {
    fn handle_event(&mut self, event: UiEvent) -> bool {
        if let UiEvent::Resize { width, height } = event {
            self.width = width;
            self.height = height;
            self.rebuild_layout();
            return true;
        }

        let mut dirty = false;
        dirty |= self.btn_mode_toggle.on_event(event);
        if self.btn_mode_toggle.take_click() {
            self.menu_open = !self.menu_open;
            return true;
        }
        dirty |= self.btn_hist_toggle.on_event(event);
        if self.btn_hist_toggle.take_click() {
            self.history_open = !self.history_open;
            self.menu_open = false;
            return true;
        }

        if self.menu_open {
            if let UiEvent::MouseDown { pos, button: 1 } = event {
                let mr = Rect::new(8, 36, 150, 64);
                if mr.contains(pos) {
                    let idx = ((pos.y - mr.y - 4) / 28).clamp(0, 1);
                    self.mode = if idx == 0 { CalcMode::Standard } else { CalcMode::Scientific };
                    self.menu_open = false;
                    self.rebuild_layout();
                    return true;
                }
                self.menu_open = false;
                return true;
            }
        }

        if self.history_open {
            if let UiEvent::MouseDown { pos, button: 1 } = event {
                let hr = Rect::new(8, 164, self.width as i32 - 16, (self.height as i32 - 172).max(120));
                if hr.contains(pos) {
                    let idx = (pos.y - (hr.y + 36)) / 40;
                    if idx >= 0 {
                        if let Some((e_str, res_str)) = self.history.get(idx as usize).cloned() {
                            self.expr = e_str;
                            self.current_input = res_str;
                            self.just_evaluated = true;
                            self.history_open = false;
                            return true;
                        }
                    }
                    return false;
                }
                self.history_open = false;
                return true;
            }
        }

        let mut clicked_mem = None;
        for (btn, tok) in &mut self.mem_buttons {
            dirty |= btn.on_event(event);
            if btn.take_click() {
                clicked_mem = Some(*tok);
            }
        }
        if let Some(tok) = clicked_mem {
            self.apply_token(tok);
            return true;
        }

        let mut clicked_tok = None;
        for (btn, tok, _) in &mut self.buttons {
            dirty |= btn.on_event(event);
            if btn.take_click() {
                clicked_tok = Some(*tok);
            }
        }
        if let Some(tok) = clicked_tok {
            self.apply_token(tok);
            return true;
        }

        if let UiEvent::Key { code, pressed: true, ch, .. } = event {
            if code == 1 {
                self.apply_token("C");
                return true;
            }
            if code == 14 {
                self.apply_token("<-");
                return true;
            }
            if code == 28 {
                self.apply_token("=");
                return true;
            }
            if let Some(c) = ch {
                match c {
                    '0'..='9' => {
                        let mut s = [0u8; 4];
                        let tok = c.encode_utf8(&mut s);
                        self.apply_token(tok);
                        return true;
                    }
                    '.' | ',' => {
                        self.apply_token(".");
                        return true;
                    }
                    '+' => {
                        self.apply_token("+");
                        return true;
                    }
                    '-' => {
                        self.apply_token("-");
                        return true;
                    }
                    '*' => {
                        self.apply_token("*");
                        return true;
                    }
                    '/' => {
                        self.apply_token("/");
                        return true;
                    }
                    '^' => {
                        self.apply_token("^");
                        return true;
                    }
                    '%' => {
                        self.apply_token("%");
                        return true;
                    }
                    '(' => {
                        self.apply_token("(");
                        return true;
                    }
                    ')' => {
                        self.apply_token(")");
                        return true;
                    }
                    '=' => {
                        self.apply_token("=");
                        return true;
                    }
                    'c' | 'C' => {
                        self.apply_token("C");
                        return true;
                    }
                    _ => {}
                }
            }
        }

        dirty
    }
}

fn main() {
    let w = 340;
    let h = 500;
    let mut client = SpaceClient::connect("Calculator", w, h).expect("connect failed");
    let mut app = CalcApp::new(w, h);
    client.run(&mut app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_precedence_and_edge_cases() {
        assert_eq!(Parser::eval("2+3*4").unwrap(), 14.0);
        assert_eq!(Parser::eval("(2+3)*4").unwrap(), 20.0);
        assert_eq!(Parser::eval("-2^2").unwrap(), -4.0);
        assert_eq!(Parser::eval("(-2)^2").unwrap(), 4.0);
        assert_eq!(Parser::eval("2^-2").unwrap(), 0.25);
        assert_eq!(Parser::eval("+5 + -2").unwrap(), 3.0);
        assert_eq!(Parser::eval("2(3+4)").unwrap(), 14.0);
        assert_eq!(Parser::eval("50%").unwrap(), 0.5);
        assert_eq!(Parser::eval("sqrt(16)").unwrap(), 4.0);
        assert!(Parser::eval("sqrt(-1)").is_err());
        assert!(Parser::eval("1/0").is_err());
        assert!(Parser::eval("ln(0)").is_err());
    }

    #[test]
    fn windows_calc_state_machine_chaining() {
        let mut app = CalcApp::new(340, 500);
        for tok in ["2", "+", "3", "="] {
            app.apply_token(tok);
        }
        assert_eq!(app.current_input, "5");
        // Pressing operator after '=' continues from 5 instead of corrupting precedence
        for tok in ["*", "2", "="] {
            app.apply_token(tok);
        }
        assert_eq!(app.current_input, "10");
        // Pressing digit after '=' starts fresh
        app.apply_token("7");
        assert_eq!(app.current_input, "7");
        assert_eq!(app.expr, "");
        // Percent on 100 + 10 % => 100 + 10 = 110
        app.apply_token("C");
        for tok in ["1", "0", "0", "+", "1", "0", "%", "="] {
            app.apply_token(tok);
        }
        assert_eq!(app.current_input, "110");
    }
}
