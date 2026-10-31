//! CHANGE as AutoCAD 2024 runs it from AutoLISP's `command` (accoreconsole, every object read back
//! with `entget`).

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{EntityCodec, set_entity_codec};
use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::doc::color::Color;
use serde_json::json;

/// The measured drawing, its objects named in LISP: lines `a` (0,0)–(50,0) and `b` (0,20)–(50,30)
/// on layer L1, circle `c` (centre (100,0), radius 10), text `t` "ABC" at (0,50) 2.5 high, circle
/// `e` ((200,0), 3), arc `r` ((150,0), 10, 0°–90°) and polyline `p` (0,100)–(50,100)–(50,120); then
/// `code`.
fn run(code: &str) -> Session {
    // `entmake` and `entget` go through the DXF codec, as in the app.
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    let mut s = Session::new();
    s.doc_mut().unwrap().ensure_layer("L1");
    for (v, made) in [
        ("a", "(0 . \"LINE\") (10 0.0 0.0 0.0) (11 50.0 0.0 0.0)"),
        ("b", "(0 . \"LINE\") (8 . \"L1\") (10 0.0 20.0 0.0) (11 50.0 30.0 0.0)"),
        ("c", "(0 . \"CIRCLE\") (10 100.0 0.0 0.0) (40 . 10.0)"),
        ("t", "(0 . \"TEXT\") (10 0.0 50.0 0.0) (40 . 2.5) (1 . \"ABC\")"),
        ("e", "(0 . \"CIRCLE\") (10 200.0 0.0 0.0) (40 . 3.0)"),
        ("r", "(0 . \"ARC\") (10 150.0 0.0 0.0) (40 . 10.0) (50 . 0.0) (51 . 1.5707963267948966)"),
        ("p", "(0 . \"LWPOLYLINE\") (90 . 3) (70 . 0) (10 0.0 100.0) (10 50.0 100.0) (10 50.0 120.0)"),
    ] {
        s.cmdline(&format!("(setq {v} (progn (entmake '({made})) (entlast)))")).unwrap();
    }
    s.log.clear();
    s.cmdline(&format!("(progn {code})")).unwrap();
    s
}

/// The `i`th object drawn.
fn kind(s: &Session, i: usize) -> EntityKind {
    s.doc().unwrap().model.iter().nth(i).unwrap().kind.clone()
}

fn line(s: &Session, i: usize) -> [[f64; 3]; 2] {
    let EntityKind::Line(l) = kind(s, i) else { panic!("not a line") };
    [[l.a.x, l.a.y, l.a.z], [l.b.x, l.b.y, l.b.z]]
}

fn radius(s: &Session, i: usize) -> f64 {
    let EntityKind::Circle(c) = kind(s, i) else { panic!("not a circle") };
    c.radius
}

fn text(s: &Session, i: usize) -> cadcraft_engine::doc::Text {
    match kind(s, i) {
        EntityKind::Text(t) => t,
        EntityKind::AttDef(a) => a.text,
        _ => panic!("not a text"),
    }
}

