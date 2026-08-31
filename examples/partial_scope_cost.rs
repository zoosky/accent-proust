//! Measures what `{% partial %}` expansion costs as `config.partials` grows.
//!
//! `{% partial %}` scopes a partial's body by handing the transformer a config
//! whose `variables` differ and whose everything-else is the caller's. When
//! those shared maps were owned rather than shared, that scoping copied the
//! site's whole partial corpus once per expansion, so the cost of expanding
//! *one* partial grew with how many partials the site had.
//!
//! Run with `cargo run --release --example partial_scope_cost`. Measured on an
//! M-series laptop, 50 expansions x 20 runs, before and after the four shared
//! maps moved behind an `Arc`:
//!
//! | partials in config | owned maps | shared maps |
//! |--------------------|-----------:|------------:|
//! | 1                  |    8.05 us |     0.88 us |
//! | 10                 |    25.1 us |     0.92 us |
//! | 50                 |    80.8 us |     0.89 us |
//! | 200                |     276 us |     0.89 us |
//! | 1000               |    1.51 ms |     0.91 us |
//!
//! The shape is the finding, not the absolute numbers: the left column is
//! linear in the number of partials the config holds and the right column is
//! flat. Nothing enforces this -- a timing assertion would be flaky in CI --
//! so it is an example you can re-run rather than a test.

use std::time::Instant;

use proust::{parse, transform};

const EXPANSIONS: u32 = 50;
const RUNS: u32 = 20;

fn main() {
    let partial_source = "Some partial prose with a {% if $x %}branch{% /if %}.\n";
    let mut page_source = String::new();
    for _ in 0..EXPANSIONS {
        page_source.push_str("{% partial file=\"p0.md\" /%}\n\n");
    }
    let page = parse::parse(&page_source);

    for partial_count in [1usize, 10, 50, 200, 1000] {
        let sources: Vec<String> = (0..partial_count)
            .map(|_| partial_source.to_string())
            .collect();
        let mut config = proust::builtins::config();
        for (i, source) in sources.iter().enumerate() {
            config
                .partials_mut()
                .insert(format!("p{i}.md"), parse::parse(source));
        }
        config.variables = Some(proust::validate::Variables::new());

        let _ = transform::transform(&page, &config);

        let start = Instant::now();
        for _ in 0..RUNS {
            std::hint::black_box(transform::transform(&page, &config));
        }
        let elapsed = start.elapsed();
        println!(
            "partials={partial_count:>5}  total {elapsed:>12.3?}   per expansion {:>10.3?}",
            elapsed / (RUNS * EXPANSIONS),
        );
    }
}
