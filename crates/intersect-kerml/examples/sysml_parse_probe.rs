//! Handwritten measurement host (not generated): time the SysML parse of
//! small snippets, to find which constructs make the parser slow.
//!
//! Usage: `sysml_parse_probe ROOT SNIPPETS [REPEAT]`. `SNIPPETS` is a text
//! file of SysML sources separated by lines holding only `---`; each is
//! parsed `REPEAT` times (default 3) into a fresh `FrozenStore`, and the
//! fastest time is printed with the snippet's first line and its diagnostics.
//! The first (cold) time is printed too: later repeats reuse the runtime
//! prediction memo the first one filled. `INTERSECT_PREDICTION_MEMO` selects
//! the memo mode (`off`, `on` (default) or `verify`).
use intersect_kerml::{frozen_load, native_pilot::NativePilotSession};
use intersect_model::{
    core::frozen::FrozenStore,
    definitions::conversion::{InvocationContext, InvocationObserver, ProviderId},
};
use intersect_text::Source;
use std::{path::PathBuf, rc::Rc, time::Instant};

struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

const CORPUS: &str = "fixtures/native-pilot/v1/sources";
const DEFINITIONS: [&str; 4] = [
    "org.omg.sysml/model/SysML.ecore",
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext",
    "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext",
    "org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext",
];

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .ok_or("usage: sysml_parse_probe ROOT SNIPPETS [REPEAT]")?,
    );
    let snippets_path = args.next().ok_or("missing SNIPPETS")?;
    let repeat: usize = match args.next() {
        Some(n) => n.parse().map_err(|e| format!("REPEAT: {e}"))?,
        None => 3,
    };
    let text =
        std::fs::read_to_string(&snippets_path).map_err(|e| format!("{snippets_path}: {e}"))?;
    let snippets: Vec<String> = text
        .replace("\r\n", "\n")
        .split("\n---\n")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let definitions = DEFINITIONS
        .iter()
        .map(|p| {
            Ok(Source {
                uri: p.to_string(),
                text: std::fs::read_to_string(root.join(CORPUS).join(p))
                    .map_err(|e| format!("{p}: {e}"))?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let clock = Instant::now();
    let preparation = InvocationContext::new(Rc::new(Observer));
    let session = NativePilotSession::prepare(&definitions, &preparation)?;
    eprintln!("prepared in {:.1} s", clock.elapsed().as_secs_f64());
    session.with_definitions(|_kerml, _, sysml, sysml_contracts| {
        let metamodel = frozen_load::frozen_metamodel(sysml)?;
        frozen_load::with_sysml(sysml, sysml_contracts, &preparation, |language| {
            let memo = language.language.prediction_memo();
            memo.set_mode(frozen_load::prediction_memo_mode_from_env()?);
            for (index, snippet) in snippets.iter().enumerate() {
                let source = Source {
                    uri: format!("probe{index}.sysml"),
                    text: snippet.clone(),
                };
                let mut best = f64::MAX;
                let mut cold = None;
                let mut diagnostics = Vec::new();
                for _ in 0..repeat {
                    let mut store = FrozenStore::new(metamodel.clone());
                    let clock = Instant::now();
                    let result = language.parse(&source, &mut store);
                    let elapsed = clock.elapsed().as_secs_f64() * 1e3;
                    cold.get_or_insert(elapsed);
                    best = best.min(elapsed);
                    diagnostics = match result {
                        Ok((evaluation, _, _)) => evaluation
                            .diagnostics
                            .iter()
                            .map(|d| d.message.clone())
                            .collect(),
                        Err(error) => vec![error],
                    };
                }
                let first = snippet.lines().next().unwrap_or_default();
                println!(
                    "{best:>10.1} ms (cold {:>10.1} ms)  {:>6} B  {first}{}",
                    cold.unwrap_or_default(),
                    snippet.len(),
                    if diagnostics.is_empty() {
                        String::new()
                    } else {
                        format!("  !! {}", diagnostics.join("; "))
                    }
                );
            }
            eprintln!("prediction memo: {:?}", memo.stats());
            Ok(())
        })
    })
}
