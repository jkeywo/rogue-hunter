//! Browser WebAssembly ASCII client.
//!
//! A purpose-built Canvas/HTML view over the shared client core: the map
//! grid draws to a canvas, panels render as HTML, and every input becomes
//! the same semantic intents the terminal client uses. Active runs persist
//! to browser localStorage as share codes.

use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, Document, Element, HtmlCanvasElement};

use rh_client_core::view::{CellColor, OverlayView, PanelLabels, RunView, ScreenView};
use rh_client_core::{ClientSession, Intent, Screen};
use rh_core::events::EventKind;
use rh_core::geometry::{Direction, Point, MAP_HEIGHT, MAP_WIDTH};

const CELL_W: f64 = 16.0;
const CELL_H: f64 = 24.0;
const SAVE_KEY: &str = "rogue-hunter-active-run";

#[wasm_bindgen]
pub struct WebClient {
    session: ClientSession,
}

#[wasm_bindgen]
impl WebClient {
    /// Build the client; restores a saved run from localStorage if present.
    #[wasm_bindgen(constructor)]
    pub fn new(nonce: f64) -> Result<WebClient, JsValue> {
        console_error_panic_hook();
        let catalogue = rh_content::load_embedded()
            .map_err(|error| JsValue::from_str(&format!("content: {error}")))?;
        let mut session = ClientSession::new(catalogue, nonce as u64);
        if let Some(code) = load_saved() {
            session.restore(&code);
        }
        Ok(WebClient { session })
    }

    /// Canvas pixel width/height the page should allocate.
    pub fn canvas_width(&self) -> u32 {
        (MAP_WIDTH as f64 * CELL_W) as u32
    }

    pub fn canvas_height(&self) -> u32 {
        (MAP_HEIGHT as f64 * CELL_H) as u32
    }

    /// Handle a keyboard event (`event.key`); returns true when state changed.
    pub fn handle_key(&mut self, key: &str, ctrl: bool) -> bool {
        if ctrl {
            return false;
        }
        let Some(intent) = self.translate_key(key) else {
            return false;
        };
        self.session.handle(intent);
        self.persist();
        true
    }

    pub fn handle_paste(&mut self, text: &str) {
        self.session.handle(Intent::Paste(text.to_owned()));
        self.persist();
    }

    /// Mouse moved over the canvas at pixel coordinates.
    pub fn hover(&mut self, x: f64, y: f64) {
        match cell_at(x, y) {
            Some(point) => self.session.handle(Intent::Hover(point)),
            None => self.session.handle(Intent::HoverClear),
        }
    }

    pub fn hover_clear(&mut self) {
        self.session.handle(Intent::HoverClear);
    }

    /// Mouse click on the canvas at pixel coordinates.
    pub fn click(&mut self, x: f64, y: f64) {
        if let Some(point) = cell_at(x, y) {
            self.session.handle(Intent::Click(point));
            self.persist();
        }
    }

    /// Click on a menu/list row (splash options, overlay items, list entries).
    pub fn choose(&mut self, index: u32) {
        self.session.handle(Intent::Select(index as usize));
        self.persist();
    }

    /// Move the menu highlight to a row the mouse is over, without choosing
    /// it. Keeps the detail pane following the pointer.
    pub fn hover_row(&mut self, index: u32) {
        self.session.handle(Intent::HoverRow(index as usize));
    }

    /// Fire the action at `index` in the on-screen action panel (a click).
    pub fn do_action(&mut self, index: u32) {
        self.session.handle(Intent::DoAction(index as usize));
        self.persist();
    }

    /// The current share code for the clipboard button.
    pub fn share_code(&self) -> Option<String> {
        self.session.share_code()
    }

    /// Deterministic state digest, for cross-client identity checks.
    pub fn state_digest(&self) -> Option<String> {
        self.session
            .run
            .as_ref()
            .map(|run| format!("{:016x}", run.state_digest()))
    }

