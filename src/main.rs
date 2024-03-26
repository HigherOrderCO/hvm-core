#![cfg_attr(feature = "trace", feature(const_type_name))]

use clap::{Args, Parser, Subcommand};
use hvmc::{
  ast::{Book, Net, Tree},
  host::Host,
  run::{DynNet, Mode, Trg},
  stdlib::create_host,
  *,
};

use std::{
  fs, io,
  path::Path,
  process::{self, Stdio},
  str::FromStr,
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

fn main() {
  if cfg!(feature = "trace") {
    trace::set_hook();
  }
  if cfg!(feature = "_full_cli") {
    let cli = FullCli::parse();
    match cli.mode {
      CliMode::Compile { file, transform_opts, output } => {
        let output = output.as_deref().or_else(|| file.strip_suffix(".hvmc")).unwrap_or_else(|| {
          eprintln!("file missing `.hvmc` extension; explicitly specify an output path with `--output`.");
          process::exit(1);
        });
        let host = create_host(&load_book(&[file.clone()], &transform_opts));
        compile_executable(output, host).unwrap();
      }
      CliMode::Run { run_opts, mut transform_opts, file, args } => {
        // Don't pre-reduce or prune the entry point
        transform_opts.pre_reduce_skip.push(args.entry_point.clone());
        transform_opts.prune_entrypoints.push(args.entry_point.clone());
        let host = create_host(&load_book(&[file], &transform_opts));
        run(host, run_opts, args);
      }
      CliMode::Reduce { run_opts, transform_opts, files, exprs } => {
        let host = create_host(&load_book(&files, &transform_opts));
        let exprs: Vec<_> = exprs.iter().map(|x| Net::from_str(x).unwrap()).collect();
        reduce_exprs(host, &exprs, &run_opts);
      }
      CliMode::Transform { transform_opts, files } => {
        let book = load_book(&files, &transform_opts);
        println!("{}", book);
      }
    }
  } else {
    let cli = BareCli::parse();
    let host = hvmc::gen::host();
    run(Arc::new(Mutex::new(host)), cli.opts, cli.args);
  }
  if cfg!(feature = "trace") {
    hvmc::trace::_read_traces(usize::MAX);
  }
}

#[derive(Parser, Debug)]
#[command(
  author,
  version,
  about = "A massively parallel Interaction Combinator evaluator",
  long_about = r##"
A massively parallel Interaction Combinator evaluator

Examples: 
$ hvmc run examples/church_encoding/church.hvm
$ hvmc run examples/addition.hvmc "#16" "#3"
$ hvmc compile examples/addition.hvmc
$ hvmc reduce examples/addition.hvmc -- "a & @mul ~ (#3 (#4 a))"
$ hvmc reduce -- "a & #3 ~ <* #4 a>""##
)]
struct FullCli {
  #[command(subcommand)]
  pub mode: CliMode,
}

#[derive(Parser, Debug)]
#[command(author, version)]
struct BareCli {
  #[command(flatten)]
  pub opts: RuntimeOpts,
  #[command(flatten)]
  pub args: RunArgs,
}

#[derive(Subcommand, Clone, Debug)]
#[command(author, version)]
enum CliMode {
  /// Compile a hvm-core program into a Rust crate.
  Compile {
    /// hvm-core file to compile.
    file: String,
    #[arg(short = 'o', long = "output")]
    /// Output path; defaults to the input file with `.hvmc` stripped.
    output: Option<String>,
    #[command(flatten)]
    transform_opts: TransformOpts,
  },
  /// Run a program, optionally passing a list of arguments to it.
  Run {
    /// Name of the file to load.
    file: String,
    #[command(flatten)]
    args: RunArgs,
    #[command(flatten)]
    run_opts: RuntimeOpts,
    #[command(flatten)]
    transform_opts: TransformOpts,
  },
  /// Reduce hvm-core expressions to their normal form.
  ///
  /// The expressions are passed as command-line arguments.
  /// It is also possible to load files before reducing the expression,
  /// which makes it possible to reference definitions from the file
  /// in the expression.
  Reduce {
    #[arg(required = false)]
    /// Files to load before reducing the expressions.
    ///
    /// Multiple files will act as if they're concatenated together.
    files: Vec<String>,
    #[arg(required = false, last = true)]
    /// Expressions to reduce.
    ///
    /// The normal form of each expression will be
    /// printed on a new line. This list must be separated from the file list
    /// with a double dash ('--').
    exprs: Vec<String>,
    #[command(flatten)]
    run_opts: RuntimeOpts,
    #[command(flatten)]
    transform_opts: TransformOpts,
  },
  /// Transform a hvm-core program using one of the optimization passes.
  Transform {
    /// Files to load before reducing the expressions.
    ///
    /// Multiple files will act as if they're concatenated together.
    #[arg(required = true)]
    files: Vec<String>,
    #[command(flatten)]
    transform_opts: TransformOpts,
  },
}

