//! Debug helper: dumps extracted PDF structure for the fixtures (run with --ignored).
use quire_doc::memsink::MemSink;
use quire_qtx::{Reader, Token};

#[test]
#[ignore]
fn dump() {
    let dir = std::env::var("QUIRE_DUMP_DIR").unwrap_or_else(|_| "/tmp".into());
    for (name, bytes) in [
        ("tracemonkey", &include_bytes!("../fixtures/tracemonkey.pdf")[..]),
        ("basicapi", &include_bytes!("../fixtures/basicapi.pdf")[..]),
        ("issue1002", &include_bytes!("../fixtures/issue1002.pdf")[..]),
        ("alphatrans", &include_bytes!("../fixtures/alphatrans.pdf")[..]),
    ] {
        let mut sink = MemSink::default();
        quire_doc::pdf::ingest(&bytes, name, &mut sink).unwrap();
        let mut out = String::new();
        for (t, b, _) in &sink.chapters {
            out.push_str(&format!("=== chapter {:?}\n", t));
            for tok in Reader::new(b) {
                match tok {
                    Token::Text(s) => out.push_str(&s),
                    Token::Para(k) => out.push_str(&format!("\n[{k:?}] ")),
                    Token::Style(f) => out.push_str(&format!("<{f}>")),
                    Token::Anchor(a) => out.push_str(&format!("\n#{a}\n")),
                    Token::Image { id, w, h } => out.push_str(&format!("\n[image {id} {w}x{h}]\n")),
                    other => out.push_str(&format!("{other:?}")),
                }
            }
        }
        out.push_str(&format!(
            "\nTOC: {:?}\nimages: {}\nmeta: {:?}\n",
            sink.toc.iter().map(|t| (t.title.clone(), t.chapter, t.depth)).collect::<Vec<_>>(),
            sink.images.len(),
            sink.meta
        ));
        std::fs::write(format!("{dir}/{name}.txt"), out).unwrap();
    }
}
