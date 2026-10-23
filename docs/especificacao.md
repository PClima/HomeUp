# HomeUp — especificação clean-room (MVP: núcleo de modelagem)

HomeUp é um modelador 3D desktop, nativo (Rust + wgpu + egui), **inspirado no fluxo de trabalho do SketchUp**.
É uma reimplementação clean-room: nada aqui veio do binário, código, ícones ou assets do SketchUp.
A referência foi só a **documentação pública** da Trimble, capturada com o [REA](https://github.com/morluto/rea)
(`capture-browser-scenario`, árvore de acessibilidade, Chrome headless) em 2026-10-22:

| Página (help.sketchup.com/en/sketchup/…) | Usada para |
|---|---|
| `introducing-drawing-basics-and-concepts` | Linha, faces automáticas, divisão de face, inferência, travas por seta/Shift |
| `drawing-basic-shapes` | Retângulo, VCB `a,b`, inferência de quadrado / seção áurea |
| `pushing-and-pulling-shapes-3d` | Push/Pull para fora e para dentro, furo passante, distância exata |
| `selecting-geometry` | Clique, Ctrl/Shift, caixa esquerda×direita, duplo clique |
| `erasing-and-undoing` | Borracha, apagar aresta remove faces, desfazer/refazer |
| `viewing-model` | Orbitar, Pan, Zoom no cursor, Zoom Extents, atalhos |

## 1. Modelo de dados

- **Vértices** (f64, metros), **arestas** (par de vértices) e **faces** (contorno externo + furos, mais a normal).
- Eixos: vermelho = X, verde = Y, azul = Z (Z para cima).
- Faces têm frente (branca) e verso (azul-acinzentado), como no SketchUp.
- **Faces nascem das arestas**: para cada plano afetado por uma edição, o HomeUp recalcula as regiões
  fechadas do grafo plano de arestas. A travessia de semi-arestas usa ordenação angular; filamentos e pontes são descartados.
  Uma região vira face se (a) já estava coberta por uma face antiga daquele plano ou (b) é fechada por uma
  aresta nova. Isso reproduz os comportamentos descritos na documentação:
  - fechar um contorno cria a face;
  - uma linha de borda a borda **divide** a face;
  - um retângulo desenhado dentro de uma face vira **furo + face interna**;
  - apagar a aresta entre duas faces coplanares **funde** as duas; apagar a aresta de um contorno remove a face.

## 2. Ferramentas (MVP)

| Ferramenta | Atalho | Comportamento |
|---|---|---|
| Selecionar | Espaço | Clique, Ctrl adiciona, Shift alterna, Ctrl+Shift remove. Caixa para a direita = só o que está inteiro dentro; para a esquerda = tudo o que for tocado. Duplo clique numa face = face + arestas. Del apaga. |
| Linha | L | Clique-clique, continua encadeada até fechar uma face ou Esc. VCB: comprimento. |
| Retângulo | R | No plano do chão, numa face ou no plano escolhido pelas setas. VCB `largura,comprimento` (ou `;`). Inferência de quadrado e de seção áurea. |
| Push/Pull | P | Clique na face, mova e clique de novo. Ctrl força uma nova extrusão. VCB: distância, que pode ser redigitada depois da operação. Empurrar até a face oposta paralela abre um furo passante. |
| Borracha | E | Clique ou arraste sobre as arestas. A aresta e as faces que ela delimita somem (ou se fundem, se forem coplanares). |
| Orbitar / Pan / Zoom | O / H / Z | Botão do meio orbita; Shift+botão do meio faz pan; a roda dá zoom no cursor; Shift+Z = Zoom Extents. |
| Desfazer / Refazer | Ctrl+Z / Ctrl+Y | Snapshot do modelo inteiro a cada operação. |

## 3. Motor de inferência

Prioridade: **Extremidade** (verde) > **Ponto médio** (ciano) > **Na aresta** (vermelho) > **Na face** (azul)
> **Origem** > **eixos** a partir do ponto-âncora (vermelho/verde/azul) > **paralelo** à última aresta
sobrevoada (magenta) > plano de desenho.
Travas: ↑ = azul, → = vermelho, ← = verde, Shift = trava a direção atual. Esc cancela.

## 4. Caixa de medidas (VCB)

Basta digitar com uma ferramenta ativa e confirmar com Enter. Unidades: `m` (padrão), `cm`, `mm`. A vírgula decimal é aceita
(`1,5` na Linha/Push-Pull). No Retângulo, `2,3` ou `2;3` = 2 × 3.
Logo depois de concluir uma operação, digitar um valor **reaplica** essa operação com a nova medida.

## 5. Arquivo

Formato próprio `.homeup` (JSON). Exportação para `.obj` (malha triangulada).

## Fora do MVP (próximas fases)

Mover/Copiar, Rotacionar, Escala, Fita métrica/guias, Arco/Círculo/Polígono, Grupos/Componentes,
interseção de arestas que se cruzam, inferências "a partir do ponto" e perpendicular, materiais,
projeção paralela/vistas padrão, cenas, importação de `.skp` (pelo SDK oficial da Trimble, se um dia fizer sentido).

---

# Referência: SketchUp Free (web), a partir dos prints do Pedro — status em 2026-10-23

✅ feito · ⚠️ parcial · ❌ falta · ⛔ fora de escopo

## Ferramentas (menu "…")

| Grupo | Ferramenta | Status |
|---|---|---|
| Seleção | Selecionar, Laço, Borracha, Pintar, Material de amostra | ✅ |
| Desenho | Linha, Mão livre, Retângulo, Retângulo girado, Círculo, Polígono | ✅ |
| | Arco (2 pontos), Arco pelo centro, Arco de 3 pontos, Pizza | ✅ |
| Modificar | Push/Pull, Equidistância (Offset), Siga-me, Mover, Rotacionar, Escala, Virar | ✅ |
| Sólidos | Revestimento externo, União, Subtrair, Interseção, Recortar, Dividir | ✅ (CSG por BSP; resultado com faces coplanares fundidas) |
| Anotação | Fita métrica, Dimensões, Transferidor, Eixos, Texto, Texto 3D, Plano de seção, Etiqueta | ✅ |
| Câmera | Orbitar, Pan, Zoom, Janela de zoom, Extensões, Posicionar câmera, Girar, Percorrer, Anterior/Próxima | ✅ |
| IA | AI Assistant, AI Render | ❌ (exige chave de API; não iniciado) |
| Interface | Barra de ícones com submenus, busca de ferramentas, "Editar barra" | ✅ |

## Painéis da bandeja

| Painel | Status |
|---|---|
| Informações da entidade | ✅ área (nas unidades do modelo), material frontal e de verso, etiqueta, oculta; grupos: nome, material, etiqueta, oculto, projetar sombras |
| Componentes | ⚠️ lista, inserir, limpar não usados (sem miniaturas) · 3D Warehouse ⛔ (substituído pela importação de arquivos baixados e pela Biblioteca) |
| Biblioteca | ✅ móveis do Poly Haven (85 na categoria móveis + assentos, mesas, iluminação, plantas…) e materiais do ambientCG (2000+) e do Poly Haven, com busca, miniaturas, cache em disco e créditos; tudo CC0 |
| Instrutor | ✅ texto por ferramenta (sem animação) |
| Materiais | ✅ cores, opacidade e **texturas de imagem** (embutidas no .homeup), tamanho real do ladrilho, imagem própria, miniaturas |
| Estilos | ✅ 7 presets + edição (faces, raio-X, arestas, cor das arestas, céu/chão, fundo) |
| Etiquetas | ✅ visibilidade, cor, renomear, ordenar, limpar, busca, colorir por etiqueta, ferramenta Etiqueta |
| Sombras | ✅ sombras reais (shadow map) no modelo e no chão, fuso, horário, data, claro/escuro, neblina, localização (cidades + lat/long + norte) |
| Cenas | ✅ adicionar, atualizar, ir (com transição), renomear, apagar, abas |
| Exibição | ✅ reexibir todos/selecionados/último, objetos ocultos, geometria oculta, colorir por etiquetas, planos/cortes/preenchimento de seção, eixos, norte verdadeiro, guias, apagar guias, ocultar o resto do modelo |
| Atenuar/Suavizar | ✅ ângulo + coplanares, aplicado à seleção |
| Informações do modelo | ✅ unidades de comprimento/área/volume/ângulo, precisão, tamanho de texto/cota |

## Menu ☰

| Item | Status |
|---|---|
| Página inicial (recentes) · Novo · Abrir · Abrir recente · Salvar como | ✅ |
| Importar (SKP, OBJ, glTF/GLB, STL, DAE) | ✅ formatos abertos · ⚠️ SKP aguarda o SDK oficial |
| Exportar (OBJ, STL, PNG) · Imprimir | ✅ |
| Configurações do aplicativo (página inicial, figura de escala, salvamento automático, escala da interface) | ✅ |
| Adicionar localização | ✅ (painel Sombras) |
| Compartilhar · Baixar | ⛔ nuvem Trimble |

## Viewport

Céu e chão em degradê · figura de escala (componente face-me) · eixos negativos pontilhados · plano de seção com corte e preenchimento · sombras · neblina → ✅

## Verificação

- 39 testes automáticos (`cargo test`): geometria, booleanas, Siga-me, interseção, importadores, texto 3D, seção, sol, câmera, shader WGSL (naga).
- Capturas sem entrada simulada: `HOMEUP_SNAPSHOT=out.png HOMEUP_SNAPSHOT_SETUP=demo,shadows=900,panel=shadows` (ver `src/snapshot.rs`).
- ⚠️ As interações com mouse de cada ferramenta nova ainda não foram exercitadas por uma pessoa.

---

# SketchUp 2026 desktop — referência (print do Pedro + documentação via REA, 2026-10-23)

Páginas capturadas com o REA (`capture-browser-scenario`, help.sketchup.com/en/sketchup/…): `adding-colors-and-textures-materials`,
`editing-materials`, `adding-environment-to-models`, `choosing-style`, `creating-and-editing-style`, `edge-styles`,
`organizing-model`, `grouping-geometry`, `working-hierarchies-outliner`, `user-interface`, `toolbars`, `dialog-boxes-and-trays`.

| Recurso | Comportamento documentado | Status no HomeUp |
|---|---|---|
| Materiais fotorrealistas (PBR) | Cor, imagem com tamanho do ladrilho, opacidade; **Metalicidade**, **Rugosidade** (com opção Inverter), **Normal** e **Oclusão de ambiente**, cada um com valor e mapa opcional | ✅ Fatores + mapas por material; shader GGX/Schlick com reflexo do ambiente; mapas baixados do ambientCG (NormalGL, Roughness, Metalness, AO) e Poly Haven (nor_gl + arm), lidos do glTF (normal, metallicRoughness G/B, occlusion) e do OBJ (`norm`) |
| Painel Materiais | Prévia, nome, Criar/Amostrar/Padrão, abas Selecionar/Editar | ✅ Esfera de prévia renderizada na CPU, abas, slots de mapa com miniatura |
| Estilo "Materiais fotorrealistas" | Só esse estilo de faces usa os mapas PBR e o ambiente | ✅ `FaceStyle::Photoreal` (padrão) |
| Ambientes | Panoramas 360° (HDRI/EXR) para fundo e luz; usar como cúpula do céu, usar para reflexos, rotação, exposição do céu e dos reflexos | ✅ Painel Ambientes; .hdr/.exr/.jpg/.png importado ou HDRI do Poly Haven (tonemap para 8 bits) |
| Arestas do estilo | Arestas, Arestas traseiras, Perfis (px), Profundidade, Extensão (px), Pontas (px), Tremido; cor igual / por material / por eixo | ✅ Arestas, traseiras (tracejadas), perfis (pós-processo por descontinuidade de profundidade), extensão (no vertex shader), cor por material/eixo, profundidade, pontas e tremido (arestas largas desenhadas como quads no vertex shader) |
| Oclusão de ambiente | Escurecimento em cantos (estilo e mapas) | ✅ SSAO em pós-processamento + mapa AO do material |
| Sombras (desktop) | Em faces, No chão, Das arestas | ✅ Em faces, No chão, Das arestas (arestas soltas entram no mapa de sombras) |
| Outliner | Árvore de grupos/componentes; busca, por etiqueta, renomear, ocultar, bloquear, seleção sincronizada, zoom | ✅ Inclui planos de seção (ativar, ocultar, renomear, inverter, zoom, apagar) |
| Bloquear | Grupos/componentes bloqueados não podem ser editados | ✅ Mover/Rotacionar/Escala/Apagar/Editar ignoram itens bloqueados |
| Edição | Recortar, Copiar, Colar, Colar no lugar | ✅ Área de transferência interna (Colar segue o cursor com a ferramenta Mover) |
| Bandeja padrão | Painéis empilhados e recolhíveis, Gerenciar bandejas | ✅ Bandeja empilhada; Janela > Painéis da bandeja escolhe o que aparece |
| Barras | Padrão, Principal, Estilos, Sombras (data/hora), Câmera, Vistas, Seção, Sólidos; Large Tool Set à esquerda; abas de cenas com prévia; status com geolocalização, info do modelo, dicas e Medidas | ✅ Tudo, com Exibir > Barras de ferramentas. A interface simplificada (estilo web) continua em Janela > Interface |
| Menus | Arquivo, Editar, Exibir, Câmera, Desenho, Ferramentas, Procedimentos, Janela, Extensões, Ajuda | ✅ (Extensões é só um aviso: sem Ruby) |
| Miniaturas | Componentes e cenas com miniatura | ✅ Rasterizador próprio na CPU (`thumbs.rs`) |
| Biblioteca | Progresso de download | ✅ Porcentagem e barra sobre o item |

# Revisão de paridade com a ajuda do SketchUp desktop (2026-10-23, via REA)

Índice completo da ajuda capturado com o REA (~100 artigos). Artigos lidos para esta rodada:
`sketchup-copy-along`, `procedural-tools`, `matching-photo-model-or-model-photo`, `positioning-textures`,
`watermarking-model`, `applying-dashed-lines-layers`, `hatch-patterns`, `importing-and-using-2d-images`,
`generate-report`, `setting-templates`, `stretching-geometry`, `dividing-splitting-and-exploding-lines-and-faces`,
`calculating-material-area`, `replacing-material`, `using-credits-feature`, `creating-terrain-scratch`,
`modeling-terrain-and-other-rounded-shapes`, `modeling-specific-shapes-objects-and-building-features-3d`.

| Recurso | Comportamento documentado | Status no HomeUp |
|---|---|---|
| Dividir | Menu de contexto da aresta; o cursor escolhe o número de segmentos, ou digita-se | ✅ (linhas retas; curvas pedem Explodir curva antes) |
| Soldar arestas / Explodir curva | Une arestas encadeadas numa curva / separa | ✅ |
| Área | Área da seleção; Área do material (painel Materiais) | ✅ |
| Pintar: substituição | Shift = todos iguais no modelo, Ctrl = conectados, Ctrl+Shift = mesmo objeto | ✅ |
| Modelos (templates) | Novo a partir de modelo, Salvar como modelo, modelo padrão | ✅ 6 modelos prontos + modelos próprios em `%APPDATA%\HomeUp\templates` |
| Gerar relatório | Filtro modelo/seleção e nível, agrupar por atributos, colunas, CSV | ✅ (CSV com `;`; modelos de relatório .grt ficam de fora) |
| Importar imagem 2D | Como imagem (dois cliques), como textura, como foto combinada | ✅ |
| Marcas d'água | Fundo ou sobreposição, máscara, mesclar, esticada/lado a lado/posicionada, ordem | ✅ Todas as marcas de fundo (uma passada por camada, na ordem da lista) |
| Combinar foto | Barras de pontos de fuga vermelhas/verdes, origem, eixo azul para escala, cena própria, projetar texturas | ✅ Calibração por 2 pontos de fuga, com giro (roll) da câmera; projeção exata (UV projetiva) |
| Posicionar textura | Pinos vermelho (mover), verde (escala/rotação), azul (escala/inclinação), amarelo (distorcer); espelhar, girar, redefinir | ✅ Inclui pino amarelo (distorcer) e Pinos livres — homografia guardada como UV projetiva |
| Tracejados por etiqueta | Coluna Tracejado no painel Etiquetas; opção no estilo | ✅ Também nas arestas largas (croqui) |
| Hachuras | Painel Hachuras; por estilo (Override) ou por material | ✅ 7 padrões internos, importação de .PAT (até 8 famílias, 6 traços) e hachura de imagem. ❌ .skh (formato da Trimble sem especificação pública) |
| Copiar ao longo (Procedimentos) | Caminho inteiro, cada segmento, nos cantos; por quantidade/distância/espaço; escada/estrada; margem; componente procedural editável | ✅ |
| Sandbox | A partir de contornos, do zero, Smoove, Carimbar, Projetar, Adicionar detalhe, Inverter aresta | ✅ Carimbar e Projetar recortam o contorno na malha e a mantêm triangulada |
| Classificador | Tipo IFC em grupos/componentes, exportação IFC | ✅ 14 tipos IFC4, coluna no relatório, Arquivo > Exportar > IFC (IFC4, malha triangulada). Não validado em visualizador IFC externo |
| Créditos | Autores do modelo e dos componentes | ✅ Local (nome nas preferências); sem conta Trimble |
| Fora do escopo | Componentes dinâmicos, Materiais com IA, Extension Warehouse, LayOut | ❌ Ver justificativa abaixo |

**Por que ficam de fora:** Componentes dinâmicos dependem de uma linguagem de fórmulas e atributos proprietária (sem especificação pública completa); Materiais com IA usam um serviço generativo na nuvem da Trimble; Extension Warehouse executa extensões Ruby da API do SketchUp; LayOut é outro aplicativo (pranchas 2D).

# Leitor nativo de .skp (2026-10-23)

Sem acesso ao SketchUp SDK, o formato foi decifrado a partir de dois arquivos reais (SketchUp 2025 e 2027): um
cabeçalho UTF-16 seguido de um ZIP com `model.dat` (árvore de registros `u16 tag, u32 tamanho, conteúdo`),
`materials/<nome>/material.xml` + imagens, estilos e miniaturas. A tabela de tags está no topo de `src/skpfile.rs`.
Validação: os dois arquivos abrem no HomeUp com a mesma vista das miniaturas que o próprio SketchUp grava.

| Recurso | Status |
|---|---|
| Geometria (vértices, arestas, faces com furos), curvas e arcos | ✅ |
| Componentes, grupos (lista própria `0x138D`), transformações, "olhar para a câmera" | ✅ |
| Materiais: cor, transparência, textura e tamanho, metal, rugosidade, normal, AO | ✅ (posicionamento de textura por face ainda não decifrado: usa o mapeamento padrão) |
| Etiquetas e tracejados (12 estilos do SketchUp → 5 do HomeUp), etiqueta por aresta | ✅ (visibilidade das etiquetas ainda não decifrada: todas visíveis) |
| Arestas suavizadas / ocultas, instâncias ocultas | ✅ (indicadores deduzidos das amostras) |
| Câmera e localização | ✅ |
| Cenas, planos de seção, cotas, textos, estilos do arquivo | ❌ ainda não lidos |
| Arquivos anteriores ao SketchUp 2021 | ❌ formato antigo, outro leitor |
| Gravar .skp | ❌ (Salvar grava .homeup) |

# Fluxo de interiores: planta → projeto → render (2026-10-23)

| Etapa | O que entrou | Limites |
|---|---|---|
| Exportar imagem | Render fora da tela em qualquer tamanho até o limite da GPU (8K testado como opção; 4K verificado), sem interface, PNG/JPG | — |
| Escala pela Fita métrica | Medir e digitar a medida real redimensiona o modelo ou o contexto aberto | — |
| Luminárias | Pontual, spot (abertura/borda), painel (tamanho); cor/temperatura e lúmens; componentes com geometria emissiva; aparecem na vista em tempo real | Na vista em tempo real as luminárias não fazem sombra (no render sim) |
| Render fotorrealista | Path tracing na **GPU** (compute shader WGSL, mesma cena/BVH/materiais da versão CPU, despacho em blocos de 256×256 para não acionar o watchdog do driver, imagem dividida em faixas se passar do limite de buffer); a CPU (todos os núcleos) é a alternativa automática se a GPU falhar ou com `HOMEUP_PT_CPU=1`. BVH com SAH, GGX + Lambert, vidro fino, mapas PBR, NEE para luminárias e sol, céu/HDRI, exposição automática + EV, ACES, rejeição de pixels atípicos + filtro à-trous guiado | Sem cáusticas e sem subsuperfície; sala de teste 1280×720 com 128 amostras: **GPU ≈ 9 s**, CPU ≈ 75 s em 12 núcleos; 4K com 256 amostras na GPU ≈ 155 s |
| Planta DXF | Linhas, polilinhas com arco (bulge), círculos, arcos, elipses, splines, blocos → componentes, cotas (gráfico), camadas → etiquetas, unidade do arquivo, faces nas áreas fechadas | DXF binário e textos/hachuras do CAD ficam de fora |
| Planta PDF | Caminhos vetoriais (linhas, curvas, retângulos), XObjects de formulário, fluxos de objetos, página escolhida, escala 1:N | PDF escaneado → Importar imagem; PDF criptografado não abre |
| DWG | Mensagem orientando a salvar como DXF | Formato fechado da Autodesk |

