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
    /// Where picks find this body when it is drawn elsewhere: a component animating to its
    /// solved placement (G1d mate animation) is picked at that placement.
    #[serde(default)]
    pub pick_placement: Option<PlacementDto>,
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
    /// None: the same bodies as the previous frame (the webview leaves them out when only
    /// the camera, hover or the like changed: a 10 000-component assembly is 1.4 MB of
    /// JSON, G2). [`SceneDto::keep_bodies`] fills them in.
    #[serde(default)]
    pub bodies: Option<Vec<BodyDto>>,
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
    /// The view is moving: a rotate, pan or zoom, a view transition (G2b). The webview
    /// sends a frame with this false when the motion stops.
    #[serde(default)]
    pub moving: bool,
    /// Options > Performance > Level of detail for this document: 0 Off, 1 (more detail) to
    /// [`MAX_LOD_SLIDER`] (less); 0 when the option does not apply (a single-body part).
    #[serde(default)]
    pub lod: u32,
}

/// The right end of the Level of detail slider (SOLIDWORKS' `swLevelOfDetail` runs from 0
/// to 20).
pub const MAX_LOD_SLIDER: u32 = 20;

/// The level of detail of part bodies while the view moves, for a Level of detail slider
/// position: Off draws the finest level; 1 to 20 allow from half a pixel to 16 pixels of
/// chordal deviation, evenly on a log scale (each step about 1.2 times the last).
///
/// SOLIDWORKS states the slider as a frame rate to reach while spinning (0 off, 20 the most
/// detail removed) and drops detail until it is reached; this maps it to a fixed error
/// instead, so the same view always draws the same levels (🔍 in FEATURE_SPEC).
pub fn moving_lod(slider: u32) -> LodChoice {
    match slider.min(MAX_LOD_SLIDER) {
        0 => LodChoice::Finest,
        v => LodChoice::Auto { max_error_px: 0.5 * 32f64.powf((v - 1) as f64 / (MAX_LOD_SLIDER - 1) as f64) },
    }
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
    pub fn bodies(&self) -> &[BodyDto] {
        self.bodies.as_deref().unwrap_or(&[])
    }

    /// Takes the previous frame's bodies when this frame left them out.
    pub fn keep_bodies(&mut self, previous: Option<&mut SceneDto>) {
        if self.bodies.is_none() {
            self.bodies = previous.and_then(|p| p.bodies.take());
        }
    }

    /// The scene picks use when some body is drawn away from where it is (`pick_placement`):
    /// `scene` (this frame's, converted) with those bodies at their pick placements. None
    /// when every body is picked where it is drawn.
    pub fn pick_scene(&self, scene: &Scene) -> Option<Scene> {
        let moved: HashMap<u32, PlacementDto> = self.bodies().iter().filter_map(|b| b.pick_placement.map(|p| (b.id, p))).collect();
        if moved.is_empty() {
            return None;
        }
        let mut s = scene.clone();
        for b in s.bodies.iter_mut() {
            if let Some(p) = moved.get(&b.id) {
                b.placement = p.to();
            }
        }
        Some(s)
    }

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
        let lod = if self.moving { moving_lod(self.lod) } else { LodChoice::Finest };
        Scene {
            style: style(&self.style),
            options,
            bodies: self
                .bodies()
                .iter()
                .filter_map(|b| {
                    let mesh = *meshes.get(&b.mesh)?;
                    let role = match b.role.as_str() {
                        "preview" => BodyRole::Preview { cut: false },
                        "previewCut" => BodyRole::Preview { cut: true },
                        "highlight" => BodyRole::Highlight,
                        _ => BodyRole::Part,
                    };
                    Some(BodyItem {
                        id: b.id,
                        mesh,
                        placement: b.placement.map(PlacementDto::to).unwrap_or_default(),
                        role,
                        color: rgb(&b.color),
                        transparent: b.transparent,
                        // Previews and interference volumes keep the finest level.
                        lod: if role == BodyRole::Part { lod } else { LodChoice::Finest },
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The JSON the webview sends for an assembly of `n` occurrences (`src/native/view.ts`).
    fn assembly_frame(n: u32) -> String {
        let bodies: Vec<String> = (0..n)
            .map(|k| {
                format!(
                    r##"{{"id":{k},"mesh":{},"role":"part","color":"#c9ccd1","placement":{{"position":[{},{},{}],"rotation":[0.92,0,0,0.38]}},"transparent":false}}"##,
                    1 + k % 100,
                    (k % 25) as f64 * 45.0,
                    ((k / 25) % 20) as f64 * 45.0,
                    (k / 500) as f64 * 45.0
                )
            })
            .collect();
        format!(
            r##"{{"style":"shadedEdges","options":{{"hover":"#ff9a1f","selected":"#2e8bff","edges":"#1a1a1a","sketchInactive":"#5a5a5a","tempShaded":"#ffff80","bgMode":"scene","bgTop":"#ffffff","bgBottom":"#c8cdd4","planeFront":"#c6d6ee","planeBack":"#e8dcc2","planeTransparency":82,"shadedPlanes":true,"hiddenEdgesDashed":true,"shadedFaceHighlight":true,"dynamicHighlight":true,"triad":true,"msaa":true}},"bodies":[{}],"planes":[],"axes":[],"points":[],"csys":[],"sketches":[],"originShown":true,"originPickable":true,"section":null,"hover":null,"selected":[],"retain":[],"seen":100,"seq":1}}"##,
            bodies.join(",")
        )
    }

    #[test]
    fn assembly_frames_convert() {
        let mut first: SceneDto = serde_json::from_str(&assembly_frame(3)).unwrap();
        let meshes: HashMap<u32, MeshId> = (1..=100).map(|h| (h, MeshId::default())).collect();
        let scene = first.to(&meshes, &HashMap::new());
        assert_eq!(scene.bodies.len(), 3);
        assert_eq!(scene.bodies[2].placement.translation, DVec3::new(90.0, 0.0, 0.0));
        assert!(first.pick_scene(&scene).is_none());
        // A frame without bodies keeps the previous frame's.
        let mut next: SceneDto = serde_json::from_str(&assembly_frame(3).replace(r#""bodies":["#, r#""unused":["#)).unwrap();
        assert!(next.bodies.is_none());
        next.keep_bodies(Some(&mut first));
        assert_eq!(next.to(&meshes, &HashMap::new()).bodies.len(), 3);
        // A frame with bodies keeps its own.
        let mut again: SceneDto = serde_json::from_str(&assembly_frame(2)).unwrap();
        again.keep_bodies(Some(&mut next));
        assert_eq!(again.bodies().len(), 2);
    }

    /// G2b: Options > Performance > Level of detail. Off draws the finest level; on, more
    /// of the slider allows more error, from half a pixel to 16 pixels.
    #[test]
    fn the_level_of_detail_slider_sets_the_threshold() {
        assert_eq!(moving_lod(0), LodChoice::Finest);
        let px = |v| match moving_lod(v) {
            LodChoice::Auto { max_error_px } => max_error_px,
            l => panic!("{v}: {l:?}"),
        };
        assert!((px(1) - 0.5).abs() < 1e-12);
        assert!((px(MAX_LOD_SLIDER) - 16.0).abs() < 1e-9);
        for v in 1..MAX_LOD_SLIDER {
            assert!(px(v + 1) > px(v), "less detail to the right");
        }
        assert_eq!(moving_lod(MAX_LOD_SLIDER + 5), moving_lod(MAX_LOD_SLIDER));
    }

    /// G2b: a frame drawn while the view moves draws the parts at the slider's level of
    /// detail; previews and interference volumes, and every body at rest, at the finest.
    #[test]
    fn moving_frames_coarsen_the_parts() {
        let meshes: HashMap<u32, MeshId> = (1..=100).map(|h| (h, MeshId::default())).collect();
        let base = assembly_frame(3).replace(
            r##""role":"part","color":"#c9ccd1","placement":{"position":[90,0,0]"##,
            r##""role":"highlight","color":"#ff2020","placement":{"position":[90,0,0]"##,
        );
        let frame = |extra: &str| -> SceneDto { serde_json::from_str(&base.replace(r#""seq":1"#, &format!(r#""seq":1{extra}"#))).unwrap() };
        let lods = |s: &SceneDto| s.to(&meshes, &HashMap::new()).bodies.iter().map(|b| b.lod).collect::<Vec<_>>();
        // Frames without the fields (older webviews) are at rest.
        let rest = frame("");
        assert!(!rest.moving && rest.lod == 0);
        assert_eq!(lods(&rest), vec![LodChoice::Finest; 3]);
        let moving = frame(r#","moving":true,"lod":10"#);
        assert_eq!(lods(&moving), vec![moving_lod(10), moving_lod(10), LodChoice::Finest], "the highlight stays at the finest level");
        assert!(matches!(moving_lod(10), LodChoice::Auto { .. }));
        // Off, or at rest: the finest level.
        assert_eq!(lods(&frame(r#","moving":true,"lod":0"#)), vec![LodChoice::Finest; 3]);
        assert_eq!(lods(&frame(r#","moving":false,"lod":10"#)), vec![LodChoice::Finest; 3]);
        // The slider changes the level: more error allowed to the right.
        let px = |s: &SceneDto| match s.to(&meshes, &HashMap::new()).bodies[0].lod {
            LodChoice::Auto { max_error_px } => max_error_px,
            _ => 0.0,
        };
        assert!(px(&frame(r#","moving":true,"lod":20"#)) > px(&moving));
        // A frame that leaves the bodies out (the camera moved) keeps the previous bodies
        // and is drawn at its own level.
        let mut first = frame("");
        let mut next: SceneDto = serde_json::from_str(&base.replace(r#""bodies":["#, r#""unused":["#).replace(r#""seq":1"#, r#""seq":2,"moving":true,"lod":10"#)).unwrap();
        next.keep_bodies(Some(&mut first));
        assert_eq!(lods(&next)[0], moving_lod(10));
    }

    /// G2: what a frame of a 10 000-component assembly costs the app before the renderer
    /// (parse the webview's JSON, convert it). `cargo test -p oc-app --release -- --ignored
    /// --nocapture assembly_frame_cost`.
    #[test]
    #[ignore]
    fn assembly_frame_cost() {
        let json = assembly_frame(10_000);
        let json_same = assembly_frame(0).replace(r#""bodies":[],"#, "");
        let mut same = 0.0;
        let meshes: HashMap<u32, MeshId> = (1..=100).map(|h| (h, MeshId::default())).collect();
        let n = 50;
        let (mut parse, mut convert) = (0.0, 0.0);
        let mut prev: Option<SceneDto> = None;
        for i in 0..2 * n {
            // Every other frame leaves the bodies out, as camera moves do.
            let t = std::time::Instant::now();
            let mut dto: SceneDto = if i % 2 == 0 { serde_json::from_str(&json).unwrap() } else { serde_json::from_str(&json_same).unwrap() };
            dto.keep_bodies(prev.as_mut());
            let t1 = std::time::Instant::now();
            let scene = dto.to(&meshes, &HashMap::new());
            assert_eq!(scene.bodies.len(), 10_000);
            let t2 = std::time::Instant::now();
            if i % 2 == 0 {
                parse += (t1 - t).as_secs_f64();
            } else {
                same += (t1 - t).as_secs_f64();
            }
            convert += (t2 - t1).as_secs_f64();
            prev = Some(dto);
        }
        let ms = |s: f64| s * 1e3 / n as f64;
        println!(
            "10 000 bodies, {:.1} MB of JSON: parse {:.2} ms (bodies left out: {:.3} ms), convert {:.2} ms per frame",
            json.len() as f64 / 1e6,
            ms(parse),
            ms(same),
            ms(convert) / 2.0
        );
    }
}
