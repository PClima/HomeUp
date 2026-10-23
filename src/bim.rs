//! Classifier (IFC types on groups/components), IFC4 export with tessellated
//! geometry, and Credits (model and component authors, claimed locally).

use crate::app::HomeUpApp;
use crate::doc::Document;
use crate::geom::Geom;
use eframe::egui::{self, RichText};
use glam::{DMat4, DVec3};
use std::fmt::Write as _;

/// IFC entity, label, and how many attributes follow `Tag` in IFC4.
pub const IFC_TYPES: [(&str, &str, usize); 14] = [
    ("IfcWall", "Parede", 1),
    ("IfcSlab", "Laje / piso", 1),
    ("IfcRoof", "Telhado", 1),
    ("IfcDoor", "Porta", 5),
    ("IfcWindow", "Janela", 5),
    ("IfcColumn", "Pilar", 1),
    ("IfcBeam", "Viga", 1),
    ("IfcStair", "Escada", 1),
    ("IfcRailing", "Guarda-corpo", 1),
    ("IfcCovering", "Revestimento", 1),
    ("IfcPlate", "Placa", 1),
    ("IfcMember", "Elemento estrutural", 1),
    ("IfcFurniture", "Mobiliário", 1),
    ("IfcBuildingElementProxy", "Outro elemento", 1),
];

pub fn ifc_label(t: &str) -> &'static str {
    IFC_TYPES.iter().find(|x| x.0 == t).map(|x| x.1).unwrap_or("Sem classificação")
}

/// STEP string literal (quotes doubled, non-ASCII as \X2\…\X0\).
fn step_str(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                out.push_str("\\X2\\");
                for u in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "{:04X}", u);
                }
                out.push_str("\\X0\\");
            }
        }
    }
    out.push('\'');
    out
}

fn real(v: f64) -> String {
    format!("{:.6}", if v.abs() < 1e-12 { 0.0 } else { v })
}

/// IFC GlobalId (22 characters of the IFC base-64 alphabet).
fn guid(seed: u64) -> String {
    const A: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    let mut s = String::new();
    s.push(A[(x % 4) as usize] as char);
    for _ in 0..21 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.push(A[(x % 64) as usize] as char);
    }
    s
}

fn tris_of(doc: &Document, g: &Geom, xf: DMat4, depth: usize, pts: &mut Vec<DVec3>, idx: &mut Vec<[usize; 3]>) {
    for f in &g.faces {
        for t in g.triangulate_face(f) {
            let base = pts.len();
            for v in t {
                pts.push(xf.transform_point3(g.p(v)));
            }
            idx.push([base + 1, base + 2, base + 3]);
        }
    }
    if depth < 24 {
        for i in &g.instances {
            if !doc.inst_visible(i) {
                continue;
            }
            if let Some(d) = doc.defs.get(i.def) {
                tris_of(doc, &d.geom, xf * doc.eff_xf(i), depth + 1, pts, idx);
            }
        }
    }
}

