//! What to draw: a plain description the caller rebuilds whenever app state changes
//! (cheap: meshes are referenced by [`MeshId`], uploaded once). It carries what
//! `viewport.ts` reads from the app in `sync()`: the display style, bodies, the preview
//! body, reference planes, axes, points, coordinate systems, inactive sketches, the origin,
//! the section plane, and the hover and selection that drive highlights.
//!
//! Visibility rules that depend on the document (which planes are shown, which sketches are
//! consumed) stay with the caller; the renderer draws what it is given.
use crate::color::Rgb;
use glam::{DQuat, DVec3};

/// A mesh uploaded with [`crate::Renderer::add_mesh`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MeshId(pub(crate) u32);

/// A plane name image uploaded with [`crate::Renderer::add_label`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LabelId(pub(crate) u32);

/// View > Display Style (`DisplayStyle` in viewport.ts).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DisplayStyle {
    /// Shaded With Edges (the default).
    #[default]
    ShadedEdges,
    Shaded,
    /// Hidden Lines Removed.
    HiddenLinesRemoved,
    /// Hidden Lines Visible: hidden edges grey, dashed or solid ([`Options::hidden_edges_dashed`]).
    HiddenLinesVisible,
    Wireframe,
}

/// Something under the pointer, or selected. Mirrors `SelItem` for graphics-area entities:
/// sketches and reference axes, points and coordinate systems are all `{kind: "feature"}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hit {
    Origin,
    Face { body: u32, index: u32 },
    Edge { body: u32, index: u32 },
    Vertex { body: u32, index: u32 },
    Plane(u32),
    Feature(u32),
}

/// What a body is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BodyRole {
    /// A body of the part: shaded per the display style, pickable, highlighted.
    #[default]
    Part,
    /// The feature preview (`buildTool`): transparent yellow with yellow edges over the part,
    /// not pickable. A cut preview is more opaque and drawn over everything.
    Preview { cut: bool },
}

/// Which level of detail to draw.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum LodChoice {
    /// Level 0 (the Image Quality), as today's viewer.
    #[default]
    Finest,
    /// The coarsest level whose chordal deviation stays under this many pixels.
    Auto { max_error_px: f64 },
    Level(usize),
}

/// Where a body's mesh is drawn: `world = rotation · mesh + translation` (an assembly
/// instance; identity for a part). Many bodies may share one mesh: geometry is uploaded once
/// and instances are transforms (plan section 4, rendering).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub rotation: DQuat,
    pub translation: DVec3,
}

impl Default for Placement {
    fn default() -> Self {
        Placement::IDENTITY
    }
}

impl Placement {
    pub const IDENTITY: Placement = Placement { rotation: DQuat::IDENTITY, translation: DVec3::ZERO };

    pub fn apply(&self, p: DVec3) -> DVec3 {
        self.rotation * p + self.translation
    }

    pub fn is_identity(&self) -> bool {
        *self == Placement::IDENTITY
    }

    /// World point to mesh coordinates.
    pub fn inverse_apply(&self, p: DVec3) -> DVec3 {
        self.rotation.inverse() * (p - self.translation)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BodyItem {
    /// The caller's id, reported in [`Hit`]s.
    pub id: u32,
    pub mesh: MeshId,
    pub placement: Placement,
    pub role: BodyRole,
    /// Shaded colour (part colour, `doc.display.color` or `fc.shading`).
    pub color: Rgb,
    /// `doc.display.transparent`: 45 % opaque, no depth writes.
    pub transparent: bool,
    pub lod: LodChoice,
    /// Faces of a fillet or chamfer being previewed: tinted preview yellow.
    pub preview_faces: Vec<u32>,
}

impl BodyItem {
    pub fn part(id: u32, mesh: MeshId, color: Rgb) -> BodyItem {
        BodyItem {
            id,
            mesh,
            placement: Placement::IDENTITY,
            role: BodyRole::Part,
            color,
            transparent: false,
            lod: LodChoice::Finest,
            preview_faces: vec![],
        }
    }
}

/// A plane frame: origin and unit axes in world millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: DVec3,
    pub x: DVec3,
    pub y: DVec3,
    pub normal: DVec3,
}

impl Frame {
    pub fn to_world(&self, u: f64, v: f64) -> DVec3 {
        self.origin + self.x * u + self.y * v
    }
    /// Front (XY), Top (XZ), Right (YZ) as in `STD_FRAMES` (`src/core/math.ts`).
    pub fn front() -> Frame {
        Frame {
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::Y,
            normal: DVec3::Z,
        }
    }
    pub fn top() -> Frame {
        Frame {
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::NEG_Z,
            normal: DVec3::Y,
        }
    }
    pub fn right() -> Frame {
        Frame {
            origin: DVec3::ZERO,
            x: DVec3::NEG_Z,
            y: DVec3::Y,
            normal: DVec3::X,
        }
    }
}

/// A reference plane to draw (the caller decides which are shown).
#[derive(Clone, Debug, PartialEq)]
pub struct PlaneItem {
    pub id: u32,
    pub frame: Frame,
    /// `[u0, u1, v0, v1]` in the frame ([`crate::planes::auto_extents`] when not stored).
    pub extents: [f64; 4],
    /// The plane's name, drawn in the plane at its top-left corner.
    pub label: Option<LabelId>,
}

/// A reference axis: a dash-dot line through `point` along `dir`, `length` long.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisItem {
    pub id: u32,
    pub point: DVec3,
    pub dir: DVec3,
    /// [`crate::planes::ref_axis_length`] of the model box.
    pub length: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointItem {
    pub id: u32,
    pub point: DVec3,
}

/// A reference coordinate system: three screen-constant arrows.
#[derive(Clone, Debug, PartialEq)]
pub struct CsysItem {
    pub id: u32,
    pub frame: Frame,
}

/// An inactive sketch, already sampled to world line segments by the caller.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchItem {
    pub id: u32,
    pub segments: Vec<[DVec3; 2]>,
    /// The profile of the feature whose PropertyManager is open: dark, over everything.
    pub profile: bool,
}

