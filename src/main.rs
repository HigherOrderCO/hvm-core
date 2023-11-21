#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use std::env;
use std::fs;

use hvmc::ast;
use hvmc::run;

#[cfg(not(feature = "hvm_cli_options"))]
fn main() {
  let args: Vec<String> = env::args().collect();
  let book = run::Book::new();
  let mut net = run::Net::new(1 << 28);
  net.boot(ast::name_to_val("main"));
  let start_time = std::time::Instant::now();
  net.normal(&book);
  println!("{}", ast::show_runtime_net(&net));
  print_stats(&net, start_time);
}

#[cfg(feature = "hvm_cli_options")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
  let args: Vec<String> = env::args().collect();
  let help = "help".to_string();
  let action = args.get(1).unwrap_or(&help);
  let f_name = args.get(2);
  match action.as_str() {
    "run" => {
      if let Some(file_name) = f_name {
        let (book, mut net) = load(file_name);
        let start_time = std::time::Instant::now();
        net.normal(&book);
        println!("{}", ast::show_runtime_net(&net));
        if args.len() >= 4 && args[3] == "-s" {
          print_stats(&net, start_time);
        }
      } else {
        println!("Usage: hvmc run <file.hvmc> [-s]");
        std::process::exit(1);
      }
    }
    "compile" => {
      if let Some(file_name) = f_name {
        let (book, _) = load(file_name);
        compile_book_to_rust_crate(file_name, &book)?;
        compile_rust_crate_to_executable(file_name)?;
      } else {
        println!("Usage: hvmc compile <file.hvmc>");
        std::process::exit(1);
      }
    }
    "gen-cuda-book" => {
      if let Some(file_name) = f_name {
        let book = load(file_name).0;
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
    }
  }
  Ok(())
}

fn print_stats(net: &run::Net, start_time: std::time::Instant) {
  println!("RWTS   : {}", net.anni + net.comm + net.eras + net.dref + net.oper);
  println!("- ANNI : {}", net.anni);
  println!("- COMM : {}", net.comm);
  println!("- ERAS : {}", net.eras);
  println!("- DREF : {}", net.dref);
  println!("- OPER : {}", net.oper);
  println!("TIME   : {:.3} s", (start_time.elapsed().as_millis() as f64) / 1000.0);
  println!("RPS    : {:.3} m", (net.rewrites() as f64) / (start_time.elapsed().as_millis() as f64) / 1000.0);
}

// Load file and generate net
fn load(file: &str) -> (run::Book, run::Net) {
  let file = fs::read_to_string(file).unwrap();
  let book = ast::book_to_runtime(&ast::do_parse_book(&file), run::call_native());
  let mut net = run::Net::new(1 << 28);
  net.boot(ast::name_to_val("main"));
  return (book, net);
}

pub fn compile_book_to_rust_crate(f_name: &str, book: &run::Book) -> Result<(), std::io::Error> {
  let fns_rs = hvmc::jit::compile_book(book);
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
  fs::write(".hvm/src/lib.rs", include_str!("../src/lib.rs"))?;
  fs::write(".hvm/src/main.rs", include_str!("../src/main.rs"))?;
  fs::write(".hvm/src/run.rs", include_str!("../src/run.rs"))?;
  fs::write(".hvm/src/jit.rs", include_str!("../src/jit.rs"))?;
  fs::write(".hvm/src/exe.rs", include_str!("../src/exe.rs"))?;
  fs::write(".hvm/src/fns.rs", fns_rs.compile_to_rust_fns())?;
  return Ok(());
}

pub fn compile_rust_crate_to_executable(f_name: &str) -> Result<(), std::io::Error> {
  let output = std::process::Command::new("cargo").current_dir("./.hvm").arg("build").arg("--release").output()?;
  let target = format!("./{}", f_name.replace(".hvmc", ""));
  if std::path::Path::new(&target).exists() {
    fs::remove_file(&target)?;
  }
  fs::copy("./.hvm/target/release/hvmc", target)?;
  return Ok(());
}

// TODO: move to hvm-cuda repo
pub fn gen_cuda_book(book: &run::Book) -> String {
  use std::collections::BTreeMap;

  // Sort the book.defs by key
  let mut defs = BTreeMap::new();
  for i in 0 .. book.defs.len() {
    if book.defs[i].node.len() > 0 {
      defs.insert(i as run::Val, book.defs[i].clone());
    }
  }

  // Initializes code
  let mut code = String::new();

  // Generate function ids
  for (i, id) in defs.keys().enumerate() {
    code.push_str(&format!("const u32 F_{} = 0x{:x};\n", crate::ast::val_to_name(*id), id));
  }
  code.push_str("\n");

  // Create book
  code.push_str("u32 BOOK_DATA[] = {\n");

  // Generate book data
  for (i, (id, net)) in defs.iter().enumerate() {
    let node_len = net.node.len();
    let rdex_len = net.rdex.len();

    code.push_str(&format!("  // @{}\n", crate::ast::val_to_name(*id)));

    // Collect all pointers from root, nodes and rdex into a single buffer
    code.push_str(&format!("  // .nlen\n"));
    code.push_str(&format!("  0x{:08X},\n", node_len));
    code.push_str(&format!("  // .rlen\n"));
    code.push_str(&format!("  0x{:08X},\n", rdex_len));

    // .node
    code.push_str("  // .node\n");
    for (i, node) in net.node.iter().enumerate() {
      code.push_str(&format!("  0x{:08X},", node.0.data()));
      code.push_str(&format!(" 0x{:08X},", node.1.data()));
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
      code.push_str(&format!("  0x{:08X},", a.data()));
      code.push_str(&format!(" 0x{:08X},", b.data()));
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
  for (i, id) in defs.keys().enumerate() {
    code.push_str(&format!("  0x{:08X}, 0x{:08X}, // @{}\n", id, index, crate::ast::val_to_name(*id)));
    index += 2 + 2 * defs[id].node.len() as u32 + 2 * defs[id].rdex.len() as u32;
  }

  code.push_str("};");

  return code;
}
