//! Tray panels: Tags, Styles, Shadows, Display, Soften/Smooth, Model Info, Instructor.

use crate::app::{HomeUpApp, In, Tool};
use crate::doc::Owner;
use crate::settings::{EdgeColor, FaceStyle, Style, Tag, CITIES};
use crate::units::{AreaUnit, LenUnit, VolUnit};
use eframe::egui::{self, Color32, RichText};

const TAG_COLORS: [[u8; 3]; 8] =
    [[230, 126, 34], [52, 152, 219], [46, 204, 113], [155, 89, 182], [241, 196, 15], [231, 76, 60], [26, 188, 156], [127, 140, 141]];

/// Instructor texts: (title, description, steps, modifiers, tips).
fn instructor(t: Tool) -> (&'static str, &'static str, &'static [&'static str], &'static [&'static str], &'static [&'static str]) {
    match t {
        Tool::Select => (
            "Ferramenta Selecionar",
            "Selecione itens e objetos no modelo.",
            &["Clique em um item ou objeto."],
            &["Ctrl = adicionar ao conjunto de seleção.", "Shift = adicionar/subtrair.", "Shift+Ctrl = subtrair."],
            &[
                "Clique duas vezes numa face para selecioná-la com todas as arestas.",
                "Clique duas vezes numa aresta para selecionar a borda e as faces.",
                "Clique três vezes para selecionar tudo o que está conectado.",
                "Clique duas vezes num grupo para editá-lo.",
                "Ctrl+A seleciona tudo; Ctrl+T limpa a seleção.",
            ],
        ),
        Tool::Line => (
            "Ferramenta Linha",
            "Desenha arestas. Arestas que fecham um contorno plano criam uma face.",
            &["Clique o ponto inicial.", "Clique o ponto final (ou digite o comprimento + Enter).", "Continue clicando para encadear; Esc encerra."],
            &["Setas = travar eixo vermelho/verde/azul.", "Shift = travar a inferência atual.", "Seta para baixo = paralelo à última aresta."],
            &["Uma linha de borda a borda divide a face.", "Linhas que se cruzam são divididas no cruzamento."],
        ),
        Tool::Rect | Tool::RotRect => (
            "Ferramenta Retângulo",
            "Desenha retângulos (e faces) em qualquer plano.",
            &["Clique o primeiro canto.", "Clique o canto oposto ou digite \"largura;comprimento\"."],
            &["Setas antes do primeiro clique = escolher o plano."],
            &["Fique atento às dicas Quadrado e Seção áurea.", "Desenhado sobre uma face, o retângulo a divide."],
        ),
        Tool::Circle | Tool::Polygon => (
            "Ferramenta Círculo / Polígono",
            "Desenha círculos e polígonos regulares.",
            &["Clique o centro.", "Clique para definir o raio (ou digite o raio)."],
            &["Digite \"24s\" para mudar o número de lados.", "Setas = escolher o plano."],
            &["Ao extrudar um círculo, as laterais ficam suavizadas."],
        ),
        Tool::Arc | Tool::ArcCenter | Tool::Arc3 | Tool::Pie => (
            "Ferramentas de Arco",
            "Arco de 2 pontos, pelo centro, de 3 pontos e pizza.",
            &["Siga as dicas na barra de status para cada variante."],
            &["Digite \"12s\" para o número de segmentos.", "O ângulo encaixa a cada 15°."],
            &["Uma pizza cria uma face fechada."],
        ),
        Tool::PushPull => (
            "Ferramenta Empurrar/Puxar",
            "Transforma uma face em volume, ou remove volume.",
            &["Clique numa face.", "Mova o cursor e clique (ou digite a distância)."],
            &["Toque Ctrl = criar uma nova extrusão em vez de mover a face."],
            &["Duplo clique numa face repete a última distância.", "Empurre até a face oposta para abrir um furo."],
        ),
        Tool::Move => (
            "Ferramenta Mover",
            "Move, copia e estica geometria.",
            &["Selecione algo (ou passe o mouse sobre).", "Clique o ponto base.", "Clique o destino (ou digite a distância)."],
            &["Toque Ctrl = copiar.", "Depois de copiar: \"x3\" cria 3 cópias, \"/3\" distribui 3 cópias."],
            &["Mover uma face ou aresta estica a geometria conectada."],
        ),
        Tool::Rotate => (
            "Ferramenta Rotacionar",
            "Gira a seleção em torno de um ponto.",
            &["Clique o centro.", "Clique a referência.", "Clique o ângulo (ou digite-o)."],
            &["Toque Ctrl = copiar (matriz radial com \"x6\")."],
            &["O transferidor encaixa a cada 15°."],
        ),
        Tool::Scale => (
            "Ferramenta Escala",
            "Redimensiona a seleção pelas alças.",
            &["Selecione algo.", "Arraste uma alça verde.", "Clique (ou digite o fator)."],
            &["Toque Ctrl = escalar pelo centro."],
            &["Fator -1 espelha a seleção."],
        ),
        Tool::Tape => (
            "Ferramenta Fita métrica",
            "Mede distâncias e cria guias.",
            &["Clique num ponto e depois em outro para medir.", "Clique numa aresta e arraste para criar uma guia paralela."],
            &[],
            &["Digite o valor para posicionar a guia exatamente."],
        ),
        Tool::Paint => (
            "Ferramenta Pintar",
            "Aplica materiais às faces e aos grupos.",
            &["Escolha um material no painel Materiais.", "Clique na face."],
            &["Alt = material de amostra (conta-gotas).", "Shift = trocar todas as faces com o mesmo material."],
            &["Pintar um grupo pinta as faces sem material dentro dele."],
        ),
        Tool::Eraser => (
            "Ferramenta Borracha",
            "Apaga arestas, guias, cotas e textos.",
            &["Clique ou arraste sobre as entidades."],
            &["Shift = ocultar.", "Ctrl = suavizar.", "Ctrl+Shift = desfazer a suavização."],
            &["Apagar uma aresta remove as faces que ela delimitava."],
        ),
        Tool::FollowMe => (
            "Ferramenta Siga-me",
            "Varre um perfil ao longo de um caminho.",
            &["Selecione as arestas do caminho.", "Clique na face do perfil."],
            &[],
            &["Use um círculo ao longo de um caminho para fazer tubos.", "Caminhos fechados não geram tampas."],
        ),
        _ => ("Ferramenta", "Siga as instruções na barra de status.", &[], &[], &[]),
    }
}