#[derive(Args, Clone, Debug)]
struct TransformOpts {
  #[arg(short = 'O', value_delimiter = ' ', action = clap::ArgAction::Append)]
  /// Enables or disables transformation passes.
  transform_passes: Vec<TransformPass>,

  #[arg(long = "pre-reduce-skip", value_delimiter = ' ', action = clap::ArgAction::Append)]
  /// Names of the definitions that should not get pre-reduced.
  ///
  /// For programs that don't take arguments
  /// and don't have side effects this is usually the entry point of the
  /// program (otherwise, the whole program will get reduced to normal form).
  pre_reduce_skip: Vec<String>,
  #[arg(long = "pre-reduce-memory", value_parser = parse_abbrev_number::<usize>)]
  /// How much memory to allocate when pre-reducing.
  ///
  /// Supports abbreviations such as '4G' or '400M'.
  pre_reduce_memory: Option<usize>,
  #[arg(long = "pre-reduce-rewrites", default_value = "100M", value_parser = parse_abbrev_number::<u64>)]
  /// Maximum amount of rewrites to do when pre-reducing.
  ///
  /// Supports abbreviations such as '4G' or '400M'.
  pre_reduce_rewrites: u64,
  /// Names of the definitions that should not get pruned.
  #[arg(long = "prune-entrypoints", default_value = "main")]
  prune_entrypoints: Vec<String>,
}

#[derive(Args, Clone, Debug)]
struct RuntimeOpts {
  #[arg(short = 's', long = "stats")]
  /// Show performance statistics.
  show_stats: bool,
  #[arg(short = '1', long = "single")]
  /// Single-core mode (no parallelism).
  single_core: bool,
  #[arg(short = 'l', long = "lazy")]
  /// Lazy mode.
  ///
  /// Lazy mode only expands references that are reachable
  /// by a walk from the root of the net. This leads to a dramatic slowdown,
  /// but allows running programs that would expand indefinitely otherwise.
  lazy_mode: bool,
  #[arg(short = 'm', long = "memory", value_parser = parse_abbrev_number::<usize>)]
  /// How much memory to allocate on startup.
  ///
  /// Supports abbreviations such as '4G' or '400M'.
  memory: Option<usize>,
}

#[derive(Args, Clone, Debug)]
struct RunArgs {
  #[arg(short = 'e', default_value = "main")]
  /// Name of the definition that will get reduced.
  entry_point: String,
  /// List of arguments to pass to the program.
  ///
  /// Arguments are passed using the lambda-calculus interpretation
  /// of interaction combinators. So, for example, if the arguments are
  /// "#1" "#2" "#3", then the expression that will get reduced is
  /// `r & @main ~ (#1 (#2 (#3 r)))`.
  args: Vec<String>,
}

macro_rules! transform_passes {
  ($($pass:ident: $name:literal $(| $alias:literal)*),* $(,)?) => {
    #[derive(Clone, Debug)]
    #[allow(non_camel_case_types)]
    pub enum TransformPass {
      all(bool),
      $($pass(bool),)*
    }

    #[derive(Default)]
    struct TransformPasses {
      $($pass: bool),*
    }

    impl TransformPasses {
      fn set_all(&mut self) {
        $(self.$pass = true;)*
      }
    }

    impl clap::ValueEnum for TransformPass {
      fn value_variants<'a>() -> &'a [Self] {
        &[
          Self::all(true), Self::all(false),
          $(Self::$pass(true), Self::$pass(false),)*
        ]
      }

      fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        use TransformPass::*;
        Some(match self {
          all(true) => clap::builder::PossibleValue::new("all"),
          all(false) => clap::builder::PossibleValue::new("no-all"),
          $(
            $pass(true) => clap::builder::PossibleValue::new($name)$(.alias($alias))*,
            $pass(false) => clap::builder::PossibleValue::new(concat!("no-", $name))$(.alias(concat!("no-", $alias)))*,
          )*
        })
      }
    }

    impl TransformPass {
      fn passes_from_cli(args: &Vec<Self>) -> TransformPasses {
        use TransformPass::*;
        let mut opts = TransformPasses::default();
        for arg in args {
          match arg {
            all(true) => opts.set_all(),
            all(false) => opts = TransformPasses::default(),
            $(&$pass(b) => opts.$pass = b,)*
          }
        }
        opts
      }
    }
  };
}

