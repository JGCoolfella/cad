//! The scene and camera as the webview sends them (`src/native/view.ts`), and their
//! conversion to `oc-render`'s [`Scene`] and [`Camera`]. The webview decides what is
//! shown (document visibility rules, hover, selection); this only translates.
use glam::{DQuat, DVec3};
use oc_render::{
    AxisItem, Background, BodyItem, BodyRole, Camera, CsysItem, DisplayStyle, Frame, Hit, LabelId, LodChoice, MeshId, Options,
    OriginItem, PlaneItem, PointItem, Rgb, Scene, Section, SelectorItem, SketchItem,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

type V3 = [f64; 3];

fn v(a: V3) -> DVec3 {
    DVec3::from_array(a)
}

#[derive(Deserialize, Clone, Copy)]
pub struct FrameDto {
    pub origin: V3,
    pub x: V3,
    pub y: V3,
    pub normal: V3,
}

impl FrameDto {
    fn to(self) -> Frame {
        Frame { origin: v(self.origin), x: v(self.x), y: v(self.y), normal: v(self.normal) }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BodyDto {
    pub id: u32,
    /// Engine mesh handle.
    pub mesh: u32,
    /// `"part"`, `"preview"`, `"previewCut"` or `"highlight"` (an interference volume).
    pub role: String,
    pub color: String,
    #[serde(default)]
    pub transparent: bool,
    #[serde(default)]
    pub preview_faces: Vec<u32>,
    /// Assembly occurrences: where this mesh is drawn (`rotation` a quaternion w, x, y, z).
    #[serde(default)]
    pub placement: Option<PlacementDto>,
    /// The body's own display style (`style` values; G1d component display modes), else the
    /// scene's.
    #[serde(default)]
    pub style: Option<String>,
    /// False: drawn but not picked (a component SmartMates is dragging).
    #[serde(default = "yes")]
    pub pickable: bool,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize, Clone, Copy)]
pub struct PlacementDto {
    pub position: V3,
    pub rotation: [f64; 4],
}

impl PlacementDto {
    fn to(self) -> oc_render::Placement {
        let [w, x, y, z] = self.rotation;
        oc_render::Placement { rotation: DQuat::from_xyzw(x, y, z, w).normalize(), translation: v(self.position) }
    }
}

#[derive(Deserialize)]
pub struct PlaneDto {
    pub id: u32,
    pub frame: FrameDto,
    pub extents: [f64; 4],
    /// Label key from `view_add_label`.
    pub label: Option<u32>,
}

#[derive(Deserialize)]
pub struct AxisDto {
    pub id: u32,
    pub point: V3,
    pub dir: V3,
    pub length: f64,
}

#[derive(Deserialize)]
pub struct PointDto {
    pub id: u32,
    pub point: V3,
}

#[derive(Deserialize)]
pub struct CsysDto {
    pub id: u32,
    pub frame: FrameDto,
}

#[derive(Deserialize)]
pub struct SketchDto {
    pub id: u32,
    /// Flat world coordinates, two points per segment.
    pub segments: Vec<f64>,
    #[serde(default)]
    pub profile: bool,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(tag = "k", rename_all = "lowercase")]
pub enum HitDto {
    Origin,
    Face { body: u32, index: u32 },
    Edge { body: u32, index: u32 },
    Vertex { body: u32, index: u32 },
    Plane { i: u32 },
    Feature { i: u32 },
}

impl From<Hit> for HitDto {
    fn from(h: Hit) -> HitDto {
        match h {
            Hit::Origin => HitDto::Origin,
            Hit::Face { body, index } => HitDto::Face { body, index },
            Hit::Edge { body, index } => HitDto::Edge { body, index },
            Hit::Vertex { body, index } => HitDto::Vertex { body, index },
            Hit::Plane(i) => HitDto::Plane { i },
            Hit::Feature(i) => HitDto::Feature { i },
        }
    }
}

impl HitDto {
    fn to(&self) -> Hit {
        match *self {
            HitDto::Origin => Hit::Origin,
            HitDto::Face { body, index } => Hit::Face { body, index },
            HitDto::Edge { body, index } => Hit::Edge { body, index },
            HitDto::Vertex { body, index } => Hit::Vertex { body, index },
            HitDto::Plane { i } => Hit::Plane(i),
            HitDto::Feature { i } => Hit::Feature(i),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionsDto {
    pub hover: String,
    pub selected: String,
    pub edges: String,
    pub sketch_inactive: String,
    pub temp_shaded: String,
    /// `"scene"`, `"gradient"` or `"plain"`.
    pub bg_mode: String,
    pub bg_top: String,
    pub bg_bottom: String,
    pub plane_front: String,
    pub plane_back: String,
    pub plane_transparency: f64,
    pub shaded_planes: bool,
    pub hidden_edges_dashed: bool,
    pub shaded_face_highlight: bool,
    pub dynamic_highlight: bool,
    pub triad: bool,
    pub msaa: bool,
    /// Plain line width in device px (`Options::line_width`; 1.5 when absent).
    #[serde(default)]
    pub line_width: Option<f32>,
}

#[derive(Deserialize)]
pub struct SectionDto {
    pub origin: V3,
    pub normal: V3,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneDto {
    /// `"shadedEdges" | "shaded" | "hlr" | "hlv" | "wireframe"`.
    pub style: String,
    pub options: OptionsDto,
    pub bodies: Vec<BodyDto>,
    pub planes: Vec<PlaneDto>,
    pub axes: Vec<AxisDto>,
    pub points: Vec<PointDto>,
    pub csys: Vec<CsysDto>,
    pub sketches: Vec<SketchDto>,
    pub origin_shown: bool,
    pub origin_pickable: bool,
    pub section: Option<SectionDto>,
    pub hover: Option<HitDto>,
    pub selected: Vec<HitDto>,
    /// Mesh handles the webview still holds (its rebuild and preview results).
    pub retain: Vec<u32>,
    /// The newest mesh handle the webview has received; newer ones are still in flight.
    pub seen: u32,
    /// Frame number; picks name the frame whose scene they need (`view_pick`).
    #[serde(default)]
    pub seq: u32,
    /// The View Selector when it is open: the hovered facet.
    #[serde(default)]
    pub selector: Option<SelectorDto>,
}

#[derive(Deserialize)]
pub struct SelectorDto {
    pub hover: Option<u32>,
}

/// The webview's camera (`ViewCamera` in `src/render/camera.ts`): the same fields as
/// [`Camera`], so both project identically.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CameraDto {
    pub target: V3,
    /// x, y, z, w.
    pub quat: [f64; 4],
    pub height: f64,
    pub perspective: bool,
    pub width: f64,
    pub height_px: f64,
}

impl CameraDto {
    pub fn to(&self) -> Camera {
        let mut c = Camera::default();
        c.target = v(self.target);
        let [x, y, z, w] = self.quat;
        c.quat = DQuat::from_xyzw(x, y, z, w).normalize();
        c.height = self.height;
        c.perspective = self.perspective;
        c.resize(self.width, self.height_px);
        c.view_name = None;
        c
    }
}

fn rgb(s: &str) -> Rgb {
    if s.starts_with('#') && (s.len() == 7 || s.len() == 4) {
        Rgb::hex(s)
    } else {
        Rgb::hex("#808080")
    }
}

fn style(s: &str) -> DisplayStyle {
    match s {
        "shaded" => DisplayStyle::Shaded,
        "hlr" => DisplayStyle::HiddenLinesRemoved,
        "hlv" => DisplayStyle::HiddenLinesVisible,
        "wireframe" => DisplayStyle::Wireframe,
        _ => DisplayStyle::ShadedEdges,
    }
}

impl SceneDto {
    /// `meshes` maps engine handles to uploaded meshes, `labels` label keys to images;
    /// bodies whose mesh is not uploaded are left out.
    pub fn to(&self, meshes: &HashMap<u32, MeshId>, labels: &HashMap<u32, LabelId>) -> Scene {
        let o = &self.options;
        let options = Options {
            hover: rgb(&o.hover),
            selected: rgb(&o.selected),
            edges: rgb(&o.edges),
            sketch_inactive: rgb(&o.sketch_inactive),
            temp_shaded: rgb(&o.temp_shaded),
            background: match o.bg_mode.as_str() {
                "plain" => Background::Plain(rgb(&o.bg_bottom)),
                "gradient" => Background::Gradient { top: rgb(&o.bg_top), bottom: rgb(&o.bg_bottom) },
                _ => Background::Scene { top: rgb(&o.bg_top), mid: Rgb::hex("#eef0f3"), bottom: rgb(&o.bg_bottom) },
            },
            plane_front: rgb(&o.plane_front),
            plane_back: rgb(&o.plane_back),
            plane_transparency: o.plane_transparency,
            shaded_planes: o.shaded_planes,
            hidden_edges_dashed: o.hidden_edges_dashed,
            shaded_face_highlight: o.shaded_face_highlight,
            dynamic_highlight: o.dynamic_highlight,
            triad: o.triad,
            msaa: if o.msaa { 4 } else { 1 },
            line_width: o.line_width.filter(|w| w.is_finite() && *w > 0.0).unwrap_or(Options::default().line_width),
            ..Options::default()
        };
        Scene {
            style: style(&self.style),
            options,
            bodies: self
                .bodies
                .iter()
                .filter_map(|b| {
                    let mesh = *meshes.get(&b.mesh)?;
                    Some(BodyItem {
                        id: b.id,
                        mesh,
                        placement: b.placement.map(PlacementDto::to).unwrap_or_default(),
                        role: match b.role.as_str() {
                            "preview" => BodyRole::Preview { cut: false },
                            "previewCut" => BodyRole::Preview { cut: true },
                            "highlight" => BodyRole::Highlight,
                            _ => BodyRole::Part,
                        },
                        color: rgb(&b.color),
                        transparent: b.transparent,
                        lod: LodChoice::Finest,
                        preview_faces: b.preview_faces.clone(),
                        style: b.style.as_deref().map(style),
                        pickable: b.pickable,
                    })
                })
                .collect(),
            planes: self
                .planes
                .iter()
                .map(|p| PlaneItem { id: p.id, frame: p.frame.to(), extents: p.extents, label: p.label.and_then(|l| labels.get(&l).copied()) })
                .collect(),
            axes: self.axes.iter().map(|a| AxisItem { id: a.id, point: v(a.point), dir: v(a.dir), length: a.length }).collect(),
            points: self.points.iter().map(|p| PointItem { id: p.id, point: v(p.point) }).collect(),
            csys: self.csys.iter().map(|c| CsysItem { id: c.id, frame: c.frame.to() }).collect(),
            sketches: self
                .sketches
                .iter()
                .map(|s| SketchItem {
                    id: s.id,
                    segments: s
                        .segments
                        .chunks_exact(6)
                        .map(|c| [DVec3::new(c[0], c[1], c[2]), DVec3::new(c[3], c[4], c[5])])
                        .collect(),
                    profile: s.profile,
                })
                .collect(),
            origin: OriginItem { shown: self.origin_shown, pickable: self.origin_pickable },
            section: self.section.as_ref().map(|s| Section { origin: v(s.origin), normal: v(s.normal) }),
            hover: self.hover.as_ref().map(HitDto::to),
            selected: self.selected.iter().map(HitDto::to).collect(),
            view_selector: self.selector.as_ref().map(|s| SelectorItem { hover: s.hover }),
        }
    }
}
