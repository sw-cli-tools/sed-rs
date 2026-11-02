//! Differential test: for random scripts in the v0.2 grammar and random
//! inputs, the current library must agree with the frozen v0.2.0 library
//! (tests/support/v0_2.rs) on whether the script parses, and on the output
//! bytes of `process` and `apply`.
//!
//! The grammar leaves out what was changed on purpose (unknown `s` flags,
//! which are now errors) and what loops forever (backward branches).

#[path = "support/v0_2.rs"]
mod v0_2;

use proptest::prelude::*;

fn pattern() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "", "a", "b", "ab", "^", "$", "^a", "b$", ".", "a*", "[ab]", "[^a]", "a+", r"\(a\)",
        r"\(a\)\(b\)", r"a\{2\}", r"a\+", r"a\?", r"a\|b", r"\n", r"\.", "[[:space:]]", " *",
        "(", "x?",
    ])
    .prop_map(String::from)
}

fn replacement() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "", "X", "&", "[&]", r"\&", r"\1", r"\2", r"\n", r"\t", "$", r"\$", r"\\", "$1", "a b",
    ])
    .prop_map(String::from)
}

fn flags() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["", "g", "i", "I", "gi", "ig"]).prop_map(String::from)
}

fn delimiter() -> impl Strategy<Value = char> {
    prop::sample::select(vec!['/', '|', '#', ','])
}

fn address() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "", "", "", "1", "2", "3", "0", "$", "/a/", "/^$/", "/b$/", r"/\n/", "/(/",
    ])
    .prop_flat_map(|a| {
        let a = a.to_string();
        prop::sample::select(vec!["", "", "!", " !", "! "])
            .prop_map(move |n| if a.is_empty() { a.clone() } else { format!("{a}{n}") })
    })
}

fn subst() -> impl Strategy<Value = String> {
    (address(), delimiter(), pattern(), replacement(), flags()).prop_map(|(a, d, p, r, f)| {
        let esc = |s: &str| s.replace(d, &format!("\\{d}"));
        format!("{a}s{d}{}{d}{}{d}{f}", esc(&p), esc(&r))
    })
}

fn command() -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        4 => subst(),
        1 => address().prop_map(|a| format!("{a}d")),
        1 => address().prop_map(|a| format!("{a}N")),
        1 => address().prop_map(|a| format!("{a}b")),
        1 => address().prop_map(|a| format!("{a}bend")),
    ];
    leaf.prop_recursive(2, 6, 3, |inner| {
        (address(), prop::collection::vec(inner, 1..3))
            .prop_map(|(a, body)| format!("{a}{{{}}}", body.join(";")))
    })
}

/// Expressions for `-e`, with a trailing `:end` so `bend` always resolves.
fn script() -> impl Strategy<Value = Vec<String>> {
    (
        prop::collection::vec(command(), 1..5),
        prop::sample::select(vec![";", "\n", " ; "]),
        any::<bool>(),
    )
        .prop_map(|(cmds, sep, split)| {
            let mut exprs = if split {
                cmds
            } else {
                vec![cmds.join(sep)]
            };
            exprs.push(":end".to_string());
            exprs
        })
}

fn input() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(
        prop::sample::select(vec![b'a', b'b', b'\n', b'\n', b' ', b'\r', b'x', b'(']),
        0..24,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    #[test]
    fn matches_v0_2_0(exprs in script(), input in input()) {
        let refs: Vec<&str> = exprs.iter().map(String::as_str).collect();
        let old = v0_2::Sed::parse(&refs);
        let new = sed_rs::Sed::parse(&refs);
        prop_assert_eq!(old.is_ok(), new.is_ok(), "parse disagrees on {:?}", exprs);
        if let (Ok(old), Ok(new)) = (old, new) {
            let mut a = Vec::new();
            let mut b = Vec::new();
            let ra = old.process(&input[..], &mut a);
            let rb = new.process(&input[..], &mut b);
            prop_assert_eq!(ra.is_ok(), rb.is_ok());
            prop_assert_eq!(&a, &b, "process differs on {:?}", exprs);
            if let Ok(line) = std::str::from_utf8(&input) {
                let line = line.lines().next().unwrap_or("");
                prop_assert_eq!(old.apply(line), new.apply(line), "apply differs on {:?}", exprs);
            }
        }
    }
}
