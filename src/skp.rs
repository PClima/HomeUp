//! SKP import through the official SketchUp C SDK (SketchUpAPI.dll), loaded at
//! runtime. HomeUp builds and runs without it; `.skp` import is enabled when
//! the DLL is found. The SDK is downloaded by the user from
//! https://developer.sketchup.com (Trimble account + license acceptance) and
//! extracted into `third_party/sketchup-sdk/` (git-ignored).
//!
//! NOTE: the FFI signatures below follow the public SketchUp C API and must be
//! checked against the headers shipped with the SDK version in use.

use crate::import::{ImportMat, Imported, MeshPart};
use glam::{DMat4, DVec3};
use libloading::{Library, Symbol};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::path::{Path, PathBuf};

#[repr(C)]
#[derive(Clone, Copy)]
struct Ref {
    ptr: *mut c_void,
}

impl Ref {
    const NULL: Ref = Ref { ptr: std::ptr::null_mut() };
    fn is_null(&self) -> bool {
        self.ptr.is_null()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point3D {
    x: f64,
    y: f64,
    z: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Transformation {
    values: [f64; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Color {
    red: u8,
    green: u8,
    blue: u8,
    alpha: u8,
}

type Res = i32;
const OK: Res = 0;

/// Candidate locations of SketchUpAPI.dll.
pub fn sdk_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("HOMEUP_SKP_SDK") {
        v.push(PathBuf::from(p).join("SketchUpAPI.dll"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            v.push(dir.join("SketchUpAPI.dll"));
            // target/<profile>/homeup.exe -> repo root
            if let Some(repo) = dir.parent().and_then(|d| d.parent()) {
                v.push(repo.join("third_party/sketchup-sdk/binaries/sketchup/x64/SketchUpAPI.dll"));
            }
        }
    }
    v.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("third_party/sketchup-sdk/binaries/sketchup/x64/SketchUpAPI.dll"));
    v
}

pub fn find_sdk() -> Option<PathBuf> {
    sdk_candidates().into_iter().find(|p| p.exists())
}

fn open_lib(path: &Path) -> Result<Library, String> {
    #[cfg(windows)]
    unsafe {
        // Resolve the SDK's dependent DLLs from its own folder.
        use libloading::os::windows::{Library as WinLib, LOAD_WITH_ALTERED_SEARCH_PATH};
        WinLib::load_with_flags(path, LOAD_WITH_ALTERED_SEARCH_PATH).map(Library::from).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    unsafe {
        Library::new(path).map_err(|e| e.to_string())
    }
}

struct Api<'l> {
    lib: &'l Library,
}

macro_rules! sym {
    ($api:expr, $name:literal, $ty:ty) => {{
        let s: Symbol<$ty> = unsafe { $api.lib.get($name.as_bytes()) }.map_err(|e| format!("{}: {e}", $name))?;
        s
    }};
}

type FnVoid = unsafe extern "C" fn();
type FnModelFromFile = unsafe extern "C" fn(*mut Ref, *const c_char) -> Res;
/// SUModelCreateFromFileWithStatus(SUModelRef*, const char*, enum SUModelLoadStatus*)
type FnModelFromFileStatus = unsafe extern "C" fn(*mut Ref, *const c_char, *mut i32) -> Res;
type FnRefOut = unsafe extern "C" fn(Ref, *mut Ref) -> Res;
type FnRelease = unsafe extern "C" fn(*mut Ref) -> Res;
type FnCount = unsafe extern "C" fn(Ref, *mut usize) -> Res;
type FnList = unsafe extern "C" fn(Ref, usize, *mut Ref, *mut usize) -> Res;
type FnXf = unsafe extern "C" fn(Ref, *mut Transformation) -> Res;
type FnMeshCreate = unsafe extern "C" fn(*mut Ref, Ref) -> Res;
type FnPoints = unsafe extern "C" fn(Ref, usize, *mut Point3D, *mut usize) -> Res;
type FnIndices = unsafe extern "C" fn(Ref, usize, *mut usize, *mut usize) -> Res;
type FnColor = unsafe extern "C" fn(Ref, *mut Color) -> Res;
type FnDouble = unsafe extern "C" fn(Ref, *mut f64) -> Res;
type FnToElem = unsafe extern "C" fn(Ref) -> Ref;
type FnStrCreate = unsafe extern "C" fn(*mut Ref) -> Res;
type FnStrGet = unsafe extern "C" fn(Ref, usize, *mut c_char, *mut usize) -> Res;
type FnNameOut = unsafe extern "C" fn(Ref, *mut Ref) -> Res;

struct Walker<'a> {
    get_num_faces: Symbol<'a, FnCount>,
    get_faces: Symbol<'a, FnList>,
    get_num_groups: Symbol<'a, FnCount>,
    get_groups: Symbol<'a, FnList>,
    get_num_insts: Symbol<'a, FnCount>,
    get_insts: Symbol<'a, FnList>,
    group_entities: Symbol<'a, FnRefOut>,
    group_xf: Symbol<'a, FnXf>,
    group_elem: Symbol<'a, FnToElem>,
    inst_def: Symbol<'a, FnRefOut>,
    inst_xf: Symbol<'a, FnXf>,
    inst_elem: Symbol<'a, FnToElem>,
    def_entities: Symbol<'a, FnRefOut>,
    elem_material: Symbol<'a, FnRefOut>,
    face_material: Symbol<'a, FnRefOut>,
    mesh_create: Symbol<'a, FnMeshCreate>,
    mesh_release: Symbol<'a, FnRelease>,
    mesh_num_verts: Symbol<'a, FnCount>,
    mesh_verts: Symbol<'a, FnPoints>,
    mesh_num_tris: Symbol<'a, FnCount>,
    mesh_indices: Symbol<'a, FnIndices>,
    mat_color: Symbol<'a, FnColor>,
    mat_opacity: Symbol<'a, FnDouble>,
    mat_name: Symbol<'a, FnNameOut>,
    str_create: Symbol<'a, FnStrCreate>,
    str_get: Symbol<'a, FnStrGet>,
    str_release: Symbol<'a, FnRelease>,
    mats: HashMap<usize, usize>,
    out: Imported,
}

impl Walker<'_> {
    fn list(&self, count: &FnCount, get: &FnList, ents: Ref) -> Vec<Ref> {
        let mut n = 0usize;
        unsafe {
            if count(ents, &mut n) != OK || n == 0 {
                return vec![];
            }
            let mut v = vec![Ref::NULL; n];
            let mut got = 0usize;
            if get(ents, n, v.as_mut_ptr(), &mut got) != OK {
                return vec![];
            }
            v.truncate(got);
            v
        }
    }

    fn material(&mut self, m: Ref) -> Option<usize> {
        if m.is_null() {
            return None;
        }
        let key = m.ptr as usize;
        if let Some(i) = self.mats.get(&key) {
            return Some(*i);
        }
        let mut c = Color::default();
        let mut op = 1.0f64;
        let mut name = format!("SKP {}", self.out.materials.len() + 1);
        unsafe {
            (self.mat_color)(m, &mut c);
            (self.mat_opacity)(m, &mut op);
            let mut s = Ref::NULL;
            if (self.str_create)(&mut s) == OK {
                if (self.mat_name)(m, &mut s) == OK {
                    let mut buf = vec![0 as c_char; 512];
                    let mut len = 0usize;
                    if (self.str_get)(s, buf.len(), buf.as_mut_ptr(), &mut len) == OK {
                        let bytes: Vec<u8> = buf[..len.min(buf.len())].iter().map(|b| *b as u8).collect();
                        let t = String::from_utf8_lossy(&bytes).trim_end_matches('\0').to_string();
                        if !t.is_empty() {
                            name = t;
                        }
                    }
                }
                (self.str_release)(&mut s);
            }
        }
        let i = self.out.materials.len();
        self.out.materials.push(ImportMat { name, color: [c.red, c.green, c.blue], alpha: (op as f32).clamp(0.05, 1.0), ..Default::default() });
        self.mats.insert(key, i);
        Some(i)
    }

    fn walk(&mut self, ents: Ref, xf: DMat4, inherited: Option<usize>, depth: usize) {
        if depth > 64 {
            return;
        }
        let flip = xf.determinant() < 0.0;
        for face in self.list(&self.get_num_faces, &self.get_faces, ents) {
            let mut fm = Ref::NULL;
            let mat = unsafe {
                if (self.face_material)(face, &mut fm) == OK { self.material(fm) } else { None }
            }
            .or(inherited);
            let mut mesh = Ref::NULL;
            unsafe {
                if (self.mesh_create)(&mut mesh, face) != OK {
                    continue;
                }
                let (mut nv, mut nt) = (0usize, 0usize);
                (self.mesh_num_verts)(mesh, &mut nv);
                (self.mesh_num_tris)(mesh, &mut nt);
                let mut pts = vec![Point3D::default(); nv];
                let mut got = 0usize;
                (self.mesh_verts)(mesh, nv, pts.as_mut_ptr(), &mut got);
                let mut idx = vec![0usize; nt * 3];
                let mut goti = 0usize;
                (self.mesh_indices)(mesh, nt * 3, idx.as_mut_ptr(), &mut goti);
                (self.mesh_release)(&mut mesh);
                let positions = pts[..got.min(nv)].iter().map(|p| xf.transform_point3(DVec3::new(p.x, p.y, p.z))).collect();
                let tris = idx[..goti.min(idx.len())]
                    .chunks_exact(3)
                    .map(|t| if flip { [t[0] as u32, t[2] as u32, t[1] as u32] } else { [t[0] as u32, t[1] as u32, t[2] as u32] })
                    .collect();
                self.out.parts.push(MeshPart { positions, tris, material: mat, uvs: Vec::new() });
            }
        }
        for g in self.list(&self.get_num_groups, &self.get_groups, ents) {
            let mut t = Transformation { values: [0.0; 16] };
            let mut sub = Ref::NULL;
            let mut gm = Ref::NULL;
            unsafe {
                if (self.group_xf)(g, &mut t) != OK || (self.group_entities)(g, &mut sub) != OK {
                    continue;
                }
                let elem = (self.group_elem)(g);
                let m = if (self.elem_material)(elem, &mut gm) == OK { self.material(gm) } else { None };
                self.walk(sub, xf * DMat4::from_cols_array(&t.values), m.or(inherited), depth + 1);
            }
        }
        for inst in self.list(&self.get_num_insts, &self.get_insts, ents) {
            let mut t = Transformation { values: [0.0; 16] };
            let (mut def, mut sub, mut im) = (Ref::NULL, Ref::NULL, Ref::NULL);
            unsafe {
                if (self.inst_xf)(inst, &mut t) != OK || (self.inst_def)(inst, &mut def) != OK || (self.def_entities)(def, &mut sub) != OK {
                    continue;
                }
                let elem = (self.inst_elem)(inst);
                let m = if (self.elem_material)(elem, &mut im) == OK { self.material(im) } else { None };
                self.walk(sub, xf * DMat4::from_cols_array(&t.values), m.or(inherited), depth + 1);
            }
        }
    }
}

pub fn load_skp(path: &Path) -> Result<Imported, String> {
    let dll = find_sdk().ok_or_else(|| {
        "Importar .skp requer o SketchUp SDK oficial. Baixe em developer.sketchup.com e extraia em \
         HomeUp/third_party/sketchup-sdk (ou defina HOMEUP_SKP_SDK)."
            .to_string()
    })?;
    let lib = open_lib(&dll)?;
    let api = Api { lib: &lib };
    let init = sym!(api, "SUInitialize", FnVoid);
    let term = sym!(api, "SUTerminate", FnVoid);
    let from_file = sym!(api, "SUModelCreateFromFile", FnModelFromFile);
    let model_entities = sym!(api, "SUModelGetEntities", FnRefOut);
    let model_release = sym!(api, "SUModelRelease", FnRelease);
    let mut w = Walker {
        get_num_faces: sym!(api, "SUEntitiesGetNumFaces", FnCount),
        get_faces: sym!(api, "SUEntitiesGetFaces", FnList),
        get_num_groups: sym!(api, "SUEntitiesGetNumGroups", FnCount),
        get_groups: sym!(api, "SUEntitiesGetGroups", FnList),
        get_num_insts: sym!(api, "SUEntitiesGetNumInstances", FnCount),
        get_insts: sym!(api, "SUEntitiesGetInstances", FnList),
        group_entities: sym!(api, "SUGroupGetEntities", FnRefOut),
        group_xf: sym!(api, "SUGroupGetTransform", FnXf),
        group_elem: sym!(api, "SUGroupToDrawingElement", FnToElem),
        inst_def: sym!(api, "SUComponentInstanceGetDefinition", FnRefOut),
        inst_xf: sym!(api, "SUComponentInstanceGetTransform", FnXf),
        inst_elem: sym!(api, "SUComponentInstanceToDrawingElement", FnToElem),
        def_entities: sym!(api, "SUComponentDefinitionGetEntities", FnRefOut),
        elem_material: sym!(api, "SUDrawingElementGetMaterial", FnRefOut),
        face_material: sym!(api, "SUFaceGetFrontMaterial", FnRefOut),
        mesh_create: sym!(api, "SUMeshHelperCreate", FnMeshCreate),
        mesh_release: sym!(api, "SUMeshHelperRelease", FnRelease),
        mesh_num_verts: sym!(api, "SUMeshHelperGetNumVertices", FnCount),
        mesh_verts: sym!(api, "SUMeshHelperGetVertices", FnPoints),
        mesh_num_tris: sym!(api, "SUMeshHelperGetNumTriangles", FnCount),
        mesh_indices: sym!(api, "SUMeshHelperGetVertexIndices", FnIndices),
        mat_color: sym!(api, "SUMaterialGetColor", FnColor),
        mat_opacity: sym!(api, "SUMaterialGetOpacity", FnDouble),
        mat_name: sym!(api, "SUMaterialGetName", FnNameOut),
        str_create: sym!(api, "SUStringCreate", FnStrCreate),
        str_get: sym!(api, "SUStringGetUTF8", FnStrGet),
        str_release: sym!(api, "SUStringRelease", FnRelease),
        mats: HashMap::new(),
        out: Imported { meters_per_unit: Some(0.0254), z_up: Some(true), ..Default::default() },
    };
    let cpath = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    unsafe {
        init();
        let mut model = Ref::NULL;
        // newer SDKs report when the file comes from a more recent SketchUp
        let with_status: Option<Symbol<FnModelFromFileStatus>> = lib.get(b"SUModelCreateFromFileWithStatus").ok();
        let mut status = 0i32;
        let r = match &with_status {
            Some(f) => f(&mut model, cpath.as_ptr(), &mut status),
            None => from_file(&mut model, cpath.as_ptr()),
        };
        if r != OK || model.is_null() {
            term();
            return Err(format!("O SDK não conseguiu abrir o arquivo (código {r}). Versão do .skp mais nova que o SDK?"));
        }
        if status == 1 {
            w.out.warning = Some("o arquivo foi salvo por um SketchUp mais novo que o SDK; atualize o SDK se faltar algo.".into());
        }
        let mut ents = Ref::NULL;
        if model_entities(model, &mut ents) == OK {
            w.walk(ents, DMat4::IDENTITY, None, 0);
        }
        model_release(&mut model);
        term();
    }
    Ok(w.out)
}
