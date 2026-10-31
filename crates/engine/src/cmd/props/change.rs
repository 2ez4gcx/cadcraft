//! CHANGE: change objects by a change point, or change their properties, as AutoCAD 2024 does.
//!
//! "Specify change point or [Properties]" follows "Select objects", which leaves objects on locked
//! layers out as CHPROP does. Properties is CHPROP's loop with Elev added: the elevation is set
//! when the command ends, and objects whose points differ in z keep theirs ("Cannot change
//! elevation of objects with differing Z coordinates."). A change point, or Enter, goes through the
//! objects in the order they were selected. Measured from AutoLISP's `command` (accoreconsole,
//! every object read back with `entget`):
//! - each line moves its end nearer the point to it: (0,0)–(50,0) with (60,10) gives
//!   (0,0)–(60,10), with (−5,5) gives (−5,5)–(50,0). Under ORTHOMODE the moved end stays level or
//!   plumb with the other one ((60,0)). Enter leaves lines alone;
//! - a circle passes through the point (centre (100,0) and (115,0): radius 15), or asks "Specify
//!   new circle radius <no change>";
//! - a text is placed by the point (the alignment point of justified text), or asks "Specify new
//!   text insertion point <no change>"; then "Enter new text style", "Specify new height" (not
//!   with a style of fixed height), "Specify new rotation angle" and "Enter new text". An
//!   attribute definition asks "Enter new tag", "Enter new prompt" and "Enter new default value" in
//!   place of the text. A style that does not exist is refused: `Style "5" not defined - use STYLE
//!   command.`;
//! - a block reference is placed by the point, its attributes moving and turning with it, or asks
//!   "Specify new block insertion point <no change>"; then "Specify new block rotation angle";
//! - the point places a circle, text or block reference only when it is the one object selected:
//!   with others, "Change point ignored" comes before that object's questions, which are asked as
//!   for Enter;
//! - other objects (arcs, polylines…) are left alone, and with nothing to change it says "No
//!   changeable object selected".

use std::collections::{HashSet, VecDeque};

use cadcraft_doc::{Attrib, EntityKind, Handle, Text};
use cadcraft_geom::{Mat3, Vec2, Vec3};
use serde_json::{Value, json};

use super::super::machines::{SelOutcome, SelectPhase, number};
use super::super::{bad, f64_req, point_req, targets};
use super::chprop::{ChpropM, begin_unlocked, feed_unlocked, locked};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

/// A question CHANGE asks about one object.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ask {
    /// Not a question: "Change point ignored" as the object comes up.
    Ignored,
    Radius,
    TextPoint,
    Style,
    Height,
    Rotation,
    Value,
    Tag,
    AttPrompt,
    Default,
    BlockPoint,
    BlockRotation,
}

enum Stage {
    Select,
    Point,
    Properties(Box<ChpropM>),
    /// The questions left, in order, each about one object.
    Asking(VecDeque<(Handle, Ask)>),
}

pub(super) struct ChangeM {
    select: SelectPhase,
    handles: Vec<Handle>,
    stage: Stage,
}

impl ChangeM {
    pub(super) fn new() -> Self {
        ChangeM { select: SelectPhase::default(), handles: Vec::new(), stage: Stage::Select }
    }

    /// Apply the change point `p` (`None`: Enter) and line up the questions it leaves.
    fn change_point(&mut self, s: &mut Session, p: Option<Vec2>) -> Result<Step> {
        let alone = self.handles.len() == 1;
        let ortho = s.drafting_settings().orthomode;
        let mut asks = VecDeque::new();
        let mut changeable = false;
        for &h in &self.handles {
            let Some(kind) = s.doc()?.entity(h).map(|e| e.kind.clone()) else { continue };
            if matches!(kind, EntityKind::Line(_)) {
                if let Some(p) = p {
                    move_end(s, h, p, ortho)?;
                    changeable = true;
                }
                continue;
            }
            let Some(questions) = questions(&kind) else { continue };
            changeable = true;
            let placed = p.filter(|_| alone);
            if p.is_some() && !alone {
                asks.push_back((h, Ask::Ignored));
            }
            if let Some(q) = placed {
                place(s, h, q)?;
            }
            // The point answers the first question: the radius or the insertion point.
            asks.extend(questions.iter().skip(usize::from(placed.is_some())).map(|a| (h, *a)));
        }
        if !changeable {
            s.echo("No changeable object selected");
            return Ok(Step::Done);
        }
        self.stage = Stage::Asking(asks);
        self.advance(s)
    }

