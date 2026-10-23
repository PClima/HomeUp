//! Generate Report: tables of the groups/components in the model (quantities,
//! sizes, tags, materials, area), grouped by chosen attributes, exported as CSV.

use crate::app::HomeUpApp;
use crate::doc::Document;
use crate::geom::Geom;
use eframe::egui::{self, RichText};
use glam::{DMat4, DVec3};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Attr {
    Definition,
    Instance,
    Tag,
    Material,
    LenX,
    LenY,
    LenZ,
    PosX,
    PosY,
    PosZ,
    Area,
    Level,
    Ifc,
}

impl Attr {
    pub const ALL: [Attr; 13] =
        [Attr::Definition, Attr::Instance, Attr::Ifc, Attr::Tag, Attr::Material, Attr::LenX, Attr::LenY, Attr::LenZ, Attr::PosX, Attr::PosY, Attr::PosZ, Attr::Area, Attr::Level];
    pub fn label(self) -> &'static str {
        match self {
            Attr::Definition => "Nome da definição",
            Attr::Instance => "Nome da instância",
            Attr::Tag => "Etiqueta",
            Attr::Material => "Material",
            Attr::LenX => "LenX",
            Attr::LenY => "LenY",
            Attr::LenZ => "LenZ",
            Attr::PosX => "Posição X",
            Attr::PosY => "Posição Y",
            Attr::PosZ => "Posição Z",
            Attr::Area => "Área das faces",
            Attr::Level => "Nível",
            Attr::Ifc => "Tipo IFC",
        }
    }
    fn numeric(self) -> bool {
        matches!(self, Attr::LenX | Attr::LenY | Attr::LenZ | Attr::PosX | Attr::PosY | Attr::PosZ | Attr::Area)
    }
}

/// One group/component instance with its attribute values.
#[derive(Clone, Debug)]
pub struct Item {
    pub def: String,
    pub name: String,
    pub tag: String,
    pub material: String,
    pub len: DVec3,
    pub pos: DVec3,
    pub area: f64,
    pub level: usize,
    pub ifc: String,
}

impl Item {
    fn text(&self, a: Attr) -> String {
        match a {
            Attr::Definition => self.def.clone(),
            Attr::Instance => self.name.clone(),
            Attr::Tag => self.tag.clone(),
            Attr::Material => self.material.clone(),
            Attr::Level => self.level.to_string(),
            Attr::Ifc => self.ifc.clone(),
            _ => fmt(a, self.num(a)),
        }
    }
    fn num(&self, a: Attr) -> f64 {
        match a {
            Attr::LenX => self.len.x,
            Attr::LenY => self.len.y,
            Attr::LenZ => self.len.z,
            Attr::PosX => self.pos.x,
            Attr::PosY => self.pos.y,
            Attr::PosZ => self.pos.z,
            Attr::Area => self.area,
            _ => 0.0,
        }
    }
}

fn fmt(a: Attr, v: f64) -> String {
    if a == Attr::Area {
        crate::units::format_area(v)
    } else {
        crate::units::format_len(v)
    }
}

#[derive(Clone, Debug)]
pub struct ReportCfg {
    pub selection_only: bool,
    /// Deepest nesting level included (2 = top-level objects only).
    pub max_level: usize,
    pub group_by: Vec<Attr>,
    pub columns: Vec<Attr>,
}

impl Default for ReportCfg {
    /// "Component Quantities": group by definition name and count.
    fn default() -> Self {
        ReportCfg { selection_only: false, max_level: 2, group_by: vec![Attr::Definition], columns: vec![Attr::LenX, Attr::LenY, Attr::LenZ, Attr::Area] }
    }
}

fn geom_area(doc: &Document, g: &Geom, xf: DMat4, depth: usize) -> f64 {
    let mut a: f64 = g.faces.iter().map(|f| crate::editops::face_area_xf(g, f, xf)).sum();
    if depth < 24 {
        for i in &g.instances {
            if let Some(d) = doc.defs.get(i.def) {
                a += geom_area(doc, &d.geom, xf * doc.eff_xf(i), depth + 1);
            }
        }
    }
    a
}

