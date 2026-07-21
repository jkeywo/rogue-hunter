//! Browser WebAssembly ASCII client.
//!
//! A purpose-built Canvas/HTML view over the shared client core: the map
//! grid draws to a canvas, panels render as HTML, and every input becomes
//! the same semantic intents the terminal client uses. Active runs persist
//! to browser localStorage as share codes.

use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, Document, Element, HtmlCanvasElement};

use rh_client_core::view::{CellColor, OverlayView, PanelLabels, RunView, ScreenView};
use rh_client_core::{ClientSession, Intent, Key, SaveAction, Screen};
use rh_core::events::EventKind;
use rh_core::geometry::{Point, MAP_HEIGHT, MAP_WIDTH};

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

    /// Whether a click-to-walk is still under way, so the page knows to
    /// schedule the next step.
    pub fn walking(&self) -> bool {
        self.session.walking()
    }

    /// Take one more step of a walk in progress; returns whether more remain.
    pub fn step_walk(&mut self) -> bool {
        let more = self.session.step_walk();
        self.persist();
        more
    }

    /// Click on a menu/list row (splash options, overlay items, list entries).
    pub fn choose(&mut self, index: u32) {
        self.session.handle(Intent::Select(index as usize));
        self.persist();
    }

    /// Move the menu highlight to a row the mouse is over, without choosing
    /// it. Keeps the detail pane following the pointer.
    ///
    /// Returns whether anything actually moved. The caller must not redraw
    /// when nothing did: redrawing replaces the row under the cursor, the
    /// browser sees a new element there and fires `mouseover` again, and the
    /// press and release of a real click land on different nodes — so no
    /// click event is ever produced. That is what made the menus unclickable.
    pub fn hover_row(&mut self, index: u32) -> bool {
        let before = self.selection();
        self.session.handle(Intent::HoverRow(index as usize));
        self.selection() != before
    }

    /// Which row the current screen or modal has highlighted.
    fn selection(&self) -> Option<usize> {
        if let Some(rh_client_core::Modal::Menu { selected, .. }) = &self.session.modal {
            return Some(*selected);
        }
        match &self.session.screen {
            Screen::Splash { selected }
            | Screen::HunterSelect { selected, .. }
            | Screen::Grimoire { selected }
            | Screen::Relationships { selected }
            | Screen::RegionMap { selected }
            | Screen::EventLog { selected } => Some(*selected),
            _ => None,
        }
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
        key_of(key).and_then(|key| self.session.intent_for_key(key))
    }
}

/// Map a browser `event.key` string onto the shared platform-neutral key.
/// What the key *means* is the session's business, not this client's.
fn key_of(key: &str) -> Option<Key> {
    Some(match key {
        "ArrowUp" => Key::Up,
        "ArrowDown" => Key::Down,
        "ArrowLeft" => Key::Left,
        "ArrowRight" => Key::Right,
        "Enter" => Key::Enter,
        "Escape" => Key::Escape,
        "Backspace" => Key::Backspace,
        "Tab" => Key::Tab,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "Clear" => Key::Clear,
        k => {
            let mut chars = k.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Key::Char(c),
                _ => return None,
            }
        }
    })
}

