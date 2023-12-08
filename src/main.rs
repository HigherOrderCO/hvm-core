#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use std::collections::HashSet;
use std::env;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::AtomicIsize;

use hvmc::ast;
use hvmc::fns;
use hvmc::jit;
use hvmc::run;
use hvmc::u60;

#[cfg(not(feature = "hvm_cli_options"))]
fn main() {
  let args: Vec<String> = env::args().collect();
  let opts = args.iter().skip(3).map(|s| s.as_str()).collect::<HashSet<_>>();
  let book = run::Book::new();
  let data = run::Heap::init(1 << 28);
  let mut net = run::Net::new(&data);
  net.boot(ast::name_to_val("main"));
  let start_time = std::time::Instant::now();
  if opts.contains("-1") {
    net.normal(&book);
  } else {
    net.parallel_normal(&book);
  }
  println!("{}", ast::show_runtime_net(&net));
  print_stats(&net, start_time);
}

#[cfg(feature = "hvm_cli_options")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
  let data = run::Heap::init(1 << 29);
  let args: Vec<String> = env::args().collect();
  let help = "help".to_string();
  let opts = args.iter().skip(3).map(|s| s.as_str()).collect::<HashSet<_>>();
  let action = args.get(1).unwrap_or(&help);
  let f_name = args.get(2);
  match action.as_str() {
    "run" => {
      if let Some(file_name) = f_name {
        let (book, mut net) = load(&data, file_name);
        let elapsed = if opts.contains("-1") {
          net.normal(&book)
        } else {
          net.parallel_normal(&book)
        };
        println!("{}", ast::show_runtime_net(&net));
        if opts.contains("-s") {
          print_stats(&net, elapsed);
        }
      } else {
        println!("Usage: hvmc run <file.hvmc> [-s]");
        std::process::exit(1);
      }
    }
    "compile" => {
      if let Some(file_name) = f_name {
        let (book, _) = load(&data, file_name);
        compile_book_to_rust_crate(file_name, &book)?;
        compile_rust_crate_to_executable(file_name)?;
      } else {
        println!("Usage: hvmc compile <file.hvmc>");
        std::process::exit(1);
      }
    }
    "gen-cuda-book" => {
      if let Some(file_name) = f_name {
        let book = load(&data, file_name).0;
        println!("{}", gen_cuda_book(&book));
      } else {
        println!("Usage: hvmc gen-cuda-book <file.hvmc>");
        std::process::exit(1);
      }
    }
    _ => {
      println!("Usage: hvmc <cmd> <file.hvmc> [-s]");
      println!("Commands:");
      println!("  run           - Run the given file");
      println!("  compile       - Compile the given file to an executable");
      println!("  gen-cuda-book - Generate a CUDA book from the given file");
      println!("Options:");
      println!("  [-s] Show stats, including rewrite count");
      println!("  [-1] Single-core mode (no parallelism)");
    }
  }
  Ok(())
}

fn print_stats(net: &run::Net, elapsed: std::time::Duration) {
  println!("RWTS   : {}", net.rewrites());
  println!("- ANNI : {}", net.rwts.anni);
  println!("- COMM : {}", net.rwts.comm);
  println!("- ERAS : {}", net.rwts.eras);
  println!("- DREF : {}", net.rwts.dref);
  println!("- OPER : {}", net.rwts.oper);
  println!("TIME   : {:.3} s", (elapsed.as_millis() as f64) / 1000.0);
  println!("RPS    : {:.3} m", (net.rewrites() as f64) / (elapsed.as_millis() as f64) / 1000.0);
}

// Load file and generate net
fn load<'a>(data: &'a run::Data, file: &str) -> (run::Book, run::Net<'a>) {
  let Ok(file) = fs::read_to_string(file) else {
      eprintln!("Input file not found");
      std::process::exit(1);
  };
  let book = ast::book_to_runtime(&ast::do_parse_book(&file));
  let mut net = run::Net::new(&data);
  net.boot(ast::name_to_val("main"));
  return (book, net);
}