/// Collect instances (level 2 = children of the model root).
pub fn collect(doc: &Document, g: &Geom, xf: DMat4, level: usize, only: Option<&std::collections::HashSet<usize>>, max_level: usize, out: &mut Vec<Item>) {
    if level > max_level || level > 24 {
        return;
    }
    for (k, inst) in g.instances.iter().enumerate() {
        if only.is_some_and(|s| !s.contains(&k)) {
            continue;
        }
        let Some(d) = doc.defs.get(inst.def) else { continue };
        let m = xf * doc.eff_xf(inst);
        let (lo, hi) = doc.def_bounds(inst.def).unwrap_or((DVec3::ZERO, DVec3::ZERO));
        let mut a = DVec3::splat(f64::MAX);
        let mut b = DVec3::splat(f64::MIN);
        for c in 0..8 {
            let p = m.transform_point3(DVec3::new(if c & 1 == 0 { lo.x } else { hi.x }, if c & 2 == 0 { lo.y } else { hi.y }, if c & 4 == 0 { lo.z } else { hi.z }));
            a = a.min(p);
            b = b.max(p);
        }
        out.push(Item {
            def: if d.group { "Grupo".into() } else { d.name.clone() },
            name: inst.name.clone(),
            tag: inst.tag.and_then(|t| doc.tags.get(t)).map(|t| t.name.clone()).unwrap_or_else(|| "Sem etiqueta".into()),
            material: inst.material.and_then(|t| doc.materials.get(t)).map(|t| t.name.clone()).unwrap_or_default(),
            len: b - a,
            pos: m.transform_point3(DVec3::ZERO),
            area: geom_area(doc, &d.geom, m, 0),
            level,
            ifc: d.ifc.as_deref().map(|t| format!("{} ({t})", crate::bim::ifc_label(t))).unwrap_or_default(),
        });
        collect(doc, &d.geom, m, level + 1, None, max_level, out);
    }
}

/// Build the table: header + rows. Grouped reports add "Quantidade"; string
/// columns become "value (count)" sets, numeric ones totals.
pub fn table(items: &[Item], cfg: &ReportCfg) -> (Vec<String>, Vec<Vec<String>>) {
    if cfg.group_by.is_empty() {
        let header = cfg.columns.iter().map(|a| a.label().to_string()).collect();
        let rows = items.iter().map(|it| cfg.columns.iter().map(|a| it.text(*a)).collect()).collect();
        return (header, rows);
    }
    let mut header: Vec<String> = cfg.group_by.iter().map(|a| a.label().to_string()).collect();
    header.push("Quantidade".into());
    for a in &cfg.columns {
        header.push(if a.numeric() { format!("{} (total)", a.label()) } else { a.label().to_string() });
    }
    let mut groups: Vec<(Vec<String>, Vec<&Item>)> = Vec::new();
    for it in items {
        let key: Vec<String> = cfg.group_by.iter().map(|a| it.text(*a)).collect();
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(it),
            None => groups.push((key, vec![it])),
        }
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    let rows = groups
        .into_iter()
        .map(|(key, members)| {
            let mut row = key;
            row.push(members.len().to_string());
            for a in &cfg.columns {
                if a.numeric() && matches!(a, Attr::Area) {
                    row.push(fmt(*a, members.iter().map(|m| m.num(*a)).sum()));
                } else {
                    // set of values with counts
                    let mut set: Vec<(String, usize)> = Vec::new();
                    for m in &members {
                        let v = m.text(*a);
                        match set.iter_mut().find(|s| s.0 == v) {
                            Some(s) => s.1 += 1,
                            None => set.push((v, 1)),
                        }
                    }
                    row.push(set.iter().map(|(v, n)| format!("{v} ({n})")).collect::<Vec<_>>().join(", "));
                }
            }
            row
        })
        .collect();
    (header, rows)
}

