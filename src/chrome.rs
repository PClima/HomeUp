//! Window chrome in the SketchUp-for-Web style: icon toolbar with flyouts, the
//! "…" tool palette, tool search, right panel strip and panel tray.

use crate::app::{HomeUpApp, Tool, TOOL_GROUPS};
use crate::icons::{self, Icon};
use eframe::egui::{self, Color32, Id, Order, Pos2, Rect, RichText, Stroke, Vec2};

/// Toolbar slots; slots with more than one tool open a flyout.
pub const SLOTS: &[&[Tool]] = &[
    &[Tool::Select, Tool::Lasso],
    &[Tool::Eraser],
    &[Tool::Line, Tool::Freehand],
    &[Tool::Rect, Tool::RotRect, Tool::Circle, Tool::Polygon],
    &[Tool::Arc, Tool::ArcCenter, Tool::Arc3, Tool::Pie],
    &[Tool::PushPull, Tool::FollowMe],
    &[Tool::Offset],
    &[Tool::Move],
    &[Tool::Rotate, Tool::Flip],
    &[Tool::Scale],
    &[Tool::Tape, Tool::Dimension, Tool::Protractor, Tool::Text, Tool::Text3d, Tool::Axes, Tool::Section],
    &[Tool::Paint, Tool::Sample],
    &[Tool::Orbit],
    &[Tool::Pan],
    &[Tool::Zoom, Tool::ZoomWindow],
];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PanelKind {
    Hatch,
    MatchPhoto,
    Outliner,
    Environments,
    Entity,
    Components,
    Library,
    Instructor,
    Materials,
    Styles,
    Tags,
    Shadows,
    Scenes,
    Display,
    Soften,
    ModelInfo,
}