transform_passes! {
  pre_reduce: "pre-reduce" | "pre",
  coalesce_ctrs: "coalesce-ctrs" | "coalesce",
  encode_adts: "encode-adts" | "adts",
  eta_reduce: "eta-reduce" | "eta",
  inline: "inline",
  prune: "prune",
}

fn run(host: Arc<Mutex<Host>>, opts: RuntimeOpts, args: RunArgs) {
  let mut net = Net { root: Tree::Ref { nam: args.entry_point }, redexes: vec![] };
  for arg in args.args {
    let arg: Net = Net::from_str(&arg).unwrap();
    net.redexes.extend(arg.redexes);
    net.apply_tree(arg.root);
  }

  reduce_exprs(host, &[net], &opts);
}
/// Turn a string representation of a number, such as '1G' or '400K', into a
/// number.
///
/// This return a [`u64`] instead of [`usize`] to ensure that parsing CLI args
/// doesn't fail on 32-bit systems. We want it to fail later on, when attempting
/// to run the program.
fn parse_abbrev_number<T: TryFrom<u64>>(arg: &str) -> Result<T, String>
where
  <T as TryFrom<u64>>::Error: core::fmt::Debug,
{
  let (base, scale) = match arg.to_lowercase().chars().last() {
    None => return Err("Mem size argument is empty".to_string()),
    Some('k') => (&arg[0 .. arg.len() - 1], 1u64 << 10),
    Some('m') => (&arg[0 .. arg.len() - 1], 1u64 << 20),
    Some('g') => (&arg[0 .. arg.len() - 1], 1u64 << 30),
    Some('t') => (&arg[0 .. arg.len() - 1], 1u64 << 40),
    Some(_) => (arg, 1),
  };
  let base = base.parse::<u64>().map_err(|e| e.to_string())?;
  Ok((base * scale).try_into().map_err(|e| format!("{:?}", e))?)
}

fn load_book(files: &[String], transform_opts: &TransformOpts) -> Book {
  let mut book = files
    .iter()
    .map(|name| {
      let contents = fs::read_to_string(name).unwrap_or_else(|_| {
        eprintln!("Input file {:?} not found", name);
        process::exit(1);
      });
      contents.parse::<Book>().unwrap_or_else(|e| {
        eprintln!("Parsing error {e}");
        process::exit(1);
      })
    })
    .fold(Book::default(), |mut acc, i| {
      acc.nets.extend(i.nets);
      acc
    });
  let transform_passes = TransformPass::passes_from_cli(&transform_opts.transform_passes);

  if transform_passes.pre_reduce {
    if transform_passes.eta_reduce {
      for (_, def) in &mut book.nets {
        def.eta_reduce();
      }
    }
    book.pre_reduce(
      &|x| transform_opts.pre_reduce_skip.iter().any(|y| x == y),
      transform_opts.pre_reduce_memory,
      transform_opts.pre_reduce_rewrites,
    );
  }
  for (_, def) in &mut book.nets {
    if transform_passes.eta_reduce {
      def.eta_reduce();
    }
    for tree in def.trees_mut() {
      if transform_passes.coalesce_ctrs {
        tree.coalesce_constructors();
      }
      if transform_passes.encode_adts {
        tree.encode_scott_adts();
      }
    }
  }
  if transform_passes.inline {
    loop {
      let inline_changed = book.inline();
      if inline_changed.is_empty() {
        break;
      }
      if !(transform_passes.eta_reduce || transform_passes.encode_adts) {
        break;
      }
      for name in inline_changed {
        let def = book.get_mut(&name).unwrap();
        if transform_passes.eta_reduce {
          def.eta_reduce();
        }
        if transform_passes.encode_adts {
          for tree in def.trees_mut() {
            tree.encode_scott_adts();
          }
        }
      }
    }
  }
  if transform_passes.prune {
    book.prune(&transform_opts.prune_entrypoints);
  }
  book
}