/// IFC4 (Reference View) file: project / site / building / storey, one element
/// per top-level group or component, loose geometry as a proxy.
pub fn export_ifc(doc: &Document, name: &str) -> String {
    let mut d = String::new();
    let mut n = 100usize;
    let mut next = |d: &mut String, body: String| -> usize {
        n += 1;
        let _ = writeln!(d, "#{n}={body};");
        n
    };
    let mut g = 0u64;
    let mut gid = || {
        g += 1;
        guid(g ^ name.len() as u64)
    };
    let _ = writeln!(d, "#1=IFCCARTESIANPOINT((0.,0.,0.));");
    let _ = writeln!(d, "#2=IFCDIRECTION((0.,0.,1.));");
    let _ = writeln!(d, "#3=IFCDIRECTION((1.,0.,0.));");
    let _ = writeln!(d, "#4=IFCAXIS2PLACEMENT3D(#1,#2,#3);");
    let _ = writeln!(d, "#5=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#4,$);");
    let _ = writeln!(d, "#6=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);");
    let _ = writeln!(d, "#7=IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.);");
    let _ = writeln!(d, "#8=IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.);");
    let _ = writeln!(d, "#9=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);");
    let _ = writeln!(d, "#10=IFCUNITASSIGNMENT((#6,#7,#8,#9));");
    let _ = writeln!(d, "#11=IFCPROJECT({},$,{},$,$,$,$,(#5),#10);", step_str(&gid()), step_str(name));
    let _ = writeln!(d, "#12=IFCLOCALPLACEMENT($,#4);");
    let _ = writeln!(d, "#13=IFCSITE({},$,{},$,$,#12,$,$,.ELEMENT.,$,$,$,$,$);", step_str(&gid()), step_str(&doc.location.name));
    let _ = writeln!(d, "#14=IFCBUILDING({},$,'Edificação',$,$,#12,$,$,.ELEMENT.,$,$,$);", step_str(&gid()));
    let _ = writeln!(d, "#15=IFCBUILDINGSTOREY({},$,'Pavimento térreo',$,$,#12,$,$,.ELEMENT.,0.);", step_str(&gid()));
    let _ = writeln!(d, "#16=IFCRELAGGREGATES({},$,$,$,#11,(#13));", step_str(&gid()));
    let _ = writeln!(d, "#17=IFCRELAGGREGATES({},$,$,$,#13,(#14));", step_str(&gid()));
    let _ = writeln!(d, "#18=IFCRELAGGREGATES({},$,$,$,#14,(#15));", step_str(&gid()));
    let mut elements = Vec::new();
    let mut element = |d: &mut String, label: &str, object_type: &str, ty: &str, extra: usize, pts: &[DVec3], idx: &[[usize; 3]], gidv: String| {
        if idx.is_empty() {
            return;
        }
        let coords: Vec<String> = pts.iter().map(|p| format!("({},{},{})", real(p.x), real(p.y), real(p.z))).collect();
        let tris: Vec<String> = idx.iter().map(|t| format!("({},{},{})", t[0], t[1], t[2])).collect();
        let cl = next(d, format!("IFCCARTESIANPOINTLIST3D(({}),$)", coords.join(",")));
        let fs = next(d, format!("IFCTRIANGULATEDFACESET(#{cl},$,$,({}),$)", tris.join(",")));
        let sr = next(d, format!("IFCSHAPEREPRESENTATION(#5,'Body','Tessellation',(#{fs}))"));
        let pd = next(d, format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{sr}))"));
        let tail = vec!["$"; extra].join(",");
        let e = next(d, format!("{}({},$,{},$,{},#12,#{pd},$,{tail})", ty.to_uppercase(), step_str(&gidv), step_str(label), step_str(object_type)));
        elements.push(e);
    };
    // loose geometry at the root
    let (mut pts, mut idx) = (Vec::new(), Vec::new());
    let loose = Geom { instances: Vec::new(), ..doc.root.clone() };
    tris_of(doc, &loose, DMat4::IDENTITY, 0, &mut pts, &mut idx);
    element(&mut d, "Geometria solta", "", "IfcBuildingElementProxy", 1, &pts, &idx, gid());
    for inst in &doc.root.instances {
        if !doc.inst_visible(inst) {
            continue;
        }
        let Some(def) = doc.defs.get(inst.def) else { continue };
        let (mut pts, mut idx) = (Vec::new(), Vec::new());
        tris_of(doc, &def.geom, doc.eff_xf(inst), 1, &mut pts, &mut idx);
        let ty = def.ifc.clone().unwrap_or_else(|| "IfcBuildingElementProxy".into());
        let extra = IFC_TYPES.iter().find(|t| t.0 == ty).map(|t| t.2).unwrap_or(1);
        let label = if inst.name.is_empty() { def.name.clone() } else { inst.name.clone() };
        element(&mut d, &label, &def.name, &ty, extra, &pts, &idx, gid());
    }
    let list: Vec<String> = elements.iter().map(|e| format!("#{e}")).collect();
    if !list.is_empty() {
        next(&mut d, format!("IFCRELCONTAINEDINSPATIALSTRUCTURE({},$,$,$,({}),#15)", step_str(&gid()), list.join(",")));
    }
    let mut out = String::new();
    out.push_str("ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition [ReferenceView]'),'2;1');\n");
    let _ = writeln!(out, "FILE_NAME({},'2026-10-23T00:00:00',(''),(''),'HomeUp','HomeUp {}','');", step_str(&format!("{name}.ifc")), env!("CARGO_PKG_VERSION"));
    out.push_str("FILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n");
    out.push_str(&d);
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    out
}