    /// Render the whole frame into the page.
    pub fn render(&self) -> Result<(), JsValue> {
        let document = document()?;
        let view = self.session.view();
        match &view.screen {
            ScreenView::Run(run) => {
                show(&document, "game", true)?;
                show(&document, "fullscreen", false)?;
                self.draw_map(&document, run)?;
                set_html(&document, "side", &side_html(run, &view.labels))?;
                set_html(&document, "actions", &actions_html(run, &view.labels))?;
                set_html(&document, "log", &log_html(run, &view.status))?;
                match &run.overlay {
                    Some(overlay) => {
                        show(&document, "overlay", true)?;
                        set_html(&document, "overlay", &overlay_html(overlay, &view.labels))?;
                    }
                    None => show(&document, "overlay", false)?,
                }
            }
            other => {
                show(&document, "game", false)?;
                show(&document, "overlay", false)?;
                show(&document, "fullscreen", true)?;
                set_html(
                    &document,
                    "fullscreen",
                    &fullscreen_html(other, &view.status, &view.labels),
                )?;
            }
        }
        Ok(())
    }
}

impl WebClient {
    fn translate_key(&self, key: &str) -> Option<Intent> {
        let text_entry = matches!(
            self.session.screen,
            Screen::SeedEntry { .. } | Screen::CodeEntry { .. }
        );
        if text_entry {
            return match key {
                "Enter" => Some(Intent::Confirm),
                "Escape" => Some(Intent::Cancel),
                "Backspace" => Some(Intent::Backspace),
                k if k.chars().count() == 1 => k.chars().next().map(Intent::Char),
                _ => None,
            };
        }
        let in_menu = self.session.modal.is_some() || !matches!(self.session.screen, Screen::Run);
        match key {
            "Escape" => Some(Intent::Cancel),
            "Enter" => Some(Intent::Confirm),
            "ArrowUp" if in_menu => Some(Intent::Up),
            "ArrowDown" if in_menu => Some(Intent::Down),
            "ArrowUp" => Some(Intent::Move(Direction::North)),
            "ArrowDown" => Some(Intent::Move(Direction::South)),
            "ArrowLeft" => Some(Intent::Move(Direction::West)),
            "ArrowRight" => Some(Intent::Move(Direction::East)),
            "h" => Some(Intent::Move(Direction::West)),
            "j" if !in_menu => Some(Intent::Move(Direction::South)),
            "k" if !in_menu => Some(Intent::Move(Direction::North)),
            "j" => Some(Intent::Down),
            "k" => Some(Intent::Up),
            "l" => Some(Intent::Move(Direction::East)),
            "y" => Some(Intent::Move(Direction::NorthWest)),
            "u" => Some(Intent::Move(Direction::NorthEast)),
            "b" => Some(Intent::Move(Direction::SouthWest)),
            "n" => Some(Intent::Move(Direction::SouthEast)),
            "." | " " => Some(Intent::Wait),
            "e" => Some(Intent::Interact),
            "f" => Some(Intent::Fire),
            "F" => Some(Intent::FireSilver),
            "a" => Some(Intent::Aim),
            "p" => Some(Intent::PowerAttack),
            "s" => Some(Intent::Sprint),
            "x" => Some(Intent::SetSnare),
            "K" => Some(Intent::KillingBlow),
            "q" => Some(Intent::Draught),
            "c" => Some(Intent::Charm),
            "g" => Some(Intent::Grimoire),
            "r" => Some(Intent::Relationships),
            "v" => Some(Intent::RegionMap),
            "L" => Some(Intent::EventLog),
            ";" => Some(Intent::ToggleLook),
            // Numpad movement (NumLock on gives digits; off gives nav keys).
            "1" if !in_menu => Some(Intent::Move(Direction::SouthWest)),
            "2" if !in_menu => Some(Intent::Move(Direction::South)),
            "3" if !in_menu => Some(Intent::Move(Direction::SouthEast)),
            "4" if !in_menu => Some(Intent::Move(Direction::West)),
            "5" if !in_menu => Some(Intent::Wait),
            "6" if !in_menu => Some(Intent::Move(Direction::East)),
            "7" if !in_menu => Some(Intent::Move(Direction::NorthWest)),
            "8" if !in_menu => Some(Intent::Move(Direction::North)),
            "9" if !in_menu => Some(Intent::Move(Direction::NorthEast)),
            "Home" if !in_menu => Some(Intent::Move(Direction::NorthWest)),
            "PageUp" if !in_menu => Some(Intent::Move(Direction::NorthEast)),
            "End" if !in_menu => Some(Intent::Move(Direction::SouthWest)),
            "PageDown" if !in_menu => Some(Intent::Move(Direction::SouthEast)),
            "Clear" if !in_menu => Some(Intent::Wait),
            _ => None,
        }
    }

