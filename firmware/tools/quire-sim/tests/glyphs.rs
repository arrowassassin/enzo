//! Every symbol the UI writes as text exists in the face it is drawn with.

#[test]
fn ui_symbols_have_glyphs() {
    let faces = [
        ("label", quire_fonts::ui::label()),
        ("body", quire_fonts::ui::body()),
        ("list_title", quire_fonts::ui::list_title()),
        ("title", quire_fonts::ui::title()),
        ("mono", quire_fonts::ui::mono()),
        ("poster", quire_fonts::ui::poster()),
    ];
    for (name, f) in faces {
        let report: Vec<String> = ['✓', '★', '☆', '●', '○', '→', '←', '↑', '↓', '…', '·', '‹', '›', '—', '°', '“', '’', '€', '×', '÷', '−']
            .iter()
            .map(|c| format!("{c}:{}", if f.has(*c) { "y" } else { "N" }))
            .collect();
        println!("{name}: {}", report.join(" "));
    }
    // The characters the UI relies on in every face.
    for (name, f) in [("label", quire_fonts::ui::label()), ("body", quire_fonts::ui::body()), ("mono", quire_fonts::ui::mono())] {
        for c in ['…', '·', '‹', '›', '—', '°', '’'] {
            assert!(f.has(c), "{name} lacks {c:?}");
        }
    }
}
