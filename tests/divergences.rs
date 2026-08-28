//! The divergence budget is checked, not merely written down.
//!
//! The rule this enforces is "divergences are declared, never discovered": a
//! behaviour difference found while chasing a conformance failure is either a
//! bug to fix or a new entry in `DIVERGENCES.md`, added in the same pull
//! request that finds it.
//!
//! A rule like that decays unless something notices. Pinning the count means a
//! new entry cannot be added quietly and, more importantly, that an entry
//! cannot be *removed* quietly either -- deleting a divergence is a claim that
//! upstream behaviour is now matched, which deserves the same scrutiny as
//! declaring one.
//!
//! Changing the number here is expected. Changing it without touching
//! `DIVERGENCES.md` is not possible, which is the entire point.

/// The day-one budget: four from the porting strategy (fences default to
/// `process=false`, pulldown-cmark rather than markdown-it, synchronous
/// transform hooks, no React renderers), three CommonMark precedence rules the
/// conformance corpus cannot arbitrate, and the `allowIndentation` option,
/// which upstream reaches by patching markdown-it and which the host forbids
/// reaching at all.
const DECLARED_DIVERGENCES: usize = 8;

const DIVERGENCES: &str = include_str!("../DIVERGENCES.md");

fn entries() -> Vec<&'static str> {
    DIVERGENCES
        .lines()
        .filter(|line| line.starts_with("## "))
        .collect()
}

#[test]
fn divergence_count_matches_the_declared_budget() {
    let found = entries();
    assert_eq!(
        found.len(),
        DECLARED_DIVERGENCES,
        "DIVERGENCES.md lists {} entries but the declared budget is {}. \
         If this change adds or removes a divergence, update \
         DECLARED_DIVERGENCES in the same commit and say why in the pull \
         request. Entries found: {:#?}",
        found.len(),
        DECLARED_DIVERGENCES,
        found
    );
}

#[test]
fn every_divergence_states_a_reason() {
    // An entry without a reason is a note, not a declaration: the next person
    // cannot tell whether emulating upstream was rejected or never considered.
    let sections: Vec<&str> = DIVERGENCES.split("\n## ").skip(1).collect();
    for section in sections {
        let title = section.lines().next().unwrap_or(section);
        assert!(
            section.contains("**Why:**"),
            "divergence {title:?} has no '**Why:**' paragraph"
        );
    }
}

#[test]
fn the_error_vocabulary_is_named_as_non_divergent() {
    // Upstream validation error ids are the one thing that may never diverge,
    // because external tooling binds to them. The file has to say so.
    assert!(
        DIVERGENCES.contains("error vocabulary never diverges"),
        "DIVERGENCES.md no longer states that error ids never diverge"
    );
}