    fn persist(&self) {
        let storage = web_sys::window().and_then(|window| window.local_storage().ok().flatten());
        let Some(storage) = storage else { return };
        match (&self.session.screen, self.session.share_code()) {
            (Screen::CaseReport, _) | (Screen::Splash { .. }, _) => {
                let _ = storage.remove_item(SAVE_KEY);
            }
            (_, Some(code)) => {
                let _ = storage.set_item(SAVE_KEY, &code);
            }
            _ => {}
        }
    }

    fn draw_map(&self, document: &Document, run: &RunView) -> Result<(), JsValue> {
        let canvas: HtmlCanvasElement = document
            .get_element_by_id("map")
            .ok_or_else(|| JsValue::from_str("missing #map"))?
            .dyn_into()?;
        let context: CanvasRenderingContext2d = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("no 2d context"))?
            .dyn_into()?;
        context.set_fill_style_str("#0a0a08");
        context.fill_rect(
            0.0,
            0.0,
            f64::from(canvas.width()),
            f64::from(canvas.height()),
        );
        context.set_font("bold 18px 'Courier New', monospace");
        context.set_text_align("center");
        context.set_text_baseline("middle");
        for y in 0..MAP_HEIGHT as usize {
            for x in 0..MAP_WIDTH as usize {
                let cell = run.cells[y * MAP_WIDTH as usize + x];
                if cell.glyph == ' ' {
                    continue;
                }
                context.set_fill_style_str(css_color(cell.color));
                context.fill_text(
                    &cell.glyph.to_string(),
                    (x as f64 + 0.5) * CELL_W,
                    (y as f64 + 0.5) * CELL_H,
                )?;
            }
        }
        // The look cursor: an outline box over the inspected tile.
        if let Some(cursor) = run.cursor {
            context.set_stroke_style_str(if run.looking { "#ffd75f" } else { "#87afff" });
            context.set_line_width(2.0);
            context.stroke_rect(
                f64::from(cursor.x) * CELL_W + 1.0,
                f64::from(cursor.y) * CELL_H + 1.0,
                CELL_W - 2.0,
                CELL_H - 2.0,
            );
        }
        Ok(())
    }
}

fn cell_at(x: f64, y: f64) -> Option<Point> {
    let column = (x / CELL_W).floor() as i16;
    let row = (y / CELL_H).floor() as i16;
    let point = Point::new(column, row);
    point.in_bounds().then_some(point)
}

fn css_color(color: CellColor) -> &'static str {
    match color {
        CellColor::Hunter => "#ffd75f",
        CellColor::Npc => "#5fd7d7",
        CellColor::Enemy => "#ff5f5f",
        CellColor::Villain => "#ff3030",
        CellColor::VillainVulnerable => "#ff87ff",
        CellColor::Terrain => "#9e9e8e",
        CellColor::TerrainDim => "#4a4a42",
        CellColor::Feature => "#87afff",
        CellColor::Opportunity => "#87ff87",
        CellColor::Exit => "#5fffff",
        CellColor::Snare => "#5fd75f",
        CellColor::Unseen => "#000000",
    }
}

