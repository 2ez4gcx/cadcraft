//! AREA, PERIMETER and DISTANCE as AutoCAD 2024 leaves them after AREA, DIST and MEASUREGEOM
//! (measured with accoreconsole, running these scripts on these drawings): AREA is the area just
//! measured, or the running total in Add and Subtract mode, PERIMETER the perimeter of the last
//! area; DISTANCE the distance measured, or the total of DIST's Multiple points. After DIST's two
//! points LASTPOINT is still the first; after Multiple points it is the last.

use cadcraft_engine::doc::{Common, EntityKind, Handle};
use cadcraft_engine::geom::{Spline, Vec2};
use cadcraft_engine::{Session, sysvars};
use serde_json::{Value, json};

fn var(s: &Session, name: &str) -> f64 {
    sysvars::get(s, name).and_then(|v| v.as_f64()).unwrap_or_else(|| panic!("{name} not a number"))
}

fn lastpoint(s: &Session) -> (f64, f64) {
    let v = sysvars::get(s, "LASTPOINT").unwrap();
    (v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
}

fn close(got: f64, want: f64) -> bool {
    (got - want).abs() <= 1e-9 * (1.0 + want.abs())
}

fn handle(v: &Value) -> Handle {
    Handle::parse_hex(v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap()).unwrap()
}

fn lisp(s: &mut Session, expr: &str) {
    s.cmdline(expr).unwrap();
    assert!(s.running.is_none(), "{expr}: {}", s.prompt_text());
}

/// The closed polyline with an arc and the circle the Add-mode scripts pick.
fn p_and_c(s: &mut Session) -> (Handle, Handle) {
    let p = handle(&s.execute("pline", &json!({ "vertices": [[0, 0], { "p": [40, 0], "bulge": 0.5 }, [40, 30], [0, 30]], "closed": true })).unwrap());
    let c = handle(&s.execute("circle", &json!({ "center": [150, 15], "radius": 10 })).unwrap());
    (p, c)
}

#[test]
fn area_of_points_sets_area_and_perimeter() {
    let mut s = Session::new();
    lisp(&mut s, "(command \"_.AREA\" '(0.0 0.0) '(40.0 0.0) '(40.0 30.0) '(0.0 30.0) \"\")");
    assert!(close(var(&s, "AREA"), 1200.0) && close(var(&s, "PERIMETER"), 140.0));
}

#[test]
fn add_mode_totals_the_area_and_keeps_the_last_perimeter() {
    // Points, then the circle as an object.
    let mut s = Session::new();
    let (_, c) = p_and_c(&mut s);
    lisp(
        &mut s,
        &format!(
            "(command \"_.AREA\" \"_A\" '(0.0 0.0) '(40.0 0.0) '(40.0 30.0) '(0.0 30.0) \"\" \"_O\" (list (handent \"{}\") '(160.0 15.0)) \"\" \"\")",
            c.hex()
        ),
    );
    assert!(close(var(&s, "AREA"), 1514.159265358979), "{}", var(&s, "AREA"));
    assert!(close(var(&s, "PERIMETER"), 62.83185307179586), "{}", var(&s, "PERIMETER"));
    // Both as objects.
    let mut s = Session::new();
    let (p, c) = p_and_c(&mut s);
    lisp(
        &mut s,
        &format!(
            "(command \"_.AREA\" \"_A\" \"_O\" (list (handent \"{}\") '(0.0 15.0)) (list (handent \"{}\") '(160.0 15.0)) \"\" \"\")",
            p.hex(),
            c.hex()
        ),
    );
    assert!(close(var(&s, "AREA"), 1671.411490437671), "{}", var(&s, "AREA"));
    assert!(close(var(&s, "PERIMETER"), 62.83185307179586), "{}", var(&s, "PERIMETER"));
}

#[test]
fn subtract_mode_takes_the_area_off_the_total() {
    // At large coordinates: the polyline added, the circle subtracted.
    let mut s = Session::new();
    let p = handle(
        &s.execute(
            "pline",
            &json!({ "vertices": [[-35000, 81000], { "p": [-33000, 81000], "bulge": 0.5 }, [-33000, 82500], [-35000, 82500]], "closed": true }),
        )
        .unwrap(),
    );
    let c = handle(&s.execute("circle", &json!({ "center": [-27000, 81750], "radius": 500 })).unwrap());
    lisp(
        &mut s,
        &format!(
            "(command \"_.AREA\" \"_A\" \"_O\" (list (handent \"{}\") '(-35000.0 81750.0)) \"\" \"_S\" \"_O\" (list (handent \"{}\") '(-27000.0 81750.0)) \"\" \"\")",
            p.hex(),
            c.hex()
        ),
    );
    assert!(close(var(&s, "AREA"), 2607732.399299283), "{}", var(&s, "AREA"));
    assert!(close(var(&s, "PERIMETER"), 3141.592653589793), "{}", var(&s, "PERIMETER"));
}

#[test]
fn area_of_an_open_spline_closes_it_by_its_chord() {
    let control = [(0.8, 0.25), (0.82, 0.24), (0.84, 0.25), (0.84, 0.28), (0.82, 0.29), (0.8, 0.28), (0.8, 0.25)];
    let spline = Spline {
        degree: 3,
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0, 4.0, 4.0],
        control: control.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
        weights: Vec::new(),
        fit: Vec::new(),
        closed: false,
        fit_opts: Default::default(),
    };
    let mut s = Session::new();
    let space = s.space();
    let h = s.doc_mut().unwrap().add(&space, Common::default(), EntityKind::Spline(spline)).unwrap();
    lisp(&mut s, &format!("(command \"_.AREA\" \"_O\" (list (handent \"{}\") '(0.8 0.265)))", h.hex()));
    assert!(close(var(&s, "AREA"), 0.001308333333333334), "{}", var(&s, "AREA"));
    // The spline's length, 0.13046952991870 by an independent integration. AutoCAD reported
    // 0.1304733129441191, 2.9e-5 longer than that: its own approximation of the length.
    assert!(close(var(&s, "PERIMETER"), 0.1304695299187), "{}", var(&s, "PERIMETER"));
}

