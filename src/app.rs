use crate::camera::Camera;
use crate::doc::{Document, Guide, SaveFile, Scene};
use crate::geom::{Geom, Plane, Sel};
use crate::import::{ImportOpts, Imported, UNITS};
use crate::pick::{axis_color, InferState, PickCache, SnapKind, axes};
use crate::render::Renderer3D;
use crate::scene::{build, SceneOpts};
use crate::units::format_len;
use eframe::egui::{self, Align2, Color32, FontId, Key, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use glam::{DMat4, DVec2, DVec3};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Line,
    Rect,
    Circle,
    Polygon,
    Arc,
    ArcCenter,
    Arc3,
    Pie,
    Freehand,
    RotRect,
    Lasso,
    Sample,
    FollowMe,
    Flip,
    OuterShell,
    Union,
    Subtract,
    SolidIntersect,
    Trim,
    Split,
    Dimension,
    Text,
    Text3d,
    Protractor,
    Axes,
    Section,
    Tag,
    ZoomWindow,
    PositionCamera,
    LookAround,
    Walk,
    PushPull,
    Offset,
    Move,
    Rotate,
    Scale,
    Tape,
    Paint,
    Eraser,
    Orbit,
    Pan,
    Zoom,
    Place(usize),
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Selecionar",
            Tool::Line => "Linha",
            Tool::Rect => "Retângulo",
            Tool::Circle => "Círculo",
            Tool::Polygon => "Polígono",
            Tool::Arc => "Arco",
            Tool::ArcCenter => "Arco pelo centro",
            Tool::Arc3 => "Arco de 3 pontos",
            Tool::Pie => "Pizza",
            Tool::Freehand => "Mão livre",
            Tool::RotRect => "Retângulo girado",
            Tool::Lasso => "Laço",
            Tool::Sample => "Material de amostra",
            Tool::FollowMe => "Siga-me",
            Tool::Flip => "Virar",
            Tool::OuterShell => "Revestimento externo",
            Tool::Union => "União",
            Tool::Subtract => "Subtrair",
            Tool::SolidIntersect => "Interseção",
            Tool::Trim => "Recortar",
            Tool::Split => "Dividir",
            Tool::Dimension => "Dimensões",
            Tool::Text => "Texto",
            Tool::Text3d => "Texto 3D",
            Tool::Protractor => "Transferidor",
            Tool::Axes => "Eixos",
            Tool::Section => "Plano de seção",
            Tool::Tag => "Etiqueta",
            Tool::ZoomWindow => "Janela de zoom",
            Tool::PositionCamera => "Posicionar câmera",
            Tool::LookAround => "Girar",
            Tool::Walk => "Percorrer",
            Tool::PushPull => "Push/Pull",
            Tool::Offset => "Offset",
            Tool::Move => "Mover",
            Tool::Rotate => "Rotacionar",
            Tool::Scale => "Escala",
            Tool::Tape => "Fita métrica",
            Tool::Paint => "Pintar",
            Tool::Eraser => "Borracha",
            Tool::Orbit => "Orbitar",
            Tool::Pan => "Pan",
            Tool::Zoom => "Zoom",
            Tool::Place(_) => "Inserir componente",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "Espaço",
            Tool::Line => "L",
            Tool::Rect => "R",
            Tool::Circle => "C",
            Tool::Polygon => "-",
            Tool::Arc => "A",
            Tool::ArcCenter | Tool::Arc3 | Tool::Pie | Tool::Freehand | Tool::RotRect => "-",
            Tool::Lasso => "Shift+Espaço",
            Tool::Sample => "Alt no Pintar",
            Tool::Dimension | Tool::Text | Tool::Text3d | Tool::Protractor | Tool::Axes | Tool::Section | Tool::Tag => "-",
            Tool::ZoomWindow | Tool::PositionCamera | Tool::LookAround | Tool::Walk => "-",
            Tool::FollowMe | Tool::Flip | Tool::OuterShell | Tool::Union | Tool::Subtract | Tool::SolidIntersect | Tool::Trim | Tool::Split => "-",
            Tool::PushPull => "P",
            Tool::Offset => "F",
            Tool::Move => "M",
            Tool::Rotate => "Q",
            Tool::Scale => "S",
            Tool::Tape => "T",
            Tool::Paint => "B",
            Tool::Eraser => "E",
            Tool::Orbit => "O",
            Tool::Pan => "H",
            Tool::Zoom => "Z",
            Tool::Place(_) => "-",
        }
    }
}

pub const TOOL_GROUPS: [&[Tool]; 6] = [
    &[Tool::Select, Tool::Lasso, Tool::Eraser, Tool::Paint, Tool::Sample],
    &[Tool::Line, Tool::Freehand, Tool::Rect, Tool::RotRect, Tool::Circle, Tool::Polygon, Tool::Arc, Tool::ArcCenter, Tool::Arc3, Tool::Pie],
    &[Tool::PushPull, Tool::Offset, Tool::FollowMe, Tool::Move, Tool::Rotate, Tool::Scale, Tool::Flip],
    &[Tool::Tape, Tool::Dimension, Tool::Protractor, Tool::Axes, Tool::Text, Tool::Text3d, Tool::Section, Tool::Tag],
    &[Tool::OuterShell, Tool::Union, Tool::Subtract, Tool::SolidIntersect, Tool::Trim, Tool::Split],
    &[Tool::Orbit, Tool::Pan, Tool::Zoom, Tool::ZoomWindow, Tool::PositionCamera, Tool::LookAround, Tool::Walk],
];

#[derive(Clone, Debug)]
pub enum XKind {
    Move(DVec3),
    Rotate { c: DVec3, n: DVec3, ang: f64 },
    Scale { a: DVec3, mask: DVec3, t: f64 },
}

impl XKind {
    /// World-space matrix; `frac` scales the operation (arrays).
    pub fn matrix(&self, frac: f64) -> DMat4 {
        match *self {
            XKind::Move(v) => DMat4::from_translation(v * frac),
            XKind::Rotate { c, n, ang } => {
                DMat4::from_translation(c) * DMat4::from_axis_angle(n.normalize(), ang * frac) * DMat4::from_translation(-c)
            }
            XKind::Scale { a, mask, t } => {
                let t = if frac == 1.0 { t } else { t.powf(frac) };
                DMat4::from_translation(a) * DMat4::from_scale(DVec3::ONE + mask * (t - 1.0)) * DMat4::from_translation(-a)
            }
        }
    }
}

#[derive(Clone)]
pub enum ToolState {
    Idle,
    LineFrom(DVec3),
    RectFrom { p0: DVec3, plane: Plane, u: DVec3, v: DVec3 },
    CircleFrom { c: DVec3, n: DVec3, u: DVec3 },
    ArcStart(DVec3),
    ArcChord { a: DVec3, b: DVec3 },
    Pushing { face: usize, origin: DVec3, normal: DVec3, d: f64, press: Pos2 },
    Offsetting { face: usize, press: Pos2 },
    Moving { base: DVec3, press: Pos2 },
    RotCenter { c: DVec3, n: DVec3 },
    RotAngle { c: DVec3, n: DVec3, r0: DVec3, press: Pos2 },
    Scaling { anchor: DVec3, grip: DVec3, mask: DVec3, press: Pos2 },
    TapeFrom { p: DVec3, edge_dir: Option<DVec3> },
    ArcCenterR { c: DVec3, n: DVec3 },
    ArcSweep { c: DVec3, n: DVec3, a: DVec3 },
    Arc3Start(DVec3),
    Arc3Mid { a: DVec3, m: DVec3 },
    Freehand { pts: Vec<DVec3>, plane: Plane },
    RotRect1 { p0: DVec3, n: DVec3 },
    RotRect2 { p0: DVec3, p1: DVec3, n: DVec3 },
    Lasso { pts: Vec<Pos2> },
    SolidPick { first: usize },
    DimFrom(DVec3),
    DimOffset { a: DVec3, b: DVec3 },
    TextLeader { anchor: DVec3, text: String },
    ProtRef { c: DVec3, n: DVec3 },
    ProtAngle { c: DVec3, n: DVec3, r0: DVec3 },
    AxesX { o: DVec3 },
    AxesY { o: DVec3, x: DVec3 },
    Box { start: Pos2 },
    Erasing { edges: std::collections::HashSet<usize>, guides: std::collections::HashSet<usize> },
}

#[derive(Clone)]
#[allow(dead_code)]
pub enum LastAction {
    Line { before: Document, start: DVec3, dir: DVec3 },
    Rect { before: Document, p0: DVec3, u: DVec3, v: DVec3, su: f64, sv: f64, w: f64, h: f64 },
    Circle { before: Document, c: DVec3, n: DVec3, u: DVec3, r: f64, polygon: bool },
    Arc { before: Document, a: DVec3, b: DVec3, w: DVec3, s: f64 },
    PushPull { before: Document, face: usize, d: f64, normal: DVec3, copy: bool },
    Offset { before: Document, face: usize, d: f64 },
    Xform { before: Document, sel: Sel, kind: XKind, copy: bool },
}

/// Per-frame pointer input for the viewport tools.
pub struct In {
    pub px: DVec2,
    pub pos: Pos2,
    pub pressed: bool,
    pub released: bool,
    pub dbl: bool,
    pub triple: bool,
    pub dragging_far: bool,
    pub mods: egui::Modifiers,
    pub delta: DVec2,
    pub primary_down: bool,
}

#[derive(Default)]
pub struct Overlay {
    pub lines: Vec<(DVec3, DVec3, Color32, f32)>,
    pub dashed: Vec<(DVec3, DVec3, Color32)>,
    pub squares: Vec<(DVec3, Color32)>,
    pub tooltip: Option<(String, Color32)>,
    pub box_rect: Option<(Pos2, Pos2)>,
    pub screen_poly: Vec<Pos2>,
}

pub(crate) enum MenuAction {
    Erase,
    Group,
    Component,
    Explode,
    Edit,
    Close,
    Unique,
    Reverse,
    Soften,
    Hide,
    Connected,
    UnhideAll,
    DeleteGuides,
    IntersectModel,
    IntersectSel,
    OuterShell,
    Hide2,
    UnhideAll2,
    Divide,
    Weld,
    ExplodeCurve,
    Area,
    TexPosition,
    TexReset,
}