fn said(s: &Session, line: &str) -> usize {
    s.log.iter().filter(|l| l.as_str() == line).count()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn the_change_point_moves_the_nearer_end_of_each_line() {
    let s = run("(command \"_.CHANGE\" a \"\" '(60.0 10.0))");
    assert_eq!(line(&s, 0), [[0.0, 0.0, 0.0], [60.0, 10.0, 0.0]]);
    let s = run("(command \"_.CHANGE\" a \"\" '(-5.0 5.0))");
    assert_eq!(line(&s, 0), [[-5.0, 5.0, 0.0], [50.0, 0.0, 0.0]]);
    let s = run("(command \"_.CHANGE\" a b \"\" '(60.0 10.0))");
    assert_eq!((line(&s, 0)[1], line(&s, 1)), ([60.0, 10.0, 0.0], [[0.0, 20.0, 0.0], [60.0, 10.0, 0.0]]));
    // Under ortho the moved end stays level or plumb with the other one.
    let s = run("(setvar \"ORTHOMODE\" 1) (command \"_.CHANGE\" a b \"\" '(60.0 10.0))");
    assert_eq!((line(&s, 0)[1], line(&s, 1)[1]), ([60.0, 0.0, 0.0], [60.0, 20.0, 0.0]));
    // Arcs and polylines are left alone: with a line the point still moves it, without one there
    // is nothing to change. Enter leaves lines alone too.
    let s = run("(command \"_.CHANGE\" a r p \"\" '(60.0 10.0))");
    assert_eq!(line(&s, 0)[1], [60.0, 10.0, 0.0]);
    assert_eq!(said(&s, "No changeable object selected"), 0, "{:?}", s.log);
    for code in ["(command \"_.CHANGE\" r \"\" '(160.0 10.0))", "(command \"_.CHANGE\" p \"\" '(60.0 130.0))", "(command \"_.CHANGE\" a \"\" \"\")"] {
        let s = run(code);
        assert_eq!(said(&s, "No changeable object selected"), 1, "{code}: {:?}", s.log);
        assert!(s.running.is_none(), "{code}");
        assert_eq!(line(&s, 0)[1], [50.0, 0.0, 0.0], "{code}");
    }
}

#[test]
fn a_circle_passes_through_the_point_or_takes_a_radius() {
    let s = run("(command \"_.CHANGE\" c \"\" '(115.0 0.0))");
    assert!(near(radius(&s, 2), 15.0));
    let s = run("(command \"_.CHANGE\" c \"\" \"\" 20.0)");
    assert!(near(radius(&s, 2), 20.0));
    // With other objects the point is ignored, said as each circle comes up.
    let s = run("(command \"_.CHANGE\" c e \"\" '(110.0 10.0) 8.0)");
    assert_eq!((radius(&s, 2), radius(&s, 4)), (8.0, 3.0));
    assert_eq!(said(&s, "Change point ignored"), 2, "{:?}", s.log);
    assert!(s.prompt_text().contains("Specify new circle radius <no change>"), "{}", s.prompt_text());
    let s = run("(command \"_.CHANGE\" a c \"\" '(60.0 10.0) 12.0)");
    assert_eq!((line(&s, 0)[1], radius(&s, 2)), ([60.0, 10.0, 0.0], 12.0));
    assert_eq!(said(&s, "Change point ignored"), 1, "{:?}", s.log);
}

#[test]
fn a_text_is_placed_by_the_point_then_asked_the_rest() {
    let s = run("(command \"_.CHANGE\" t \"\" '(10.0 60.0) \"\" 4.0 \"\" \"\")");
    let t = text(&s, 3);
    assert_eq!((t.insert.x, t.insert.y, t.height, t.value.as_str()), (10.0, 60.0, 4.0, "ABC"));
    // Enter asks for the insertion point first.
    let s = run("(command \"_.CHANGE\" t \"\" \"\" \"\" \"\" 5.0 30 \"XYZ\")");
    let t = text(&s, 3);
    assert_eq!((t.insert.x, t.insert.y, t.height, t.value.as_str()), (0.0, 50.0, 5.0, "XYZ"));
    assert!(near(t.rotation.to_degrees(), 30.0));
    // A style that does not exist is refused, and asked for again.
    let s = run("(command \"_.CHANGE\" t \"\" \"\" \"\" \"5\")");
    assert_eq!(said(&s, "Style \"5\" not defined - use STYLE command."), 1, "{:?}", s.log);
    assert!(s.prompt_text().contains("Enter new text style <Standard>"), "{}", s.prompt_text());
    // Justified text is placed by its alignment point, with the point or after Enter.
    for (justify, code) in [("(72 . 1) (73 . 2)", "'(10.0 60.0) \"\" \"\" \"\" \"\""), ("(72 . 2)", "\"\" '(10.0 60.0) \"\" \"\" \"\" \"\"")] {
        let s = run(&format!(
            "(entmake '((0 . \"TEXT\") (10 0.0 50.0 0.0) (40 . 2.5) (1 . \"ABC\") {justify} (11 0.0 50.0 0.0))) (command \"_.CHANGE\" (entlast) \"\" {code})"
        ));
        let at = text(&s, 7).align_pt.unwrap();
        assert_eq!((at.x, at.y), (10.0, 60.0), "{justify}");
    }
}

#[test]
fn a_style_of_fixed_height_is_not_asked_for_a_height() {
    let mut s = run("");
    s.execute("style", &json!({ "name": "FIXH", "height": 5.0 })).unwrap();
    s.cmdline("(entmake '((0 . \"TEXT\") (10 0.0 80.0 0.0) (40 . 5.0) (1 . \"FIX\") (7 . \"FIXH\")))").unwrap();
    s.cmdline("(command \"_.CHANGE\" (entlast) \"\" \"\" \"\" \"\" 30 \"Q\")").unwrap();
    let t = text(&s, 7);
    assert_eq!((t.height, t.value.as_str()), (5.0, "Q"));
    assert!(near(t.rotation.to_degrees(), 30.0));
    assert!(s.running.is_none());
}

#[test]
fn objects_are_asked_about_in_the_order_selected() {
    let s = run(
        "(setq d (progn (entmake '((0 . \"TEXT\") (10 0.0 70.0 0.0) (40 . 2.5) (1 . \"DEF\"))) (entlast))) (command \"_.CHANGE\" t d \"\" \"\" \"\" \"\" \"\" \"\" \"X1\" \"\" \"\" \"\" \"\" \"X2\")",
    );
    assert_eq!((text(&s, 3).value.as_str(), text(&s, 7).value.as_str()), ("X1", "X2"));
    let s = run("(command \"_.CHANGE\" t c \"\" \"\" \"\" \"\" \"\" \"\" \"TT\" 7.0)");
    assert_eq!((text(&s, 3).value.as_str(), radius(&s, 2)), ("TT", 7.0));
    let s = run("(command \"_.CHANGE\" c t \"\" \"\" 7.0 \"\" \"\" \"\" \"\" \"TT\")");
    assert_eq!((text(&s, 3).value.as_str(), radius(&s, 2)), ("TT", 7.0));
}

/// Block K: a circle and attribute TAG1 (default V) 5 above its base point, inserted at (200,0)
/// with the default value.
const BLOCK: &str = "(entmake '((0 . \"BLOCK\") (2 . \"K\") (70 . 2) (10 0.0 0.0 0.0))) (entmake '((0 . \"CIRCLE\") (10 0.0 0.0 0.0) (40 . 3.0))) (entmake '((0 . \"ATTDEF\") (10 0.0 5.0 0.0) (40 . 2.5) (1 . \"V\") (3 . \"P\") (2 . \"TAG1\") (70 . 0))) (entmake '((0 . \"ENDBLK\"))) (command \"_.-INSERT\" \"K\" '(200.0 0.0) \"\" \"\" 0 \"\")";

#[test]
fn a_block_reference_is_placed_and_turned_with_its_attributes() {
    let s = run(&format!("{BLOCK} (command \"_.CHANGE\" (entlast) \"\" '(250.0 50.0) 90)"));
    let EntityKind::Insert(i) = kind(&s, 7) else { panic!("not a block reference") };
    assert_eq!((i.insert.x, i.insert.y), (250.0, 50.0));
    assert!(near(i.rotation.to_degrees(), 90.0));
    let a = &i.attribs.first().expect("the attribute").text;
    assert!(near(a.insert.x, 245.0) && near(a.insert.y, 50.0), "{:?}", a.insert);
    assert!(near(a.rotation.to_degrees(), 90.0));
    let s = run(&format!("{BLOCK} (command \"_.CHANGE\" (entlast) \"\" \"\" '(220.0 30.0) 90)"));
    let EntityKind::Insert(i) = kind(&s, 7) else { panic!("not a block reference") };
    assert_eq!((i.insert.x, i.insert.y), (220.0, 30.0));
    assert!(near(i.rotation.to_degrees(), 90.0));
}

#[test]
fn an_attribute_definition_is_asked_its_tag_prompt_and_default() {
    let s = run(
        "(entmake '((0 . \"ATTDEF\") (10 300.0 0.0 0.0) (40 . 2.5) (1 . \"V\") (3 . \"P\") (2 . \"TAG1\") (70 . 0))) (command \"_.CHANGE\" (entlast) \"\" '(310.0 10.0) \"\" \"\" \"\" \"T2\" \"P2\" \"V2\")",
    );
    let EntityKind::AttDef(a) = kind(&s, 7) else { panic!("not an attribute definition") };
    assert_eq!((a.text.insert.x, a.text.insert.y), (310.0, 10.0));
    assert_eq!((a.tag.as_str(), a.prompt.as_str(), a.text.value.as_str()), ("T2", "P2", "V2"));
}

#[test]
fn objects_on_locked_layers_are_left_out() {
    let s = run("(command \"_.-LAYER\" \"_LO\" \"L1\" \"\") (command \"_.CHANGE\" a b \"\" '(60.0 10.0))");
    assert_eq!((line(&s, 0)[1], line(&s, 1)[1]), ([60.0, 10.0, 0.0], [50.0, 30.0, 0.0]));
    assert_eq!(said(&s, "1 was on a locked layer."), 1, "{:?}", s.log);
}

#[test]
fn properties_add_elev_which_is_set_as_the_command_ends() {
    let s = run("(command \"_.CHANGE\" a \"\" \"_P\" \"_C\" 2 \"_E\" 10.0 \"\")");
    let d = s.doc().unwrap();
    assert_eq!(d.model.iter().next().unwrap().common.color, Color::Index(2));
    assert_eq!(line(&s, 0), [[0.0, 0.0, 10.0], [50.0, 0.0, 10.0]]);
    let s = run("(command \"_.CHANGE\" a c \"\" \"_P\" \"_LA\" \"L1\" \"_T\" 3.0 \"\")");
    for e in s.doc().unwrap().model.iter().take(3).step_by(2) {
        assert_eq!((e.common.layer.as_str(), e.common.thickness), ("L1", 3.0));
    }
    let s = run("(command \"_.CHANGE\" c t r p \"\" \"_P\" \"_E\" 5.0 \"\")");
    let EntityKind::Circle(c) = kind(&s, 2) else { panic!() };
    let EntityKind::Arc(r) = kind(&s, 5) else { panic!() };
    let EntityKind::LwPolyline(p) = kind(&s, 6) else { panic!() };
    assert_eq!((c.center.z, text(&s, 3).insert.z, r.center.z, p.elevation), (5.0, 5.0, 5.0, 5.0));
    // A line between two heights keeps them, said as the command ends; the elevation asked shows
    // it varies.
    let mut s = run(
        "(setq l (progn (entmake '((0 . \"LINE\") (10 0.0 200.0 0.0) (11 50.0 200.0 10.0))) (entlast))) (command \"_.CHANGE\" l \"\" \"_P\" \"_E\")",
    );
    assert!(s.prompt_text().contains("Specify new elevation <varies>"), "{}", s.prompt_text());
    s.cmdline("(command 5.0 \"\")").unwrap();
    assert_eq!(line(&s, 7), [[0.0, 200.0, 0.0], [50.0, 200.0, 10.0]]);
    assert_eq!(said(&s, "Cannot change elevation of objects with differing Z coordinates."), 1, "{:?}", s.log);
}

#[test]
fn change_by_json() {
    let mut s = run("");
    let h: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    let r = s.execute("change", &json!({ "handles": [h[0], h[2]], "point": [60, 10] })).unwrap();
    assert_eq!((r["changed"].as_u64(), r["ignored"].as_u64()), (Some(1), Some(1)), "{r}");
    assert_eq!((line(&s, 0)[1], radius(&s, 2)), ([60.0, 10.0, 0.0], 10.0));
    s.execute("change", &json!({ "handles": [h[2]], "point": [115, 0] })).unwrap();
    assert!(near(radius(&s, 2), 15.0));
    s.execute("layer.set", &json!({ "name": "L1", "locked": true })).unwrap();
    let r = s.execute("change", &json!({ "handles": [h[0], h[1]], "elevation": 4.0 })).unwrap();
    assert_eq!((r["changed"].as_u64(), r["locked"].as_u64()), (Some(1), Some(1)), "{r}");
    assert_eq!((line(&s, 0)[0][2], line(&s, 1)[0][2]), (4.0, 0.0));
    // Neither a point nor an elevation, or one that can't be read, is refused.
    for bad in [json!({ "handles": [h[0]] }), json!({ "handles": [h[0]], "point": "nowhere" }), json!({ "handles": [h[0]], "elevation": "high" })] {
        assert!(s.execute("change", &bad).is_err(), "{bad}");
    }
    assert_eq!(line(&s, 0), [[0.0, 0.0, 4.0], [60.0, 10.0, 4.0]]);
}