#[test]
fn dist_sets_the_distance_and_leaves_lastpoint_at_the_first_point() {
    let mut s = Session::new();
    lisp(&mut s, "(command \"_.DIST\" '(0.5 0.25) '(0.53 0.29))");
    assert!(close(var(&s, "DISTANCE"), 0.05), "{}", var(&s, "DISTANCE"));
    let (x, y) = lastpoint(&s);
    assert!(close(x, 0.5) && close(y, 0.25), "{x}, {y}");
}

#[test]
fn dist_with_multiple_points_sets_the_total_and_lastpoint_at_the_last() {
    let mut s = Session::new();
    lisp(&mut s, "(command \"_.DIST\" '(0.0 0.0) \"_M\" '(40.0 0.0) '(40.0 30.0) '(0.0 30.0) \"\")");
    assert!(close(var(&s, "DISTANCE"), 110.0), "{}", var(&s, "DISTANCE"));
    let (x, y) = lastpoint(&s);
    assert!(close(x, 0.0) && close(y, 30.0), "{x}, {y}");
}

#[test]
fn measuregeom_sets_them_as_dist_and_area_do() {
    let mut s = Session::new();
    // (AutoCAD's MEASUREGEOM asks for the next option after each measurement until eXit; here it
    // ends after one.)
    lisp(&mut s, "(command \"_.MEASUREGEOM\" \"_D\" '(0.0 0.0) '(30.0 40.0))");
    assert!(close(var(&s, "DISTANCE"), 50.0), "{}", var(&s, "DISTANCE"));
    lisp(&mut s, "(command \"_.MEASUREGEOM\" \"_AR\" '(0.5 0.25) '(0.54 0.25) '(0.54 0.28) '(0.5 0.28) \"\")");
    assert!(close(var(&s, "AREA"), 0.0012) && close(var(&s, "PERIMETER"), 0.14), "{} {}", var(&s, "AREA"), var(&s, "PERIMETER"));
}

#[test]
fn the_json_forms_set_them_too_and_they_are_read_only() {
    let mut s = Session::new();
    s.execute("area", &json!({ "points": [[0, 0], [40, 0], [40, 30], [0, 30]] })).unwrap();
    s.execute("dist", &json!({ "p1": [0, 0], "p2": [30, 40] })).unwrap();
    assert_eq!((var(&s, "AREA"), var(&s, "PERIMETER"), var(&s, "DISTANCE")), (1200.0, 140.0, 50.0));
    for name in ["AREA", "PERIMETER", "DISTANCE"] {
        assert!(s.execute("setvar", &json!({ "name": name, "value": 1 })).is_err(), "{name}");
    }
}