pub struct HomeUpApp {
    pub doc: Document,
    pub path: Vec<usize>,
    pub undo: Vec<(Document, Vec<usize>)>,
    pub redo: Vec<(Document, Vec<usize>)>,
    pub cam: Camera,
    pub cam_anim: Option<(Camera, Camera, f64)>,
    pub tool: Tool,
    pub state: ToolState,
    pub sel: Sel,
    pub vcb: String,
    pub last: Option<LastAction>,
    pub last_push: Option<f64>,
    pub renderer: Option<Renderer3D>,
    /// GPU device for the photoreal renderer (None = CPU only)
    pub gpu_ctx: Option<crate::gpupt::GpuCtx>,
    pub dirty: bool,
    pub cache_dirty: bool,
    pub cache: PickCache,
    pub snap: Option<crate::pick::Snap>,
    pub inf: InferState,
    pub hover_face: Option<usize>,
    pub hover_inst: Option<usize>,
    pub plane_lock: Option<usize>,
    pub copy_mode: bool,
    pub ctrl_held: bool,
    ctrl_down: bool,
    ctrl_tap: bool,
    pub circle_sides: usize,
    pub polygon_sides: usize,
    pub arc_sides: usize,
    pub cur_mat: Option<usize>,
    pub offset_d: f64,
    pub arc_sweep: f64,
    pub rot_rect_w: f64,
    pub file: Option<PathBuf>,
    pub modified: bool,
    pub message: String,
    title: String,
    pub show_help: bool,
    pub show_about: bool,
    pub last_open_panel: Option<crate::chrome::PanelKind>,
    pub show_tray: bool,
    press_pos: Option<Pos2>,
    pub live_measure: Option<String>,
    pub last_rect_preview: Option<(f64, f64)>,
    pub preview: Option<Document>,
    pub preview_sel: Option<Sel>,
    had_preview: bool,
    pub overlay: Overlay,
    pub viewport_min: Pos2,
    pub edit_mat: Option<usize>,
    import_dialog: Option<PendingImport>,
    pub open_panel: Option<crate::chrome::PanelKind>,
    pub slot_sel: Vec<usize>,
    pub flyout: Option<usize>,
    pub show_more: bool,
    pub search: String,
    pub snapshot: Option<crate::snapshot::Snapshot>,
    pub preview_dim: Option<crate::doc::Dimension>,
    pub edit_label: Option<usize>,
    pub text3d: Option<crate::annot::Text3dDialog>,
    pub last_px: DVec2,
    pub section_cut: Vec<(DVec3, DVec3)>,
    pub display: Display,
    pub tag_search: String,
    pub cur_tag: Option<usize>,
    pub soften_coplanar: bool,
    pub soften_angle: f32,
    pub soften_dragging: bool,
    pub last_hidden: Sel,
    pub eye_height: f64,
    pub walk_anchor: Option<Pos2>,
    pub repaint: bool,
    pub view_back: Vec<Camera>,
    pub view_fwd: Vec<Camera>,
    pub view_last: Option<([f64; 6], f64)>,
    pub prefs: crate::filemenu::Prefs,
    pub show_home: bool,
    pub show_prefs: bool,
    pub last_autosave: f64,
    pub shot_status: crate::filemenu::PendingShot,
    pub viewport_rect: Rect,
    pub edit_toolbar: bool,
    pub library: Option<crate::library::Library>,
    pub tex_thumbs: std::collections::HashMap<(usize, usize), egui::TextureHandle>,
    pub ui_thumbs: std::collections::HashMap<String, (u64, egui::TextureHandle)>,
    pub clipboard: Option<crate::clipboard::Clip>,
    /// Divide in progress: (edge, segments).
    pub divide: Option<(usize, usize)>,
    pub show_templates: bool,
    pub show_save_template: bool,
    pub template_name: String,
    pub template_as_default: bool,
    pub show_report: bool,
    pub report_cfg: crate::report::ReportCfg,
    pub divide_typed: bool,
    pub pending_image: Option<crate::imageimport::PendingImage>,
    pub image_place: Option<crate::imageimport::ImagePlace>,
    pub image_mat: Option<usize>,
    /// Scene whose matched photo is showing (until the camera moves away).
    pub photo_scene: Option<usize>,
    pub photo_cam: Option<[f64; 6]>,
    pub mp_edit: bool,
    pub mp_hide_model: bool,
    pub mp_grip: Option<usize>,
    pub texpos: Option<crate::texpos::TexPos>,
    pub tape_last: Option<f64>,
    pub tape_resize: Option<(f64, f64)>,
    pub show_export: bool,
    pub export_cfg: crate::export::ExportCfg,
    pub pending_raster: Option<crate::export::PendingRaster>,
    pub render_view: Option<crate::pathtrace::RenderView>,
    pub pending_cad: Option<crate::cadimport::PendingCad>,
    pub copy_along: Option<crate::procedural::AlongMode>,
    pub sandbox: Option<(crate::sandbox::SandboxTool, crate::sandbox::SandboxState)>,
    pub sandbox_radius: f64,
    pub sandbox_offset: f64,
    pub sandbox_spacing: f64,
    /// Text put on the system clipboard after a copy (so Ctrl+V reaches us).
    pub clip_text: Option<String>,
    pub thumb_budget: usize,
    /// Document revision (bumped on every checkpoint, undo and redo).
    pub rev: u64,
    pub mat_tab_edit: bool,
    pub style_tab: u8,
    /// Panels shown in the stacked desktop tray.
    pub tray_open: std::collections::HashSet<crate::chrome::PanelKind>,
    pub scene_sel: usize,
    pub outliner_filter: String,
    pub outliner_by_tag: bool,
    pub outliner_rename: Option<(Vec<usize>, String)>,
    /// Section plane picked in the Outliner (highlighted in the view).
    pub outliner_section: Option<usize>,
    pub section_rename: Option<(usize, String)>,
}

/// Display settings (Exibição panel).
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    pub section_planes: bool,
    pub section_cuts: bool,
    pub section_fill: bool,
    pub cut_width: f32,
    pub fill_color: [u8; 3],
    pub axes: bool,
    pub guides: bool,
    pub hidden_geometry: bool,
    pub hidden_objects: bool,
    pub color_by_tag: bool,
    pub hide_rest: bool,
    pub true_north: bool,
}

impl Default for Display {
    fn default() -> Self {
        Display {
            section_planes: true,
            section_cuts: true,
            section_fill: true,
            cut_width: 3.0,
            fill_color: [40, 40, 40],
            axes: true,
            guides: true,
            hidden_geometry: false,
            hidden_objects: false,
            color_by_tag: false,
            hide_rest: false,
            true_north: false,
        }
    }
}

pub struct PendingImport {
    path: PathBuf,
    imp: Imported,
    opts: ImportOpts,
}