/// CSV with ';' (numbers use the decimal comma).
pub fn to_csv(header: &[String], rows: &[Vec<String>]) -> String {
    let cell = |s: &str| if s.contains(';') || s.contains('"') || s.contains('\n') { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
    let mut out = String::from("\u{feff}");
    for r in std::iter::once(header.to_vec()).chain(rows.iter().cloned()) {
        out += &r.iter().map(|c| cell(c)).collect::<Vec<_>>().join(";");
        out += "\r\n";
    }
    out
}

impl HomeUpApp {
    pub(crate) fn report_window(&mut self, ctx: &egui::Context) {
        if !self.show_report {
            return;
        }
        let mut open = true;
        let mut export = None;
        egui::Window::new("Gerar relatório").open(&mut open).collapsible(false).default_width(620.0).show(ctx, |ui| {
            let cfg = &mut self.report_cfg;
            ui.horizontal(|ui| {
                ui.label("Incluir:");
                ui.radio_value(&mut cfg.selection_only, false, "Modelo inteiro");
                ui.radio_value(&mut cfg.selection_only, true, "Seleção atual");
                ui.separator();
                ui.label("Níveis:");
                ui.add(egui::DragValue::new(&mut cfg.max_level).range(2..=12)).on_hover_text("2 = só objetos do nível superior; mais = inclui aninhados");
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Agrupar por:").strong());
                for a in Attr::ALL.into_iter().filter(|a| !a.numeric()) {
                    let mut on = cfg.group_by.contains(&a);
                    if ui.checkbox(&mut on, a.label()).changed() {
                        if on {
                            cfg.group_by.push(a);
                            cfg.columns.retain(|c| *c != a);
                        } else {
                            cfg.group_by.retain(|c| *c != a);
                        }
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Colunas:").strong());
                for a in Attr::ALL.into_iter().filter(|a| !cfg.group_by.contains(a)) {
                    let mut on = cfg.columns.contains(&a);
                    if ui.checkbox(&mut on, a.label()).changed() {
                        if on {
                            cfg.columns.push(a);
                        } else {
                            cfg.columns.retain(|c| *c != a);
                        }
                    }
                }
            });
            if ui.small_button("Restaurar \"Quantidades de componentes\"").clicked() {
                *cfg = ReportCfg::default();
            }
            ui.separator();
            let cfg = self.report_cfg.clone();
            let mut items = Vec::new();
            let only = cfg.selection_only.then(|| self.sel.insts.clone());
            collect(&self.doc, self.doc.geom(&self.path), self.doc.xf(&self.path), 2, only.as_ref(), cfg.max_level, &mut items);
            let (header, rows) = table(&items, &cfg);
            egui::ScrollArea::both().max_height(320.0).show(ui, |ui| {
                egui::Grid::new("report-grid").striped(true).show(ui, |ui| {
                    for h in &header {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for r in &rows {
                        for c in r {
                            ui.label(c);
                        }
                        ui.end_row();
                    }
                });
            });
            if rows.is_empty() {
                ui.weak("Nenhum grupo ou componente encontrado.");
            }
            ui.separator();
            if ui.add_enabled(!rows.is_empty(), egui::Button::new("Baixar CSV…")).clicked() {
                export = Some(to_csv(&header, &rows));
            }
        });
        if let Some(csv) = export {
            if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("relatorio.csv").save_file() {
                match std::fs::write(&p, csv) {
                    Ok(()) => self.message = format!("Relatório salvo em {}", p.display()),
                    Err(e) => self.message = format!("Erro ao salvar o relatório: {e}"),
                }
            }
        }
        if !open {
            self.show_report = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn component_quantities() {
        let mut doc = Document::with_scale_figure();
        let inst = doc.root.instances[0].clone();
        doc.root.instances.push(crate::geom::Instance { xf: DMat4::from_translation(DVec3::X * 3.0), ..inst });
        let mut items = Vec::new();
        collect(&doc, &doc.root, DMat4::IDENTITY, 2, None, 2, &mut items);
        assert_eq!(items.len(), 2);
        let (h, rows) = table(&items, &ReportCfg::default());
        assert_eq!(h[1], "Quantidade");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][1], "2");
        let csv = to_csv(&h, &rows);
        assert!(csv.lines().count() == 2 && csv.contains(';'));
    }
}