/// Section View: everything of the part on the side `normal` points to is cut away and the
/// cut is capped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    pub origin: DVec3,
    pub normal: DVec3,
}

/// Viewport background (System Options > Colors, `ui.bgMode`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Background {
    /// The default: radial gradient (ellipse at 50 % 45 %) top → mid at 45 % → bottom.
    Scene { top: Rgb, mid: Rgb, bottom: Rgb },
    /// Linear, top to bottom.
    Gradient { top: Rgb, bottom: Rgb },
    Plain(Rgb),
    /// Leave the target's alpha at zero (for compositing tests).
    Transparent,
}

/// System Options and Document Properties the viewport reads, with the app's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub hover: Rgb,
    pub selected: Rgb,
    pub edges: Rgb,
    pub sketch_inactive: Rgb,
    /// `color.tempShaded`, the preview body colour.
    pub temp_shaded: Rgb,
    pub background: Background,
    /// Document Properties > Plane Display.
    pub plane_front: Rgb,
    pub plane_back: Rgb,
    /// 0..=100, percent transparent.
    pub plane_transparency: f64,
    /// `display.shadedPlanes`.
    pub shaded_planes: bool,
    /// `display.hiddenEdges` = dashed (true) or solid.
    pub hidden_edges_dashed: bool,
    /// `display.shadedFaceHighlight`.
    pub shaded_face_highlight: bool,
    /// `display.dynamicHighlight` (hover highlight of faces and edges).
    pub dynamic_highlight: bool,
    /// `display.triad`.
    pub triad: bool,
    /// 🔍 Face highlights in Hidden Lines Removed / Visible and Wireframe. Off matches
    /// today's viewer, where the white emissive HLR material washes the tint out.
    pub face_highlight_in_line_styles: bool,
    /// 🔍 Show vertices that only lie on closed edges (the start of a full circle) or
    /// seams. SOLIDWORKS (Parasolid) has none; today's viewer shows them.
    pub closed_edge_vertices: bool,
    /// MSAA samples: 4 (`performance.msaa`) or 1.
    pub msaa: u32,
    /// Width of plain lines in device pixels: model edges, hidden edges, inactive sketches,
    /// reference axes, coordinate systems, the View Selector's outlines. 1.5 matches the
    /// weight of the WebGL lines in the Electron app (ANGLE draws them about 1.5 device px
    /// wide under MSAA); 1.0 is a literal one-pixel line, which looks stippled on shallow
    /// slopes under 4× MSAA (F1, F2a owner decision 2).
    pub line_width: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            hover: Rgb::hex("#ff9a1f"),
            selected: Rgb::hex("#2e8bff"),
            edges: Rgb::hex("#1a1a1a"),
            sketch_inactive: Rgb::hex("#5a5a5a"),
            temp_shaded: Rgb::hex("#ffff80"),
            background: Background::Scene {
                top: Rgb::hex("#ffffff"),
                mid: Rgb::hex("#eef0f3"),
                bottom: Rgb::hex("#c8cdd4"),
            },
            plane_front: Rgb::hex("#c6d6ee"),
            plane_back: Rgb::hex("#e8dcc2"),
            plane_transparency: 82.0,
            shaded_planes: true,
            hidden_edges_dashed: true,
            shaded_face_highlight: true,
            dynamic_highlight: true,
            triad: true,
            face_highlight_in_line_styles: false,
            closed_edge_vertices: false,
            msaa: 4,
            line_width: 1.5,
        }
    }
}

/// The part origin marker (blue arrows, drawn over everything).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct OriginItem {
    /// Drawn (View > Hide/Show > Origins, or hovered / selected; hidden in a sketch, which
    /// draws its own).
    pub shown: bool,
    /// Pickable: shown by the option and not in a sketch (`pick()` in viewport.ts).
    pub pickable: bool,
}

/// The 3D View Selector cube ([`crate::selector`]), drawn over the model at the centre
/// of the viewport while it is open.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SelectorItem {
    /// Facet under the pointer (index into [`crate::selector::facets`]), tinted light blue.
    pub hover: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub style: DisplayStyle,
    pub options: Options,
    pub bodies: Vec<BodyItem>,
    pub planes: Vec<PlaneItem>,
    pub axes: Vec<AxisItem>,
    pub points: Vec<PointItem>,
    pub csys: Vec<CsysItem>,
    pub sketches: Vec<SketchItem>,
    pub origin: OriginItem,
    pub section: Option<Section>,
    pub hover: Option<Hit>,
    pub selected: Vec<Hit>,
    /// The View Selector, when open.
    pub view_selector: Option<SelectorItem>,
}

impl Scene {
    pub fn is_selected(&self, h: Hit) -> bool {
        self.selected.contains(&h)
    }
    pub fn is_hovered(&self, h: Hit) -> bool {
        self.hover == Some(h)
    }
}
