#[macro_use]
extern crate clap;
extern crate grep_cli;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tabwriter;
extern crate termcolor;
extern crate unicode_width;
extern crate walkdir;

use std::collections::BTreeMap;
use std::error::Error;
use std::io::{self, Write};
use std::process;
use std::result;

use regex::Regex;

use app::Args;
use data::{Benchmarks, Benchmark};

macro_rules! err {
    ($($tt:tt)*) => { Box::<::std::error::Error>::from(format!($($tt)*)); }
}

macro_rules! fail {
    ($($tt:tt)*) => { return Err(err!($($tt)*)); }
}

mod app;
mod data;
mod output;

type Result<T> = result::Result<T, Box<Error>>;

fn main() {
    if let Err(err) = try_main() {
        eprintln!("{}", err);
        process::exit(1);
    }
}

fn try_main() -> Result<()> {
    let args = Args::parse();
    let benchmarks = args.benchmarks()?;

    if args.baselines() {
        let mut stdout = io::stdout();
        for baseline in benchmarks.by_baseline.keys() {
            writeln!(stdout, "{}", baseline)?;
        }
        return Ok(());
    }
    if let Some(baseline) = args.export() {
        let mut stdout = io::stdout();
        let basedata = match benchmarks.by_baseline.get(&baseline) {
            Some(basedata) => basedata,
            None => fail!("failed to find baseline '{}'", baseline),
        };
        serde_json::to_writer_pretty(&mut stdout, basedata)?;
        writeln!(stdout, "")?;
        return Ok(());
    }

    let filter = args.filter()?;
    let mut comps = match args.group()? {
        None => group_by_baseline(&benchmarks, filter.as_ref()),
        Some(re) => group_by_regex(&benchmarks, &re, filter.as_ref()),
    };
    if let Some(threshold) = args.threshold()? {
        comps.retain(|comp| comp.biggest_difference() > threshold);
    }
    if comps.is_empty() {
        fail!("no benchmark comparisons to show");
    }

    let mut wtr = args.stdout();
    if args.list() {
        output::rows(&mut wtr, &comps)?;
    } else {
        output::columns(&mut wtr, &comps)?;
    }
    wtr.flush()?;
    Ok(())
}

fn group_by_baseline(
    benchmarks: &Benchmarks,
    filter: Option<&Regex>,
) -> Vec<output::Comparison> {
    let mut byname: BTreeMap<String, Vec<output::Benchmark>> = BTreeMap::new();
    for base_benchmarks in benchmarks.by_baseline.values() {
        for (name, benchmark) in base_benchmarks.benchmarks.iter() {
            if filter.map_or(false, |re| !re.is_match(name)) {
                continue;
            }
            let output_benchmark = output::Benchmark::from_data(benchmark)
                .name(benchmark.baseline());
            byname
                .entry(name.to_string())
                .or_insert(vec![])
                .push(output_benchmark);
        }
    }
    byname
        .into_iter()
        .map(|(name, benchmarks)| output::Comparison::new(&name, benchmarks))
        .collect()
}

fn group_by_regex(
    benchmarks: &Benchmarks,
    group_by: &Regex,
    filter: Option<&Regex>,
) -> Vec<output::Comparison> {
    let mut byname: BTreeMap<String, Vec<output::Benchmark>> = BTreeMap::new();
    for base_benchmarks in benchmarks.by_baseline.values() {
        for (name, benchmark) in base_benchmarks.benchmarks.iter() {
            if filter.map_or(false, |re| !re.is_match(name)) {
                continue;
            }
            let (bench, cmp) = match benchmark_names(&benchmark, group_by) {
                None => continue,
                Some((bench, cmp)) => (bench, cmp),
            };
            let output_benchmark = output::Benchmark::from_data(benchmark)
                .name(&bench);
            byname
                .entry(cmp)
                .or_insert(vec![])
                .push(output_benchmark);
        }
    }
    byname
        .into_iter()
        .map(|(name, benchmarks)| output::Comparison::new(&name, benchmarks))
        .collect()
}

fn benchmark_names(
    benchmark: &Benchmark,
    group_by: &Regex,
) -> Option<(String, String)> {
    assert!(group_by.captures_len() > 1);

    let caps = match group_by.captures(benchmark.benchmark_name()) {
        None => return None,
        Some(caps) => caps,
    };

    let mut bench_name = benchmark.benchmark_name().to_string();
    let mut cmp_name = String::new();
    let mut offset = 0;
    for option in caps.iter().skip(1) {
        let m = match option {
            None => continue,
            Some(m) => m,
        };
        cmp_name.push_str(m.as_str());
        // Strip everything that doesn't match capturing groups. The leftovers
        // are our benchmark name.
        bench_name.drain((m.start() - offset)..(m.end() - offset));
        offset += m.end() - m.start();
    }
    // Add the baseline name to the benchmark to disambiguate it from
    // benchmarks with the same name in other baselines.
    bench_name.insert_str(0, &format!("{}/", benchmark.baseline()));

    Some((bench_name, cmp_name))
}
