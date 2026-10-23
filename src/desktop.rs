//! SketchUp 2026 desktop layout: toolbars docked on top (Standard, Principal,
//! Styles, Shadows, Camera, Views, Section, Solid Tools), the two-column Large
//! Tool Set on the left, scene tabs above the drawing area, the stacked
//! Default Tray on the right and the status bar with the Measurements box.

use crate::app::{HomeUpApp, Tool};
use crate::chrome::PanelKind;
use crate::icons::{self, Icon};
use crate::icons2::UiIcon;
use crate::settings::FaceStyle;
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    New,
    Open,
    Save,
    Cut,
    Copy,
    Paste,
    Delete,
    Undo,
    Redo,
    Print,
    ModelInfo,
    MakeComponent,
    ZoomExtents,
    PrevView,
    View(f64, f64),
    Face(FaceStyle),
    XRay,
    Edges,
    BackEdges,
    Ao,
    SecPlanes,
    SecCuts,
    SecFill,
    Shadows,
    Contours,
    Sandbox(crate::sandbox::SandboxTool),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tb {
    T(Tool),
    A(Act),
}

use Act::*;
use Tb::{A, T};

pub const TOOLBARS: [(&str, &[Tb]); 9] = [
    ("Padrão", &[A(New), A(Open), A(Save), A(Cut), A(Copy), A(Paste), A(Delete), A(Undo), A(Redo), A(Print), A(ModelInfo)]),
    ("Principal", &[T(Tool::Select), A(MakeComponent), T(Tool::Paint), T(Tool::Eraser)]),
    (
        "Estilos",
        &[
            A(XRay),
            A(BackEdges),
            A(Edges),
            A(Ao),
            A(Face(FaceStyle::Wireframe)),
            A(Face(FaceStyle::HiddenLine)),
            A(Face(FaceStyle::Monochrome)),
            A(Face(FaceStyle::Shaded)),
            A(Face(FaceStyle::Photoreal)),
        ],
    ),
    ("Sombras", &[A(Shadows)]),
    (
        "Câmera",
        &[
            T(Tool::Orbit),
            T(Tool::Pan),
            T(Tool::Zoom),
            T(Tool::ZoomWindow),
            A(ZoomExtents),
            A(PrevView),
            T(Tool::PositionCamera),
            T(Tool::LookAround),
            T(Tool::Walk),
        ],
    ),
    (
        "Vistas",
        &[A(View(-45.0, 35.26)), A(View(-90.0, 89.9)), A(View(-90.0, 0.0)), A(View(0.0, 0.0)), A(View(180.0, 0.0)), A(View(90.0, 0.0)), A(View(-90.0, -89.9))],
    ),
    ("Seção", &[T(Tool::Section), A(SecPlanes), A(SecCuts), A(SecFill)]),
    ("Ferramentas de sólidos", &[T(Tool::OuterShell), T(Tool::SolidIntersect), T(Tool::Union), T(Tool::Subtract), T(Tool::Trim), T(Tool::Split)]),
    (
        "Sandbox",
        &[
            A(Contours),
            A(Sandbox(crate::sandbox::SandboxTool::FromScratch)),
            A(Sandbox(crate::sandbox::SandboxTool::Smoove)),
            A(Sandbox(crate::sandbox::SandboxTool::Stamp)),
            A(Sandbox(crate::sandbox::SandboxTool::Drape)),
            A(Sandbox(crate::sandbox::SandboxTool::AddDetail)),
            A(Sandbox(crate::sandbox::SandboxTool::FlipEdge)),
        ],
    ),
];