fn reduce_exprs(host: Arc<Mutex<Host>>, exprs: &[Net], opts: &RuntimeOpts) {
  let heap = run::Heap::new(opts.memory).expect("memory allocation failed");
  for expr in exprs {
    let mut net = DynNet::new(&heap, opts.lazy_mode);
    dispatch_dyn_net!(&mut net => {
      host.lock().unwrap().encode_net(net, Trg::port(net.root.as_var()), expr);
      let start_time = Instant::now();
      if opts.single_core {
        net.normal();
      } else {
        net.parallel_normal();
      }
      let elapsed = start_time.elapsed();
      println!("{}", host.lock().unwrap().readback(net));
      if opts.show_stats {
        print_stats(net, elapsed);
      }
    });
  }
}

fn print_stats<M: Mode>(net: &run::Net<M>, elapsed: Duration) {
  eprintln!("RWTS   : {:>15}", pretty_num(net.rwts.total()));
  eprintln!("- ANNI : {:>15}", pretty_num(net.rwts.anni));
  eprintln!("- COMM : {:>15}", pretty_num(net.rwts.comm));
  eprintln!("- ERAS : {:>15}", pretty_num(net.rwts.eras));
  eprintln!("- DREF : {:>15}", pretty_num(net.rwts.dref));
  eprintln!("- OPER : {:>15}", pretty_num(net.rwts.oper));
  eprintln!("TIME   : {:.3?}", elapsed);
  eprintln!("RPS    : {:.3} M", (net.rwts.total() as f64) / (elapsed.as_millis() as f64) / 1000.0);
}

fn pretty_num(n: u64) -> String {
  n.to_string()
    .as_bytes()
    .rchunks(3)
    .rev()
    .map(|x| std::str::from_utf8(x).unwrap())
    .flat_map(|x| ["_", x])
    .skip(1)
    .collect()
}

fn compile_executable(target: &str, host: Arc<Mutex<host::Host>>) -> Result<(), io::Error> {
  let gen = compile::compile_host(&host.lock().unwrap());
  let outdir = ".hvm";
  if Path::new(&outdir).exists() {
    fs::remove_dir_all(outdir)?;
  }
  let cargo_toml = include_str!("../Cargo.toml");
  let cargo_toml = cargo_toml.split("##--COMPILER-CUTOFF--##").next().unwrap();

  macro_rules! include_files {
    ($([$($prefix:ident)*])? $mod:ident {$($sub:tt)*} $($rest:tt)*) => {
      fs::create_dir_all(concat!(".hvm/src/", $($(stringify!($prefix), "/",)*)? stringify!($mod)))?;
      include_files!([$($($prefix)* $mod)?] $($sub)*);
      include_files!([$($($prefix)*)?] $mod $($rest)*);
    };
    ($([$($prefix:ident)*])? $file:ident $($rest:tt)*) => {
      fs::write(
        concat!(".hvm/src/", $($(stringify!($prefix), "/",)*)* stringify!($file), ".rs"),
        include_str!(concat!($($(stringify!($prefix), "/",)*)* stringify!($file), ".rs")),
      )?;
      include_files!([$($($prefix)*)?] $($rest)*);
    };
    ($([$($prefix:ident)*])?) => {};
  }

  fs::create_dir_all(".hvm/src")?;
  fs::write(".hvm/Cargo.toml", cargo_toml)?;
  fs::write(".hvm/src/gen.rs", gen)?;

  include_files! {
    ast
    compile
    fuzz
    host {
      calc_labels
      encode
      readback
    }
    lib
    main
    ops
    run {
      addr
      allocator
      def
      dyn_net
      instruction
      interact
      linker
      net
      node
      parallel
      port
      wire
    }
    stdlib
    trace
    transform {
      coalesce_ctrs
      encode_adts
      eta_reduce
      inline
      pre_reduce
      prune
    }
    util {
      apply_tree
      array_vec
      bi_enum
      create_var
      deref
      maybe_grow
      stats
    }
  }

  let output = process::Command::new("cargo")
    .current_dir(".hvm")
    .arg("build")
    .arg("--release")
    .stderr(Stdio::inherit())
    .output()?;
  if !output.status.success() {
    process::exit(1);
  }

  fs::copy(".hvm/target/release/hvmc", target)?;

  Ok(())
}
