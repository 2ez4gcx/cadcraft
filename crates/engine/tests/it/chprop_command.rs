//! CHPROP as AutoCAD 2024 runs it from AutoLISP's `command` (accoreconsole, every object read back
//! with `entget`).

use cadcraft_engine::Session;
use cadcraft_engine::doc::Lineweight;
use cadcraft_engine::doc::color::{Color, Rgb};
use serde_json::json;

/// A session with line A (0,0)-(50,0) on layer 0 and line B (0,20)-(50,30) on layer L1, as `a` and
/// `b` in LISP, after `code`; the properties of A then B.
fn run(code: &str) -> (Session, [cadcraft_engine::doc::Common; 2]) {
    let mut s = Session::new();
    s.doc_mut().unwrap().ensure_layer("L1");
    s.execute("line", &json!({ "points": [[0, 0], [50, 0]] })).unwrap();
    s.cmdline("(setq a (entlast))").unwrap();
    s.execute("line", &json!({ "points": [[0, 20], [50, 30]] })).unwrap();
    s.cmdline("(setq b (entlast))").unwrap();
    s.cmdline("(command \"_.CHPROP\" b \"\" \"_LA\" \"L1\" \"\")").unwrap();
    s.cmdline(&format!("(progn {code})")).unwrap();
    if s.running.is_some() {
        s.cancel();
    }
    let common = {
        let d = s.doc().unwrap();
        let mut it = d.model.iter().map(|e| e.common.clone());
        [it.next().unwrap(), it.next().unwrap()]
    };
    (s, common)
}

#[test]
fn chprop_sets_each_property_as_autocad() {
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_C\" 1 \"\")");
    assert_eq!(a.color, Color::Index(1));
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_C\" \"blue\" \"\")");
    assert_eq!(a.color, Color::Index(5));
    let (_, [_, b]) = run("(command \"_.CHPROP\" b \"\" \"_C\" \"_BYBLOCK\" \"\")");
    assert_eq!(b.color, Color::ByBlock);
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_C\" 3 \"\") (command \"_.CHPROP\" a \"\" \"_C\" \"_BYLAYER\" \"\")");
    assert_eq!(a.color, Color::ByLayer);
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_C\" \"_T\" \"255,128,0\" \"\")");
    assert_eq!(a.color, Color::True(Rgb(255, 128, 0)));
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_LA\" \"L1\" \"\")");
    assert_eq!(a.layer, "L1");
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_LT\" \"DASHED\" \"\")");
    assert_eq!(a.linetype, "DASHED");
    // Not loaded yet: loaded and given, as AutoCAD loads it from acadiso.lin.
    let (s, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_LT\" \"PHANTOM2\" \"\")");
    assert_eq!(a.linetype, "PHANTOM2");
    assert!(s.doc().unwrap().linetype("PHANTOM2").is_some());
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_S\" 2.5 \"\")");
    assert_eq!(a.ltscale, 2.5);
    let (s, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_LW\" 0.33 \"\")");
    assert_eq!(a.lineweight, Lineweight::Mm100(35));
    assert!(s.log.iter().any(|l| l == "Lineweight rounded to nearest valid value of 0.35 mm"), "{:?}", s.log);
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_T\" 5.0 \"\")");
    assert_eq!(a.thickness, 5.0);
    // Several objects and several properties in one go.
    let (_, [a, b]) = run("(command \"_.CHPROP\" a b \"\" \"_C\" 3 \"_LA\" \"L1\" \"_LW\" 0.5 \"\")");
    for c in [a, b] {
        assert_eq!((c.color, c.layer.as_str(), c.lineweight), (Color::Index(3), "L1", Lineweight::Mm100(50)));
    }
    let (_, [a, _]) = run(
        "(command \"_.CHPROP\" a \"\" \"_C\" 3 \"\") (command \"_.CHPROP\" a \"\" \"_C\" \"_BYLAYER\" \"_LT\" \"_BYBLOCK\" \"_LW\" \"_BYLAYER\" \"\")",
    );
    assert_eq!((a.color, a.linetype.as_str(), a.lineweight), (Color::ByLayer, "ByBlock", Lineweight::ByLayer));
}

#[test]
fn chprop_refuses_what_autocad_refuses() {
    let (s, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_LA\" \"KHONGCO\" \"\")");
    assert_eq!(a.layer, "0");
    assert!(s.log.iter().any(|l| l == "Cannot find layer \"KHONGCO\"."), "{:?}", s.log);
    for n in [0, 256, 300] {
        let (s, [a, _]) = run(&format!("(command \"_.CHPROP\" a \"\" \"_C\" {n} \"\")"));
        assert_eq!(a.color, Color::ByLayer, "{n}");
        assert!(s.log.iter().any(|l| l == "Invalid color number."), "{n}: {:?}", s.log);
    }
    // `L` is LAyer, LType, ltScale or LWeight: none of them.
    let (s, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_L\")");
    assert_eq!(a.layer, "0");
    assert!(s.log.iter().any(|l| l == "Ambiguous response, please clarify..."), "{:?}", s.log);
}