impl HomeUpApp {
    pub(crate) fn export_ifc_file(&mut self) {
        let name = self.file.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or("modelo".into());
        let Some(p) = rfd::FileDialog::new().add_filter("IFC", &["ifc"]).set_file_name(format!("{name}.ifc")).save_file() else { return };
        match std::fs::write(&p, export_ifc(&self.doc, &name)) {
            Ok(()) => self.message = format!("IFC exportado: {}", p.display()),
            Err(e) => self.message = format!("Erro ao exportar IFC: {e}"),
        }
    }

    /// Entity Info: classification of a group/component.
    pub(crate) fn classifier_ui(&mut self, ui: &mut egui::Ui, def: usize) {
        let cur = self.doc.defs[def].ifc.clone();
        let mut pick = None;
        ui.horizontal(|ui| {
            ui.label("Tipo IFC:");
            egui::ComboBox::from_id_salt("ifc-type").selected_text(cur.as_deref().map(ifc_label).unwrap_or("Sem classificação")).show_ui(ui, |ui| {
                if ui.selectable_label(cur.is_none(), "Sem classificação").clicked() {
                    pick = Some(None);
                }
                for (t, l, _) in IFC_TYPES {
                    if ui.selectable_label(cur.as_deref() == Some(t), format!("{l}  ({t})")).clicked() {
                        pick = Some(Some(t.to_string()));
                    }
                }
            });
        });
        if let Some(p) = pick {
            self.checkpoint_keep_sel();
            self.doc.defs[def].ifc = p;
        }
        let author = self.doc.defs[def].author.clone();
        ui.horizontal(|ui| {
            ui.label("Autor:");
            ui.label(author.unwrap_or_else(|| "—".into()));
            if !self.prefs.author_name.trim().is_empty() && ui.small_button("Reivindicar").on_hover_text("Registrar você como autor deste componente").clicked() {
                self.checkpoint_keep_sel();
                self.doc.defs[def].author = Some(self.prefs.author_name.trim().to_string());
            }
        });
    }

    /// Model Info > Credits.
    pub(crate) fn credits_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Créditos").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Seu nome:");
                if ui.text_edit_singleline(&mut self.prefs.author_name).lost_focus() {
                    self.prefs.save();
                }
            });
            let name = self.prefs.author_name.trim().to_string();
            if ui.add_enabled(!name.is_empty() && !self.doc.credits.contains(&name), egui::Button::new("Reivindicar crédito do modelo")).clicked() {
                self.checkpoint_keep_sel();
                self.doc.credits.push(name);
            }
            ui.label(RichText::new("Autores do modelo").strong());
            if self.doc.credits.is_empty() {
                ui.weak("Ninguém reivindicou este modelo.");
            }
            for c in self.doc.credits.clone() {
                ui.label(format!("• {c}"));
            }
            let mut comp: Vec<String> = self.doc.defs.iter().filter_map(|d| d.author.clone()).collect();
            comp.sort();
            comp.dedup();
            ui.label(RichText::new("Autores de componentes").strong());
            if comp.is_empty() {
                ui.weak("Nenhum componente com autor.");
            }
            for c in comp {
                ui.label(format!("• {c}"));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ifc_export_has_elements() {
        let mut doc = Document::with_scale_figure();
        doc.defs[0].ifc = Some("IfcFurniture".into());
        doc.root.add_polygon(&[DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y], None, false, DVec3::new(0.2, 0.3, 5.0));
        let s = export_ifc(&doc, "Teste ç");
        assert!(s.starts_with("ISO-10303-21;"));
        assert!(s.contains("FILE_SCHEMA(('IFC4'))"));
        assert!(s.contains("IFCFURNITURE("));
        assert!(s.contains("IFCBUILDINGELEMENTPROXY("));
        assert_eq!(s.matches("IFCTRIANGULATEDFACESET(").count(), 2);
        assert!(s.contains("\\X2\\00E7\\X0\\"), "non-ASCII is encoded");
        assert!(s.trim_end().ends_with("END-ISO-10303-21;"));
        assert_eq!(guid(7).len(), 22);
    }
}
