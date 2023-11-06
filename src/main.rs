#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use hvmc::ast;
#[cfg(feature = "cuda")]
use hvmc::cuda::host::{gen_cuda_book_data, run_on_gpu};
use hvmc::run;
use std::env;
use std::fs;

// TODO: Proper error handling in `main` function
fn main() -> Result<(), Box<dyn std::error::Error>> {
  let args: Vec<String> = env::args().collect();

  if args.len() < 3 {
    println!("Usage: hvmc <cmd> <file.hvmc> [-s]");
    std::process::exit(1);
  }

  let action = &args[1];
  let f_name = &args[2];

  match action.as_str() {
    "run" => {
      let (book, mut net) = load(f_name);
      let start_time = std::time::Instant::now();
      net.expand(&book, run::ROOT);
      net.normal(&book);
      println!("{}", ast::show_runtime_net(&net));
      if args.len() >= 4 && args[3] == "-s" {
        println!("");
        println!("RWTS   : {}", net.anni + net.comm + net.eras + net.dref + net.oper);
        println!("- ANNI : {}", net.anni);
        println!("- COMM : {}", net.comm);
        println!("- ERAS : {}", net.eras);
        println!("- DREF : {}", net.dref);
        println!("- OPER : {}", net.oper);
        println!("TIME   : {:.3} s", (start_time.elapsed().as_millis() as f64) / 1000.0);
        println!("RPS    : {:.3} m", (net.rewrites() as f64) / (start_time.elapsed().as_millis() as f64) / 1000.0);
      }
    }
    #[cfg(feature = "cuda")]
    "run-gpu" => {
      let book = load(f_name).0;
      run_on_gpu(&book, "main")?;
    }
    "gen-cuda-book" => {
      let book = load(f_name).0;
      println!("{}", gen_cuda_book(&book));
    }
    _ => {
      println!("Invalid command. Usage: hvmc <cmd> <file.hvmc>");
    }
  }
  Ok(())
}

// Load file and generate net
fn load(file: &str) -> (run::Book, run::Net) {
  let file = fs::read_to_string(file).unwrap();
  let book = &ast::do_parse_book(&file);
  let book = ast::book_to_runtime(book);
  let mut net = run::Net::new(1 << 28);
  net.boot(ast::name_to_val("main"));
  return (book, net);
}

// Compile to a CUDA book (TODO: move to another repo)
pub fn gen_cuda_book(book: &run::Book) -> String {
  use std::collections::BTreeMap;

  // Sort the book.defs by key
  let mut defs = BTreeMap::new();
  for i in 0 .. book.defs.len() {
    if book.defs[i].port.len() > 0 {
      defs.insert(i as u32, book.defs[i].clone());
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
    let node_len = net.port.len();
    let rdex_len = net.rdex.len();

    code.push_str(&format!("  // @{}\n", crate::ast::val_to_name(*id)));

    // Collect all pointers from root, nodes and rdex into a single buffer
    code.push_str(&format!("  // .nlen\n"));
    code.push_str(&format!("  0x{:08X},\n", node_len));
    code.push_str(&format!("  // .rlen\n"));
    code.push_str(&format!("  0x{:08X},\n", rdex_len));

    // .node
    code.push_str("  // .node\n");
    for (i, node) in net.port.iter().enumerate() {
      code.push_str(&format!("  0x{:08X},", node.data()));
      code.push_str(&format!(" 0x{:08X},", node.data()));
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
    index += 2 + 2 * defs[id].port.len() as u32 + 2 * defs[id].rdex.len() as u32;
  }

  code.push_str("};");

  return code;
}