    /// Pass over what needs no answer: say "Change point ignored", and skip the height of a text
    /// whose style has a fixed one. Done when nothing is left to ask.
    fn advance(&mut self, s: &mut Session) -> Result<Step> {
        let Stage::Asking(asks) = &mut self.stage else { return Ok(Step::Continue) };
        while let Some(&(h, ask)) = asks.front() {
            match ask {
                Ask::Ignored => s.echo("Change point ignored"),
                Ask::Height if fixed_height(s, h) => {}
                _ => return Ok(Step::Continue),
            }
            asks.pop_front();
        }
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

/// The questions about an object of `kind`, the first answered by the change point when it is
/// the one object; `None` when the change point leaves it alone.
fn questions(kind: &EntityKind) -> Option<&'static [Ask]> {
    Some(match kind {
        EntityKind::Circle(_) => &[Ask::Radius],
        EntityKind::Text(_) => &[Ask::TextPoint, Ask::Style, Ask::Height, Ask::Rotation, Ask::Value],
        EntityKind::AttDef(_) => &[Ask::TextPoint, Ask::Style, Ask::Height, Ask::Rotation, Ask::Tag, Ask::AttPrompt, Ask::Default],
        EntityKind::Insert(_) => &[Ask::BlockPoint, Ask::BlockRotation],
        _ => return None,
    })
}

fn text_of(kind: &EntityKind) -> Option<&Text> {
    match kind {
        EntityKind::Text(t) | EntityKind::AttDef(Attrib { text: t, .. }) => Some(t),
        _ => None,
    }
}

fn text_of_mut(kind: &mut EntityKind) -> Option<&mut Text> {
    match kind {
        EntityKind::Text(t) | EntityKind::AttDef(Attrib { text: t, .. }) => Some(t),
        _ => None,
    }
}

/// Where a circle, text or block reference is measured from: its centre, the point it is placed by
/// (the alignment point of justified text) or its insertion point.
fn anchor(kind: &EntityKind) -> Option<Vec2> {
    match kind {
        EntityKind::Circle(c) => Some(c.center.xy()),
        EntityKind::Insert(i) => Some(i.insert.xy()),
        k => text_of(k).map(Text::justify_point),
    }
}

/// Whether `h` is a text whose style has a fixed height (CHANGE does not ask for one then).
fn fixed_height(s: &Session, h: Handle) -> bool {
    let Ok(d) = s.doc() else { return false };
    d.entity(h).and_then(|e| text_of(&e.kind)).and_then(|t| d.text_style(&t.style)).is_some_and(|st| st.height > 0.0)
}

/// Move the end of line `h` nearer `p` to it; under ortho, level or plumb with the other end.
fn move_end(s: &mut Session, h: Handle, p: Vec2, ortho: bool) -> Result<()> {
    let settings = s.drafting_settings();
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Line(l) = &mut e.kind {
            let (end, other) = if l.a.xy().dist(p) <= l.b.xy().dist(p) { (&mut l.a, l.b) } else { (&mut l.b, l.a) };
            let q = if ortho { crate::snap::iso::constrain(&settings, other.xy(), p) } else { p };
            end.x = q.x;
            end.y = q.y;
        }
    })?;
    Ok(())
}

/// Place circle, text or block reference `h` by `q`: a circle passes through it, a text or block
/// reference moves there (with its attributes).
fn place(s: &mut Session, h: Handle, q: Vec2) -> Result<()> {
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Circle(c) = &mut e.kind {
            let r = c.center.xy().dist(q);
            if r > 0.0 && r.is_finite() {
                c.radius = r;
            }
        } else if let Some(at) = anchor(&e.kind) {
            e.kind.transform(&Mat3::translate(q - at));
        }
    })?;
    Ok(())
}