impl HomeUpApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let renderer = cc.wgpu_render_state.as_ref().map(Renderer3D::new);
        let mut style = (*cc.egui_ctx.global_style()).clone();
        style.visuals = egui::Visuals::light();
        cc.egui_ctx.set_global_style(style);
        let prefs = crate::filemenu::Prefs::load();
        cc.egui_ctx.set_zoom_factor(prefs.ui_zoom);
        HomeUpApp {
            doc: if prefs.scale_figure { Document::with_scale_figure() } else { Document::new() },
            path: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            cam: Camera::default(),
            cam_anim: None,
            tool: Tool::Line,
            state: ToolState::Idle,
            sel: Sel::default(),
            vcb: String::new(),
            last: None,
            last_push: None,
            renderer,
            gpu_ctx: cc.wgpu_render_state.as_ref().map(|rs| crate::gpupt::GpuCtx { device: rs.device.clone(), queue: rs.queue.clone() }),
            dirty: true,
            cache_dirty: true,
            cache: PickCache::default(),
            snap: None,
            inf: InferState::default(),
            hover_face: None,
            hover_inst: None,
            plane_lock: None,
            copy_mode: false,
            ctrl_held: false,
            ctrl_down: false,
            ctrl_tap: false,
            circle_sides: 24,
            polygon_sides: 6,
            arc_sides: 12,
            cur_mat: Some(1),
            offset_d: 0.0,
            arc_sweep: 0.0,
            rot_rect_w: 0.0,
            file: None,
            modified: false,
            message: String::new(),
            title: String::new(),
            show_help: false,
            show_about: false,
            last_open_panel: None,
            show_tray: true,
            press_pos: None,
            live_measure: None,
            last_rect_preview: None,
            preview: None,
            preview_sel: None,
            had_preview: false,
            overlay: Overlay::default(),
            viewport_min: Pos2::ZERO,
            edit_mat: None,
            import_dialog: None,
            open_panel: Some(crate::chrome::PanelKind::Entity),
            slot_sel: vec![0; crate::chrome::SLOTS.len()],
            flyout: None,
            show_more: false,
            search: String::new(),
            snapshot: crate::snapshot::Snapshot::from_env(),
            preview_dim: None,
            edit_label: None,
            text3d: None,
            last_px: DVec2::ZERO,
            section_cut: Vec::new(),
            display: Display::default(),
            tag_search: String::new(),
            cur_tag: None,
            soften_coplanar: false,
            soften_angle: 30.0,
            soften_dragging: false,
            last_hidden: Sel::default(),
            eye_height: crate::camtools::EYE_HEIGHT,
            walk_anchor: None,
            repaint: false,
            view_back: Vec::new(),
            view_fwd: Vec::new(),
            view_last: None,
            show_home: prefs.start_page,
            prefs,
            show_prefs: false,
            last_autosave: 0.0,
            shot_status: Default::default(),
            viewport_rect: Rect::NOTHING,
            edit_toolbar: false,
            library: None,
            tex_thumbs: Default::default(),
            ui_thumbs: Default::default(),
            clipboard: None,
            divide: None,
            show_templates: false,
            show_save_template: false,
            template_name: String::new(),
            template_as_default: false,
            show_report: false,
            report_cfg: Default::default(),
            divide_typed: false,
            pending_image: None,
            image_place: None,
            image_mat: None,
            photo_scene: None,
            photo_cam: None,
            mp_edit: false,
            mp_hide_model: false,
            mp_grip: None,
            texpos: None,
            tape_last: None,
            tape_resize: None,
            show_export: false,
            export_cfg: Default::default(),
            pending_raster: None,
            render_view: None,
            pending_cad: None,
            copy_along: None,
            sandbox: None,
            sandbox_radius: 2.0,
            sandbox_offset: 1.0,
            sandbox_spacing: 1.0,
            clip_text: None,
            thumb_budget: 0,
            rev: 0,
            mat_tab_edit: false,
            style_tab: 0,
            tray_open: [crate::chrome::PanelKind::Entity, crate::chrome::PanelKind::Materials, crate::chrome::PanelKind::Shadows, crate::chrome::PanelKind::Scenes].into(),
            scene_sel: 0,
            outliner_filter: String::new(),
            outliner_by_tag: false,
            outliner_rename: None,
            outliner_section: None,
            section_rename: None,
        }
    }

    // ---------------------------------------------------------------- context helpers

    pub fn ctx_xf(&self) -> DMat4 {
        self.doc.xf(&self.path)
    }
    pub fn to_local(&self, p: DVec3) -> DVec3 {
        self.ctx_xf().inverse().transform_point3(p)
    }
    pub fn local_eye(&self) -> DVec3 {
        self.to_local(self.cam.eye())
    }
    /// A world-space transform expressed in the active context's coordinates.
    pub fn local_matrix(&self, m: DMat4) -> DMat4 {
        let c = self.ctx_xf();
        c.inverse() * m * c
    }
    pub fn active(&self) -> &Geom {
        self.doc.geom(&self.path)
    }
    pub fn active_mut(&mut self) -> &mut Geom {
        self.doc.geom_mut(&self.path)
    }

    pub fn enter_context(&mut self, inst: usize) {
        self.cancel();
        self.path.push(inst);
        self.sel.clear();
        self.dirty = true;
        self.cache_dirty = true;
        self.message = format!("Editando {}", self.doc.defs[self.doc.def_at(&self.path).unwrap()].name);
    }

    pub fn exit_context(&mut self) {
        self.cancel();
        if let Some(i) = self.path.pop() {
            self.sel = Sel::default();
            self.sel.insts.insert(i);
        }
        // procedural components follow edits of their path
        self.regen_all_procedures();
        self.dirty = true;
        self.cache_dirty = true;
        self.message.clear();
    }

    // ---------------------------------------------------------------- history

    pub fn checkpoint_keep_sel(&mut self) {
        self.rev += 1;
        self.undo.push((self.doc.clone(), self.path.clone()));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.modified = true;
        self.dirty = true;
        self.cache_dirty = true;
    }

    pub fn checkpoint(&mut self) {
        self.checkpoint_keep_sel();
        self.sel.clear();
    }

    /// Checkpoint and return the document as it was before the operation.
    pub fn begin_op(&mut self) -> Document {
        self.checkpoint();
        self.undo.last().unwrap().0.clone()
    }

    pub(crate) fn do_undo(&mut self) {
        self.rev += 1;
        self.cancel();
        if let Some((d, p)) = self.undo.pop() {
            let cur = (std::mem::replace(&mut self.doc, d), std::mem::replace(&mut self.path, p));
            self.redo.push(cur);
            self.after_history();
        }
    }

    pub(crate) fn do_redo(&mut self) {
        self.rev += 1;
        self.cancel();
        if let Some((d, p)) = self.redo.pop() {
            let cur = (std::mem::replace(&mut self.doc, d), std::mem::replace(&mut self.path, p));
            self.undo.push(cur);
            self.after_history();
        }
    }

    pub(crate) fn after_history(&mut self) {
        if !self.doc.path_valid(&self.path) {
            self.path.clear();
        }
        self.last = None;
        self.sel.clear();
        self.dirty = true;
        self.cache_dirty = true;
        self.modified = true;
    }

    pub fn cancel(&mut self) {
        self.state = ToolState::Idle;
        self.vcb.clear();
        self.inf.lock = None;
        self.dirty = true;
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.cancel();
        self.last = None;
        self.plane_lock = None;
        self.copy_mode = false;
        self.tool = t;
        if t == Tool::Text3d {
            self.text3d = Some(Default::default());
        }
    }

    pub fn apply_selection(&mut self, s: Sel, mods: egui::Modifiers) {
        if mods.ctrl && mods.shift {
            self.sel.faces.retain(|f| !s.faces.contains(f));
            self.sel.edges.retain(|e| !s.edges.contains(e));
            self.sel.insts.retain(|e| !s.insts.contains(e));
        } else if mods.ctrl {
            self.sel.faces.extend(s.faces);
            self.sel.edges.extend(s.edges);
            self.sel.insts.extend(s.insts);
        } else if mods.shift {
            for f in s.faces {
                if !self.sel.faces.remove(&f) {
                    self.sel.faces.insert(f);
                }
            }
            for e in s.edges {
                if !self.sel.edges.remove(&e) {
                    self.sel.edges.insert(e);
                }
            }
            for e in s.insts {
                if !self.sel.insts.remove(&e) {
                    self.sel.insts.insert(e);
                }
            }
        } else {
            self.sel = s;
        }
        self.dirty = true;
    }

    // ---------------------------------------------------------------- files

    pub(crate) fn new_file_pub(&mut self) {
        self.new_file();
    }

    pub(crate) fn open_file_pub(&mut self) {
        self.open_file();
    }

    pub(crate) fn new_file(&mut self) {
        self.doc = if self.prefs.scale_figure { Document::with_scale_figure() } else { Document::new() };
        let tpl = self.prefs.default_template.clone();
        if tpl != "builtin:0" {
            self.apply_template(&tpl);
        }
        self.path.clear();
        self.undo.clear();
        self.redo.clear();
        self.file = None;
        self.modified = false;
        self.last = None;
        self.cancel();
        self.sel.clear();
        self.cache_dirty = true;
        self.cam = Camera { size: self.cam.size, ..Camera::default() };
    }

    pub(crate) fn open_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Modelos (HomeUp, SketchUp)", &["homeup", "skp"])
            .add_filter("Modelo HomeUp", &["homeup"])
            .add_filter("SketchUp", &["skp"])
            .pick_file()
        else {
            return;
        };
        self.open_path(path);
    }

    pub fn open_path(&mut self, path: PathBuf) {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("skp")) {
            self.open_skp(path);
            return;
        }
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<SaveFile>(&s).map_err(|e| e.to_string()))
        {
            Ok(f) => {
                self.new_file();
                self.doc = f.doc;
                self.cam.set_array(f.camera);
                self.cam.ortho = f.ortho;
                self.message = format!("Aberto: {}", path.display());
                self.add_recent(&path);
                self.show_home = false;
                self.file = Some(path);
            }
            Err(e) => self.message = format!("Erro ao abrir: {e}"),
        }
    }

    pub(crate) fn save_file(&mut self, save_as: bool) {
        let path = match (&self.file, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let Some(p) = rfd::FileDialog::new().add_filter("Modelo HomeUp", &["homeup"]).set_file_name("modelo.homeup").save_file()
                else {
                    return;
                };
                p
            }
        };
        self.save_path(path);
    }

    pub fn save_path(&mut self, path: PathBuf) {
        let f = SaveFile { format: "homeup".into(), version: 2, doc: self.doc.clone(), camera: self.cam.to_array(), ortho: self.cam.ortho };
        match serde_json::to_string(&f).map_err(|e| e.to_string()).and_then(|s| std::fs::write(&path, s).map_err(|e| e.to_string())) {
            Ok(()) => {
                self.message = format!("Salvo: {}", path.display());
                self.add_recent(&path);
                self.file = Some(path);
                self.modified = false;
            }
            Err(e) => self.message = format!("Erro ao salvar: {e}"),
        }
    }

    pub(crate) fn import_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Modelos 3D", &["skp", "obj", "gltf", "glb", "stl", "dae"])
            .add_filter("SketchUp", &["skp"])
            .add_filter("Wavefront OBJ", &["obj"])
            .add_filter("glTF", &["gltf", "glb"])
            .add_filter("STL", &["stl"])
            .add_filter("Collada", &["dae"])
            .pick_file()
        else {
            return;
        };
        self.start_import(path);
    }

    pub fn start_import(&mut self, path: PathBuf) {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("skp")) {
            // native reader first; the official SDK (if installed) is the fallback
            match self.import_skp(&path) {
                Ok(()) => return,
                Err(e) if crate::skp::find_sdk().is_none() => {
                    self.message = format!("Falha ao importar o .skp: {e}");
                    return;
                }
                Err(_) => {}
            }
        }
        match crate::import::load(&path) {
            Ok(imp) if imp.triangle_count() == 0 => self.message = "O arquivo não contém faces.".into(),
            Ok(imp) => {
                let opts = crate::import::default_opts(&path, &imp);
                self.import_dialog = Some(PendingImport { path, imp, opts });
            }
            Err(e) => self.message = format!("Falha ao importar: {e}"),
        }
    }

    pub(crate) fn finish_import(&mut self, p: PendingImport) {
        self.checkpoint_keep_sel();
        let mut g = crate::import::to_geom(&p.imp, &p.opts, &mut self.doc.materials, &mut self.doc.textures);
        crate::import::recenter(&mut g);
        let name = p.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("Importado".into());
        let tris = g.faces.len();
        self.doc.defs.push(crate::doc::Definition { name: name.clone(), geom: g, group: false, face_me: false, procedure: None, ifc: None, author: None, light: None });
        let d = self.doc.defs.len() - 1;
        self.set_tool(Tool::Place(d));
        self.message = format!("{name} importado ({tris} faces). Clique para posicionar.");
        if let Some(w) = &p.imp.warning {
            self.message = format!("{}  Aviso: {w}", self.message);
        }
    }

    pub(crate) fn import_window(&mut self, ctx: &egui::Context) {
        let Some(p) = self.import_dialog.as_mut() else { return };
        let mut go = false;
        let mut cancel = false;
        egui::Window::new("Importar modelo").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.strong(p.path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
            ui.label(format!("{} triângulos, {} materiais", p.imp.triangle_count(), p.imp.materials.len()));
            ui.separator();
            egui::Grid::new("imp").num_columns(2).show(ui, |ui| {
                ui.label("Unidade do arquivo:");
                let cur = UNITS.iter().find(|u| (u.1 - p.opts.meters_per_unit).abs() < 1e-12).map(|u| u.0.to_string()).unwrap_or_else(|| format!("{} m", p.opts.meters_per_unit));
                egui::ComboBox::from_id_salt("imp_units").selected_text(cur).show_ui(ui, |ui| {
                    for (n, m) in UNITS {
                        ui.selectable_value(&mut p.opts.meters_per_unit, m, n);
                    }
                });
                ui.end_row();
                ui.label("Eixo vertical:");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut p.opts.z_up, true, "Z para cima");
                    ui.radio_value(&mut p.opts.z_up, false, "Y para cima");
                });
                ui.end_row();
                ui.label("Suavizar arestas até:");
                ui.add(egui::Slider::new(&mut p.opts.smooth_deg, 0.0..=60.0).suffix("°"));
                ui.end_row();
            });
            if p.imp.meters_per_unit.is_some() || p.imp.z_up.is_some() {
                ui.weak("Unidade/eixo pré-preenchidos com as informações do próprio arquivo.");
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Importar").clicked() {
                    go = true;
                }
                if ui.button("Cancelar").clicked() {
                    cancel = true;
                }
            });
        });
        if go {
            let p = self.import_dialog.take().unwrap();
            self.finish_import(p);
        } else if cancel {
            self.import_dialog = None;
        }
    }

    pub(crate) fn export_obj(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Wavefront OBJ", &["obj"]).set_file_name("modelo.obj").save_file() else {
            return;
        };
        match std::fs::write(&path, self.doc.to_obj()) {
            Ok(()) => self.message = format!("Exportado: {}", path.display()),
            Err(e) => self.message = format!("Erro ao exportar: {e}"),
        }
    }

    // ---------------------------------------------------------------- commands

    pub(crate) fn select_all(&mut self) {
        let g = self.active();
        self.sel = Sel {
            faces: (0..g.faces.len()).collect(),
            edges: (0..g.edges.len()).collect(),
            insts: (0..g.instances.len()).collect(),
        };
        self.dirty = true;
    }

    pub(crate) fn delete_selection(&mut self) {
        self.drop_locked();
        if self.sel.is_empty() {
            return;
        }
        let sel = self.sel.clone();
        self.checkpoint();
        let eye = self.local_eye();
        self.active_mut().delete_sel(&sel, eye);
    }

    pub(crate) fn make_group(&mut self, group: bool) {
        if self.sel.is_empty() {
            self.message = "Selecione a geometria primeiro.".into();
            return;
        }
        let sel = self.sel.clone();
        self.checkpoint();
        let path = self.path.clone();
        if let Some(i) = self.doc.make_def(&path, &sel, group) {
            self.sel.insts.insert(i);
            self.message = format!("{} criado.", self.doc.defs.last().unwrap().name);
        }
    }

    pub(crate) fn explode_selection(&mut self) {
        let mut insts: Vec<usize> = self.sel.insts.iter().copied().collect();
        if insts.is_empty() {
            return;
        }
        insts.sort_unstable();
        self.checkpoint();
        let path = self.path.clone();
        let mut sel = Sel::default();
        for i in insts.into_iter().rev() {
            sel = self.doc.explode(&path, i);
        }
        self.sel = sel;
    }

    pub(crate) fn zoom_extents(&mut self) {
        match self.doc.bounds() {
            Some((lo, hi)) => self.cam.frame_bounds(lo, hi),
            None => self.cam.frame_bounds(DVec3::splat(-1.0), DVec3::splat(3.0)),
        }
    }

    pub(crate) fn set_view(&mut self, yaw_deg: f64, pitch_deg: f64) {
        self.cam.yaw = yaw_deg.to_radians();
        self.cam.pitch = pitch_deg.to_radians();
        self.zoom_extents();
    }

    pub(crate) fn go_scene(&mut self, k: usize, now: f64) {
        if self.doc.scenes.get(k).is_some_and(|s| s.photo.is_some()) {
            self.enter_photo_scene(k);
            return;
        }
        self.photo_scene = None;
        self.photo_cam = None;
        let Some(s) = self.doc.scenes.get(k) else { return };
        let mut to = self.cam.clone();
        to.set_array(s.cam);
        to.ortho = s.ortho;
        to.roll = 0.0;
        self.cam_anim = Some((self.cam.clone(), to, now));
    }

    pub(crate) fn add_scene(&mut self) {
        let n = self.doc.scenes.len() + 1;
        self.checkpoint_keep_sel();
        self.doc.scenes.push(Scene { name: format!("Cena {n}"), cam: self.cam.to_array(), ortho: self.cam.ortho, photo: None });
    }

    pub(crate) fn run_menu(&mut self, a: MenuAction) {
        match a {
            MenuAction::Erase => self.delete_selection(),
            MenuAction::Group => self.make_group(true),
            MenuAction::Component => self.make_group(false),
            MenuAction::Explode => self.explode_selection(),
            MenuAction::Edit => {
                if let Some(i) = self.sel.insts.iter().next().copied() {
                    self.enter_context(i);
                }
            }
            MenuAction::Close => self.exit_context(),
            MenuAction::Unique => {
                if let Some(i) = self.sel.insts.iter().next().copied() {
                    self.checkpoint_keep_sel();
                    let path = self.path.clone();
                    self.doc.make_unique(&path, i);
                }
            }
            MenuAction::Reverse => {
                let fs = self.sel.faces.clone();
                self.checkpoint_keep_sel();
                self.active_mut().reverse_faces(&fs);
            }
            MenuAction::Soften | MenuAction::Hide => {
                let es = self.sel.edges.clone();
                let soften = matches!(a, MenuAction::Soften);
                self.checkpoint_keep_sel();
                for e in es {
                    let edge = &mut self.active_mut().edges[e];
                    if soften {
                        edge.soft = true;
                    } else {
                        edge.hidden = true;
                    }
                }
            }
            MenuAction::Connected => {
                let g = self.active();
                let seed = g.sel_vertices(&self.sel);
                self.sel = g.connected(&seed);
                self.dirty = true;
            }
            MenuAction::UnhideAll => {
                self.checkpoint_keep_sel();
                for e in &mut self.active_mut().edges {
                    e.hidden = false;
                }
            }
            MenuAction::IntersectModel => self.intersect_selection(true),
            MenuAction::IntersectSel => self.intersect_selection(false),
            MenuAction::OuterShell => self.outer_shell_selection(),
            MenuAction::Hide2 => self.hide_selection(),
            MenuAction::Divide => self.start_divide(),
            MenuAction::Weld => self.weld_selection(),
            MenuAction::ExplodeCurve => self.explode_curve_selection(),
            MenuAction::TexPosition => self.start_texture_position(),
            MenuAction::TexReset => {
                self.checkpoint_keep_sel();
                let fs: Vec<usize> = self.sel.faces.iter().copied().collect();
                for f in fs {
                    self.active_mut().faces[f].uvs = None;
                }
                self.dirty = true;
            }
            MenuAction::Area => {
                let a = self.selection_area();
                self.message = format!("Área de {} faces: {}", self.sel.faces.len(), crate::units::format_area(a));
            }
            MenuAction::UnhideAll2 => self.unhide(crate::panels::UnhideWhat::All),
            MenuAction::DeleteGuides => {
                if !self.doc.guides.is_empty() {
                    self.checkpoint_keep_sel();
                    self.doc.guides.clear();
                }
            }
        }
    }

    // ---------------------------------------------------------------- keyboard

    pub(crate) fn handle_keys(&mut self, ctx: &egui::Context) {
        let mods = ctx.input(|i| i.modifiers);
        // A Ctrl "tap" (press + release with no other key or click) toggles copy mode.
        let events = ctx.input(|i| i.events.clone());
        for e in &events {
            match e {
                egui::Event::ModifiersChanged(m) => {
                    if m.ctrl && !self.ctrl_down {
                        self.ctrl_tap = true;
                    }
                    if !m.ctrl && self.ctrl_down && self.ctrl_tap {
                        if matches!(self.tool, Tool::Move | Tool::Rotate | Tool::PushPull | Tool::Scale) {
                            self.copy_mode = !self.copy_mode;
                            self.dirty = true;
                        }
                        self.ctrl_tap = false;
                    }
                    self.ctrl_down = m.ctrl;
                }
                egui::Event::Key { pressed: true, .. } | egui::Event::PointerButton { pressed: true, .. } => self.ctrl_tap = false,
                _ => {}
            }
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }
        for ev in events {
            match ev {
                egui::Event::Copy => {
                    self.copy_selection();
                }
                egui::Event::Cut => self.cut_selection(),
                egui::Event::Paste(_) => {
                    if ctx.input(|i| i.modifiers.shift) {
                        self.paste_in_place();
                    } else {
                        self.paste();
                    }
                }
                egui::Event::Text(t) => {
                    for ch in t.chars() {
                        let starts = ch.is_ascii_digit() || "-.,;/*".contains(ch) || (ch == 'x' && self.last.is_some());
                        let cont = !self.vcb.is_empty() && (starts || "mcsxdeg° ".contains(ch.to_ascii_lowercase()));
                        if (self.vcb.is_empty() && starts) || cont {
                            self.vcb.push(ch.to_ascii_lowercase());
                        }
                    }
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let typing = !self.vcb.is_empty();
                    if modifiers.command {
                        match key {
                            Key::Z if modifiers.shift => self.do_redo(),
                            Key::Z => self.do_undo(),
                            Key::Y => self.do_redo(),
                            Key::S => self.save_file(modifiers.shift),
                            Key::O => self.open_file(),
                            Key::N => self.new_file(),
                            Key::A => self.select_all(),
                            Key::T => {
                                self.sel.clear();
                                self.dirty = true;
                            }
                            Key::G => self.make_group(true),
                            Key::V if modifiers.shift => {
                                self.paste_in_place();
                            }
                            Key::F => ctx.memory_mut(|m| m.request_focus(egui::Id::unique(crate::chrome::SEARCH_ID))),
                            _ => {}
                        }
                        continue;
                    }
                    match key {
                        Key::Enter => self.apply_vcb(),
                        Key::Backspace if typing => {
                            self.vcb.pop();
                        }
                        Key::Escape => {
                            if self.texpos.is_some() || self.copy_along.is_some() || self.sandbox.is_some() {
                                self.texpos = None;
                                self.copy_along = None;
                                self.sandbox = None;
                                self.preview = None;
                                self.live_measure = None;
                                self.dirty = true;
                            } else if self.image_place.is_some() || self.mp_edit {
                                self.image_place = None;
                                self.mp_edit = false;
                                self.mp_hide_model = false;
                                self.dirty = true;
                                self.live_measure = None;
                            } else if self.divide.is_some() {
                                self.divide = None;
                                self.live_measure = None;
                                self.message.clear();
                            } else if typing {
                                self.vcb.clear();
                            } else if !matches!(self.state, ToolState::Idle) {
                                self.cancel();
                            } else if !self.sel.is_empty() {
                                self.sel.clear();
                                self.dirty = true;
                            } else if !self.path.is_empty() {
                                self.exit_context();
                            } else {
                                self.plane_lock = None;
                            }
                        }
                        Key::Delete => self.delete_selection(),
                        Key::ArrowUp | Key::ArrowRight | Key::ArrowLeft => {
                            let i = match key {
                                Key::ArrowUp => 2,
                                Key::ArrowRight => 0,
                                _ => 1,
                            };
                            let plane_tool = matches!(self.tool, Tool::Rect | Tool::Circle | Tool::Polygon | Tool::Rotate)
                                && matches!(self.state, ToolState::Idle);
                            if plane_tool {
                                self.plane_lock = if self.plane_lock == Some(i) { None } else { Some(i) };
                            } else {
                                let same = self.inf.lock == Some(axes()[i]);
                                self.inf.lock = if same { None } else { Some(axes()[i]) };
                            }
                        }
                        Key::ArrowDown => {
                            if let Some(d) = self.inf.last_edge_dir {
                                let same = self.inf.lock == Some(d);
                                self.inf.lock = if same { None } else { Some(d) };
                            }
                        }
                        _ if typing => {}
                        Key::Space if modifiers.shift => self.set_tool(Tool::Lasso),
                        Key::Space => self.set_tool(Tool::Select),
                        Key::L => self.set_tool(Tool::Line),
                        Key::R => self.set_tool(Tool::Rect),
                        Key::C => self.set_tool(Tool::Circle),
                        Key::A => self.set_tool(Tool::Arc),
                        Key::P => self.set_tool(Tool::PushPull),
                        Key::F => self.set_tool(Tool::Offset),
                        Key::M => self.set_tool(Tool::Move),
                        Key::Q => self.set_tool(Tool::Rotate),
                        Key::S => self.set_tool(Tool::Scale),
                        Key::T => self.set_tool(Tool::Tape),
                        Key::B => self.set_tool(Tool::Paint),
                        Key::E => self.set_tool(Tool::Eraser),
                        Key::O => self.set_tool(Tool::Orbit),
                        Key::H => self.set_tool(Tool::Pan),
                        Key::G => self.make_group(false),
                        Key::Z if modifiers.shift => self.zoom_extents(),
                        Key::Z => self.set_tool(Tool::Zoom),
                        Key::F1 => self.show_help = !self.show_help,
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        // Shift locks the current linear inference while held.
        if mods.shift {
            if self.inf.shift_lock.is_none() {
                if let Some(s) = self.snap {
                    if let (Some(d), SnapKind::Axis(_) | SnapKind::Parallel | SnapKind::Perpendicular | SnapKind::PerpFace) = (s.dir, s.kind) {
                        self.inf.shift_lock = Some((d, s.kind));
                    }
                }
            }
        } else {
            self.inf.shift_lock = None;
        }
    }

    // ---------------------------------------------------------------- viewport

    pub(crate) fn viewport(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let rect = ui.max_rect();
        self.viewport_min = rect.min;
        self.viewport_rect = rect;
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        self.cam.size = DVec2::new(rect.width() as f64, rect.height() as f64);

        let (hover, delta, scroll, mods) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.delta(), i.smooth_scroll_delta.y, i.modifiers));
        let menu_open = resp.context_menu_opened();
        let inside = !menu_open && (hover.is_some_and(|p| rect.contains(p)) && resp.hovered() || resp.dragged());
        let px = hover.map(|p| DVec2::new((p.x - rect.min.x) as f64, (p.y - rect.min.y) as f64));
        let (pressed, released, dbl, triple, middle, primary_down, secondary) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.pointer.button_double_clicked(PointerButton::Primary),
                i.pointer.button_triple_clicked(PointerButton::Primary),
                i.pointer.button_down(PointerButton::Middle),
                i.pointer.primary_down(),
                i.pointer.secondary_pressed(),
            )
        });
        let pressed = pressed && inside;
        self.ctrl_held = mods.ctrl && matches!(self.tool, Tool::Move | Tool::Rotate | Tool::PushPull | Tool::Scale);
        if pressed {
            self.press_pos = hover;
        }
        let d2 = DVec2::new(delta.x as f64, delta.y as f64);
        if inside && middle {
            if mods.shift { self.cam.pan(d2) } else { self.cam.orbit(d2) }
            self.cam_anim = None;
        }
        if inside && scroll.abs() > 0.0 {
            if let Some(px) = px {
                let (o, d) = self.cam.ray(px);
                let focus = self
                    .cache
                    .pick_face(o, d)
                    .map(|(t, _)| o + d * t)
                    .or_else(|| Plane::new(DVec3::Z, DVec3::ZERO).intersect_ray(o, d).filter(|p| p.distance(o) < self.cam.dist * 20.0))
                    .unwrap_or(self.cam.target);
                self.cam.zoom_at(focus, (-(scroll as f64) * 0.0025).exp());
                self.cam_anim = None;
            }
        }

        if self.cache_dirty {
            self.cache = PickCache::build(&self.doc, &self.path);
            self.cache_dirty = false;
            self.dirty = true;
        }

        // right click selects what is under the cursor before the menu opens
        if secondary && inside {
            if let Some(px) = px {
                let e = self.entity_at(px);
                let contained = e.faces.is_subset(&self.sel.faces) && e.edges.is_subset(&self.sel.edges) && e.insts.is_subset(&self.sel.insts);
                if !e.is_empty() && !contained {
                    self.sel = e;
                    self.dirty = true;
                }
            }
        }

        self.snap = None;
        self.overlay = Overlay::default();
        self.preview_dim = None;
        self.preview = None;
        self.preview_sel = None;
        self.live_measure = None;
        let (old_hf, old_hi) = (self.hover_face, self.hover_inst);
        self.hover_face = None;
        self.hover_inst = None;
        if let (true, Some(px), Some(pos)) = (inside && !middle, px, hover) {
            let dragging_far = self.press_pos.is_some_and(|a| a.distance(pos) > 5.0);
            let i = In { px, pos, pressed, released, dbl, triple, dragging_far, mods, delta: d2, primary_down };
            if !(self.match_photo_frame(&i)
                || self.texpos_frame(&i)
                || self.image_frame(&i)
                || self.divide_frame(&i)
                || self.copy_along_frame(&i)
                || self.sandbox_frame(&i))
            {
                self.tool_frame(&i);
            }
        } else if released {
            // finish an eraser/box drag that ended outside the viewport
            if matches!(self.state, ToolState::Box { .. } | ToolState::Erasing { .. }) {
                self.state = ToolState::Idle;
            }
        }
        if released {
            self.press_pos = None;
        }
        if (self.hover_face, self.hover_inst) != (old_hf, old_hi) {
            self.dirty = true;
        }
        if self.cache_dirty {
            self.cache = PickCache::build(&self.doc, &self.path);
            self.cache_dirty = false;
            self.dirty = true;
        }

        // ---- render 3D
        if let (Some(rs), true) = (frame.wgpu_render_state(), self.renderer.is_some()) {
            let has_preview = self.preview.is_some();
            if self.dirty || has_preview || self.had_preview {
                let show_sel = !matches!(self.state, ToolState::Pushing { .. });
                let doc = self.preview.as_ref().unwrap_or(&self.doc);
                let sel = self.preview_sel.as_ref().unwrap_or(&self.sel);
                let mut scene = build(
                    doc,
                    &SceneOpts { path: &self.path, sel, show_sel, highlight_face: self.hover_face, highlight_inst: self.hover_inst, axes: self.display.axes, color_by_tag: self.display.color_by_tag, hidden_geometry: self.display.hidden_geometry },
                );
                if self.mp_hide_model && self.photo_scene.is_some() {
                    scene = Default::default();
                }
                scene.lights = crate::lights::gpu_lights(doc);
                self.section_cut.clear();
                if let Some(pl) = crate::annot::active_section(doc) {
                    let groups = crate::annot::section_segments_grouped(doc, &self.path, &pl);
                    if self.display.section_fill {
                        let nrm = pl.n.as_vec3().to_array();
                        let (bu, bv) = pl.basis();
                        for (mat, segs) in &groups {
                            let st = &doc.style;
                            let mut h = if st.override_hatch { None } else { mat.and_then(|m| doc.materials.get(m)).and_then(|m| m.hatch.clone()) }
                                .unwrap_or_else(|| st.section_hatch.clone());
                            if st.override_hatch && h.pattern == crate::hatch::HatchPattern::Solid && h == crate::hatch::Hatch::default() {
                                // classic solid fill follows Display > fill color
                                h.fill = self.display.fill_color;
                            }
                            let a = (h.angle_deg as f64).to_radians();
                            let hu = (bu * a.cos() + bv * a.sin()).as_vec3();
                            let (fc, lc) = (crate::scene::rgb(h.fill), crate::scene::rgb(h.line));
                            for t in crate::annot::section_fill(segs, &pl) {
                                for p in t {
                                    let p = (p - pl.n * 1e-4).as_vec3().to_array();
                                    scene.tris.push(crate::render::Vertex {
                                        pos: p,
                                        nrm,
                                        col: fc,
                                        back: lc,
                                        uv: [0.0; 2],
                                        layer: match (h.image.filter(|t| *t < doc.textures.len() && *t < crate::tex::MAX_LAYERS), h.custom.filter(|k| *k < doc.hatch_pats.len())) {
                                            (Some(t), _) => -1000.0 - t as f32,
                                            (None, Some(k)) => -100.0 - k as f32,
                                            _ => -20.0 - h.pattern.index(),
                                        },
                                        tan: [hu.x, hu.y, hu.z, h.scale.max(0.001)],
                                    });
                                }
                            }
                        }
                    }
                    self.section_cut = groups.into_iter().flat_map(|g| g.1).collect();
                    scene.pats = crate::hatch::gpu_pats(&doc.hatch_pats);
                }
                self.renderer.as_mut().unwrap().set_textures(rs, &doc.textures);
                let env_img = doc.env.image.and_then(|i| doc.env_images.get(i));
                self.renderer.as_mut().unwrap().set_environment(rs, env_img);
                self.renderer.as_mut().unwrap().upload(rs, &scene);
                self.dirty = false;
            }
            self.had_preview = has_preview;
            self.sync_watermark(rs);
            let size = [(rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32];
            let vpd = self.cam.view_proj();
            let clip = crate::annot::active_section(&self.doc).map(|p| [p.n.x as f32, p.n.y as f32, p.n.z as f32, p.d as f32]);
            let fp = self.frame_params(vpd, clip, ppp);
            let tex = self.renderer.as_mut().unwrap().render(rs, size, &fp);
            self.run_pending_raster(rs, ppp);
            ui.painter().image(tex, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        } else {
            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(214, 226, 236));
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "wgpu indisponível", FontId::proportional(18.0), Color32::RED);
        }

        self.paint_overlay(ui, rect);

        // ---- context menu
        let mut action: Option<MenuAction> = None;
        let single_inst = self.sel.insts.len() == 1 && self.sel.faces.is_empty() && self.sel.edges.is_empty();
        let is_component = single_inst && {
            let g = self.active();
            let d = g.instances[*self.sel.insts.iter().next().unwrap()].def;
            !self.doc.defs[d].group
        };
        let has_geom = !self.sel.faces.is_empty() || !self.sel.edges.is_empty();
        let in_ctx = !self.path.is_empty();
        let has_sel = !self.sel.is_empty();
        let has_faces = !self.sel.faces.is_empty();
        let has_edges = !self.sel.edges.is_empty();
        let multi_inst = self.sel.insts.len() >= 2;
        resp.context_menu(|ui| {
            if single_inst {
                if ui.button(if is_component { "Editar componente" } else { "Editar grupo" }).clicked() {
                    action = Some(MenuAction::Edit);
                }
                if ui.button("Explodir").clicked() {
                    action = Some(MenuAction::Explode);
                }
                if is_component && ui.button("Tornar único").clicked() {
                    action = Some(MenuAction::Unique);
                }
                ui.separator();
            }
            if has_sel {
                if ui.button("Apagar").clicked() {
                    action = Some(MenuAction::Erase);
                }
                if ui.button("Ocultar").clicked() {
                    action = Some(MenuAction::Hide2);
                }
                if ui.button("Criar grupo (Ctrl+G)").clicked() {
                    action = Some(MenuAction::Group);
                }
                if ui.button("Criar componente (G)").clicked() {
                    action = Some(MenuAction::Component);
                }
            }
            if has_geom && ui.button("Selecionar tudo conectado").clicked() {
                action = Some(MenuAction::Connected);
            }
            if has_faces {
                if ui.button("Área da seleção").clicked() {
                    action = Some(MenuAction::Area);
                }
                let textured = self.sel.faces.len() == 1
                    && self.sel.faces.iter().next().and_then(|f| self.active().faces.get(*f)).and_then(|f| f.material).and_then(|m| self.doc.materials.get(m)).is_some_and(|m| m.texture.is_some());
                if textured {
                    ui.menu_button("Textura", |ui| {
                        if ui.button("Posicionar").clicked() {
                            action = Some(MenuAction::TexPosition);
                        }
                        if ui.button("Redefinir posição").clicked() {
                            action = Some(MenuAction::TexReset);
                        }
                    });
                }
                if ui.button("Inverter faces").clicked() {
                    action = Some(MenuAction::Reverse);
                }
                ui.menu_button("Interseção de faces", |ui| {
                    if ui.button("Com o modelo").clicked() {
                        action = Some(MenuAction::IntersectModel);
                    }
                    if ui.button("Com a seleção").clicked() {
                        action = Some(MenuAction::IntersectSel);
                    }
                });
            }
            if multi_inst && ui.button("Revestimento externo").clicked() {
                action = Some(MenuAction::OuterShell);
            }
            if has_edges {
                if self.sel.edges.len() == 1 && ui.button("Dividir").clicked() {
                    action = Some(MenuAction::Divide);
                }
                if self.sel.edges.len() >= 2 && ui.button("Soldar arestas").clicked() {
                    action = Some(MenuAction::Weld);
                }
                let in_curve = self.sel.edges.iter().next().is_some_and(|e| self.active().edges.get(*e).is_some_and(|x| x.curve.is_some()));
                if in_curve && ui.button("Explodir curva").clicked() {
                    action = Some(MenuAction::ExplodeCurve);
                }
                if ui.button("Suavizar arestas").clicked() {
                    action = Some(MenuAction::Soften);
                }
                if ui.button("Ocultar arestas").clicked() {
                    action = Some(MenuAction::Hide);
                }
            }
            if in_ctx {
                ui.separator();
                if ui.button("Fechar grupo/componente").clicked() {
                    action = Some(MenuAction::Close);
                }
            }
            ui.separator();
            if ui.button("Reexibir tudo").clicked() {
                action = Some(MenuAction::UnhideAll2);
            }
            if !has_sel && !in_ctx {
                ui.label("Nada selecionado");
            }
        });
        if let Some(a) = action {
            self.run_menu(a);
        }

        if inside {
            ui.ctx().set_cursor_icon(match self.tool {
                Tool::Orbit | Tool::Pan => egui::CursorIcon::Grab,
                Tool::Zoom => egui::CursorIcon::ZoomIn,
                Tool::Select => egui::CursorIcon::Default,
                Tool::Move => egui::CursorIcon::Move,
                Tool::Paint => egui::CursorIcon::PointingHand,
                _ => egui::CursorIcon::Crosshair,
            });
        }
    }

    /// Sun, fog, background and shadow-map parameters for this frame.
    pub(crate) fn frame_params(&self, vpd: DMat4, clip: Option<[f32; 4]>, ppp: f32) -> crate::render::FrameParams {
        let st = &self.doc.style;
        let sh = &self.doc.shadows;
        let sun = crate::settings::sun_direction(sh, &self.doc.location);
        let sun_on = sh.on && sun.z > 0.02;
        let (lo, hi) = self.doc.bounds().unwrap_or((DVec3::splat(-1.0), DVec3::splat(1.0)));
        let c = (lo + hi) * 0.5;
        let r = ((hi - lo).length() * 0.5).max(1.0);
        // the light frustum also covers the shadow cast on the ground
        let reach = r * (1.0 + 1.0 / sun.z.max(0.25)).min(6.0);
        let up = if sun.z.abs() > 0.99 { DVec3::Y } else { DVec3::Z };
        let leye = c + sun * reach * 3.0;
        let light_vp = crate::camera::orthographic_rh(reach, reach, 0.01, reach * 6.0) * crate::camera::look_at_rh(leye, c, up);
        use crate::settings::FaceStyle;
        let bg = crate::scene::rgb(st.bg);
        let env = &self.doc.env;
        crate::render::FrameParams {
            vp: vpd.as_mat4(),
            inv_vp: vpd.inverse().as_mat4(),
            clip,
            sky: st.sky,
            bg: [bg[0], bg[1], bg[2]],
            sun: sun_on.then(|| sun.as_vec3().to_array()),
            light: sh.light / 100.0,
            dark: sh.dark / 100.0,
            fog: sh.fog.then_some((sh.fog_start, sh.fog_end)),
            eye: self.cam.eye().as_vec3().to_array(),
            light_vp: light_vp.as_mat4(),
            ground: (sun_on && sh.on_ground).then(|| ([c.x as f32, c.y as f32], reach as f32)),
            shadows_on_faces: sh.on_faces,
            photoreal: st.face == FaceStyle::Photoreal,
            env_image: env.image.is_some_and(|i| i < self.doc.env_images.len()),
            env_rotation: env.rotation_deg.to_radians(),
            sky_exposure: env.sky_exposure,
            refl_exposure: env.refl_exposure,
            skydome: env.skydome,
            reflections: env.reflections && st.face == FaceStyle::Photoreal,
            ao: (
                if st.ao && matches!(st.face, FaceStyle::Photoreal | FaceStyle::Shaded | FaceStyle::Monochrome) { st.ao_strength } else { 0.0 },
                (r as f32 * 0.08).clamp(0.15, 1.2),
            ),
            profiles: if st.face == FaceStyle::Wireframe { 0.0 } else { st.profiles },
            back_edges: st.back_edges,
            px_scale: ppp,
            edge: [st.depth_cue, st.endpoints, st.jitter, {
                let e = self.cam.eye();
                let mut far = 1.0f64;
                for k in 0..8 {
                    let p = DVec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z });
                    far = far.max(p.distance(e));
                }
                far as f32
            }],
        }
    }

    pub(crate) fn paint_overlay(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let painter = ui.painter_at(rect);
        let to_pos = |v: DVec2| rect.min + Vec2::new(v.x as f32, v.y as f32);
        let cam = &self.cam;
        // negative axes, dotted (like SketchUp)
        let org = crate::pick::axes_origin();
        for (k, a) in axes().iter().enumerate().filter(|_| self.display.axes) {
            if let Some((p, q)) = cam.project_segment(org, org - *a * (cam.dist * 400.0 + 1000.0)) {
                let c = axis_color(k);
                let c = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 200);
                dashed(&painter, to_pos(p), to_pos(q), Stroke::new(1.0, c), 1.5, 3.0, rect);
            }
        }
        // guides
        let l = cam.dist * 300.0;
        for g in self.doc.guides.iter().filter(|_| self.display.guides) {
            match g {
                Guide::Line { p, dir } => {
                    if let Some((a, b)) = cam.project_segment(*p - *dir * l, *p + *dir * l) {
                        dashed(&painter, to_pos(a), to_pos(b), Stroke::new(1.0, Color32::from_gray(90)), 6.0, 4.0, rect);
                    }
                }
                Guide::Point { p } => {
                    if let Some(s) = cam.project(*p) {
                        let c = to_pos(s);
                        let st = Stroke::new(1.5, Color32::from_gray(60));
                        painter.line_segment([c - Vec2::new(5.0, 5.0), c + Vec2::new(5.0, 5.0)], st);
                        painter.line_segment([c - Vec2::new(5.0, -5.0), c + Vec2::new(5.0, -5.0)], st);
                    }
                }
            }
        }
        for (a, b, c, w) in &self.overlay.lines {
            if let Some((sa, sb)) = cam.project_segment(*a, *b) {
                painter.line_segment([to_pos(sa), to_pos(sb)], Stroke::new(*w, *c));
            }
        }
        for (a, b, c) in &self.overlay.dashed {
            if let Some((sa, sb)) = cam.project_segment(*a, *b) {
                dashed(&painter, to_pos(sa), to_pos(sb), Stroke::new(1.0, *c), 5.0, 4.0, rect);
            }
        }
        for (p, c) in &self.overlay.squares {
            if let Some(s) = cam.project(*p) {
                painter.rect_filled(Rect::from_center_size(to_pos(s), Vec2::splat(7.0)), 0.0, *c);
            }
        }
        if self.overlay.screen_poly.len() > 1 {
            let mut pts = self.overlay.screen_poly.clone();
            pts.push(pts[0]);
            painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, Color32::from_rgb(40, 80, 200)), 5.0, 3.0));
        }
        if let Some((a, b)) = self.overlay.box_rect {
            let crossing = b.x < a.x;
            let r = Rect::from_two_pos(a, b);
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(60, 110, 230, if crossing { 18 } else { 28 }));
            let stroke = Stroke::new(1.0, Color32::from_rgb(40, 80, 200));
            if crossing {
                let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                painter.extend(egui::Shape::dashed_line(&pts, stroke, 6.0, 4.0));
            } else {
                painter.rect_stroke(r, 0.0, stroke, StrokeKind::Inside);
            }
        }
        if let Some(s) = self.snap {
            if let Some((r, q)) = s.ref_line {
                if let Some((a, b)) = cam.project_segment(r, q) {
                    dashed(&painter, to_pos(a), to_pos(b), Stroke::new(1.0, s.kind.color()), 3.0, 3.0, rect);
                }
            }
            if let Some(sp) = cam.project(s.p) {
                let p = to_pos(sp);
                if s.kind != SnapKind::Free {
                    let c = if s.in_inst { Color32::from_rgb(200, 0, 200) } else { s.kind.color() };
                    painter.circle_filled(p, 5.0, c);
                    painter.circle_stroke(p, 5.0, Stroke::new(1.0, Color32::WHITE));
                }
                let label = self.overlay.tooltip.clone().map(|t| t.0).unwrap_or_else(|| s.label());
                if !label.is_empty() {
                    let galley = painter.layout_no_wrap(label, FontId::proportional(13.0), Color32::BLACK);
                    let at = p + Vec2::new(14.0, 12.0);
                    let bg = Rect::from_min_size(at, galley.size()).expand(3.0);
                    painter.rect_filled(bg, 3.0, Color32::from_rgb(255, 255, 225));
                    painter.rect_stroke(bg, 3.0, Stroke::new(1.0, s.kind.color()), StrokeKind::Outside);
                    painter.galley(at, galley, Color32::BLACK);
                }
            }
        }
        self.paint_annotations(&painter, rect);
        let mut y = 10.0;
        let mut note = |t: String, c: Color32| {
            painter.text(rect.left_top() + Vec2::new(10.0, y), Align2::LEFT_TOP, t, FontId::proportional(13.0), c);
            y += 18.0;
        };
        if let Some(d) = self.inf.lock {
            let c = axes().iter().position(|a| *a == d).map(axis_color).unwrap_or(SnapKind::Parallel.color());
            note("Direção travada (aperte a seta de novo para soltar)".into(), c);
        }
        if let Some(i) = self.plane_lock {
            note(format!("Plano travado: normal no eixo {}", ["vermelho", "verde", "azul"][i]), axis_color(i));
        }
        if self.copy_mode {
            let t = match self.tool {
                Tool::PushPull => "Modo: nova extrusão (Ctrl)",
                Tool::Scale => "Modo: escala pelo centro (Ctrl)",
                _ => "Modo: cópia (Ctrl)",
            };
            note(t.into(), Color32::from_rgb(0, 110, 60));
        }
        if !self.path.is_empty() {
            let names: Vec<String> = (1..=self.path.len()).map(|k| self.doc.defs[self.doc.def_at(&self.path[..k]).unwrap()].name.clone()).collect();
            note(format!("Editando: Modelo > {}  (Esc ou duplo clique fora para sair)", names.join(" > ")), Color32::from_gray(60));
        }
        if self.cam.ortho {
            note("Projeção paralela".into(), Color32::from_gray(80));
        }
        let ctx = ui.ctx().clone();
        self.paint_overlay_watermarks(&ctx, &painter, rect);
        self.paint_match_photo(&painter, rect);
    }

    pub(crate) fn hint(&self) -> String {
        let s = match (&self.tool, &self.state) {
            (Tool::Select, _) => "Clique seleciona (Ctrl adiciona, Shift alterna). Arraste p/ direita = janela, p/ esquerda = cruzamento. Duplo clique: face+arestas ou entra no grupo. Triplo: tudo conectado.",
            (Tool::Line, ToolState::LineFrom(_)) => "Clique o ponto final ou digite o comprimento. Setas travam o eixo; Shift trava a direção; Esc encerra.",
            (Tool::Line, _) => "Clique o ponto inicial da linha.",
            (Tool::Rect, ToolState::RectFrom { .. }) => "Clique o canto oposto ou digite \"largura;comprimento\".",
            (Tool::Rect, _) => "Clique o primeiro canto. Setas escolhem o plano (cima = azul, direita = vermelho, esquerda = verde).",
            (Tool::Circle | Tool::Polygon, ToolState::CircleFrom { .. }) => "Clique para definir o raio ou digite o raio. \"24s\" muda o número de lados.",
            (Tool::Circle | Tool::Polygon, _) => "Clique o centro. Setas escolhem o plano; \"Ns\" define o número de lados.",
            (Tool::Arc, ToolState::ArcStart(_)) => "Clique o fim do arco (ou digite o comprimento da corda).",
            (Tool::Arc, ToolState::ArcChord { .. }) => "Mova para definir a flecha e clique (ou digite a flecha). \"12s\" muda os lados.",
            (Tool::Arc, _) => "Clique o início do arco.",
            (Tool::PushPull, ToolState::Pushing { .. }) => "Mova e clique, ou digite a distância. Duplo clique repete a última distância.",
            (Tool::PushPull, _) => "Clique numa face. Toque Ctrl para alternar \"nova extrusão\".",
            (Tool::Offset, ToolState::Offsetting { .. }) => "Mova para dentro/fora e clique, ou digite a distância.",
            (Tool::Offset, _) => "Clique numa face para criar um contorno paralelo.",
            (Tool::Move, ToolState::Moving { .. }) => "Clique o destino ou digite a distância. Toque Ctrl para copiar; depois \"x3\" ou \"/3\" cria matrizes.",
            (Tool::Move, _) => "Clique o ponto base (seleciona o que estiver sob o cursor se nada estiver selecionado).",
            (Tool::Rotate, ToolState::RotCenter { .. }) => "Clique o ponto de referência do ângulo.",
            (Tool::Rotate, ToolState::RotAngle { .. }) => "Clique para girar (encaixa a cada 15°) ou digite o ângulo. Ctrl copia.",
            (Tool::Rotate, _) => "Clique o centro de rotação. Setas travam o plano do transferidor.",
            (Tool::Scale, ToolState::Scaling { .. }) => "Mova e clique, ou digite o fator de escala.",
            (Tool::Scale, _) => "Arraste uma alça verde. Cantos = uniforme. Toque Ctrl para escalar pelo centro.",
            (Tool::Tape, ToolState::TapeFrom { edge_dir: Some(_), .. }) => "Clique para criar a guia paralela ou digite a distância.",
            (Tool::Tape, ToolState::TapeFrom { .. }) => "Clique o segundo ponto para medir (cria um ponto-guia).",
            (Tool::Tape, _) => "Clique num ponto para medir, ou numa aresta para criar uma guia paralela.",
            (Tool::Paint, _) => "Clique numa face ou grupo para pintar. Alt = conta-gotas; Shift = troca todas as faces iguais.",
            (Tool::Eraser, _) => "Clique ou arraste sobre arestas/guias. Ctrl = suavizar; Shift = ocultar; Ctrl+Shift = desfazer suavização.",
            (Tool::Orbit, _) => "Arraste para orbitar. Shift+arraste = Pan. Duplo clique centraliza.",
            (Tool::Pan, _) => "Arraste para mover a vista.",
            (Tool::Zoom, _) => "Arraste para aproximar/afastar (Shift = campo de visão). Digite o campo de visão em graus.",
            (Tool::Place(_), _) => "Clique para posicionar o componente.",
            (Tool::ArcCenter | Tool::Pie, ToolState::ArcCenterR { .. }) => "Clique o início do arco (define o raio) ou digite o raio.",
            (Tool::ArcCenter | Tool::Pie, ToolState::ArcSweep { .. }) => "Mova para definir o ângulo (encaixa a cada 15°) e clique, ou digite o ângulo.",
            (Tool::ArcCenter | Tool::Pie, _) => "Clique o centro. Setas travam o plano.",
            (Tool::Arc3, ToolState::Arc3Start(_)) => "Clique um ponto por onde o arco passa.",
            (Tool::Arc3, ToolState::Arc3Mid { .. }) => "Clique o ponto final do arco.",
            (Tool::Arc3, _) => "Clique o início do arco de 3 pontos.",
            (Tool::Freehand, _) => "Arraste para desenhar à mão livre. Termine perto do início para fechar uma face.",
            (Tool::RotRect, ToolState::RotRect1 { .. }) => "Clique o fim do primeiro lado (ou digite o comprimento).",
            (Tool::RotRect, ToolState::RotRect2 { .. }) => "Mova para definir a largura e clique (ou digite a largura).",
            (Tool::RotRect, _) => "Clique o primeiro canto. Setas travam o plano.",
            (Tool::Lasso, _) => "Arraste em volta das entidades. Sentido horário = cruzamento; anti-horário = janela.",
            (Tool::Sample, _) => "Clique numa face para pegar o material (e voltar ao Pintar).",
            (Tool::Dimension, ToolState::DimFrom(_)) => "Clique o segundo ponto.",
            (Tool::Dimension, ToolState::DimOffset { .. }) => "Mova para afastar a cota e clique (ou digite o afastamento).",
            (Tool::Dimension, _) => "Clique o primeiro ponto, ou uma aresta para cotá-la inteira.",
            (Tool::Text, ToolState::TextLeader { .. }) => "Clique onde o texto ficará.",
            (Tool::Text, _) => "Clique numa entidade para anotar (duplo clique num texto edita).",
            (Tool::Text3d, _) => "Configure o texto na janela e clique em Inserir.",
            (Tool::Protractor, ToolState::ProtRef { .. }) => "Clique a linha de base do ângulo.",
            (Tool::Protractor, ToolState::ProtAngle { .. }) => "Clique para criar a guia (15° de encaixe) ou digite o ângulo.",
            (Tool::Protractor, _) => "Clique o vértice do ângulo. Setas travam o plano.",
            (Tool::Axes, ToolState::AxesX { .. }) => "Clique a direção do eixo vermelho.",
            (Tool::Axes, ToolState::AxesY { .. }) => "Clique a direção do eixo verde.",
            (Tool::Axes, _) => "Clique a nova origem dos eixos de desenho.",
            (Tool::ZoomWindow, _) => "Arraste um retângulo em volta da área a ampliar.",
            (Tool::PositionCamera, _) => "Clique onde ficar de pé (altura do olho no VCB).",
            (Tool::LookAround, _) => "Arraste para girar a cabeça. Digite a altura do olho.",
            (Tool::Walk, _) => "Arraste para cima/baixo para andar, para os lados para virar. Shift = subir/descer e andar de lado.",
            (Tool::Tag, _) => "Clique em faces ou grupos para aplicar a etiqueta ativa (painel Etiquetas). Alt = pegar a etiqueta.",
            (Tool::Section, _) => "Clique numa face para criar o plano de seção (ou use as setas para escolher o eixo).",
            (Tool::FollowMe, _) => "Selecione o caminho (arestas) com o Selecionar e depois clique na face do perfil.",
            (Tool::Flip, _) => "Clique num dos planos coloridos para espelhar a seleção (selecione algo antes).",
            (Tool::OuterShell, _) => "Selecione 2+ sólidos e clique num deles, ou clique um sólido e depois outro.",
            (Tool::Union | Tool::SolidIntersect | Tool::Split, ToolState::SolidPick { .. }) => "Clique o segundo sólido.",
            (Tool::Subtract | Tool::Trim, ToolState::SolidPick { .. }) => "Clique o sólido que será cortado.",
            (Tool::Union | Tool::Subtract | Tool::SolidIntersect | Tool::Trim | Tool::Split, _) => "Clique o primeiro sólido (grupo ou componente).",
        };
        s.to_string()
    }

    pub(crate) fn vcb_label(&self) -> &'static str {
        match (&self.tool, &self.state) {
            (Tool::Rect, _) => "Dimensões",
            (Tool::Circle | Tool::Polygon, ToolState::CircleFrom { .. }) => "Raio",
            (Tool::Circle | Tool::Polygon, _) => "Lados",
            (Tool::Arc, ToolState::ArcChord { .. }) => "Flecha",
            (Tool::PushPull, _) | (Tool::Offset, _) => "Distância",
            (Tool::Rotate, _) => "Ângulo",
            (Tool::Scale, _) => "Escala",
            (Tool::Zoom, _) => "Campo de visão",
            (Tool::Tape, _) => "Comprimento",
            _ => "Medidas",
        }
    }

    pub(crate) fn mat_name(&self, m: Option<usize>) -> String {
        match m {
            Some(i) if i < self.doc.materials.len() => self.doc.materials[i].name.clone(),
            _ => "Padrão".into(),
        }
    }

    pub(crate) fn tag_combo(&mut self, ui: &mut egui::Ui, id: &str, current: Option<usize>) -> Option<Option<usize>> {
        let mut out = None;
        let name = current.and_then(|t| self.doc.tags.get(t)).map(|t| t.name.clone()).unwrap_or("Sem etiquetas".into());
        egui::ComboBox::from_id_salt(id).selected_text(name).show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "Sem etiquetas").clicked() {
                out = Some(None);
            }
            for (k, t) in self.doc.tags.iter().enumerate() {
                if ui.selectable_label(current == Some(k), &t.name).clicked() {
                    out = Some(Some(k));
                }
            }
        });
        out
    }

    pub(crate) fn material_combo(&mut self, ui: &mut egui::Ui, id: &str, current: Option<usize>) -> Option<Option<usize>> {
        let mut out = None;
        egui::ComboBox::from_id_salt(id).selected_text(self.mat_name(current)).show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "Padrão").clicked() {
                out = Some(None);
            }
            for (k, m) in self.doc.materials.iter().enumerate() {
                if ui.selectable_label(current == Some(k), &m.name).clicked() {
                    out = Some(Some(k));
                }
            }
        });
        out
    }

    pub(crate) fn entity_info(&mut self, ui: &mut egui::Ui) {
        let (nf, ne, ni) = (self.sel.faces.len(), self.sel.edges.len(), self.sel.insts.len());
        if nf + ne + ni == 0 {
            ui.label("Nada selecionado.");
            let g = self.active();
            ui.weak(format!("{} faces, {} arestas, {} grupos/componentes", g.faces.len(), g.edges.len(), g.instances.len()));
            return;
        }
        if nf == 1 && ne <= 64 && ni == 0 && (ne == 0 || self.sel.edges.iter().all(|e| {
            let g = self.active();
            let [a, b] = g.edges[*e].v;
            g.faces[*self.sel.faces.iter().next().unwrap()].has_edge(a, b)
        })) {
            let fi = *self.sel.faces.iter().next().unwrap();
            let (area, mat) = {
                let g = self.active();
                let s = self.ctx_xf().transform_vector3(DVec3::ONE).length() / 3f64.sqrt();
                (g.face_area(&g.faces[fi]) * s * s, g.faces[fi].material)
            };
            ui.strong("Face");
            ui.label(format!("Área: {}", crate::units::format_area(area)));
            let (back, tag, hidden) = {
                let f = &self.active().faces[fi];
                (f.back_material, f.tag, f.hidden)
            };
            egui::Grid::new("face_info").num_columns(2).show(ui, |ui| {
                ui.label("Frontal:");
                if let Some(m) = self.material_combo(ui, "face_mat", mat) {
                    self.checkpoint_keep_sel();
                    self.active_mut().faces[fi].material = m;
                }
                ui.end_row();
                ui.label("Verso:");
                if let Some(m) = self.material_combo(ui, "face_back", back) {
                    self.checkpoint_keep_sel();
                    self.active_mut().faces[fi].back_material = m;
                }
                ui.end_row();
                ui.label("Etiqueta:");
                if let Some(t) = self.tag_combo(ui, "face_tag", tag) {
                    self.checkpoint_keep_sel();
                    self.active_mut().faces[fi].tag = t;
                    self.cache_dirty = true;
                }
                ui.end_row();
            });
            let mut h = hidden;
            if ui.checkbox(&mut h, "Oculta").changed() {
                self.checkpoint_keep_sel();
                self.active_mut().faces[fi].hidden = h;
                self.cache_dirty = true;
            }
            return;
        }
        if ne >= 1 && nf == 0 && ni == 0 {
            let g = self.active();
            let s = self.ctx_xf().transform_vector3(DVec3::ONE).length() / 3f64.sqrt();
            let len: f64 = self.sel.edges.iter().map(|e| g.p(g.edges[*e].v[0]).distance(g.p(g.edges[*e].v[1]))).sum::<f64>() * s;
            let first = *self.sel.edges.iter().next().unwrap();
            let curve = g.edges[first].curve.is_some() && ne > 1;
            ui.strong(if curve { "Curva" } else if ne == 1 { "Aresta" } else { "Arestas" });
            ui.label(format!("Comprimento: {}", format_len(len)));
            let (mut soft, mut hidden) = (g.edges[first].soft, g.edges[first].hidden);
            let (s0, h0) = (soft, hidden);
            ui.checkbox(&mut soft, "Suave");
            ui.checkbox(&mut hidden, "Oculta");
            if soft != s0 || hidden != h0 {
                let es = self.sel.edges.clone();
                self.checkpoint_keep_sel();
                for e in es {
                    let edge = &mut self.active_mut().edges[e];
                    edge.soft = soft;
                    edge.hidden = hidden;
                }
            }
            return;
        }
        if ni == 1 && nf == 0 && ne == 0 {
            let i = *self.sel.insts.iter().next().unwrap();
            let (def, mat) = {
                let inst = &self.active().instances[i];
                (inst.def, inst.material)
            };
            let group = self.doc.defs[def].group;
            ui.strong(if group { "Grupo" } else { "Componente" });
            let mut name = self.doc.defs[def].name.clone();
            ui.horizontal(|ui| {
                ui.label("Nome:");
                if ui.text_edit_singleline(&mut name).changed() {
                    self.doc.defs[def].name = name.clone();
                    self.modified = true;
                }
            });
            if !group {
                ui.label(format!("Instâncias: {}", self.doc.instance_count(def)));
            }
            self.classifier_ui(ui, def);
            if self.doc.defs[def].light.is_some() {
                self.light_ui(ui, def);
            }
            if self.doc.defs[def].procedure.is_some() {
                self.procedure_ui(ui, def);
            }
            ui.horizontal(|ui| {
                ui.label("Material:");
                if let Some(m) = self.material_combo(ui, "inst_mat", mat) {
                    self.checkpoint_keep_sel();
                    self.active_mut().instances[i].material = m;
                }
            });
            let (itag, ihidden, icast) = {
                let inst = &self.active().instances[i];
                (inst.tag, inst.hidden, inst.cast_shadows)
            };
            ui.horizontal(|ui| {
                ui.label("Etiqueta:");
                if let Some(t) = self.tag_combo(ui, "inst_tag", itag) {
                    self.checkpoint_keep_sel();
                    self.active_mut().instances[i].tag = t;
                    self.cache_dirty = true;
                }
            });
            let (mut h, mut c) = (ihidden, icast);
            ui.horizontal(|ui| {
                if ui.checkbox(&mut h, "Oculto").changed() {
                    self.checkpoint_keep_sel();
                    self.active_mut().instances[i].hidden = h;
                    self.cache_dirty = true;
                }
                if ui.checkbox(&mut c, "Projetar sombras").changed() {
                    self.checkpoint_keep_sel();
                    self.active_mut().instances[i].cast_shadows = c;
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Editar").clicked() {
                    self.enter_context(i);
                }
                if ui.button("Explodir").clicked() {
                    self.explode_selection();
                }
                if !group && ui.button("Tornar único").clicked() {
                    self.run_menu(MenuAction::Unique);
                }
            });
            return;
        }
        ui.strong("Seleção múltipla");
        ui.label(format!("{nf} faces, {ne} arestas, {ni} grupos/componentes"));
        ui.horizontal(|ui| {
            if ui.button("Criar grupo").clicked() {
                self.make_group(true);
            }
            if ui.button("Criar componente").clicked() {
                self.make_group(false);
            }
        });
    }

    /// Swatch texture for a document texture (cached per index and size).
    pub(crate) fn tex_thumb(&mut self, ctx: &egui::Context, t: usize) -> Option<egui::TextureId> {
        let tex = self.doc.textures.get(t)?;
        let key = (t, tex.bytes.len());
        if !self.tex_thumbs.contains_key(&key) {
            let (w, h, rgba) = tex.thumbnail(64)?;
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            let handle = ctx.load_texture(format!("mat-thumb-{t}-{}", key.1), img, egui::TextureOptions::LINEAR);
            self.tex_thumbs.insert(key, handle);
        }
        self.tex_thumbs.get(&key).map(|h| h.id())
    }

}

