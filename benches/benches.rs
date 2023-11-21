use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hvmc::{ast::*, *};
use std::{
  ffi::OsStr,
  fs,
  path::{Path, PathBuf},
  time::Duration,
};

// Loads file and generate net from hvm-core syntax
fn load_from_core<P: AsRef<Path>>(file: P) -> (run::Book, run::Net) {
  let code = fs::read_to_string(file).unwrap();
  let (size, code) = extract_size(&code);

  let book = ast::do_parse_book(code);
  let rbook = ast::book_to_runtime(&book, run::call_native());

  let mut net = run::Net::new(size);
  net.boot(name_to_val("main"));
  (rbook, net)
}

// Loads file and generate net from hvm-lang syntax
fn load_from_lang<P: AsRef<Path>>(file: P) -> (run::Book, run::Net) {
  let code = fs::read_to_string(file).unwrap();
  let (size, code) = extract_size(&code);

  let mut book = hvm_lang::term::parser::parse_definition_book(&code).unwrap();
  let (book, _) = hvm_lang::compile_book(&mut book).unwrap();
  let book = ast::book_to_runtime(&book, run::call_native());

  let mut net = run::Net::new(size);
  net.boot(name_to_val("main"));
  (book, net)
}

fn extract_size(code: &str) -> (usize, &str) {
  code
    .strip_prefix("// size = ")
    .and_then(|code| code.split_once('\n'))
    .and_then(|(size, rest)| {
      match size.split_ascii_whitespace().collect::<Vec<_>>().as_slice() {
        [a, "<<", b] => a.parse::<usize>().ok().zip(b.parse::<usize>().ok()).map(|(a, b)| a << b),
        [a] => a.parse().ok(),
        _ => None,
      }
      .map(|size| (size, rest))
    })
    .expect("failed to extract bench size")
}

fn run_programs_dir(c: &mut Criterion) {
  let root = PathBuf::from(format!("{}/benches/programs", env!("CARGO_MANIFEST_DIR")));
  run_dir(&root, None, c);
}

fn run_dir(path: &PathBuf, group: Option<String>, c: &mut Criterion) {
  let dir_entries = std::fs::read_dir(path).unwrap().flatten();

  for entry in dir_entries {
    let entry = &entry.path();

    if entry.is_dir() {
      let dir_name = entry.file_stem().unwrap().to_string_lossy();

      let group = match group {
        Some(ref group) => format!("{group}/{dir_name}"),
        None => dir_name.to_string(),
      };

      run_dir(entry, Some(group), c)
    } else {
      run_file(entry, group.clone(), c);
    }
  }
}

fn run_file(path: &PathBuf, mut group: Option<String>, c: &mut Criterion) {
  let (book, net) = match path.extension().and_then(OsStr::to_str) {
    Some("hvmc") => load_from_core(path),
    Some("hvm") => load_from_lang(path),
    _ => panic!("invalid file found: {}", path.to_string_lossy()),
  };

  let file_name = path.file_stem().unwrap().to_string_lossy();

  if cfg!(feature = "cuda") {
    group = Some(match group {
      Some(group) => format!("cuda/{group}"),
      None => "cuda".to_string(),
    });
  };

  match group {
    Some(group) => benchmark_group(&file_name, group, book, net, c),
    None => benchmark(&file_name, book, net, c),
  }
}

fn benchmark(file_name: &str, book: run::Book, net: run::Net, c: &mut Criterion) {
  c.bench_function(file_name, |b| {
    b.iter_batched(
      || net.clone(),
      |net| black_box(black_box(net).normal(black_box(&book))),
      criterion::BatchSize::SmallInput,
    );
  });
}

#[allow(unused_variables)]
fn benchmark_group(file_name: &str, group: String, book: run::Book, net: run::Net, c: &mut Criterion) {
  #[cfg(not(feature = "cuda"))]
  c.benchmark_group(group).bench_function(file_name, |b| {
    b.iter_batched(
      || net.clone(),
      |net| black_box(black_box(net).normal(black_box(&book))),
      criterion::BatchSize::SmallInput,
    );
  });

  #[cfg(feature = "cuda")]
  c.benchmark_group(group).bench_function(file_name, |b| {
    b.iter_batched(
      || cuda::host::setup_gpu(&book, "main").unwrap(),
      |(dev, global_expand_prepare, global_expand, global_rewrite, gpu_net, gpu_book)| {
        black_box(
          cuda::host::cuda_normalize_net(
            black_box(global_expand_prepare),
            black_box(global_expand),
            black_box(global_rewrite),
            black_box(&gpu_net.device_net),
            black_box(&gpu_book),
          )
          .unwrap(),
        );

        black_box(dev.synchronize().unwrap());
      },
      criterion::BatchSize::PerIteration,
    )
  });
}

fn interact_benchmark(c: &mut Criterion) {
  if cfg!(feature = "cuda") {
    return;
  }

  use ast::Tree::*;
  let mut group = c.benchmark_group("interact");
  group.sample_size(1000);

  let cases = [
    ("era-era", (Era, Era)),
    ("era-con", (Era, Ctr { lab: 0, lft: Era.into(), rgt: Era.into() })),
    ("con-con", ((Ctr { lab: 0, lft: Era.into(), rgt: Era.into() }), Ctr { lab: 0, lft: Era.into(), rgt: Era.into() })),
    ("con-dup", ((Ctr { lab: 0, lft: Era.into(), rgt: Era.into() }), Ctr { lab: 2, lft: Era.into(), rgt: Era.into() })),
  ];

  for (name, redex) in cases {
    let mut net = run::Net::new(10);
    let book = run::Book::new();
    ast::net_to_runtime(&mut net, &ast::Net { root: Era, rdex: vec![redex] });
    let (rdx_a, rdx_b) = net.rdex[0];
    group.bench_function(name, |b| {
      b.iter_batched(
        || net.clone(),
        |net| black_box(black_box(net).interact(black_box(&book), black_box(rdx_a), black_box(rdx_b))),
        criterion::BatchSize::SmallInput,
      );
    });
  }
}

criterion_group! {
  name = benches;
  config = Criterion::default()
    .measurement_time(Duration::from_millis(1000))
    .warm_up_time(Duration::from_millis(500));
  targets =
    run_programs_dir,
    interact_benchmark,
}
criterion_main!(benches);
