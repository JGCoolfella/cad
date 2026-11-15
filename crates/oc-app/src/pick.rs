//! Picking for the webview (F2b): what is under the pointer, answered by `oc-render` on
//! the render thread against the scene of a frame the webview sent.
//!
//! - a click waits for the GPU ([`Renderer::pick`]); hover is submitted with
//!   [`Renderer::pick_async`] and answered when the GPU is done, so the render thread
//!   never stalls on it;
//! - `ray` adds the faces along the pointer ray ([`Renderer::raycast_faces`]): the first
//!   for the rotate-about point and Hole Wizard positions, all of them for Select Other;
//! - with the View Selector open its facets are tried first ([`oc_render::selector`]);
//! - box selection of model edges ([`Renderer::box_edges`]).
//!
//! A query names the frame (`seq`) whose scene it needs; the render thread holds it until
//! that frame has arrived (the webview sends frames and picks on separate IPC calls,
//! which may overtake each other), and answers with the frame it used, so the webview maps
//! plane and feature indices with that frame's lists.
use crate::scene_dto::{CameraDto, HitDto};
use crate::view::Rect;
use oc_render::{selector, PendingPick, PickFilter, PickKind, Renderer, Scene};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum RayMode {
    #[default]
    None,
    First,
    All,
}

/// `app.selFilter` when the filter is on, and the Chamfer PropertyManager's edges-only rule.
#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FilterDto {
    pub only: Option<Vec<String>>,
    #[serde(default)]
    pub edges_only: bool,
}

impl FilterDto {
    pub fn to(&self) -> PickFilter {
        let kind = |k: &str| match k {
            "face" => Some(PickKind::Face),
            "edge" => Some(PickKind::Edge),
            "vertex" => Some(PickKind::Vertex),
            "plane" => Some(PickKind::Plane),
            "sketch" => Some(PickKind::Sketch),
            "ref" => Some(PickKind::Ref),
            _ => None,
        };
        PickFilter {
            only: self.only.as_ref().filter(|v| !v.is_empty()).map(|v| v.iter().filter_map(|k| kind(k)).collect()),
            edges_only: self.edges_only,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickReq {
    pub seq: u32,
    pub camera: CameraDto,
    pub rect: Rect,
    /// Logical px in the graphics area.
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub filter: FilterDto,
    /// A click: wait for the GPU. Otherwise hover: answered when the GPU is done.
    #[serde(default)]
    pub wait: bool,
    #[serde(default)]
    pub ray: RayMode,
    /// The View Selector is open: its facets are tried first.
    #[serde(default)]
    pub selector: bool,
}

#[derive(Serialize, Debug)]
pub struct RayHitDto {
    pub hit: HitDto,
    pub t: f64,
    pub point: [f64; 3],
}

#[derive(Serialize, Debug)]
pub struct FacetDto {
    pub index: u32,
    pub normal: [f64; 3],
    /// Camera up when looking from this facet ([`selector::up_for`]).
    pub up: [f64; 3],
    pub name: Option<&'static str>,
}

#[derive(Serialize, Default, Debug)]
pub struct PickReply {
    /// The frame whose scene answered.
    pub seq: u32,
    pub hit: Option<HitDto>,
    pub ray: Vec<RayHitDto>,
    pub selector: Option<FacetDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxReq {
    pub seq: u32,
    pub camera: CameraDto,
    /// Corners in logical px, in drag order.
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub crossing: bool,
}

#[derive(Serialize, Default, Debug)]
pub struct BoxReply {
    pub seq: u32,
    pub hits: Vec<HitDto>,
}

/// A pick answered at once, or submitted to the GPU and answered later.
pub enum Answer {
    Now(PickReply),
    Later(PendingPick, PickReply),
}

pub fn pick(r: &mut Renderer, scene: &Scene, seq: u32, q: &PickReq) -> Answer {
    let camera = q.camera.to();
    let mut out = PickReply { seq, ..Default::default() };
    if q.selector {
        if let Some(i) = selector::pick(&camera, q.x, q.y) {
            let f = &selector::facets()[i];
            out.selector = Some(FacetDto {
                index: i as u32,
                normal: f.normal.to_array(),
                up: selector::up_for(f.normal).to_array(),
                name: f.name,
            });
            return Answer::Now(out);
        }
    }
    if q.ray != RayMode::None {
        let hits = r.raycast_faces(scene, &camera.ray(q.x, q.y));
        let n = if q.ray == RayMode::First { 1 } else { hits.len() };
        out.ray = hits
            .into_iter()
            .take(n)
            .map(|h| RayHitDto { hit: h.hit.into(), t: h.t, point: h.point.to_array() })
            .collect();
    }
    let vp = q.rect.viewport();
    let filter = q.filter.to();
    if q.wait {
        out.hit = r.pick(scene, &camera, vp, q.x, q.y, &filter).map(HitDto::from);
        Answer::Now(out)
    } else {
        Answer::Later(r.pick_async(scene, &camera, vp, q.x, q.y, &filter), out)
    }
}

pub fn box_select(r: &Renderer, scene: &Scene, seq: u32, q: &BoxReq) -> BoxReply {
    let camera = q.camera.to();
    let hits = r.box_edges(scene, &camera, [q.x0, q.y0], [q.x1, q.y1], q.crossing);
    BoxReply { seq, hits: hits.into_iter().map(HitDto::from).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_map_to_pick_kinds() {
        let f: FilterDto = serde_json::from_str(r#"{"only":["face","ref","bogus"],"edgesOnly":true}"#).unwrap();
        assert_eq!(f.to(), PickFilter { only: Some(vec![PickKind::Face, PickKind::Ref]), edges_only: true });
        let none: FilterDto = serde_json::from_str(r#"{"only":null}"#).unwrap();
        assert_eq!(none.to(), PickFilter::default());
        let empty: FilterDto = serde_json::from_str(r#"{"only":[]}"#).unwrap();
        assert_eq!(empty.to(), PickFilter::default());
    }

    #[test]
    fn requests_parse_with_defaults() {
        let q: PickReq = serde_json::from_str(
            r#"{"seq":3,"camera":{"target":[0,0,0],"quat":[0,0,0,1],"height":100,"perspective":false,"width":800,"heightPx":600},
                "rect":{"x":0,"y":0,"w":800,"h":600,"scale":1.25},"x":10,"y":20,"ray":"all"}"#,
        )
        .unwrap();
        assert_eq!((q.seq, q.wait, q.selector, q.ray), (3, false, false, RayMode::All));
        assert_eq!(q.rect.viewport(), oc_render::Viewport::new(1000, 750, 1.25));
        let h = serde_json::to_string(&HitDto::from(oc_render::Hit::Face { body: 0, index: 4 })).unwrap();
        assert_eq!(h, r#"{"k":"face","body":0,"index":4}"#);
        assert_eq!(serde_json::to_string(&HitDto::from(oc_render::Hit::Plane(2))).unwrap(), r#"{"k":"plane","i":2}"#);
    }
}