impl WebClient {
    fn persist(&self) {
        let storage = web_sys::window().and_then(|window| window.local_storage().ok().flatten());
        let Some(storage) = storage else { return };
        // The policy lives in the session; this is only the storage I/O.
        match self.session.save_action() {
            SaveAction::Write(code) => {
                let _ = storage.set_item(SAVE_KEY, &code);
            }
            SaveAction::Clear => {
                let _ = storage.remove_item(SAVE_KEY);
            }
            SaveAction::Keep => {}
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
        // Remembered ground: dimmer than what is in sight, but still legible —
        // it was #4a4a42, which a contrast check found was below the bar for
        // reading at all. Held clearly below Terrain so the two still differ.
        CellColor::TerrainDim => "#70705f",
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
    // What is in sight, nearest first: the same list Tab walks the cursor
    // through, so the panel and the keyboard agree about what is out there.
    html.push_str(&format!("<h3>{}</h3>", escape(&labels.in_sight)));
    if run.in_sight.is_empty() {
        html.push_str(&format!(
            "<p class=\"dim\">{}</p>",
            escape(&labels.in_sight_empty)
        ));
    } else {
        html.push_str("<ul class=\"sightlist\">");
        for entry in &run.in_sight {
            // Hostility is a word, not only a colour: the class still tints it
            // for a sighted player, and the marker names it for everyone else.
            let (class, mark) = if entry.hostile {
                ("hostile", &labels.sight_hostile)
            } else {
                ("villager", &labels.sight_villager)
            };
            html.push_str(&format!(
                "<li class=\"{class}\"><span class=\"mark\">{}</span> {} {} \
                   <span class=\"note\">[{}]</span></li>",
                escape(mark),
                escape(&entry.name),
                escape(&entry.detail),
                entry.distance
            ));
        }
        html.push_str("</ul>");
    }
    html.push_str(&format!("<h3>{}</h3><ul>", escape(&labels.pack)));
    for item in &run.inventory {
        // The description rides as a tooltip: hovering a pack line is the
        // cheapest way to ask what a thing is for.
        html.push_str(&format!(
            "<li title=\"{}\">{}</li>",
            escape(&item.description),
            escape(&item.label)
        ));
    }
    html.push_str("</ul>");
    // The map key: what each glyph means, so the canvas is not the only place
    // that knowledge lives and a player never has to guess a character.
    if !run.legend.is_empty() {
        html.push_str(&format!(
            "<h3>{}</h3><ul class=\"legend\">",
            escape(&labels.legend)
        ));
        for entry in &run.legend {
            html.push_str(&format!(
                "<li><span class=\"lglyph\">{}</span> {}</li>",
                escape(&entry.glyph.to_string()),
                escape(&entry.meaning)
            ));
        }
        html.push_str("</ul>");
    }
    html
}

fn actions_html(run: &RunView, labels: &PanelLabels) -> String {
    // Real buttons, not clickable list items: an action is a thing you do, and
    // a screen reader should announce it as a button and let a keyboard reach
    // it. `aria-disabled` rather than the `disabled` attribute keeps a blocked
    // row focusable, so a player can land on it and hear *why* it is blocked —
    // which is the whole reason a disabled action carries a note.
    let mut html = format!(
        "<h3 id=\"actions-heading\">{}</h3>\
         <ul class=\"actionlist\" role=\"group\" aria-labelledby=\"actions-heading\">",
        escape(&labels.actions)
    );
    for (index, action) in run.actions.iter().enumerate() {
        let disabled = if action.enabled { "" } else { " disabled" };
        let aria_disabled = if action.enabled { "false" } else { "true" };
        let note = match &action.note {
            Some(note) => format!("<span class=\"note\"> ({})</span>", escape(note)),
            None => String::new(),
        };
        html.push_str(&format!(
            "<li><button type=\"button\" class=\"action{disabled}\" data-action=\"{index}\" \
               aria-disabled=\"{aria_disabled}\">\
               <span class=\"akey\">{}</span> {}{}</button></li>",
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
    // The status and hint are what a sighted player's eye is drawn to, so they
    // get a live region and are announced when they change. The log above is
    // not live: it is rebuilt whole each frame, and announcing the whole tail
    // every time would be a wall of noise.
    if !status.is_empty() {
        html.push_str(&format!(
            "<div class=\"status\" role=\"status\" aria-live=\"polite\">{}</div>",
            escape(status)
        ));
    }
    if let Some(hint) = &run.hint {
        html.push_str(&format!(
            "<div class=\"hint\" role=\"status\" aria-live=\"polite\">{}</div>",
            escape(hint)
        ));
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
    // A listbox: the highlight is a selection, announced with aria-selected so
    // a screen-reader user knows which row confirming will take, not just which
    // one is a different colour.
    html.push_str("<ul class=\"menu\" role=\"listbox\" tabindex=\"0\">");
    for (index, (label, blocked)) in overlay.items.iter().enumerate() {
        let selected = if index == overlay.selected {
            " selected"
        } else {
            ""
        };
        let aria = if index == overlay.selected {
            "true"
        } else {
            "false"
        };
        match blocked {
            None => html.push_str(&format!(
                "<li class=\"item{selected}\" role=\"option\" aria-selected=\"{aria}\" \
                   data-choice=\"{index}\">{}</li>",
                escape(label)
            )),
            Some(reason) => html.push_str(&format!(
                "<li class=\"item blocked{selected}\" role=\"option\" aria-selected=\"{aria}\" \
                   data-choice=\"{index}\">{} — {}</li>",
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
                "<h1>Case Report</h1><p class=\"outcome\">{}</p><p class=\"villain\">{}</p><p>{}</p><p>{}</p><p class=\"tier\">{}</p>",
                escape(&report.outcome),
                escape(&report.villain),
                escape(&report.origin),
                escape(&report.scheme),
                escape(&report.tier)
            );
            html.push_str(&format!("<h3>{}</h3><ul>", escape(&labels.preparations)));
            for note in &report.preparations {
                html.push_str(&format!("<li>{}</li>", escape(note)));
            }
            html.push_str("</ul>");
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse "#rrggbb" into linearised channels for a WCAG luminance sum.
    fn luminance(hex: &str) -> f64 {
        let channel = |s: &str| {
            let v = u8::from_str_radix(s, 16).unwrap() as f64 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let r = channel(&hex[1..3]);
        let g = channel(&hex[3..5]);
        let b = channel(&hex[5..7]);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn contrast(a: &str, b: &str) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn every_map_colour_is_legible_on_the_canvas() {
        // The glyphs are drawn on the canvas fill; checked against a stated
        // standard rather than by eye. They are 18px bold, so WCAG's 3:1 for
        // large text is the bar. Unseen is exempt: it is meant to be invisible,
        // that being what "you cannot see this tile" looks like.
        const CANVAS_BG: &str = "#0a0a08";
        const LARGE_TEXT_MIN: f64 = 3.0;
        let colours = [
            CellColor::Hunter,
            CellColor::Npc,
            CellColor::Enemy,
            CellColor::Villain,
            CellColor::VillainVulnerable,
            CellColor::Terrain,
            CellColor::TerrainDim,
            CellColor::Feature,
            CellColor::Opportunity,
            CellColor::Exit,
            CellColor::Snare,
        ];
        for colour in colours {
            let ratio = contrast(css_color(colour), CANVAS_BG);
            assert!(
                ratio >= LARGE_TEXT_MIN,
                "{colour:?} at {} has contrast {ratio:.2} on the canvas, below {LARGE_TEXT_MIN}",
                css_color(colour)
            );
        }
    }
}