/// Set a circle's radius or a text's height.
fn set_length(s: &mut Session, h: Handle, v: f64) -> Result<()> {
    s.doc_mut()?.modify_entity(h, |e| match &mut e.kind {
        EntityKind::Circle(c) => c.radius = v,
        k => {
            if let Some(t) = text_of_mut(k) {
                t.height = v;
            }
        }
    })?;
    Ok(())
}

/// Set a text's rotation, or turn a block reference (and its attributes) to `angle` about its
/// insertion point.
fn set_rotation(s: &mut Session, h: Handle, angle: f64) -> Result<()> {
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::Insert(i) = &e.kind {
            let m = Mat3::rotate_about(i.insert.xy(), angle - i.rotation);
            e.kind.transform(&m);
        } else if let Some(t) = text_of_mut(&mut e.kind) {
            t.rotation = cadcraft_geom::norm_angle(angle);
        }
    })?;
    Ok(())
}

/// Take an answer to `ask` about `h`; `false` when it was refused (the question comes again).
fn answer(s: &mut Session, h: Handle, ask: Ask, i: Input) -> Result<bool> {
    let at = s.doc()?.entity(h).and_then(|e| anchor(&e.kind)).unwrap_or(Vec2::ZERO);
    match (ask, i) {
        // Enter keeps what there is.
        (_, Input::Enter) => {}
        (Ask::TextPoint | Ask::BlockPoint, Input::Point(q)) => place(s, h, q)?,
        (Ask::TextPoint | Ask::BlockPoint, _) => return Err(EngineError::Other("Point or option keyword required.".into())),
        (Ask::Radius | Ask::Height, Input::Point(q)) if q.dist(at) > 0.0 => set_length(s, h, q.dist(at))?,
        (Ask::Radius | Ask::Height, Input::Text(t)) if number(&t).is_some_and(|v| v > 0.0 && v.is_finite()) => {
            set_length(s, h, number(&t).unwrap_or(1.0))?;
        }
        (Ask::Radius | Ask::Height, _) => return Err(EngineError::Other("Value must be positive and nonzero.".into())),
        (Ask::Rotation | Ask::BlockRotation, Input::Point(q)) => set_rotation(s, h, at.angle_to(q))?,
        (Ask::Rotation | Ask::BlockRotation, Input::Text(t)) => {
            let a = s.angle_settings().direction(&t).ok_or_else(|| EngineError::Other("Requires an angle.".into()))?;
            set_rotation(s, h, a)?;
        }
        (Ask::Style, Input::Text(t) | Input::Keyword(t)) => {
            let name = t.trim();
            let Some(style) = s.doc()?.text_style(name).map(|st| st.name.clone()) else {
                s.echo(format!("Style \"{name}\" not defined - use STYLE command."));
                return Ok(false);
            };
            s.doc_mut()?.modify_entity(h, |e| {
                if let Some(t) = text_of_mut(&mut e.kind) {
                    t.style = style;
                }
            })?;
        }
        (Ask::Value | Ask::Tag | Ask::AttPrompt | Ask::Default, Input::Text(t) | Input::Keyword(t)) => {
            s.doc_mut()?.modify_entity(h, |e| match (&mut e.kind, ask) {
                (EntityKind::AttDef(a), Ask::Tag) => a.tag = t,
                (EntityKind::AttDef(a), Ask::AttPrompt) => a.prompt = t,
                (k, _) => {
                    if let Some(x) = text_of_mut(k) {
                        x.value = t;
                    }
                }
            })?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// The question `ask` about `h`.
fn ask_prompt(s: &Session, h: Handle, ask: Ask) -> Prompt {
    let d = s.doc().ok();
    let kind = d.and_then(|d| d.entity(h)).map(|e| &e.kind);
    let at = kind.and_then(anchor);
    let text = kind.and_then(text_of);
    let deg = |a: f64| format!("{:.2}", a.to_degrees());
    match ask {
        Ask::Radius => Prompt::new("Specify new circle radius", Accept::POINT_OR_NUMBER).default("no change").base_opt(at).distance(),
        Ask::TextPoint => Prompt::new("Specify new text insertion point", Accept::POINT).default("no change"),
        Ask::BlockPoint => Prompt::new("Specify new block insertion point", Accept::POINT).default("no change"),
        Ask::Style => Prompt::new("Enter new text style", Accept::TEXT).default(text.map(|t| t.style.clone()).unwrap_or_default()),
        Ask::Height => Prompt::new("Specify new height", Accept::POINT_OR_NUMBER)
            .default(format!("{:.4}", text.map_or(0.0, |t| t.height)))
            .base_opt(at)
            .distance(),
        Ask::Rotation => {
            Prompt::new("Specify new rotation angle", Accept::POINT_OR_NUMBER).default(deg(text.map_or(0.0, |t| t.rotation))).base_opt(at)
        }
        Ask::BlockRotation => {
            let r = match kind {
                Some(EntityKind::Insert(i)) => i.rotation,
                _ => 0.0,
            };
            Prompt::new("Specify new block rotation angle", Accept::POINT_OR_NUMBER).default(deg(r)).base_opt(at)
        }
        Ask::Value => Prompt::new("Enter new text", Accept::TEXT).default(text.map(|t| t.value.clone()).unwrap_or_default()),
        Ask::Default => Prompt::new("Enter new default value", Accept::TEXT).default(text.map(|t| t.value.clone()).unwrap_or_default()),
        Ask::Tag | Ask::AttPrompt => {
            let (msg, v) = match (kind, ask) {
                (Some(EntityKind::AttDef(a)), Ask::Tag) => ("Enter new tag", a.tag.clone()),
                (Some(EntityKind::AttDef(a)), _) => ("Enter new prompt", a.prompt.clone()),
                (_, Ask::Tag) => ("Enter new tag", String::new()),
                _ => ("Enter new prompt", String::new()),
            };
            Prompt::new(msg, Accept::TEXT).default(v)
        }
        Ask::Ignored => Prompt::new("", Accept::TEXT),
    }
}

/// The elevation of an object Elev sets: `Some(None)` when its points differ in z (a line between
/// two heights), `None` for the kinds Elev leaves alone.
pub(super) fn elevation_of(kind: &EntityKind) -> Option<Option<f64>> {
    match kind {
        EntityKind::Line(l) => Some((l.a.z == l.b.z).then_some(l.a.z)),
        EntityKind::Point(p) => Some(Some(p.p.z)),
        EntityKind::Circle(c) => Some(Some(c.center.z)),
        EntityKind::Arc(a) => Some(Some(a.center.z)),
        EntityKind::LwPolyline(p) => Some(Some(p.elevation)),
        EntityKind::Insert(i) => Some(Some(i.insert.z)),
        k => text_of(k).map(|t| Some(t.insert.z)),
    }
}

/// Move `text` up by `dz`.
fn raise_text(text: &mut Text, dz: f64) {
    for p in std::iter::once(&mut text.insert).chain(text.align_pt.as_mut()) {
        p.z += dz;
    }
}

fn set_elevation(kind: &mut EntityKind, z: f64) {
    let set = |p: &mut Vec3| p.z = z;
    match kind {
        EntityKind::Line(l) => {
            set(&mut l.a);
            set(&mut l.b);
        }
        EntityKind::Point(p) => set(&mut p.p),
        EntityKind::Circle(c) => set(&mut c.center),
        EntityKind::Arc(a) => set(&mut a.center),
        EntityKind::LwPolyline(p) => p.elevation = z,
        // A block reference's attributes go up with it.
        EntityKind::Insert(i) => {
            let dz = z - i.insert.z;
            set(&mut i.insert);
            for a in &mut i.attribs {
                raise_text(&mut a.text, dz);
                if let Some(ml) = &mut a.multiline {
                    ml.mtext.insert.z += dz;
                }
            }
        }
        k => {
            if let Some(t) = text_of_mut(k) {
                let dz = z - t.insert.z;
                raise_text(t, dz);
            }
        }
    }
}

/// Set the elevation of the objects among `hs` whose points share one: those set, and how many
/// differ in z (and keep theirs).
pub(super) fn elevate(s: &mut Session, hs: &[Handle], z: f64) -> Result<(Vec<Handle>, usize)> {
    let d = s.doc_mut()?;
    let (mut set, mut differing) = (Vec::new(), 0);
    for &h in hs {
        match d.entity(h).and_then(|e| elevation_of(&e.kind)) {
            Some(Some(_)) => {
                d.modify_entity(h, |e| set_elevation(&mut e.kind, z))?;
                set.push(h);
            }
            Some(None) => differing += 1,
            None => {}
        }
    }
    Ok((set, differing))
}

/// CHANGE's JSON form: `point` changes the objects as the change point does (`ignored`: circles,
/// texts and block references it leaves alone because others were selected too), `elevation` as
/// Properties ▸ Elev does (`differingZ`: objects whose points differ in z); objects on locked
/// layers are left out (`locked`).
pub(super) fn run_change(s: &mut Session, p: &Value) -> Result<Value> {
    // A point or an elevation that can't be read is refused, and so is a call with neither.
    let point = p.get("point").map(|_| point_req("change", p, "point")).transpose()?;
    let elevation = p.get("elevation").map(|_| f64_req("change", p, "elevation")).transpose()?;
    if point.is_none() && elevation.is_none() {
        return Err(bad("change", "`point` or `elevation` is required"));
    }
    let hs = targets(s, p)?;
    let locked = locked(s, &hs);
    let free: Vec<Handle> = hs.into_iter().filter(|h| !locked.contains(h)).collect();
    let (mut changed, mut ignored, mut differing) = (HashSet::new(), 0, 0);
    if let Some(q) = point {
        let ortho = s.drafting_settings().orthomode;
        for &h in &free {
            let Some(kind) = s.doc()?.entity(h).map(|e| e.kind.clone()) else { continue };
            if matches!(kind, EntityKind::Line(_)) {
                move_end(s, h, q, ortho)?;
                changed.insert(h);
            } else if questions(&kind).is_some() {
                if free.len() == 1 {
                    place(s, h, q)?;
                    changed.insert(h);
                } else {
                    ignored += 1;
                }
            }
        }
    }
    if let Some(z) = elevation {
        let (set, n) = elevate(s, &free, z)?;
        changed.extend(set);
        differing = n;
    }
    Ok(json!({ "changed": changed.len(), "ignored": ignored, "differingZ": differing, "locked": locked.len() }))
}

impl Interactive for ChangeM {
    fn name(&self) -> &'static str {
        "CHANGE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.select = begin_unlocked(s);
        if self.select.done {
            self.handles = self.select.picked.clone();
            self.stage = Stage::Point;
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, s: &Session) -> Prompt {
        match &self.stage {
            Stage::Select => self.select.prompt(),
            Stage::Point => Prompt::new("Specify change point", Accept::POINT).kw(&["Properties"]),
            Stage::Properties(m) => m.prompt(s),
            Stage::Asking(asks) => asks.front().map_or_else(|| Prompt::new("", Accept::TEXT), |&(h, ask)| ask_prompt(s, h, ask)),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match &mut self.stage {
            Stage::Select => {
                return Ok(match feed_unlocked(&mut self.select, s, i)? {
                    SelOutcome::More => Step::Continue,
                    SelOutcome::Done(hs) => {
                        self.handles = hs;
                        self.stage = Stage::Point;
                        Step::Continue
                    }
                    SelOutcome::Empty => {
                        s.set_selection(Vec::new());
                        Step::Done
                    }
                });
            }
            Stage::Properties(m) => return m.input(s, i),
            Stage::Asking(asks) => {
                let Some(&(h, ask)) = asks.front() else { return Ok(Step::Done) };
                if !answer(s, h, ask, i)? {
                    return Ok(Step::Continue);
                }
                asks.pop_front();
                return self.advance(s);
            }
            Stage::Point => {}
        }
        match i {
            Input::Keyword(k) if k == "Properties" => {
                self.stage = Stage::Properties(Box::new(ChpropM::change_properties(self.handles.clone())));
                Ok(Step::Continue)
            }
            Input::Point(p) => self.change_point(s, Some(p)),
            Input::Enter => self.change_point(s, None),
            _ => Err(EngineError::Other("Point or option keyword required.".into())),
        }
    }
}