/// Large Tool Set, two columns per row; `None` rows are separators.
const LTS: [Option<[Tb; 2]>; 24] = [
    Some([T(Tool::Select), A(MakeComponent)]),
    Some([T(Tool::Paint), T(Tool::Eraser)]),
    None,
    Some([T(Tool::Line), T(Tool::Freehand)]),
    Some([T(Tool::Rect), T(Tool::RotRect)]),
    Some([T(Tool::Circle), T(Tool::Polygon)]),
    Some([T(Tool::Arc), T(Tool::Arc3)]),
    Some([T(Tool::ArcCenter), T(Tool::Pie)]),
    None,
    Some([T(Tool::Move), T(Tool::PushPull)]),
    Some([T(Tool::Rotate), T(Tool::FollowMe)]),
    Some([T(Tool::Scale), T(Tool::Offset)]),
    Some([T(Tool::Flip), T(Tool::Lasso)]),
    None,
    Some([T(Tool::Tape), T(Tool::Dimension)]),
    Some([T(Tool::Protractor), T(Tool::Text)]),
    Some([T(Tool::Axes), T(Tool::Text3d)]),
    None,
    Some([T(Tool::Orbit), T(Tool::Pan)]),
    Some([T(Tool::Zoom), T(Tool::ZoomWindow)]),
    Some([A(ZoomExtents), A(PrevView)]),
    Some([T(Tool::PositionCamera), T(Tool::LookAround)]),
    Some([T(Tool::Walk), T(Tool::Section)]),
    None,
];

/// Panels in the Default Tray, top to bottom.
pub const TRAY_ORDER: [PanelKind; 16] = [
    PanelKind::Entity,
    PanelKind::Materials,
    PanelKind::Components,
    PanelKind::Styles,
    PanelKind::Tags,
    PanelKind::Shadows,
    PanelKind::Environments,
    PanelKind::Scenes,
    PanelKind::Outliner,
    PanelKind::MatchPhoto,
    PanelKind::Hatch,
    PanelKind::Soften,
    PanelKind::Library,
    PanelKind::Instructor,
    PanelKind::Display,
    PanelKind::ModelInfo,
];

fn view_icon(yaw: f64, pitch: f64) -> (UiIcon, &'static str) {
    if pitch > 80.0 {
        (UiIcon::ViewTop, "Topo")
    } else if pitch < -80.0 {
        (UiIcon::ViewBottom, "Baixo")
    } else if pitch > 1.0 {
        (UiIcon::ViewIso, "Iso")
    } else if yaw == -90.0 {
        (UiIcon::ViewFront, "Frente")
    } else if yaw == 0.0 {
        (UiIcon::ViewRight, "Direita")
    } else if yaw == 180.0 {
        (UiIcon::ViewLeft, "Esquerda")
    } else {
        (UiIcon::ViewBack, "Trás")
    }
}