impl HomeUpApp {
    pub(crate) fn instructor_ui(&mut self, ui: &mut egui::Ui) {
        let (title, desc, steps, mods, tips) = instructor(self.tool);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 110.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 4.0, Color32::WHITE);
        let s = 80.0;
        crate::icons::paint(ui.painter(), egui::Rect::from_center_size(rect.center(), egui::vec2(s, s)), crate::icons::Icon::Tool(self.tool));
        ui.label(RichText::new(if title == "Ferramenta" { self.tool.label() } else { title }).strong().size(16.0));
        ui.label(desc);
        if !steps.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new("Operação da ferramenta").strong());
            for (k, s) in steps.iter().enumerate() {
                ui.label(format!("{}. {s}", k + 1));
            }
        }
        if !mods.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new("Teclas modificadoras").strong());
            for s in mods.iter() {
                ui.label(format!("• {s}"));
            }
        }
        if !tips.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new("Dicas").strong());
            for s in tips.iter() {
                ui.label(format!("• {s}"));
            }
        }
    }

    pub(crate) fn tags_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("+ Etiqueta").clicked() {
                self.checkpoint_keep_sel();
                let n = self.doc.tags.len();
                self.doc.tags.push(Tag { name: format!("Etiqueta {}", n + 1), visible: true, color: TAG_COLORS[n % TAG_COLORS.len()], ..Default::default() });
            }
            if ui.button("A-Z").on_hover_text("Ordenar por nome").clicked() {
                self.sort_tags();
            }
            if ui.button("Limpar").on_hover_text("Apagar etiquetas não usadas").clicked() {
                self.purge_tags();
            }
            ui.checkbox(&mut self.display.color_by_tag, "Cores").on_hover_text("Colorir por etiquetas");
        });
        ui.add(egui::TextEdit::singleline(&mut self.tag_search).hint_text("Buscar etiquetas..."));
        ui.separator();
        let q = crate::chrome::fold(&self.tag_search);
        ui.horizontal(|ui| {
            let r = ui.radio(self.cur_tag.is_none(), "Sem etiquetas");
            if r.clicked() {
                self.cur_tag = None;
            }
        });
        let mut del = None;
        for k in 0..self.doc.tags.len() {
            if !q.is_empty() && !crate::chrome::fold(&self.doc.tags[k].name).contains(&q) {
                continue;
            }
            ui.horizontal(|ui| {
                let t = &mut self.doc.tags[k];
                if ui.checkbox(&mut t.visible, "").on_hover_text("Visível").changed() {
                    self.dirty = true;
                    self.cache_dirty = true;
                }
                let mut c = t.color;
                if ui.color_edit_button_srgb(&mut c).changed() {
                    t.color = c;
                    self.dirty = true;
                }
                ui.add(egui::TextEdit::singleline(&mut t.name).desired_width(110.0));
                let before = t.dash;
                egui::ComboBox::from_id_salt(("tag-dash", k)).width(86.0).selected_text(t.dash.label()).show_ui(ui, |ui| {
                    for d in crate::hatch::Dash::ALL {
                        ui.selectable_value(&mut t.dash, d, d.label());
                    }
                });
                if t.dash != before {
                    self.dirty = true;
                    self.modified = true;
                }
                if ui.radio(self.cur_tag == Some(k), "").on_hover_text("Etiqueta ativa (ferramenta Etiqueta)").clicked() {
                    self.cur_tag = Some(k);
                }
                if ui.small_button("X").clicked() {
                    del = Some(k);
                }
            });
        }
        if let Some(k) = del {
            self.checkpoint_keep_sel();
            self.remove_tag(k);
        }
    }

    fn for_each_geom(&mut self, mut f: impl FnMut(&mut crate::geom::Geom)) {
        f(&mut self.doc.root);
        for d in &mut self.doc.defs {
            f(&mut d.geom);
        }
    }

    fn remove_tag(&mut self, k: usize) {
        self.doc.tags.remove(k);
        let fix = |t: &mut Option<usize>| {
            *t = match *t {
                Some(i) if i == k => None,
                Some(i) if i > k => Some(i - 1),
                o => o,
            }
        };
        self.for_each_geom(|g| {
            for e in &mut g.edges {
                fix(&mut e.tag);
            }
            for f in &mut g.faces {
                fix(&mut f.tag);
            }
            for i in &mut g.instances {
                fix(&mut i.tag);
            }
        });
        self.cur_tag = None;
        self.dirty = true;
        self.cache_dirty = true;
    }

    fn sort_tags(&mut self) {
        let mut order: Vec<usize> = (0..self.doc.tags.len()).collect();
        order.sort_by_key(|i| crate::chrome::fold(&self.doc.tags[*i].name));
        let mut map = vec![0; order.len()];
        for (new, old) in order.iter().enumerate() {
            map[*old] = new;
        }
        self.checkpoint_keep_sel();
        self.doc.tags = order.iter().map(|i| self.doc.tags[*i].clone()).collect();
        self.for_each_geom(|g| {
            for f in &mut g.faces {
                f.tag = f.tag.map(|t| map[t]);
            }
            for i in &mut g.instances {
                i.tag = i.tag.map(|t| map[t]);
            }
        });
        self.cur_tag = None;
    }

    fn purge_tags(&mut self) {
        let mut used = vec![false; self.doc.tags.len()];
        let mut mark = |t: Option<usize>| {
            if let Some(t) = t.filter(|t| *t < used.len()) {
                used[t] = true;
            }
        };
        for g in std::iter::once(&self.doc.root).chain(self.doc.defs.iter().map(|d| &d.geom)) {
            g.faces.iter().for_each(|f| mark(f.tag));
            g.instances.iter().for_each(|i| mark(i.tag));
        }
        self.checkpoint_keep_sel();
        for k in (0..used.len()).rev() {
            if !used[k] {
                self.remove_tag(k);
            }
        }
    }

    /// Tag tool: click entities to assign the active tag.
    pub(crate) fn tag_tool(&mut self, i: &In) {
        self.snap = None;
        let (o, d) = self.cam.ray(i.px);
        let hit = self.cache.pick_face(o, d).map(|h| (self.cache.tris[h.1].owner, self.cache.tris[h.1].face));
        self.hover_face = None;
        self.hover_inst = None;
        match hit {
            Some((Owner::Active, f)) => self.hover_face = Some(f),
            Some((Owner::Inst(k), _)) => self.hover_inst = Some(k),
            _ => {}
        }
        if !i.pressed {
            return;
        }
        if i.mods.alt {
            self.cur_tag = match hit {
                Some((Owner::Active, f)) => self.active().faces[f].tag,
                Some((Owner::Inst(k), _)) => self.active().instances[k].tag,
                _ => self.cur_tag,
            };
            return;
        }
        let tag = self.cur_tag;
        match hit {
            Some((Owner::Active, f)) => {
                self.checkpoint_keep_sel();
                self.active_mut().faces[f].tag = tag;
            }
            Some((Owner::Inst(k), _)) => {
                self.checkpoint_keep_sel();
                self.active_mut().instances[k].tag = tag;
            }
            _ => {}
        }
    }

    pub(crate) fn styles_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new(&self.doc.style.name).strong());
        ui.separator();
        ui.label("Biblioteca de estilos:");
        for p in Style::presets() {
            let sel = self.doc.style == p;
            if ui.selectable_label(sel, &p.name).clicked() {
                self.checkpoint_keep_sel();
                self.doc.style = p;
                self.dirty = true;
            }
        }
        ui.separator();
        ui.label(RichText::new("Editar estilo").strong());
        let mut st = self.doc.style.clone();
        ui.horizontal(|ui| {
            for (k, t) in ["Arestas", "Faces", "Fundo", "Marca d'água"].iter().enumerate() {
                ui.selectable_value(&mut self.style_tab, k as u8, *t);
            }
        });
        ui.separator();
        match self.style_tab {
            0 => {
                ui.checkbox(&mut st.edges, "Arestas");
                ui.checkbox(&mut st.back_edges, "Arestas traseiras (tracejadas)");
                ui.checkbox(&mut st.dashes, "Tracejados das etiquetas");
                ui.horizontal(|ui| {
                    let mut on = st.profiles > 0.0;
                    if ui.checkbox(&mut on, "Perfis").changed() {
                        st.profiles = if on { 2.0 } else { 0.0 };
                    }
                    if on {
                        ui.add(egui::DragValue::new(&mut st.profiles).range(1.0..=10.0).speed(0.1).suffix(" px"));
                    }
                });
                ui.horizontal(|ui| {
                    let mut on = st.extension > 0.0;
                    if ui.checkbox(&mut on, "Extensão").changed() {
                        st.extension = if on { 6.0 } else { 0.0 };
                    }
                    if on {
                        ui.add(egui::DragValue::new(&mut st.extension).range(1.0..=40.0).speed(0.2).suffix(" px"));
                    }
                });
                for (label, v, def, max) in [("Profundidade", &mut st.depth_cue, 3.0f32, 10.0f32), ("Pontas", &mut st.endpoints, 6.0, 30.0), ("Tremido", &mut st.jitter, 1.5, 8.0)] {
                    ui.horizontal(|ui| {
                        let mut on = *v > 0.0;
                        if ui.checkbox(&mut on, label).changed() {
                            *v = if on { def } else { 0.0 };
                        }
                        if on {
                            ui.add(egui::DragValue::new(v).range(0.5..=max).speed(0.1).suffix(" px"));
                        }
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Cor:");
                    egui::ComboBox::from_id_salt("edge-color-mode").selected_text(st.edge_mode.label()).show_ui(ui, |ui| {
                        for m in EdgeColor::ALL {
                            ui.selectable_value(&mut st.edge_mode, m, m.label());
                        }
                    });
                    ui.color_edit_button_srgb(&mut st.edge_color);
                });
            }
            1 => {
                egui::ComboBox::from_label("Estilo de face").selected_text(st.face.label()).show_ui(ui, |ui| {
                    for f in FaceStyle::ALL {
                        ui.selectable_value(&mut st.face, f, f.label());
                    }
                });
                ui.checkbox(&mut st.xray, "Raio-X");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut st.ao, "Oclusão de ambiente");
                    if st.ao {
                        ui.add(egui::Slider::new(&mut st.ao_strength, 0.05..=1.0).fixed_decimals(2));
                    }
                });
            }
            3 => {}
            _ => {
                ui.checkbox(&mut st.sky, "Céu e chão");
                ui.horizontal(|ui| {
                    ui.label("Cor de fundo:");
                    ui.color_edit_button_srgb(&mut st.bg);
                });
                ui.weak("Panoramas 360° ficam no painel Ambientes.");
            }
        }
        if st != self.doc.style {
            if !st.name.ends_with('*') {
                st.name.push('*');
            }
            self.doc.style = st;
            self.modified = true;
            self.dirty = true;
        }
        if self.style_tab == 3 {
            self.watermark_ui(ui);
        }
    }

    pub(crate) fn shadows_ui(&mut self, ui: &mut egui::Ui) {
        let mut s = self.doc.shadows.clone();
        ui.checkbox(&mut s.on, RichText::new("Sombras").strong());
        ui.horizontal(|ui| {
            ui.label("Fuso horário (UTC):");
            ui.add(egui::DragValue::new(&mut s.tz).range(-12.0..=14.0).speed(0.25).suffix(" h"));
        });
        let mut minutes = s.minutes as f32;
        ui.label(format!("Horário: {:02}:{:02}", s.minutes / 60, s.minutes % 60));
        if ui.add(egui::Slider::new(&mut minutes, 0.0..=1439.0).show_value(false)).changed() {
            s.minutes = minutes as u32;
        }
        const MONTHS: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];
        let mut doy = (s.month - 1) as f32 * 30.4 + s.day as f32;
        ui.label(format!("Data: {:02}/{}", s.day, MONTHS[(s.month - 1) as usize]));
        if ui.add(egui::Slider::new(&mut doy, 1.0..=365.0).show_value(false)).changed() {
            let m = ((doy - 1.0) / 30.4).floor().clamp(0.0, 11.0) as u32;
            s.month = m + 1;
            s.day = ((doy - m as f32 * 30.4).round() as u32).clamp(1, 31);
        }
        ui.add(egui::Slider::new(&mut s.light, 0.0..=100.0).text("Claro"));
        ui.add(egui::Slider::new(&mut s.dark, 0.0..=100.0).text("Escuro"));
        ui.horizontal(|ui| {
            ui.label("Exibir:");
            ui.checkbox(&mut s.on_faces, "Em faces");
            ui.checkbox(&mut s.on_ground, "No chão");
            ui.checkbox(&mut s.from_edges, "Das arestas");
        });
        egui::CollapsingHeader::new("Configurações de neblina").show(ui, |ui| {
            ui.checkbox(&mut s.fog, "Exibir neblina");
            ui.add(egui::Slider::new(&mut s.fog_start, 0.0..=500.0).text("Início (m)"));
            ui.add(egui::Slider::new(&mut s.fog_end, 1.0..=2000.0).text("Fim (m)"));
        });
        if s != self.doc.shadows {
            self.doc.shadows = s;
            self.modified = true;
            self.dirty = true;
        }
        ui.separator();
        let d = crate::settings::sun_direction(&self.doc.shadows, &self.doc.location);
        ui.weak(format!("Sol: elevação {:.0}°", d.z.asin().to_degrees()));
        egui::CollapsingHeader::new(format!("Localização: {}", self.doc.location.name)).show(ui, |ui| self.location_ui(ui));
    }

    pub(crate) fn location_ui(&mut self, ui: &mut egui::Ui) {
        let mut loc = self.doc.location.clone();
        let mut tz = self.doc.shadows.tz;
        egui::ComboBox::from_label("Cidade").selected_text(&loc.name).show_ui(ui, |ui| {
            for (n, lat, lon, z) in CITIES {
                if ui.selectable_label(loc.name == n, n).clicked() {
                    loc.name = n.into();
                    loc.lat = lat;
                    loc.lon = lon;
                    tz = z;
                }
            }
        });
        egui::Grid::new("loc").show(ui, |ui| {
            ui.label("Latitude:");
            ui.add(egui::DragValue::new(&mut loc.lat).range(-90.0..=90.0).speed(0.01).suffix("°"));
            ui.end_row();
            ui.label("Longitude:");
            ui.add(egui::DragValue::new(&mut loc.lon).range(-180.0..=180.0).speed(0.01).suffix("°"));
            ui.end_row();
            ui.label("Norte verdadeiro:");
            ui.add(egui::DragValue::new(&mut loc.north_deg).range(-360.0..=360.0).speed(0.5).suffix("°"));
            ui.end_row();
        });
        if loc != self.doc.location || tz != self.doc.shadows.tz {
            self.doc.location = loc;
            self.doc.shadows.tz = tz;
            self.modified = true;
        }
    }

    pub(crate) fn display_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Reexibir").strong());
        ui.horizontal(|ui| {
            if ui.button("Todos").clicked() {
                self.unhide(UnhideWhat::All);
            }
            if ui.button("Selecionados").clicked() {
                self.unhide(UnhideWhat::Selected);
            }
            if ui.button("Último").clicked() {
                self.unhide(UnhideWhat::Last);
            }
        });
        ui.separator();
        ui.label(RichText::new("Exibição").strong());
        let d = &mut self.display;
        let before = d.clone();
        ui.checkbox(&mut d.hidden_objects, "Objetos ocultos");
        ui.checkbox(&mut d.hidden_geometry, "Geometria oculta");
        ui.checkbox(&mut d.color_by_tag, "Colorir por etiquetas");
        ui.checkbox(&mut d.section_planes, "Planos de seção");
        ui.horizontal(|ui| {
            ui.checkbox(&mut d.section_cuts, "Cortes de seção");
            ui.add(egui::DragValue::new(&mut d.cut_width).range(1.0..=10.0).speed(0.1));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut d.section_fill, "Preenchimento de seção");
            ui.color_edit_button_srgb(&mut d.fill_color);
        });
        ui.checkbox(&mut d.axes, "Eixos");
        ui.checkbox(&mut d.true_north, "Norte verdadeiro");
        ui.checkbox(&mut d.guides, "Guias");
        let changed_hidden = before.hidden_objects != d.hidden_objects || before.hidden_geometry != d.hidden_geometry;
        if *d != before {
            self.dirty = true;
        }
        if changed_hidden {
            self.cache_dirty = true;
        }
        if ui.button("Apagar todas as guias").clicked() && !self.doc.guides.is_empty() {
            self.checkpoint_keep_sel();
            self.doc.guides.clear();
        }
        ui.separator();
        ui.label(RichText::new("Planos de seção").strong());
        let mut del = None;
        let mut activate = None;
        for k in 0..self.doc.sections.len() {
            ui.horizontal(|ui| {
                let s = &mut self.doc.sections[k];
                if ui.radio(s.active, format!("Seção {}", k + 1)).clicked() {
                    activate = Some(k);
                }
                let mut off = s.p.dot(s.n.normalize());
                if ui.add(egui::DragValue::new(&mut off).speed(0.02).suffix(" m")).on_hover_text("Deslocar ao longo da normal").changed() {
                    let n = s.n.normalize();
                    s.p += n * (off - s.p.dot(n));
                    self.dirty = true;
                }
                if ui.small_button("Inverter").clicked() {
                    s.n = -s.n;
                    self.dirty = true;
                }
                if ui.small_button("X").clicked() {
                    del = Some(k);
                }
            });
        }
        if let Some(k) = activate {
            let was = self.doc.sections[k].active;
            for s in &mut self.doc.sections {
                s.active = false;
            }
            self.doc.sections[k].active = !was;
            self.dirty = true;
        }
        if let Some(k) = del {
            self.checkpoint_keep_sel();
            self.doc.sections.remove(k);
        }
        if self.doc.sections.is_empty() {
            ui.weak("Use a ferramenta Plano de seção para criar um corte.");
        }
        ui.separator();
        ui.label(RichText::new("Editar componente").strong());
        if ui.checkbox(&mut self.display.hide_rest, "Ocultar o resto do modelo").changed() {
            self.dirty = true;
            self.cache_dirty = true;
        }
    }

    pub(crate) fn soften_ui(&mut self, ui: &mut egui::Ui) {
        let n = self.sel.edges.len();
        let mut coplanar = self.soften_coplanar;
        let mut angle = self.soften_angle;
        ui.checkbox(&mut coplanar, "Suavizar arestas coplanares");
        ui.add(egui::Slider::new(&mut angle, 0.0..=180.0).text("Ângulo").suffix("°"));
        ui.label(if n == 0 { "Sem bordas selecionadas".to_string() } else { format!("{n} arestas selecionadas") });
        let changed = coplanar != self.soften_coplanar || angle != self.soften_angle;
        self.soften_coplanar = coplanar;
        self.soften_angle = angle;
        if (changed || ui.button("Aplicar").clicked()) && n > 0 {
            if !self.soften_dragging {
                self.checkpoint_keep_sel();
                self.soften_dragging = true;
            }
            self.apply_soften();
        }
        if !ui.input(|i| i.pointer.any_down()) {
            self.soften_dragging = false;
        }
    }

    fn apply_soften(&mut self) {
        let cos = (self.soften_angle.to_radians() as f64).cos();
        let coplanar = self.soften_coplanar;
        let edges: Vec<usize> = self.sel.edges.iter().copied().collect();
        let g = self.doc.geom_mut(&self.path);
        for e in edges {
            let [a, b] = g.edges[e].v;
            let fs: Vec<glam::DVec3> = g.faces.iter().filter(|f| f.has_edge(a, b)).map(|f| f.normal).collect();
            if fs.len() != 2 {
                continue;
            }
            let d = fs[0].dot(fs[1]);
            let is_coplanar = d > 1.0 - 1e-9;
            g.edges[e].soft = if is_coplanar { coplanar } else { d >= cos };
        }
        self.dirty = true;
        self.cache_dirty = true;
    }

    pub(crate) fn model_info_ui(&mut self, ui: &mut egui::Ui) {
        self.credits_ui(ui);
        let mut u = self.doc.units.clone();
        egui::CollapsingHeader::new("Unidades de comprimento").default_open(true).show(ui, |ui| {
            egui::ComboBox::from_label("Unidade").selected_text(u.length.label()).show_ui(ui, |ui| {
                for l in LenUnit::ALL {
                    ui.selectable_value(&mut u.length, l, l.label());
                }
            });
            ui.add(egui::Slider::new(&mut u.precision, 0..=6).text("Precisão (casas decimais)"));
        });
        egui::CollapsingHeader::new("Unidades de área").show(ui, |ui| {
            egui::ComboBox::from_id_salt("area").selected_text(u.area.label()).show_ui(ui, |ui| {
                for a in AreaUnit::ALL {
                    ui.selectable_value(&mut u.area, a, a.label());
                }
            });
        });
        egui::CollapsingHeader::new("Unidades de volume").show(ui, |ui| {
            egui::ComboBox::from_id_salt("vol").selected_text(u.volume.label()).show_ui(ui, |ui| {
                for v in VolUnit::ALL {
                    ui.selectable_value(&mut u.volume, v, v.label());
                }
            });
        });
        egui::CollapsingHeader::new("Unidades de ângulo").show(ui, |ui| {
            ui.add(egui::Slider::new(&mut u.angle_precision, 0..=4).text("Precisão (graus)"));
        });
        egui::CollapsingHeader::new("Configurações de texto").show(ui, |ui| {
            ui.add(egui::Slider::new(&mut u.text_size, 8.0..=32.0).text("Tamanho do texto"));
        });
        egui::CollapsingHeader::new("Configurações de dimensão").show(ui, |ui| {
            ui.add(egui::Slider::new(&mut u.dim_size, 8.0..=32.0).text("Tamanho da cota"));
        });
        if u != self.doc.units {
            self.doc.units = u;
            self.modified = true;
        }
        ui.separator();
        let g = &self.doc.root;
        ui.weak(format!(
            "Modelo: {} faces, {} arestas, {} definições, {} materiais, {} etiquetas",
            g.faces.len(),
            g.edges.len(),
            self.doc.defs.len(),
            self.doc.materials.len(),
            self.doc.tags.len()
        ));
    }

    /// Hide the current selection (context menu "Ocultar").
    pub(crate) fn hide_selection(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        let sel = self.sel.clone();
        self.checkpoint();
        let g = self.doc.geom_mut(&self.path);
        for f in &sel.faces {
            g.faces[*f].hidden = true;
        }
        for e in &sel.edges {
            g.edges[*e].hidden = true;
        }
        for i in &sel.insts {
            g.instances[*i].hidden = true;
        }
        self.last_hidden = sel;
    }

    pub(crate) fn unhide(&mut self, what: UnhideWhat) {
        self.checkpoint_keep_sel();
        let sel = match what {
            UnhideWhat::Selected => Some(self.sel.clone()),
            UnhideWhat::Last => Some(self.last_hidden.clone()),
            UnhideWhat::All => None,
        };
        let g = self.doc.geom_mut(&self.path);
        match sel {
            None => {
                g.faces.iter_mut().for_each(|f| f.hidden = false);
                g.edges.iter_mut().for_each(|e| e.hidden = false);
                g.instances.iter_mut().for_each(|i| i.hidden = false);
            }
            Some(s) => {
                for f in &s.faces {
                    if let Some(x) = g.faces.get_mut(*f) {
                        x.hidden = false;
                    }
                }
                for e in &s.edges {
                    if let Some(x) = g.edges.get_mut(*e) {
                        x.hidden = false;
                    }
                }
                for i in &s.insts {
                    if let Some(x) = g.instances.get_mut(*i) {
                        x.hidden = false;
                    }
                }
            }
        }
        self.dirty = true;
        self.cache_dirty = true;
    }
}

#[derive(Clone, Copy)]
pub enum UnhideWhat {
    All,
    Selected,
    Last,
}