pub fn segment_hits_rect(a: Pos2, b: Pos2, r: Rect) -> bool {
    if r.contains(a) || r.contains(b) {
        return true;
    }
    let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
    let d = |a: Pos2, b: Pos2, c: Pos2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    (0..4).any(|i| {
        let (p3, p4) = (c[i], c[(i + 1) % 4]);
        let (d1, d2, d3, d4) = (d(p3, p4, a), d(p3, p4, b), d(a, b, p3), d(a, b, p4));
        (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
    })
}

impl eframe::App for HomeUpApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.snapshot_frame(&ctx);
        self.track_view(now);
        self.track_photo_scene();
        if self.repaint {
            ctx.request_repaint();
            self.repaint = false;
        }
        let ax = self.doc.axes;
        crate::pick::set_axes([ax.x, ax.y, ax.z], ax.origin);
        let u = &self.doc.units;
        crate::units::set_units(u.length, u.precision, u.area, u.volume);
        if self.doc.show_hidden != self.display.hidden_objects {
            self.doc.show_hidden = self.display.hidden_objects;
            self.cache_dirty = true;
        }
        let hide_rest = self.display.hide_rest && !self.path.is_empty();
        if self.doc.hide_rest != hide_rest {
            self.doc.hide_rest = hide_rest;
            self.cache_dirty = true;
        }
        if self.doc.view_yaw != self.cam.yaw {
            self.doc.view_yaw = self.cam.yaw;
            if self.doc.has_face_me() {
                self.dirty = true;
                self.cache_dirty = true;
            }
        }
        self.handle_keys(&ctx);
        if let Some(t) = self.clip_text.take() {
            ctx.copy_text(t);
        }
        self.thumb_budget = 3;
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        for path in dropped {
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext == "homeup" {
                self.open_path(path);
            } else if matches!(ext.as_str(), "dxf" | "pdf" | "dwg") {
                self.start_cad_import(&path);
            } else if crate::imageimport::is_image(&path) {
                self.start_image_import(&path);
            } else if crate::import::supported(&path) {
                self.start_import(path);
            } else {
                self.message = format!("Formato não suportado: .{ext}");
            }
            break;
        }
        self.import_window(&ctx);
        self.annot_windows(&ctx);
        self.poll_shot_status();
        self.poll_library(&ctx);
        self.autosave(now);
        self.start_page(&ctx);
        self.prefs_window(&ctx);

        if let Some((a, b, t0)) = &self.cam_anim {
            let t = ((now - t0) / 0.6).clamp(0.0, 1.0);
            let size = self.cam.size;
            self.cam = Camera { size, ..Camera::lerp(a, b, t) };
            if t >= 1.0 {
                self.cam_anim = None;
            }
            ctx.request_repaint();
        }

        let name = self.file.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or("Sem título".into());
        let title = format!("{}{} — HomeUp", name, if self.modified { "*" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Arquivo", |ui| {
                    if ui.button("Página inicial").clicked() {
                        self.show_home = true;
                    }
                    if ui.button("Novo (Ctrl+N)").clicked() {
                        self.new_file();
                    }
                    if ui.button("Novo a partir de modelo…").clicked() {
                        self.show_templates = true;
                    }
                    if ui.button("Abrir… (Ctrl+O)").clicked() {
                        self.open_file();
                    }
                    ui.menu_button("Abrir recente", |ui| {
                        if self.prefs.recent.is_empty() {
                            ui.weak("Nenhum arquivo recente");
                        }
                        for p in self.prefs.recent.clone() {
                            let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                            if ui.add_enabled(p.exists(), egui::Button::new(name)).on_hover_text(p.display().to_string()).clicked() {
                                self.open_path(p);
                            }
                        }
                    });
                    if ui.button("Salvar (Ctrl+S)").clicked() {
                        self.save_file(false);
                    }
                    if ui.button("Salvar como… (Ctrl+Shift+S)").clicked() {
                        self.save_file(true);
                    }
                    if ui.button("Salvar como modelo…").clicked() {
                        self.show_save_template = true;
                    }
                    ui.separator();
                    if ui.button("Importar… (skp, obj, gltf, stl, dae)").clicked() {
                        self.import_file();
                    }
                    if ui.button("Importar imagem… (png, jpg)").clicked() {
                        self.import_image_file();
                    }
                    if ui.button("Importar planta… (dxf, pdf)").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Planta", &["dxf", "pdf", "dwg"]).pick_file() {
                            self.start_cad_import(&p);
                        }
                    }
                    ui.menu_button("Exportar", |ui| {
                        if ui.button("Modelo 3D OBJ…").clicked() {
                            self.export_obj();
                        }
                        if ui.button("Modelo 3D STL…").clicked() {
                            self.export_stl();
                        }
                        if ui.button("IFC (BIM)…").clicked() {
                            self.export_ifc_file();
                        }
                        if ui.button("Imagem (alta resolução, render)…").clicked() {
                            self.show_export = true;
                        }
                        if ui.button("Captura da tela (PNG)…").clicked() {
                            self.export_png(&ctx, false);
                        }
                    });
                    if ui.button("Gerar relatório…").clicked() {
                        self.show_report = true;
                    }
                    ui.separator();
                    if ui.button("Configurações do aplicativo").clicked() {
                        self.show_prefs = true;
                    }
                    if ui.button("Adicionar localização").clicked() {
                        self.open_panel = Some(crate::chrome::PanelKind::Shadows);
                        self.show_tray = true;
                    }
                    if ui.button("Imprimir…").clicked() {
                        self.export_png(&ctx, true);
                    }
                    ui.separator();
                    if ui.button("Sair").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Editar", |ui| {
                    if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Desfazer (Ctrl+Z)")).clicked() {
                        self.do_undo();
                    }
                    if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Refazer (Ctrl+Y)")).clicked() {
                        self.do_redo();
                    }
                    ui.separator();
                    if ui.button("Recortar (Ctrl+X)").clicked() {
                        self.cut_selection();
                    }
                    if ui.button("Copiar (Ctrl+C)").clicked() {
                        self.copy_selection();
                    }
                    if ui.add_enabled(self.clipboard.is_some(), egui::Button::new("Colar (Ctrl+V)")).clicked() {
                        self.paste();
                    }
                    if ui.add_enabled(self.clipboard.is_some(), egui::Button::new("Colar no lugar (Ctrl+Shift+V)")).clicked() {
                        self.paste_in_place();
                    }
                    ui.separator();
                    if ui.button("Selecionar tudo (Ctrl+A)").clicked() {
                        self.select_all();
                    }
                    if ui.button("Limpar seleção (Ctrl+T)").clicked() {
                        self.sel.clear();
                        self.dirty = true;
                    }
                    if ui.button("Apagar (Del)").clicked() {
                        self.delete_selection();
                    }
                    ui.separator();
                    if ui.button("Criar grupo (Ctrl+G)").clicked() {
                        self.make_group(true);
                    }
                    if ui.button("Criar componente (G)").clicked() {
                        self.make_group(false);
                    }
                    if ui.button("Explodir").clicked() {
                        self.explode_selection();
                    }
                    if ui.add_enabled(!self.path.is_empty(), egui::Button::new("Fechar grupo/componente")).clicked() {
                        self.exit_context();
                    }
                    ui.separator();
                    if ui.button("Interseção de faces com o modelo").clicked() {
                        self.run_menu(MenuAction::IntersectModel);
                    }
                    if ui.button("Interseção de faces com a seleção").clicked() {
                        self.run_menu(MenuAction::IntersectSel);
                    }
                    if ui.button("Revestimento externo").clicked() {
                        self.run_menu(MenuAction::OuterShell);
                    }
                    ui.separator();
                    if ui.button("Mostrar arestas ocultas").clicked() {
                        self.run_menu(MenuAction::UnhideAll);
                    }
                    if ui.button("Apagar guias").clicked() {
                        self.run_menu(MenuAction::DeleteGuides);
                    }
                });
                self.view_menu(ui);
                ui.menu_button("Câmera", |ui| {
                    if ui.radio(!self.cam.ortho, "Perspectiva").clicked() {
                        self.cam.ortho = false;
                    }
                    if ui.radio(self.cam.ortho, "Projeção paralela").clicked() {
                        self.cam.ortho = true;
                    }
                    ui.separator();
                    for (name, yaw, pitch) in [
                        ("Topo", -90.0, 89.9),
                        ("Baixo", -90.0, -89.9),
                        ("Frente", -90.0, 0.0),
                        ("Trás", 90.0, 0.0),
                        ("Esquerda", 180.0, 0.0),
                        ("Direita", 0.0, 0.0),
                        ("Iso", -45.0, 35.26),
                    ] {
                        if ui.button(name).clicked() {
                            self.set_view(yaw, pitch);
                        }
                    }
                    ui.separator();
                    if ui.add_enabled(!self.view_back.is_empty(), egui::Button::new("Anterior")).clicked() {
                        self.previous_view(now);
                    }
                    if ui.add_enabled(!self.view_fwd.is_empty(), egui::Button::new("Próxima")).clicked() {
                        self.next_view(now);
                    }
                    ui.separator();
                    if ui.button("Zoom Extents (Shift+Z)").clicked() {
                        self.zoom_extents();
                    }
                    if ui.button("Campo de visão padrão (35°)").clicked() {
                        self.cam.fov_y = 35f64.to_radians();
                    }
                    ui.separator();
                    if ui.button("Combinar nova foto…").clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("Imagem", &crate::imageimport::IMAGE_EXTS).pick_file() {
                            match std::fs::read(&path) {
                                Ok(b) => self.start_match_photo(&path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), b),
                                Err(e) => self.message = e.to_string(),
                            }
                        }
                    }
                    if ui.add_enabled(self.photo_scene.is_some(), egui::Button::new("Editar foto combinada")).clicked() {
                        self.mp_edit = true;
                        self.open_panel = Some(crate::chrome::PanelKind::MatchPhoto);
                    }
                });
                ui.menu_button("Desenho", |ui| {
                    for t in TOOL_GROUPS[1] {
                        if ui.button(format!("{}  ({})", t.label(), t.key())).clicked() {
                            self.set_tool(*t);
                        }
                    }
                    ui.separator();
                    ui.menu_button("Luz", |ui| {
                        for k in [crate::lights::LightKind::Point, crate::lights::LightKind::Spot, crate::lights::LightKind::Panel] {
                            if ui.button(k.label()).clicked() {
                                self.new_light(k);
                            }
                        }
                    });
                    ui.menu_button("Sandbox", |ui| {
                        if ui.button("A partir de contornos").clicked() {
                            self.tins_from_contours();
                        }
                        if ui.button("A partir do zero").clicked() {
                            self.start_sandbox(crate::sandbox::SandboxTool::FromScratch);
                        }
                    });
                });
                ui.menu_button("Ferramentas", |ui| {
                    for t in TOOL_GROUPS.iter().flat_map(|g| g.iter()) {
                        if ui.button(format!("{}  ({})", t.label(), t.key())).clicked() {
                            self.set_tool(*t);
                        }
                    }
                    ui.separator();
                    ui.menu_button("Sandbox", |ui| {
                        use crate::sandbox::SandboxTool as S;
                        for t in [S::Smoove, S::Stamp, S::Drape, S::AddDetail, S::FlipEdge] {
                            if ui.button(t.label()).clicked() {
                                self.start_sandbox(t);
                            }
                        }
                    });
                });
                self.procedures_menu(ui);
                ui.menu_button("Janela", |ui| {
                    ui.checkbox(&mut self.show_tray, "Bandeja padrão");
                    ui.menu_button("Painéis da bandeja", |ui| {
                        for p in crate::desktop::TRAY_ORDER {
                            let mut on = !self.prefs.tray_hidden.iter().any(|h| h == p.title());
                            if ui.checkbox(&mut on, p.title()).changed() {
                                if on {
                                    self.prefs.tray_hidden.retain(|h| h != p.title());
                                    self.tray_open.insert(p);
                                    self.show_tray = true;
                                } else {
                                    self.prefs.tray_hidden.push(p.title().to_string());
                                }
                                self.prefs.save();
                            }
                        }
                    });
                    ui.separator();
                    ui.label(egui::RichText::new("Interface").small().color(Color32::from_gray(110)));
                    if ui.radio(self.prefs.desktop_ui, "Desktop (barras + bandeja)").clicked() {
                        self.prefs.desktop_ui = true;
                        self.prefs.save();
                    }
                    if ui.radio(!self.prefs.desktop_ui, "Simplificada (estilo web)").clicked() {
                        self.prefs.desktop_ui = false;
                        self.prefs.save();
                    }
                    ui.separator();
                    if ui.button("Configurações do aplicativo").clicked() {
                        self.show_prefs = true;
                    }
                });
                self.extensions_help_menus(ui);
                self.top_controls(ui);
            });
            // scene tabs (simplified interface; the desktop one docks them over the drawing area)
            if !self.doc.scenes.is_empty() && !self.prefs.desktop_ui {
                ui.horizontal(|ui| {
                    ui.label("Cenas:");
                    let mut go = None;
                    for (k, s) in self.doc.scenes.iter().enumerate() {
                        if ui.button(&s.name).clicked() {
                            go = Some(k);
                        }
                    }
                    if ui.small_button("+").on_hover_text("Adicionar cena").clicked() {
                        self.add_scene();
                    }
                    if let Some(k) = go {
                        self.go_scene(k, now);
                    }
                });
            }
        });

        let desktop = self.prefs.desktop_ui;
        if desktop {
            egui::Panel::top("toolbars").show(ui, |ui| {
                ui.add_space(2.0);
                self.toolbars(ui, now);
                ui.add_space(1.0);
            });
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            if desktop {
                self.status_bar(ui);
                return;
            }
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let shown = if !self.vcb.is_empty() { format!("{}|", self.vcb) } else { self.live_measure.clone().unwrap_or_default() };
                    egui::Frame::new()
                        .fill(Color32::WHITE)
                        .stroke(Stroke::new(1.0, Color32::GRAY))
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.set_min_width(170.0);
                            ui.monospace(shown);
                        });
                    ui.label(format!("{}:", self.vcb_label()));
                    ui.separator();
                    // left side: hint and last message, truncated to the space left
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        if !self.message.is_empty() {
                            ui.add(egui::Label::new(egui::RichText::new(&self.message).color(Color32::from_rgb(0, 110, 60))).truncate());
                            ui.separator();
                        }
                        ui.add(egui::Label::new(self.hint()).truncate());
                    });
                });
            });
        });

        if desktop {
            if self.prefs.large_tool_set {
                egui::Panel::left("large-tool-set").resizable(false).exact_size(68.0).show(ui, |ui| {
                    self.large_tool_set(ui, now);
                });
            }
            if self.show_tray {
                egui::Panel::right("default-tray").resizable(true).default_size(310.0).size_range(250.0..=560.0).show(ui, |ui| {
                    self.stacked_tray(ui, now);
                });
            }
            if self.prefs.scene_tabs && !self.doc.scenes.is_empty() {
                egui::Panel::top("scene-tabs").show(ui, |ui| {
                    self.scene_tabs(ui, now);
                });
            }
        } else {
            let lr = egui::Panel::left("tools").resizable(false).exact_size(48.0).show(ui, |ui| {
                self.toolbar(ui);
            });
            if std::env::var("HOMEUP_TRACE").is_ok() {
                eprintln!("left {:?} avail {:?}", lr.response.rect, ui.available_rect_before_wrap());
            }
        }

        if self.show_tray && !desktop {
            let sr = egui::Panel::right("strip").resizable(false).exact_size(44.0).show(ui, |ui| {
                self.panel_strip(ui);
            });
            if std::env::var("HOMEUP_TRACE").is_ok() {
                eprintln!("strip {:?}", sr.response.rect);
            }
            if self.open_panel.is_some() {
                let r = egui::Panel::right("tray").resizable(true).default_size(300.0).size_range(240.0..=560.0).show(ui, |ui| {
                    self.panel_tray(ui, now);
                });
                if std::env::var("HOMEUP_TRACE").is_ok() {
                    eprintln!("tray rect {:?}", r.response.rect);
                }
            }
        }

        if std::env::var("HOMEUP_TRACE").is_ok() {
            eprintln!("root max {:?} min {:?}", ui.max_rect(), ui.min_rect());
        }
        egui::CentralPanel::no_frame().show(ui, |ui| {
            self.viewport(ui, frame);
        });

        self.about_window(&ctx);
        self.template_windows(&ctx);
        self.image_import_window(&ctx);
        self.texpos_window(&ctx);
        self.export_window(&ctx);
        self.tape_resize_window(&ctx);
        self.render_window(&ctx);
        self.cad_import_window(&ctx);
        self.report_window(&ctx);
        if self.show_help {
            let mut open = true;
            egui::Window::new("Atalhos do HomeUp").open(&mut open).resizable(false).show(&ctx, |ui| {
                egui::Grid::new("help").striped(true).show(ui, |ui| {
                    for (k, v) in [
                        ("Espaço / E / B", "Selecionar / Borracha / Pintar"),
                        ("L / R / C / A", "Linha / Retângulo / Círculo / Arco"),
                        ("P / F / M / Q / S / T", "Push-Pull / Offset / Mover / Rotacionar / Escala / Fita"),
                        ("O / H / Z", "Orbitar / Pan / Zoom"),
                        ("G / Ctrl+G", "Criar componente / grupo"),
                        ("Botão do meio", "Orbitar (Shift = Pan)"),
                        ("Roda", "Zoom no cursor"),
                        ("Shift+Z", "Zoom Extents"),
                        ("Setas cima/dir./esq.", "Trava eixo (ou plano) azul / vermelho / verde"),
                        ("Seta para baixo", "Trava paralelo à última aresta"),
                        ("Shift (segurar)", "Trava a inferência atual"),
                        ("Toque em Ctrl", "Copiar (Mover/Rotacionar), nova extrusão, escala pelo centro"),
                        ("Digitar + Enter", "Medidas: 2  1,5  150cm  3;2  24s  45  x3  /3"),
                        ("Ctrl+Z / Ctrl+Y", "Desfazer / Refazer"),
                        ("Del / Esc", "Apagar seleção / Cancelar ou sair do grupo"),
                    ] {
                        ui.monospace(k);
                        ui.label(v);
                        ui.end_row();
                    }
                });
            });
            self.show_help = open;
        }
    }
}