impl Act {
    fn icon(self) -> Icon {
        match self {
            New => Icon::Ui(UiIcon::New),
            Open => Icon::Ui(UiIcon::Open),
            Save => Icon::Ui(UiIcon::Save),
            Cut => Icon::Ui(UiIcon::Cut),
            Copy => Icon::Ui(UiIcon::Copy),
            Paste => Icon::Ui(UiIcon::Paste),
            Delete => Icon::Ui(UiIcon::Delete),
            Undo => Icon::Undo,
            Redo => Icon::Redo,
            Print => Icon::Ui(UiIcon::Print),
            ModelInfo => Icon::ModelInfo,
            MakeComponent => Icon::Ui(UiIcon::MakeComponent),
            ZoomExtents => Icon::ZoomExtents,
            PrevView => Icon::Ui(UiIcon::PrevView),
            View(y, p) => Icon::Ui(view_icon(y, p).0),
            Face(f) => Icon::Ui(match f {
                FaceStyle::Wireframe => UiIcon::Wireframe,
                FaceStyle::HiddenLine => UiIcon::HiddenLine,
                FaceStyle::Monochrome => UiIcon::Mono,
                FaceStyle::Shaded => UiIcon::Textured,
                FaceStyle::Photoreal => UiIcon::Pbr,
            }),
            XRay => Icon::Ui(UiIcon::XRay),
            Edges => Icon::Ui(UiIcon::Edges),
            BackEdges => Icon::Ui(UiIcon::BackEdges),
            Ao => Icon::Ui(UiIcon::Ao),
            SecPlanes => Icon::Ui(UiIcon::SectionPlanes),
            SecCuts => Icon::Ui(UiIcon::SectionCuts),
            SecFill => Icon::Ui(UiIcon::SectionFill),
            Shadows => Icon::Shadows,
            Contours => Icon::Ui(UiIcon::SbContours),
            Sandbox(t) => Icon::Ui(match t {
                crate::sandbox::SandboxTool::FromScratch => UiIcon::SbScratch,
                crate::sandbox::SandboxTool::Smoove => UiIcon::SbSmoove,
                crate::sandbox::SandboxTool::Stamp => UiIcon::SbStamp,
                crate::sandbox::SandboxTool::Drape => UiIcon::SbDrape,
                crate::sandbox::SandboxTool::AddDetail => UiIcon::SbDetail,
                crate::sandbox::SandboxTool::FlipEdge => UiIcon::SbFlip,
            }),
        }
    }
    fn tip(self) -> String {
        match self {
            New => "Novo (Ctrl+N)".into(),
            Open => "Abrir (Ctrl+O)".into(),
            Save => "Salvar (Ctrl+S)".into(),
            Cut => "Recortar (Ctrl+X)".into(),
            Copy => "Copiar (Ctrl+C)".into(),
            Paste => "Colar (Ctrl+V)".into(),
            Delete => "Apagar (Del)".into(),
            Undo => "Desfazer (Ctrl+Z)".into(),
            Redo => "Refazer (Ctrl+Y)".into(),
            Print => "Imprimir".into(),
            ModelInfo => "Informações do modelo".into(),
            MakeComponent => "Criar componente (G)".into(),
            ZoomExtents => "Zoom Extents (Shift+Z)".into(),
            PrevView => "Vista anterior".into(),
            View(y, p) => format!("Vista: {}", view_icon(y, p).1),
            Face(f) => f.label().into(),
            XRay => "Raio-X".into(),
            Edges => "Arestas".into(),
            BackEdges => "Arestas traseiras".into(),
            Ao => "Oclusão de ambiente".into(),
            SecPlanes => "Exibir planos de seção".into(),
            SecCuts => "Exibir cortes de seção".into(),
            SecFill => "Exibir preenchimento de seção".into(),
            Shadows => "Exibir sombras".into(),
            Contours => "Sandbox: a partir de contornos (selecione as curvas de nível)".into(),
            Sandbox(t) => format!("Sandbox: {}", t.label()),
        }
    }
}

fn toolbar_frame(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(Color32::from_rgb(246, 247, 249))
        .stroke(Stroke::new(1.0, Color32::from_gray(214)))
        .corner_radius(3.0)
        .inner_margin(egui::Margin { left: 2, right: 3, top: 1, bottom: 1 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                // grip
                let (r, _) = ui.allocate_exact_size(Vec2::new(5.0, 24.0), egui::Sense::hover());
                for k in 0..5 {
                    ui.painter().circle_filled(egui::pos2(r.center().x, r.min.y + 4.0 + k as f32 * 4.0), 0.9, Color32::from_gray(150));
                }
                add(ui);
            });
        });
}

impl HomeUpApp {
    fn act_active(&self, a: Act) -> bool {
        let st = &self.doc.style;
        match a {
            Face(f) => st.face == f,
            XRay => st.xray,
            Edges => st.edges,
            BackEdges => st.back_edges,
            Ao => st.ao,
            SecPlanes => self.display.section_planes,
            SecCuts => self.display.section_cuts,
            SecFill => self.display.section_fill,
            Shadows => self.doc.shadows.on,
            Sandbox(t) => self.sandbox.as_ref().is_some_and(|s| s.0 == t),
            _ => false,
        }
    }

    fn style_edit(&mut self, f: impl FnOnce(&mut crate::settings::Style)) {
        let mut st = self.doc.style.clone();
        f(&mut st);
        if st != self.doc.style {
            if !st.name.ends_with('*') {
                st.name.push('*');
            }
            self.doc.style = st;
            self.modified = true;
            self.dirty = true;
        }
    }