fn event_css(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Combat => "ev-combat",
        EventKind::Telegraph => "ev-telegraph",
        EventKind::Clue => "ev-clue",
        EventKind::Clock => "ev-clock",
        EventKind::Social => "ev-social",
        EventKind::Item => "ev-item",
        EventKind::Travel => "ev-travel",
        EventKind::System => "ev-system",
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn side_html(run: &RunView, labels: &PanelLabels) -> String {
    let mut html = String::new();
    html.push_str(&format!("<h2>{}</h2>", escape(&run.header)));
    html.push_str(&format!(
        "<p class=\"clock\">{}</p>",
        escape(&run.clock_line)
    ));
    html.push_str(&format!(
        "<p>{} &nbsp; {}</p>",
        escape(&run.health_line),
        escape(&run.stamina_line)
    ));
    html.push_str(&format!("<p>{}</p>", escape(&run.pools_line)));
    let look_title = if run.looking {
        labels.look_cursor.as_str()
    } else {
        labels.look_plain.as_str()
    };
    html.push_str(&format!("<h3>{look_title}</h3>"));
    match &run.inspect {
        Some(text) => html.push_str(&format!("<p class=\"look\">{}</p>", escape(text))),
        None => html.push_str(&format!(
            "<p class=\"dim\">{}</p>",
            escape(&labels.look_hint)
        )),
    }
    html.push_str(&format!("<h3>{}</h3><ul>", escape(&labels.pack)));
    for item in &run.inventory {
        html.push_str(&format!("<li>{}</li>", escape(item)));
    }
    html.push_str("</ul>");
    html
}

fn actions_html(run: &RunView, labels: &PanelLabels) -> String {
    let mut html = format!(
        "<h3>{}</h3><ul class=\"actionlist\">",
        escape(&labels.actions)
    );
    for (index, action) in run.actions.iter().enumerate() {
        let disabled = if action.enabled { "" } else { " disabled" };
        let note = match &action.note {
            Some(note) => format!("<span class=\"note\"> ({})</span>", escape(note)),
            None => String::new(),
        };
        html.push_str(&format!(
            "<li class=\"action{disabled}\" data-action=\"{index}\">\
               <span class=\"akey\">{}</span> {}{}</li>",
            escape(&action.key),
            escape(&action.label),
            note
        ));
    }
    html.push_str("</ul>");
    html
}

fn log_html(run: &RunView, status: &str) -> String {
    let mut html = String::from("<div class=\"events\">");
    for (kind, text) in &run.log_tail {
        html.push_str(&format!(
            "<div class=\"{}\">{}</div>",
            event_css(*kind),
            escape(text)
        ));
    }
    if !status.is_empty() {
        html.push_str(&format!("<div class=\"status\">{}</div>", escape(status)));
    }
    html.push_str("</div>");
    html
}

fn overlay_html(overlay: &OverlayView, labels: &PanelLabels) -> String {
    let mut html = format!("<h3>{}</h3>", escape(&overlay.title));
    if overlay.items.is_empty() {
        html.push_str(&format!(
            "<p class=\"dim\">{}</p>",
            escape(&labels.direction_hint)
        ));
        return html;
    }
    html.push_str("<ul class=\"menu\">");
    for (index, (label, blocked)) in overlay.items.iter().enumerate() {
        let selected = if index == overlay.selected {
            " selected"
        } else {
            ""
        };
        match blocked {
            None => html.push_str(&format!(
                "<li class=\"item{selected}\" data-choice=\"{index}\">{}</li>",
                escape(label)
            )),
            Some(reason) => html.push_str(&format!(
                "<li class=\"item blocked{selected}\" data-choice=\"{index}\">{} — {}</li>",
                escape(label),
                escape(reason)
            )),
        }
    }
    html.push_str("</ul>");
    html
}

fn fullscreen_html(screen: &ScreenView, status: &str, labels: &PanelLabels) -> String {
    match screen {
        ScreenView::Splash {
            title,
            intro,
            bindings,
            options,
            selected,
        } => {
            let mut html = format!("<h1>{}</h1>", escape(title));
            for paragraph in intro {
                html.push_str(&format!("<p class=\"intro\">{}</p>", escape(paragraph)));
            }
            html.push_str("<table class=\"bindings\">");
            for (keys, action) in bindings {
                html.push_str(&format!(
                    "<tr><td class=\"keys\">{}</td><td>{}</td></tr>",
                    escape(keys),
                    escape(action)
                ));
            }
            html.push_str("</table><ul class=\"menu big\">");
            for (index, option) in options.iter().enumerate() {
                let selected_class = if index == *selected { " selected" } else { "" };
                html.push_str(&format!(
                    "<li class=\"item{selected_class}\" data-choice=\"{index}\">{}</li>",
                    escape(option)
                ));
            }
            html.push_str("</ul>");
            html
        }
        ScreenView::TextEntry {
            title,
            prompt,
            input,
            error,
        } => {
            let mut html = format!(
                "<h1>{}</h1><p>{}</p><p class=\"entry\">&gt; {}<span class=\"caret\">_</span></p>",
                escape(title),
                escape(prompt),
                escape(input)
            );
            if let Some(error) = error {
                html.push_str(&format!("<p class=\"error\">{}</p>", escape(error)));
            }
            html.push_str("<p class=\"hints\">Enter to confirm — Esc to go back — paste works</p>");
            html
        }
        ScreenView::List {
            title,
            entries,
            selected,
        } => {
            let mut html = format!(
                "<h1>{}</h1><div class=\"split\"><ul class=\"menu\">",
                escape(title)
            );
            let current = selected.unwrap_or(0);
            for (index, (heading, _)) in entries.iter().enumerate() {
                let selected_class = if index == current { " selected" } else { "" };
                html.push_str(&format!(
                    "<li class=\"item{selected_class}\" data-choice=\"{index}\">{}</li>",
                    escape(heading)
                ));
            }
            html.push_str("</ul><div class=\"detail\">");
            if let Some((_, body)) = entries.get(current) {
                for line in body.lines() {
                    html.push_str(&format!("<p>{}</p>", escape(line)));
                }
            }
            html.push_str(&format!(
                "</div></div><p class=\"hints\">{}</p>",
                escape(&labels.list_hint)
            ));
            html
        }
        ScreenView::CaseReport(report) => {
            let mut html = format!(
                "<h1>Case Report</h1><p class=\"outcome\">{}</p><p class=\"villain\">{}</p><p>{}</p><p>{}</p>",
                escape(&report.outcome),
                escape(&report.villain),
                escape(&report.origin),
                escape(&report.scheme)
            );
            if !report.hidden_clues.is_empty() {
                html.push_str("<h3>What you never found</h3><ul class=\"dimlist\">");
                for clue in &report.hidden_clues {
                    html.push_str(&format!("<li>{}</li>", escape(clue)));
                }
                html.push_str("</ul>");
            }
            html.push_str("<h3>The certified routes</h3>");
            for route in &report.routes {
                html.push_str("<pre class=\"route\">");
                html.push_str(&escape(route));
                html.push_str("</pre>");
            }
            html.push_str(&format!(
                "<h3>Replay share code</h3><pre id=\"share-code\" class=\"code\">{}</pre>\
                 <button id=\"copy-code\">Copy code</button>\
                 <p class=\"hints\">Enter returns to the fireside</p>",
                escape(&report.share_code)
            ));
            if !status.is_empty() {
                html.push_str(&format!("<p class=\"status\">{}</p>", escape(status)));
            }
            html
        }
        ScreenView::Run(_) => String::new(),
    }
}

fn document() -> Result<Document, JsValue> {
    web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("no document"))
}

fn set_html(document: &Document, id: &str, html: &str) -> Result<(), JsValue> {
    let element: Element = document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str("missing element"))?;
    element.set_inner_html(html);
    Ok(())
}

fn show(document: &Document, id: &str, visible: bool) -> Result<(), JsValue> {
    let element = document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str("missing element"))?;
    let class_list = element.class_list();
    if visible {
        class_list.remove_1("hidden")?;
    } else {
        class_list.add_1("hidden")?;
    }
    Ok(())
}

fn load_saved() -> Option<String> {
    web_sys::window()?
        .local_storage()
        .ok()
        .flatten()?
        .get_item(SAVE_KEY)
        .ok()
        .flatten()
}

/// Route panics to the browser console with a readable message.
fn console_error_panic_hook() {
    use std::sync::Once;
    static SET: Once = Once::new();
    SET.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            web_sys::console::error_1(&JsValue::from_str(&info.to_string()));
        }));
    });
}