/// Clip a screen segment to `r` (Liang–Barsky). None if it lies outside.
pub fn clip_segment(a: Pos2, b: Pos2, r: Rect) -> Option<(Pos2, Pos2)> {
    if !(a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite()) {
        return None;
    }
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-dx, a.x - r.min.x), (dx, r.max.x - a.x), (-dy, a.y - r.min.y), (dy, r.max.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    Some((Pos2::new(a.x + dx * t0, a.y + dy * t0), Pos2::new(a.x + dx * t1, a.y + dy * t1)))
}

/// A dashed screen segment, clipped to the viewport first (unclipped segments of
/// lines seen almost end-on can be millions of pixels long).
pub fn dashed(painter: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke, dash: f32, gap: f32, r: Rect) {
    if let Some((p, q)) = clip_segment(a, b, r.expand(4.0)) {
        painter.extend(egui::Shape::dashed_line(&[p, q], stroke, dash, gap));
    }
}

#[cfg(test)]
mod clip_tests {
    use super::*;
    #[test]
    fn clipping_bounds_huge_segments() {
        let r = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(100.0, 100.0));
        let (p, q) = clip_segment(Pos2::new(-1e6, 50.0), Pos2::new(1e6, 50.0), r).unwrap();
        assert!(p.x.abs() < 0.1 && (q.x - 100.0).abs() < 0.1, "{p:?} {q:?}");
        assert!(clip_segment(Pos2::new(-10.0, -10.0), Pos2::new(-5.0, 200.0), r).is_none());
        assert!(clip_segment(Pos2::new(f32::NAN, 0.0), Pos2::new(1.0, 1.0), r).is_none());
    }
}