    pub(crate) fn run_act(&mut self, ctx: &egui::Context, a: Act, now: f64) {
        match a {
            New => self.new_file(),
            Open => self.open_file(),
            Save => self.save_file(false),
            Cut => self.cut_selection(),
            Copy => {
                self.copy_selection();
            }
            Paste => self.paste(),
            Delete => self.delete_selection(),
            Undo => self.do_undo(),
            Redo => self.do_redo(),
            Print => self.export_png(ctx, true),
            ModelInfo => self.open_panel = Some(PanelKind::ModelInfo),
            MakeComponent => self.make_group(false),
            ZoomExtents => self.zoom_extents(),
            PrevView => self.previous_view(now),
            View(y, p) => self.set_view(y, p),
            Face(f) => self.style_edit(|s| s.face = f),
            XRay => self.style_edit(|s| s.xray = !s.xray),
            Edges => self.style_edit(|s| s.edges = !s.edges),
            BackEdges => self.style_edit(|s| s.back_edges = !s.back_edges),
            Ao => self.style_edit(|s| s.ao = !s.ao),
            SecPlanes => self.display.section_planes = !self.display.section_planes,
            SecCuts => {
                self.display.section_cuts = !self.display.section_cuts;
                self.dirty = true;
            }
            SecFill => {
                self.display.section_fill = !self.display.section_fill;
                self.dirty = true;
            }
            Shadows => {
                self.doc.shadows.on = !self.doc.shadows.on;
                self.modified = true;
                self.dirty = true;
            }
            Contours => self.tins_from_contours(),
            Sandbox(t) => self.start_sandbox(t),
        }
    }

    fn tb_button(&mut self, ui: &mut egui::Ui, b: Tb, size: f32, now: f64) {
        let ctx = ui.ctx().clone();
        match b {
            T(t) => {
                let r = icons::button(ui, Icon::Tool(t), self.tool == t, size).on_hover_text(format!("{} ({})", t.label(), t.key()));
                if r.clicked() {
                    self.set_tool(t);
                }
            }
            A(a) => {
                let enabled = match a {
                    Undo => !self.undo.is_empty(),
                    Redo => !self.redo.is_empty(),
                    Paste => self.clipboard.is_some(),
                    Cut | Copy | Delete => !self.sel.is_empty(),
                    _ => true,
                };
                let r = ui.add_enabled_ui(enabled, |ui| icons::button(ui, a.icon(), self.act_active(a), size)).inner.on_hover_text(a.tip());
                if r.clicked() && enabled {
                    self.run_act(&ctx, a, now);
                }
            }
        }
    }