pub fn compile_book_to_rust_crate(f_name: &str, book: &run::Book) -> Result<(), std::io::Error> {
  let fns_rs = jit::compile_book(book);
  let outdir = ".hvm";
  if std::path::Path::new(&outdir).exists() {
    fs::remove_dir_all(&outdir)?;
  }
  let cargo_toml = include_str!("../Cargo.toml");
  let cargo_toml = cargo_toml.split("##--COMPILER-CUTOFF--##").next().unwrap();
  let cargo_toml = cargo_toml.replace("\"hvm_cli_options\"", "");
  fs::create_dir_all(&format!("{}/src", outdir))?;
  fs::write(".hvm/Cargo.toml", cargo_toml)?;
  fs::write(".hvm/src/ast.rs", include_str!("../src/ast.rs"))?;
  fs::write(".hvm/src/jit.rs", include_str!("../src/jit.rs"))?;
  fs::write(".hvm/src/lib.rs", include_str!("../src/lib.rs"))?;
  fs::write(".hvm/src/main.rs", include_str!("../src/main.rs"))?;
  fs::write(".hvm/src/run.rs", include_str!("../src/run.rs"))?;
  fs::write(".hvm/src/u60.rs", include_str!("../src/u60.rs"))?;
  fs::write(".hvm/src/fns.rs", fns_rs)?;
  return Ok(());
}

pub fn compile_rust_crate_to_executable(f_name: &str) -> Result<(), std::io::Error> {
  let output = std::process::Command::new("cargo").current_dir("./.hvm").arg("build").arg("--release").output()?;
  let target = format!("./{}", f_name.replace(".hvmc", ""));
  if std::path::Path::new(&target).exists() {
    fs::remove_file(&target)?;
  }
  // TODO: Read executable path from cargo json output
  #[cfg(not(target_os = "windows"))]
  fs::copy("./.hvm/target/release/hvmc", target)?;
  #[cfg(target_os = "windows")]
  fs::copy("./.hvm/target/release/hvmc.exe", target + ".exe")?;
  return Ok(());
}

// TODO: move to hvm-cuda repo
pub fn gen_cuda_book(book: &run::Book) -> String {
  use std::collections::BTreeMap;

  // Sort the book.defs by key
  let mut defs = BTreeMap::new();
  for (fid, def) in book.defs.iter() {
    if def.node.len() > 0 {
      defs.insert(fid, def.clone());
    }
  }

  // Initializes code
  let mut code = String::new();

  // Generate function ids
  for (i, id) in defs.keys().enumerate() {
    code.push_str(&format!("const u32 F_{} = 0x{:x};\n", crate::ast::val_to_name(**id), id));
  }
  code.push_str("\n");

  // Create book
  code.push_str("u32 BOOK_DATA[] = {\n");

  // Generate book data
  for (i, (id, net)) in defs.iter().enumerate() {
    let node_len = net.node.len();
    let rdex_len = net.rdex.len();

    code.push_str(&format!("  // @{}\n", crate::ast::val_to_name(**id)));

    // Collect all pointers from root, nodes and rdex into a single buffer
    code.push_str(&format!("  // .nlen\n"));
    code.push_str(&format!("  0x{:08X},\n", node_len));
    code.push_str(&format!("  // .rlen\n"));
    code.push_str(&format!("  0x{:08X},\n", rdex_len));

    // .node
    code.push_str("  // .node\n");
    for (i, node) in net.node.iter().enumerate() {
      code.push_str(&format!("  0x{:08X},", node.0.0));
      code.push_str(&format!(" 0x{:08X},", node.1.0));
      if (i + 1) % 4 == 0 {
        code.push_str("\n");
      }
    }
    if node_len % 4 != 0 {
      code.push_str("\n");
    }

    // .rdex
    code.push_str("  // .rdex\n");
    for (i, (a, b)) in net.rdex.iter().enumerate() {
      code.push_str(&format!("  0x{:08X},", a.0));
      code.push_str(&format!(" 0x{:08X},", b.0));
      if (i + 1) % 4 == 0 {
        code.push_str("\n");
      }
    }
    if rdex_len % 4 != 0 {
      code.push_str("\n");
    }
  }

  code.push_str("};\n\n");

  code.push_str("u32 JUMP_DATA[] = {\n");

  let mut index = 0;
  for (i, fid) in defs.keys().enumerate() {
    code.push_str(&format!("  0x{:08X}, 0x{:08X}, // @{}\n", fid, index, crate::ast::val_to_name(**fid)));
    index += 2 + 2 * defs[fid].node.len() as u32 + 2 * defs[fid].rdex.len() as u32;
  }

  code.push_str("};");

  return code;
}