impl PanelKind {
    pub const ALL: [PanelKind; 16] = [
        PanelKind::MatchPhoto,
        PanelKind::Hatch,
        PanelKind::Entity,
        PanelKind::Outliner,
        PanelKind::Components,
        PanelKind::Library,
        PanelKind::Instructor,
        PanelKind::Materials,
        PanelKind::Styles,
        PanelKind::Tags,
        PanelKind::Shadows,
        PanelKind::Environments,
        PanelKind::Scenes,
        PanelKind::Display,
        PanelKind::Soften,
        PanelKind::ModelInfo,
    ];
    pub fn title(self) -> &'static str {
        match self {
            PanelKind::Entity => "Informações da entidade",
            PanelKind::Outliner => "Outliner",
            PanelKind::MatchPhoto => "Combinar foto",
            PanelKind::Hatch => "Hachuras",
            PanelKind::Environments => "Ambientes",
            PanelKind::Components => "Componentes",
            PanelKind::Materials => "Materiais",
            PanelKind::Scenes => "Cenas",
            PanelKind::Instructor => "Instrutor",
            PanelKind::Library => "Biblioteca",
            PanelKind::Styles => "Estilos",
            PanelKind::Tags => "Etiquetas",
            PanelKind::Shadows => "Sombras",
            PanelKind::Display => "Exibição",
            PanelKind::Soften => "Atenuar/Suavizar",
            PanelKind::ModelInfo => "Informações do modelo",
        }
    }
    pub fn icon(self) -> Icon {
        match self {
            PanelKind::Entity => Icon::EntityInfo,
            PanelKind::Outliner => Icon::Outliner,
            PanelKind::MatchPhoto => Icon::Ui(crate::icons2::UiIcon::Photo),
            PanelKind::Hatch => Icon::Ui(crate::icons2::UiIcon::SectionFill),
            PanelKind::Environments => Icon::Environments,
            PanelKind::Components => Icon::Components,
            PanelKind::Materials => Icon::Materials,
            PanelKind::Scenes => Icon::Scenes,
            PanelKind::Instructor => Icon::Instructor,
            PanelKind::Library => Icon::Library,
            PanelKind::Styles => Icon::Styles,
            PanelKind::Tags => Icon::Tags,
            PanelKind::Shadows => Icon::Shadows,
            PanelKind::Display => Icon::Display,
            PanelKind::Soften => Icon::Soften,
            PanelKind::ModelInfo => Icon::ModelInfo,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cmd {
    Tool(Tool),
    ZoomExtents,
    Group,
    Component,
    Ortho(bool),
    Import,
    ExportObj,
    AddScene,
    Undo,
    Redo,
    Panel(PanelKind),
}

pub const SEARCH_ID: &str = "tool-search";

/// Lowercase, accent-free text for searching.
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'ê' | 'è' | 'ë' => 'e',
            'í' | 'î' | 'ì' | 'ï' => 'i',
            'ó' | 'ô' | 'õ' | 'ò' | 'ö' => 'o',
            'ú' | 'û' | 'ù' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

pub fn commands() -> Vec<(String, Cmd)> {
    let mut v: Vec<(String, Cmd)> = TOOL_GROUPS.iter().flat_map(|g| g.iter()).map(|t| (t.label().to_string(), Cmd::Tool(*t))).collect();
    v.extend([
        ("Zoom Extents".to_string(), Cmd::ZoomExtents),
        ("Criar grupo".into(), Cmd::Group),
        ("Criar componente".into(), Cmd::Component),
        ("Projeção paralela".into(), Cmd::Ortho(true)),
        ("Perspectiva".into(), Cmd::Ortho(false)),
        ("Importar modelo".into(), Cmd::Import),
        ("Exportar OBJ".into(), Cmd::ExportObj),
        ("Adicionar cena".into(), Cmd::AddScene),
        ("Desfazer".into(), Cmd::Undo),
        ("Refazer".into(), Cmd::Redo),
    ]);
    for p in PanelKind::ALL {
        v.push((format!("Painel: {}", p.title()), Cmd::Panel(p)));
    }
    v
}

pub fn search_matches(q: &str) -> Vec<(String, Cmd)> {
    let q = fold(q.trim());
    if q.is_empty() {
        return vec![];
    }
    let mut out: Vec<(usize, String, Cmd)> = commands()
        .into_iter()
        .filter_map(|(name, c)| {
            let f = fold(&name);
            f.find(&q).map(|pos| (pos, name, c))
        })
        .collect();
    out.sort_by_key(|x| x.0);
    out.into_iter().map(|(_, n, c)| (n, c)).collect()
}

fn corner_triangle(p: &egui::Painter, r: Rect) {
    let b = r.right_bottom() - Vec2::new(3.0, 3.0);
    p.add(egui::Shape::convex_polygon(vec![b, b - Vec2::new(6.0, 0.0), b - Vec2::new(0.0, 6.0)], Color32::from_gray(90), Stroke::NONE));
}

impl HomeUpApp {
    pub(crate) fn run_cmd(&mut self, c: Cmd) {
        match c {
            Cmd::Tool(t) => self.set_tool(t),
            Cmd::ZoomExtents => self.zoom_extents(),
            Cmd::Group => self.make_group(true),
            Cmd::Component => self.make_group(false),
            Cmd::Ortho(o) => self.cam.ortho = o,
            Cmd::Import => self.import_file(),
            Cmd::ExportObj => self.export_obj(),
            Cmd::AddScene => self.add_scene(),
            Cmd::Undo => self.do_undo(),
            Cmd::Redo => self.do_redo(),
            Cmd::Panel(p) => self.open_panel = Some(p),
        }
    }

    /// Left icon toolbar with flyouts and the "…" palette.
    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let size = 40.0;
        ui.add_space(4.0);
        if icons::button(ui, Icon::Search, false, size).on_hover_text("Buscar ferramenta (Ctrl+F)").clicked() {
            ctx.memory_mut(|m| m.request_focus(Id::unique(SEARCH_ID)));
        }
        ui.separator();
        let mut flyout_anchor: Option<(usize, Rect)> = None;
        for (si, slot) in SLOTS.iter().enumerate() {
            if self.prefs.hidden_slots.contains(&si) && !slot.contains(&self.tool) {
                continue;
            }
            let active = slot.contains(&self.tool);
            let shown = if active { self.tool } else { slot[self.slot_sel[si].min(slot.len() - 1)] };
            let r = icons::button(ui, Icon::Tool(shown), active, size);
            if slot.len() > 1 {
                corner_triangle(ui.painter(), r.rect);
            }
            let in_corner = r.interact_pointer_pos().is_some_and(|p| p.x > r.rect.right() - 12.0 && p.y > r.rect.bottom() - 12.0);
            let r = r.on_hover_text(if slot.len() > 1 {
                format!("{} ({}) — clique no canto ou com o botão direito para mais", shown.label(), shown.key())
            } else {
                format!("{} ({})", shown.label(), shown.key())
            });
            if r.clicked() {
                if slot.len() > 1 && in_corner {
                    self.flyout = if self.flyout == Some(si) { None } else { Some(si) };
                } else {
                    self.set_tool(shown);
                    self.flyout = None;
                }
            }
            if r.secondary_clicked() && slot.len() > 1 {
                self.flyout = Some(si);
            }
            if self.flyout == Some(si) {
                flyout_anchor = Some((si, r.rect));
            }
        }
        ui.separator();
        if icons::button(ui, Icon::ZoomExtents, false, size).on_hover_text("Zoom Extents (Shift+Z)").clicked() {
            self.zoom_extents();
        }
        let more = icons::button(ui, Icon::More, self.show_more, size).on_hover_text("Mais ferramentas");
        if more.clicked() {
            self.show_more = !self.show_more;
            self.flyout = None;
        }
        let more_rect = more.rect;

        // flyout
        if let Some((si, rect)) = flyout_anchor {
            let area = egui::Area::new(Id::unique(("flyout", si))).order(Order::Foreground).fixed_pos(rect.right_top() + Vec2::new(6.0, 0.0)).show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    for (k, t) in SLOTS[si].iter().enumerate() {
                        ui.horizontal(|ui| {
                            let r = icons::button(ui, Icon::Tool(*t), self.tool == *t, 32.0);
                            let l = ui.add(egui::Label::new(format!("{}  ({})", t.label(), t.key())).sense(egui::Sense::click()));
                            if r.clicked() || l.clicked() {
                                self.slot_sel[si] = k;
                                self.set_tool(*t);
                                self.flyout = None;
                            }
                        });
                    }
                });
            });
            let clicked_out = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !rect.contains(p)));
            if clicked_out {
                self.flyout = None;
            }
        }

        // "…" palette
        if self.show_more {
            let area = egui::Area::new(Id::unique("more-tools")).order(Order::Foreground).fixed_pos(Pos2::new(more_rect.right() + 6.0, 60.0)).show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(360.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Ferramentas").strong());
                        if ui.small_button(if self.edit_toolbar { "Concluir" } else { "Editar barra" }).clicked() {
                            self.edit_toolbar = !self.edit_toolbar;
                        }
                    });
                    if self.edit_toolbar {
                        ui.weak("Escolha o que aparece na barra lateral:");
                        for (si, slot) in SLOTS.iter().enumerate() {
                            let mut on = !self.prefs.hidden_slots.contains(&si);
                            let names: Vec<&str> = slot.iter().map(|t| t.label()).collect();
                            if ui.checkbox(&mut on, names.join(" / ")).changed() {
                                if on {
                                    self.prefs.hidden_slots.retain(|s| *s != si);
                                } else {
                                    self.prefs.hidden_slots.push(si);
                                }
                                self.prefs.save();
                            }
                        }
                        ui.separator();
                    }
                    for (gi, group) in TOOL_GROUPS.iter().enumerate() {
                        let title = ["Seleção e pintura", "Desenho", "Modificar", "Anotação e construção", "Sólidos", "Câmera"][gi];
                        ui.label(egui::RichText::new(title).small().color(Color32::from_gray(110)));
                        egui::Grid::new(("more", gi)).spacing([6.0, 2.0]).show(ui, |ui| {
                            for (k, t) in group.iter().enumerate() {
                                ui.vertical_centered(|ui| {
                                    ui.set_width(58.0);
                                    let r = icons::button(ui, Icon::Tool(*t), self.tool == *t, 34.0);
                                    ui.label(egui::RichText::new(t.label()).small());
                                    if r.on_hover_text(format!("{} ({})", t.label(), t.key())).clicked() {
                                        self.set_tool(*t);
                                        self.show_more = false;
                                    }
                                });
                                if k % 5 == 4 {
                                    ui.end_row();
                                }
                            }
                        });
                        ui.separator();
                    }
                });
            });
            let clicked_out = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !more_rect.contains(p)));
            if clicked_out {
                self.show_more = false;
            }
        }
    }

    /// Undo/redo, save state and tool search, shown in the top bar.
    pub(crate) fn top_controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        if icons::button(ui, Icon::Undo, false, 22.0).on_hover_text("Desfazer (Ctrl+Z)").clicked() {
            self.do_undo();
        }
        if icons::button(ui, Icon::Redo, false, 22.0).on_hover_text("Refazer (Ctrl+Y)").clicked() {
            self.do_redo();
        }
        ui.label(egui::RichText::new(if self.modified { "Não salvo" } else { "Salvo" }).color(Color32::from_gray(120)));
        ui.separator();
        let id = Id::unique(SEARCH_ID);
        let resp = ui.add(egui::TextEdit::singleline(&mut self.search).id(id).hint_text("Buscar ferramenta (Ctrl+F)").desired_width(200.0));
        let matches = search_matches(&self.search);
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) && resp.has_focus() {
            self.search.clear();
        }
        if enter {
            if let Some((_, c)) = matches.first().cloned() {
                self.search.clear();
                self.run_cmd(c);
            }
        }
        if !self.search.is_empty() && !matches.is_empty() {
            let mut pick = None;
            egui::Area::new(Id::unique("search-results")).order(Order::Foreground).fixed_pos(resp.rect.left_bottom() + Vec2::new(0.0, 4.0)).show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(220.0);
                    for (name, c) in matches.iter().take(10) {
                        ui.horizontal(|ui| {
                            if let Cmd::Tool(t) = c {
                                icons::paint(ui.painter(), Rect::from_min_size(ui.cursor().min, Vec2::splat(18.0)), Icon::Tool(*t));
                            }
                            ui.add_space(22.0);
                            if ui.selectable_label(false, name).clicked() {
                                pick = Some(*c);
                            }
                        });
                    }
                });
            });
            if let Some(c) = pick {
                self.search.clear();
                self.run_cmd(c);
            }
        }
    }

    /// Right strip of panel icons.
    pub(crate) fn panel_strip(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        for p in PanelKind::ALL {
            let r = icons::button(ui, p.icon(), self.open_panel == Some(p), 36.0).on_hover_text(p.title());
            if r.clicked() {
                self.open_panel = if self.open_panel == Some(p) { None } else { Some(p) };
            }
        }
    }

    /// The open panel, with its title bar.
    pub(crate) fn panel_tray(&mut self, ui: &mut egui::Ui, now: f64) {
        let Some(p) = self.open_panel else { return };
        ui.horizontal(|ui| {
            ui.heading(egui::RichText::new(p.title()).size(17.0).color(Color32::from_rgb(20, 60, 110)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text("Fechar painel").clicked() {
                    self.open_panel = None;
                }
            });
        });
        ui.separator();
        let w = ui.available_width();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(w);
            self.panel_body(ui, p, now);
        });
    }

    /// Contents of a panel (shared by the single tray and the stacked tray).
    pub(crate) fn panel_body(&mut self, ui: &mut egui::Ui, p: PanelKind, now: f64) {
        {
            match p {
            PanelKind::Entity => self.entity_info(ui),
            PanelKind::Outliner => self.outliner_ui(ui),
            PanelKind::MatchPhoto => self.match_photo_ui(ui),
            PanelKind::Hatch => self.hatch_ui(ui),
            PanelKind::Environments => self.environments_ui(ui),
            PanelKind::Components => self.components_ui(ui),
            PanelKind::Materials => self.materials_ui(ui),
            PanelKind::Scenes => self.scenes_ui(ui, now),
            PanelKind::Instructor => self.instructor_ui(ui),
            PanelKind::Library => self.library_ui(ui),
            PanelKind::Styles => self.styles_ui(ui),
            PanelKind::Tags => self.tags_ui(ui),
            PanelKind::Shadows => self.shadows_ui(ui),
            PanelKind::Display => self.display_ui(ui),
            PanelKind::Soften => self.soften_ui(ui),
            PanelKind::ModelInfo => self.model_info_ui(ui),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_is_accent_insensitive() {
        let m = search_matches("retang");
        assert_eq!(m.first().map(|x| x.1), Some(Cmd::Tool(Tool::Rect)));
        let m = search_matches("PROJECAO");
        assert_eq!(m.first().map(|x| x.1), Some(Cmd::Ortho(true)));
        assert!(search_matches("").is_empty());
    }
}