    /// The docked toolbars (two rows, wrapping when the window is narrow).
    pub(crate) fn toolbars(&mut self, ui: &mut egui::Ui, now: f64) {
        let size = 26.0;
        for row in [&TOOLBARS[..4], &TOOLBARS[4..]] {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
                for (name, items) in row.iter() {
                    if self.prefs.hidden_toolbars.iter().any(|h| h == name) {
                        continue;
                    }
                    toolbar_frame(ui, |ui| {
                        for b in items.iter() {
                            self.tb_button(ui, *b, size, now);
                        }
                        if *name == "Sombras" {
                            self.shadow_sliders(ui);
                        }
                    });
                }
            });
        }
    }

    /// Month and time sliders of the Shadows toolbar.
    fn shadow_sliders(&mut self, ui: &mut egui::Ui) {
        const MONTHS: [&str; 12] = ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"];
        let s = &mut self.doc.shadows;
        let mut changed = false;
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.spacing_mut().slider_width = 120.0;
            ui.label(RichText::new(MONTHS.join("   ")).size(8.5).color(Color32::from_gray(100)));
            let mut doy = (s.month - 1) as f32 * 30.4 + s.day as f32;
            if ui.add(egui::Slider::new(&mut doy, 1.0..=365.0).show_value(false)).on_hover_text(format!("Data: {:02}/{:02}", s.day, s.month)).changed() {
                let m = ((doy - 1.0) / 30.4).floor().clamp(0.0, 11.0) as u32;
                s.month = m + 1;
                s.day = ((doy - m as f32 * 30.4).round() as u32).clamp(1, 31);
                changed = true;
            }
        });
        ui.add_space(10.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.spacing_mut().slider_width = 120.0;
            ui.label(RichText::new(format!("{:02}:{:02}", s.minutes / 60, s.minutes % 60)).size(9.0).color(Color32::from_gray(80)));
            let mut minutes = s.minutes as f32;
            if ui.add(egui::Slider::new(&mut minutes, 270.0..=1190.0).show_value(false)).on_hover_text("Hora").changed() {
                s.minutes = minutes as u32;
                changed = true;
            }
        });
        ui.add_space(2.0);
        if changed {
            self.modified = true;
        }
    }

    /// Two-column Large Tool Set docked on the left.
    pub(crate) fn large_tool_set(&mut self, ui: &mut egui::Ui, now: f64) {
        let size = 30.0;
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for row in LTS.iter() {
                match row {
                    Some([a, b]) => {
                        ui.horizontal(|ui| {
                            self.tb_button(ui, *a, size, now);
                            self.tb_button(ui, *b, size, now);
                        });
                    }
                    None => {
                        ui.add_space(2.0);
                        ui.separator();
                    }
                }
            }
        });
    }

    /// Scene tabs above the drawing area, with hover previews.
    pub(crate) fn scene_tabs(&mut self, ui: &mut egui::Ui, now: f64) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let mut go = None;
            for k in 0..self.doc.scenes.len() {
                let name = self.doc.scenes[k].name.clone();
                let sel = self.scene_sel == k;
                let text = if sel { RichText::new(&name).strong() } else { RichText::new(&name) };
                let r = ui.add(egui::Button::selectable(sel, text).corner_radius(egui::CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }));
                let r = r.on_hover_ui(|ui| {
                    if let Some(id) = self.scene_thumb(&ctx, k) {
                        ui.image((id, Vec2::new(160.0, 90.0)));
                    }
                    ui.label(&name);
                });
                if r.clicked() {
                    self.scene_sel = k;
                    go = Some(k);
                }
                r.context_menu(|ui| {
                    if ui.button("Atualizar cena").clicked() {
                        self.checkpoint_keep_sel();
                        self.doc.scenes[k].cam = self.cam.to_array();
                        self.doc.scenes[k].ortho = self.cam.ortho;
                        ui.close();
                    }
                    if ui.button("Apagar cena").clicked() {
                        self.checkpoint_keep_sel();
                        self.doc.scenes.remove(k);
                        ui.close();
                    }
                });
                if k >= self.doc.scenes.len() {
                    break;
                }
            }
            if ui.small_button("+").on_hover_text("Adicionar cena").clicked() {
                self.add_scene();
                self.scene_sel = self.doc.scenes.len() - 1;
            }
            if let Some(k) = go {
                self.go_scene(k, now);
            }
        });
    }

    /// The Default Tray: stacked, collapsible panels.
    pub(crate) fn stacked_tray(&mut self, ui: &mut egui::Ui, now: f64) {
        // a panel opened from elsewhere (menus, buttons) expands in the tray
        let mut scroll_to = None;
        if self.open_panel != self.last_open_panel {
            if let Some(p) = self.open_panel {
                self.tray_open.insert(p);
                self.prefs.tray_hidden.retain(|h| h != p.title());
                scroll_to = Some(p);
            }
            self.last_open_panel = self.open_panel;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Bandeja padrão").strong().color(Color32::from_rgb(30, 50, 80)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text("Fechar bandeja (Janela > Bandeja padrão)").clicked() {
                    self.show_tray = false;
                }
            });
        });
        ui.separator();
        let w = ui.available_width();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(w);
            for p in TRAY_ORDER {
                if self.prefs.tray_hidden.iter().any(|h| h == p.title()) {
                    continue;
                }
                let open = self.tray_open.contains(&p);
                let header = egui::Frame::new().fill(Color32::from_rgb(232, 236, 242)).inner_margin(egui::Margin::symmetric(4, 3)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::click());
                        let c = r.center();
                        let tri = if open {
                            vec![c + Vec2::new(-4.0, -2.0), c + Vec2::new(4.0, -2.0), c + Vec2::new(0.0, 3.0)]
                        } else {
                            vec![c + Vec2::new(-2.0, -4.0), c + Vec2::new(3.0, 0.0), c + Vec2::new(-2.0, 4.0)]
                        };
                        ui.painter().add(egui::Shape::convex_polygon(tri, Color32::from_gray(80), Stroke::NONE));
                        let t = ui.add(egui::Label::new(RichText::new(p.title()).strong()).sense(egui::Sense::click()));
                        let toggled = resp.clicked() || t.clicked();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("×").on_hover_text("Remover da bandeja").clicked() {
                                self.prefs.tray_hidden.push(p.title().to_string());
                                self.prefs.save();
                            }
                        });
                        toggled
                    })
                    .inner
                });
                if scroll_to == Some(p) {
                    header.response.scroll_to_me(Some(egui::Align::TOP));
                }
                if header.inner {
                    if open {
                        self.tray_open.remove(&p);
                    } else {
                        self.tray_open.insert(p);
                    }
                }
                if self.tray_open.contains(&p) {
                    egui::Frame::new().inner_margin(egui::Margin { left: 6, right: 4, top: 4, bottom: 8 }).show(ui, |ui| {
                        ui.set_max_width(w - 12.0);
                        self.panel_body(ui, p, now);
                    });
                }
                ui.add_space(1.0);
            }
        });
    }

    /// Status bar: geo-location, model info, instructor, tooltip and Measurements.
    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let loc = self.doc.location.name.clone();
            if icons::button(ui, Icon::Ui(UiIcon::Pin), false, 20.0).on_hover_text(format!("Geolocalização: {loc}")).clicked() {
                self.open_panel = Some(PanelKind::Shadows);
                self.show_tray = true;
            }
            if icons::button(ui, Icon::ModelInfo, false, 20.0).on_hover_text("Informações do modelo").clicked() {
                self.open_panel = Some(PanelKind::ModelInfo);
                self.show_tray = true;
            }
            if icons::button(ui, Icon::Ui(UiIcon::Help), false, 20.0).on_hover_text("Instrutor").clicked() {
                self.open_panel = Some(PanelKind::Instructor);
                self.show_tray = true;
            }
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let shown = if !self.vcb.is_empty() { format!("{}|", self.vcb) } else { self.live_measure.clone().unwrap_or_default() };
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0, Color32::GRAY))
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| {
                        ui.set_min_width(190.0);
                        ui.monospace(shown);
                    });
                ui.label(RichText::new("Medidas").color(Color32::from_gray(90)));
                ui.separator();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if !self.message.is_empty() {
                        ui.add(egui::Label::new(RichText::new(&self.message).color(Color32::from_rgb(0, 110, 60))).truncate());
                        ui.separator();
                    }
                    ui.add(egui::Label::new(self.hint()).truncate());
                });
            });
        });
    }

    /// Desktop-only menus: View, Procedures, Extensions, Help.
    pub(crate) fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Exibir", |ui| {
            ui.menu_button("Barras de ferramentas", |ui| {
                for (name, _) in TOOLBARS.iter() {
                    let mut on = !self.prefs.hidden_toolbars.iter().any(|h| h == name);
                    if ui.checkbox(&mut on, *name).changed() {
                        if on {
                            self.prefs.hidden_toolbars.retain(|h| h != name);
                        } else {
                            self.prefs.hidden_toolbars.push(name.to_string());
                        }
                        self.prefs.save();
                    }
                }
                let mut lts = self.prefs.large_tool_set;
                if ui.checkbox(&mut lts, "Conjunto grande de ferramentas").changed() {
                    self.prefs.large_tool_set = lts;
                    self.prefs.save();
                }
            });
            let mut tabs = self.prefs.scene_tabs;
            if ui.checkbox(&mut tabs, "Abas de cenas").changed() {
                self.prefs.scene_tabs = tabs;
                self.prefs.save();
            }
            ui.separator();
            let d = &mut self.display;
            let mut ch = false;
            ch |= ui.checkbox(&mut d.hidden_geometry, "Geometria oculta").changed();
            ch |= ui.checkbox(&mut d.hidden_objects, "Objetos ocultos").changed();
            ch |= ui.checkbox(&mut d.section_planes, "Planos de seção").changed();
            ch |= ui.checkbox(&mut d.section_cuts, "Cortes de seção").changed();
            ch |= ui.checkbox(&mut d.section_fill, "Preenchimento de seção").changed();
            ch |= ui.checkbox(&mut d.axes, "Eixos").changed();
            ch |= ui.checkbox(&mut d.guides, "Guias").changed();
            if ui.checkbox(&mut self.doc.shadows.on, "Sombras").changed() {
                self.modified = true;
                self.dirty = true;
            }
            if ui.checkbox(&mut self.doc.shadows.fog, "Neblina").changed() {
                self.modified = true;
            }
            if ch {
                self.dirty = true;
                self.cache_dirty = true;
            }
            ui.separator();
            let mut st = self.doc.style.clone();
            ui.menu_button("Estilo de aresta", |ui| {
                ui.checkbox(&mut st.edges, "Arestas");
                ui.checkbox(&mut st.back_edges, "Arestas traseiras");
                let mut prof = st.profiles > 0.0;
                if ui.checkbox(&mut prof, "Perfis").changed() {
                    st.profiles = if prof { 2.0 } else { 0.0 };
                }
                let mut ext = st.extension > 0.0;
                if ui.checkbox(&mut ext, "Extensão").changed() {
                    st.extension = if ext { 6.0 } else { 0.0 };
                }
            });
            ui.menu_button("Estilo de face", |ui| {
                ui.checkbox(&mut st.xray, "Raio-X");
                ui.separator();
                for f in FaceStyle::ALL {
                    ui.radio_value(&mut st.face, f, f.label());
                }
                ui.separator();
                ui.checkbox(&mut st.ao, "Oclusão de ambiente");
            });
            if st != self.doc.style {
                self.style_edit(|s| *s = st);
            }
            ui.menu_button("Edição de componente", |ui| {
                if ui.checkbox(&mut self.display.hide_rest, "Ocultar o resto do modelo").changed() {
                    self.cache_dirty = true;
                }
            });
            ui.menu_button("Animação", |ui| {
                if ui.button("Adicionar cena").clicked() {
                    self.add_scene();
                }
                if ui.button("Atualizar cena").clicked() && !self.doc.scenes.is_empty() {
                    let k = self.scene_sel.min(self.doc.scenes.len() - 1);
                    self.checkpoint_keep_sel();
                    self.doc.scenes[k].cam = self.cam.to_array();
                    self.doc.scenes[k].ortho = self.cam.ortho;
                }
            });
        });
    }

    pub(crate) fn procedures_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Procedimentos", |ui| {
            ui.menu_button("Copiar ao longo", |ui| {
                for m in crate::procedural::AlongMode::ALL {
                    if ui.button(m.label()).clicked() {
                        self.start_copy_along(m);
                    }
                }
            });
            for t in [Tool::FollowMe] {
                if ui.button(format!("{}  ({})", t.label(), t.key())).clicked() {
                    self.set_tool(t);
                }
            }
            ui.menu_button("Ferramentas de sólidos", |ui| {
                for t in [Tool::OuterShell, Tool::SolidIntersect, Tool::Union, Tool::Subtract, Tool::Trim, Tool::Split] {
                    if ui.button(t.label()).clicked() {
                        self.set_tool(t);
                    }
                }
            });
        });
    }

    pub(crate) fn extensions_help_menus(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Extensões", |ui| {
            ui.add_enabled(false, egui::Button::new("Gerenciador de extensões"));
            ui.weak("O HomeUp ainda não carrega extensões Ruby.");
            ui.separator();
            if ui.button("Biblioteca online (Poly Haven / ambientCG)").clicked() {
                self.open_panel = Some(PanelKind::Library);
                self.show_tray = true;
            }
        });
        ui.menu_button("Ajuda", |ui| {
            if ui.button("Atalhos de teclado (F1)").clicked() {
                self.show_help = true;
            }
            if ui.button("Instrutor").clicked() {
                self.open_panel = Some(PanelKind::Instructor);
                self.show_tray = true;
            }
            ui.separator();
            if ui.button("Sobre o HomeUp").clicked() {
                self.show_about = true;
            }
        });
    }

    pub(crate) fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = true;
        egui::Window::new("Sobre o HomeUp").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            ui.heading(format!("HomeUp {}", env!("CARGO_PKG_VERSION")));
            ui.label("Modelador 3D desktop inspirado no SketchUp (implementação própria, clean-room).");
            ui.label("Rust + wgpu + egui.");
            ui.separator();
            ui.label("Conteúdo online: Poly Haven e ambientCG (CC0).");
        });
        self.show_about = open;
    }
}
