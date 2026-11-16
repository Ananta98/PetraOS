use space_core::font;
use space_core::{Canvas, Rect};
use space_ui::{
    draw_panel_3d, AppLogic, Button, EventListener, Lifecycle, Paintable, SpaceClient, Theme,
    UiEvent,
};

struct Parser<'a> {
    chars: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn eval(expr: &str) -> Result<f64, &'static str> {
        let cleaned: String = expr.chars().filter(|c| !c.is_whitespace()).collect();
        let mut p = Parser {
            chars: cleaned.as_bytes(),
            pos: 0,
        };
        let v = p.parse_expr()?;
        if p.pos < p.chars.len() {
            return Err("Syntax error");
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

    fn parse_term(&mut self) -> Result<f64, &'static str> {
        let mut v = self.parse_power()?;
        while let Some(op) = self.peek() {
            if op == b'*' || op == b'/' {
                self.pos += 1;
                let rhs = self.parse_power()?;
                if op == b'/' && rhs == 0.0 {
                    return Err("Div by zero");
                }
                v = if op == b'*' { v * rhs } else { v / rhs };
            } else {
                break;
            }
        }
        Ok(v)
    }

    fn parse_power(&mut self) -> Result<f64, &'static str> {
        let base = self.parse_unary()?;
        if self.peek() == Some(b'^') {
            self.pos += 1;
            let exp = self.parse_power()?;
            Ok(base.powf(exp))
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> Result<f64, &'static str> {
        if self.peek() == Some(b'-') {
            self.pos += 1;
            return Ok(-self.parse_unary()?);
        }
        self.parse_primary()
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
            let ident = std::str::from_utf8(&self.chars[start..self.pos]).unwrap_or("");
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
            return match ident {
                "sin" => Ok(arg.sin()),
                "cos" => Ok(arg.cos()),
                "tan" => Ok(arg.tan()),
                "sqrt" => Ok(arg.sqrt()),
                "ln" => Ok(arg.ln()),
                "log" => Ok(arg.log10()),
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

struct CalcApp {
    expr: String,
    result: String,
    history: Vec<String>,
    buttons: Vec<(Button, &'static str)>,
}

impl CalcApp {
    fn new() -> Self {
        let layout = [
            ["sin(", "cos(", "tan(", "sqrt(", "C"],
            ["ln(", "log(", "(", ")", "<-"],
            ["7", "8", "9", "/", "^"],
            ["4", "5", "6", "*", "pi"],
            ["1", "2", "3", "-", "e"],
            ["0", ".", "=", "+", "CLR"],
        ];
        let mut buttons = Vec::new();
        for (r, row) in layout.iter().enumerate() {
            for (c, &lbl) in row.iter().enumerate() {
                let rect = Rect::new(12 + c as i32 * 84, 150 + r as i32 * 44, 78, 38);
                let mut btn = Button::new(rect, lbl);
                if lbl == "=" {
                    btn.accent = true;
                }
                buttons.push((btn, lbl));
            }
        }
        Self {
            expr: String::new(),
            result: String::from("0"),
            history: Vec::new(),
            buttons,
        }
    }

    fn apply_token(&mut self, tok: &str) {
        match tok {
            "C" => {
                self.expr.clear();
                self.result = String::from("0");
            }
            "CLR" => {
                self.history.clear();
            }
            "<-" => {
                self.expr.pop();
            }
            "=" => {
                if self.expr.is_empty() {
                    return;
                }
                match Parser::eval(&self.expr) {
                    Ok(v) => {
                        let res = format!("{:.8}", v)
                            .trim_end_matches('0')
                            .trim_end_matches('.')
                            .to_string();
                        self.history.insert(0, format!("{} = {}", self.expr, res));
                        if self.history.len() > 20 {
                            self.history.pop();
                        }
                        self.result = res;
                    }
                    Err(e) => self.result = e.to_string(),
                }
            }
            _ => self.expr.push_str(tok),
        }
    }
}

impl Lifecycle for CalcApp {}

impl Paintable for CalcApp {
    fn paint(&self, canvas: &mut dyn Canvas) {
        let t = Theme::XP_LUNA;
        let disp = Rect::new(12, 12, 414, 124);
        canvas.fill_rect(disp, t.bg_input);
        draw_panel_3d(canvas, disp, true, &t);
        canvas.draw_rect_outline(disp, 1, t.border);

        font::draw_text(canvas, disp.x + 12, disp.y + 14, &self.expr, t.text_secondary, 2);
        let rw = font::measure_text_width(&self.result, 3);
        font::draw_text(
            canvas,
            (disp.right() - rw - 14).max(disp.x + 12),
            disp.y + 52,
            &self.result,
            t.accent,
            3,
        );
        if let Some(last) = self.history.first() {
            font::draw_text(canvas, disp.x + 12, disp.y + 98, last, t.text_secondary, 2);
        }

        for (btn, _) in &self.buttons {
            btn.paint(canvas);
        }

        // History sidebar
        let hist_r = Rect::new(438, 12, 230, 398);
        canvas.fill_rect(hist_r, t.bg_surface);
        draw_panel_3d(canvas, hist_r, true, &t);
        canvas.draw_rect_outline(hist_r, 1, t.border);
        font::draw_text(canvas, hist_r.x + 10, hist_r.y + 10, "History", t.accent, 2);
        for (i, h) in self.history.iter().take(14).enumerate() {
            let short: String = h.chars().take(18).collect();
            font::draw_text(canvas, hist_r.x + 10, hist_r.y + 40 + i as i32 * 24, &short, t.text_primary, 2);
        }
    }
}

impl AppLogic for CalcApp {
    fn handle_event(&mut self, event: UiEvent) -> bool {
        let mut dirty = false;
        let mut clicked_tok = None;
        for (btn, tok) in &mut self.buttons {
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
            if code == 14 {
                self.apply_token("<-");
                return true;
            }
            if code == 28 {
                self.apply_token("=");
                return true;
            }
            if let Some(c) = ch {
                if !c.is_control() {
                    self.expr.push(c);
                    return true;
                }
            }
        }
        dirty
    }
}

fn main() {
    let mut client = SpaceClient::connect("Space Calc", 680, 424).expect("connect failed");
    let mut app = CalcApp::new();
    client.run(&mut app);
}
