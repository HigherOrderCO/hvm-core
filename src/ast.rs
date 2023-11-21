// An interaction combinator language
// ----------------------------------
// This file implements a textual syntax to interact with the runtime. It includes a pure AST for
// nets, as well as functions for parsing, stringifying, and converting pure ASTs to runtime nets.
// On the runtime, a net is represented by a list of active trees, plus a root tree. The textual
// syntax reflects this representation. The grammar is specified on this repo's README.

use crate::run;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::iter::Peekable;
use std::str::Chars;
use std::sync::Arc;

// AST
// ---

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub enum Tree {
  Era,
  Ctr { lab: u8, lft: Box<Tree>, rgt: Box<Tree> },
  Var { nam: String },
  Ref { nam: run::Val },
  Num { val: run::Val },
  Op2 { lft: Box<Tree>, rgt: Box<Tree> },
  Mat { sel: Box<Tree>, ret: Box<Tree> },
}

type Redex = Vec<(Tree, Tree)>;

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Net {
  pub root: Tree,
  pub rdex: Redex,
}

pub type Book = BTreeMap<String, Net>;

// Parser
// ------

fn skip(chars: &mut Peekable<Chars>) {
  while let Some(c) = chars.peek() {
    if *c == '/' {
      chars.next();
      while let Some(c) = chars.peek() {
        if *c == '\n' {
          break;
        }
        chars.next();
      }
    } else if !c.is_ascii_whitespace() {
      break;
    } else {
      chars.next();
    }
  }
}

pub fn consume(chars: &mut Peekable<Chars>, text: &str) -> Result<(), String> {
  skip(chars);
  for c in text.chars() {
    if chars.next() != Some(c) {
      return Err(format!("Expected '{}', found {:?}", text, chars.peek()));
    }
  }
  return Ok(());
}

pub fn parse_decimal(chars: &mut Peekable<Chars>) -> Result<u32, String> {
  let mut num: u32 = 0;
  skip(chars);
  if !chars.peek().map_or(false, |c| c.is_digit(10)) {
    return Err(format!("Expected a decimal number, found {:?}", chars.peek()));
  }
  while let Some(c) = chars.peek() {
    if !c.is_digit(10) {
      break;
    }
    num = num * 10 + c.to_digit(10).unwrap() as u32;
    chars.next();
  }
  Ok(num)
}

pub fn parse_name(chars: &mut Peekable<Chars>) -> Result<String, String> {
  let mut txt = String::new();
  skip(chars);
  if !chars.peek().map_or(false, |c| c.is_alphanumeric() || *c == '_' || *c == '.') {
    return Err(format!("Expected a name character, found {:?}", chars.peek()))
  }
  while let Some(c) = chars.peek() {
    if !c.is_alphanumeric() && *c != '_' && *c != '.' {
      break;
    }
    txt.push(*c);
    chars.next();
  }
  Ok(txt)
}

pub fn parse_opx_lit(chars: &mut Peekable<Chars>) -> Result<String, String> {
  let mut opx = String::new();
  skip(chars);
  while let Some(c) = chars.peek() {
    if !"+-=*/%<>|&^!?".contains(*c) {
      break;
    }
    opx.push(*c);
    chars.next();
  }
  Ok(opx)
}

pub fn parse_tree(chars: &mut Peekable<Chars>) -> Result<Tree, String> {
  skip(chars);
  match chars.peek() {
    Some('*') => {
      chars.next();
      Ok(Tree::Era)
    }
    Some('(') => {
      chars.next();
      let lab = 0;
      let lft = Box::new(parse_tree(chars)?);
      let rgt = Box::new(parse_tree(chars)?);
      consume(chars, ")")?;
      Ok(Tree::Ctr { lab, lft, rgt })
    }
    Some('[') => {
      chars.next();
      let lab = 1;
      let lft = Box::new(parse_tree(chars)?);
      let rgt = Box::new(parse_tree(chars)?);
      consume(chars, "]")?;
      Ok(Tree::Ctr { lab, lft, rgt })
    }
    Some('{') => {
      chars.next();
      let lab = parse_decimal(chars)? as u8;
      let lft = Box::new(parse_tree(chars)?);
      let rgt = Box::new(parse_tree(chars)?);
      consume(chars, "}")?;
      Ok(Tree::Ctr { lab, lft, rgt })
    }
    Some('@') => {
      chars.next();
      skip(chars);
      let name = parse_name(chars)?;
      Ok(Tree::Ref { nam: name_to_val(&name) })
    }
    Some('#') => {
      chars.next();
      Ok(Tree::Num { val: parse_decimal(chars)? as run::Val })
    }
    Some('<') => {
      chars.next();
      let lft = Box::new(parse_tree(chars)?);
      let rgt = Box::new(parse_tree(chars)?);
      consume(chars, ">")?;
      Ok(Tree::Op2 { lft, rgt })
    }
    Some('?') => {
      chars.next();
      let sel = Box::new(parse_tree(chars)?);
      let ret = Box::new(parse_tree(chars)?);
      Ok(Tree::Mat { sel, ret })
    }
    _ => {
      Ok(Tree::Var { nam: parse_name(chars)? })
    },
  }
}

pub fn parse_net(chars: &mut Peekable<Chars>) -> Result<Net, String> {
  let mut rdex = Vec::new();
  let root = parse_tree(chars)?;
  while let Some(c) = { skip(chars); chars.peek() } {
    if *c == '&' {
      chars.next();
      let tree1 = parse_tree(chars)?;
      consume(chars, "~")?;
      let tree2 = parse_tree(chars)?;
      rdex.push((tree1, tree2));
    } else {
      break;
    }
  }
  Ok(Net { root, rdex })
}

pub fn parse_book(chars: &mut Peekable<Chars>) -> Result<Book, String> {
  let mut book = BTreeMap::new();
  while let Some(c) = { skip(chars); chars.peek() } {
    if *c == '@' {
      chars.next();
      let name = parse_name(chars)?;
      consume(chars, "=")?;
      let net = parse_net(chars)?;
      book.insert(name, net);
    } else {
      break;
    }
  }
  Ok(book)
}

fn do_parse<T>(code: &str, parse_fn: impl Fn(&mut Peekable<Chars>) -> Result<T, String>) -> T {
  match parse_fn(&mut code.chars().peekable()) {
    Ok(result) => result,
    Err(err) => {
      eprintln!("{}", err);
      std::process::exit(1);
    }
  }
}

pub fn do_parse_tree(code: &str) -> Tree {
  do_parse(code, parse_tree)
}

pub fn do_parse_net(code: &str) -> Net {
  do_parse(code, parse_net)
}

pub fn do_parse_book(code: &str) -> Book {
  do_parse(code, parse_book)
}

// Stringifier
// -----------

pub fn show_tree(tree: &Tree) -> String {
  match tree {
    Tree::Era => {
      "*".to_string()
    }
    Tree::Ctr { lab, lft, rgt } => {
      match lab {
        0 => { format!("({} {})", show_tree(&*lft), show_tree(&*rgt)) }
        1 => { format!("[{} {}]", show_tree(&*lft), show_tree(&*rgt)) }
        _ => { format!("{{{} {} {}}}", lab, show_tree(&*lft), show_tree(&*rgt)) }
      } 
    }
    Tree::Var { nam } => {
      nam.clone()
    }
    Tree::Ref { nam } => {
      format!("@{}", val_to_name(*nam))
    }
    Tree::Num { val } => {
      format!("#{}", (*val as u32).to_string())
    }
    Tree::Op2 { lft, rgt } => {
      format!("<{} {}>", show_tree(&*lft), show_tree(&*rgt))
    }
    Tree::Mat { sel, ret } => {
      format!("? {} {}", show_tree(&*sel), show_tree(&*ret))
    }
  }
}

pub fn show_net(net: &Net) -> String {
  let mut result = String::new();
  result.push_str(&format!("{}", show_tree(&net.root)));
  for (a, b) in &net.rdex {
    result.push_str(&format!("\n& {} ~ {}", show_tree(a), show_tree(b)));
  }
  return result;
}

pub fn show_book(book: &Book) -> String {
  let mut result = String::new();
  for (name, net) in book {
    result.push_str(&format!("@{} = {}\n", name, show_net(net)));
  }
  return result;
}

pub fn show_runtime_tree(rt_net: &run::Net, ptr: run::Ptr) -> String {
  show_tree(&tree_from_runtime_go(rt_net, ptr, PARENT_ROOT, &mut HashMap::new(), &mut 0))
}

pub fn show_runtime_net(rt_net: &run::Net) -> String {
  show_net(&net_from_runtime(rt_net))
}

pub fn show_runtime_book(book: &run::Book) -> String {
  show_book(&book_from_runtime(book))
}

// Conversion
// ----------

pub fn num_to_str(mut num: usize) -> String {
  let mut txt = String::new();
  num += 1;
  while num > 0 {
    num -= 1;
    let c = ((num % 26) as u8 + b'a') as char;
    txt.push(c);
    num /= 26;
  }
  return txt.chars().rev().collect();
}

pub const fn tag_to_port(tag: run::Tag) -> run::Port {
  match tag {
    run::VR1 => run::P1,
    run::VR2 => run::P2,
    _        => unreachable!(),
  }
}

pub fn port_to_tag(port: run::Port) -> run::Tag {
  match port {
    run::P1 => run::VR1,
    run::P2 => run::VR2,
    _        => unreachable!(),
  }
}

pub fn name_to_letters(name: &str) -> Vec<u8> {
  let mut letters = Vec::new();
  for c in name.chars() {
    letters.push(match c {
      '0'..='9' => c as u8 - '0' as u8 + 0,
      'A'..='Z' => c as u8 - 'A' as u8 + 10,
      'a'..='z' => c as u8 - 'a' as u8 + 36,
      '_'       => 62,
      '.'       => 63,
      _         => panic!("Invalid character in name"),
    });
  }
  return letters;
}

pub fn letters_to_name(letters: Vec<u8>) -> String {
  let mut name = String::new();
  for letter in letters {
    name.push(match letter {
       0..= 9 => (letter - 0 + '0' as u8) as char,
      10..=35 => (letter - 10 + 'A' as u8) as char,
      36..=61 => (letter - 36 + 'a' as u8) as char,
      62      => '_',
      63      => '.',
      _       => panic!("Invalid letter in name"),
    });
  }
  return name;
}

pub fn val_to_letters(num: run::Val) -> Vec<u8> {
  let mut letters = Vec::new();
  let mut num = num;
  while num > 0 {
    letters.push((num % 64) as u8);
    num /= 64;
  }
  letters.reverse();
  return letters;
}

pub fn letters_to_val(letters: Vec<u8>) -> run::Val {
  let mut num = 0;
  for letter in letters {
    num = num * 64 + letter as run::Val;
  }
  return num;
}

pub fn name_to_val(name: &str) -> run::Val {
  letters_to_val(name_to_letters(name))
}

pub fn val_to_name(num: run::Val) -> String {
  letters_to_name(val_to_letters(num))
}

// Injection and Readback
// ----------------------

// To runtime

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Parent {
  Redex,
  Node { val: run::Val, port: run::Port },
}
const PARENT_ROOT: Parent = Parent::Node { val: run::ROOT.val(), port: tag_to_port(run::ROOT.tag()) };

pub fn tree_to_runtime_go(rt_net: &mut run::Net, tree: &Tree, vars: &mut HashMap<String, Parent>, parent: Parent) -> run::Ptr {
  match tree {
    Tree::Era => {
      run::ERAS
    }
    Tree::Ctr { lab, lft, rgt } => {
      let val = rt_net.alloc(1);
      let p1 = tree_to_runtime_go(rt_net, &*lft, vars, Parent::Node { val, port: run::P1 });
      rt_net.heap.set(val, run::P1, p1);
      let p2 = tree_to_runtime_go(rt_net, &*rgt, vars, Parent::Node { val, port: run::P2 });
      rt_net.heap.set(val, run::P2, p2);
      run::Ptr::new(*lab + run::CT0, val)
    }
    Tree::Var { nam } => {
      if let Parent::Redex = parent {
        panic!("By definition, can't have variable on active pairs.");
      };
      match vars.get(nam) {
        Some(Parent::Redex) => {
          unreachable!();
        }
        Some(Parent::Node { val: other_val, port: other_port }) => {
          match parent {
            Parent::Redex => { unreachable!(); }
            Parent::Node { val, port } => rt_net.heap.set(*other_val, *other_port, run::Ptr::new(port_to_tag(port), val)),
          }
          return run::Ptr::new(port_to_tag(*other_port), *other_val);
        }
        None => {
          vars.insert(nam.clone(), parent);
          run::NULL
        }
      }
    }
    Tree::Ref { nam } => {
      run::Ptr::new(run::REF, *nam)
    }
    Tree::Num { val } => {
      run::Ptr::new(run::NUM, *val as run::Val)
    }
    Tree::Op2 { lft, rgt } => {
      let val = rt_net.alloc(1);
      let p1 = tree_to_runtime_go(rt_net, &*lft, vars, Parent::Node { val, port: run::P1 });
      rt_net.heap.set(val, run::P1, p1);
      let p2 = tree_to_runtime_go(rt_net, &*rgt, vars, Parent::Node { val, port: run::P2 });
      rt_net.heap.set(val, run::P2, p2);
      run::Ptr::new(run::OP2, val)
    }
    Tree::Mat { sel, ret } => {
      let val = rt_net.alloc(1);
      let p1 = tree_to_runtime_go(rt_net, &*sel, vars, Parent::Node { val, port: run::P1 });
      rt_net.heap.set(val, run::P1, p1);
      let p2 = tree_to_runtime_go(rt_net, &*ret, vars, Parent::Node { val, port: run::P2 });
      rt_net.heap.set(val, run::P2, p2);
      run::Ptr::new(run::MAT, val)
    }
  }
}

pub fn tree_to_runtime(rt_net: &mut run::Net, tree: &Tree) -> run::Ptr {
  tree_to_runtime_go(rt_net, tree, &mut HashMap::new(), PARENT_ROOT)
}

pub fn net_to_runtime(rt_net: &mut run::Net, net: &Net) {
  let mut vars = HashMap::new();
  let root = tree_to_runtime_go(rt_net, &net.root, &mut vars, PARENT_ROOT);
  rt_net.heap.set_root(root);
  for (tree1, tree2) in &net.rdex {
    let ptr1 = tree_to_runtime_go(rt_net, tree1, &mut vars, Parent::Redex);
    let ptr2 = tree_to_runtime_go(rt_net, tree2, &mut vars, Parent::Redex);
    rt_net.rdex.push((ptr1, ptr2));
  }
}

pub fn book_to_runtime(book: &Book, call_native: run::CallNative) -> run::Book {
  let mut rt_book = run::Book::new();
  rt_book.call_native = call_native;
  for (name, net) in book {
    let id = name_to_val(name);
    let mut rt = run::Net::new(1 << 18);
    net_to_runtime(&mut rt, net);
    rt_book.def(id, rt.to_def());
  }
  rt_book
}

// From runtime

pub fn tree_from_runtime_go(rt_net: &run::Net, ptr: run::Ptr, parent: Parent, vars: &mut HashMap<Parent, String>, fresh: &mut usize) -> Tree {
  match ptr.tag() {
    run::ERA => {
      Tree::Era
    }
    run::REF => {
      Tree::Ref { nam: ptr.val() }
    }
    run::NUM => {
      Tree::Num { val: ptr.val() as run::Val }
    }
    run::OP1 | run::OP2 => {
      let lft = tree_from_runtime_go(rt_net, rt_net.heap.get(ptr.val(), run::P1), Parent::Node { val: ptr.val(), port: run::P1 }, vars, fresh);
      let rgt = tree_from_runtime_go(rt_net, rt_net.heap.get(ptr.val(), run::P2), Parent::Node { val: ptr.val(), port: run::P2 }, vars, fresh);
      Tree::Op2 { lft: Box::new(lft), rgt: Box::new(rgt) }
    }
    run::MAT => {
      let sel = tree_from_runtime_go(rt_net, rt_net.heap.get(ptr.val(), run::P1), Parent::Node { val: ptr.val(), port: run::P1 }, vars, fresh);
      let ret = tree_from_runtime_go(rt_net, rt_net.heap.get(ptr.val(), run::P2), Parent::Node { val: ptr.val(), port: run::P2 }, vars, fresh);
      Tree::Mat { sel: Box::new(sel), ret: Box::new(ret) }
    }
    run::VR1 | run::VR2 => {
      let key = match ptr.tag() {
        run::VR1 => Parent::Node { val: ptr.val(), port: run::P1 },
        run::VR2 => Parent::Node { val: ptr.val(), port: run::P2 },
        _        => unreachable!(),
      };
      if let Some(nam) = vars.get(&key) {
        Tree::Var { nam: nam.clone() }
      } else {
        let nam = num_to_str(*fresh);
        *fresh += 1;
        vars.insert(parent, nam.clone());
        Tree::Var { nam }
      }
    }
    _ => {
      let p1  = rt_net.heap.get(ptr.val(), run::P1);
      let p2  = rt_net.heap.get(ptr.val(), run::P2);
      let lft = tree_from_runtime_go(rt_net, p1, Parent::Node { val: ptr.val(), port: run::P1 }, vars, fresh);
      let rgt = tree_from_runtime_go(rt_net, p2, Parent::Node { val: ptr.val(), port: run::P2 }, vars, fresh);
      Tree::Ctr {
        lab: ptr.tag() - run::CT0,
        lft: Box::new(lft),
        rgt: Box::new(rgt),
      }
    }
  }
}

pub fn tree_from_runtime(rt_net: &run::Net, ptr: run::Ptr) -> Tree {
  let mut vars = HashMap::new();
  let mut fresh = 0;
  tree_from_runtime_go(rt_net, ptr, PARENT_ROOT, &mut vars, &mut fresh)
}

pub fn net_from_runtime(rt_net: &run::Net) -> Net {
  let mut vars = HashMap::new();
  let mut fresh = 0;
  let mut rdex = Vec::new();
  let root = tree_from_runtime_go(rt_net, rt_net.heap.get_root(), PARENT_ROOT, &mut vars, &mut fresh);
  for &(a, b) in &rt_net.rdex {
    let tree_a = tree_from_runtime_go(rt_net, a, Parent::Redex, &mut vars, &mut fresh);
    let tree_b = tree_from_runtime_go(rt_net, b, Parent::Redex, &mut vars, &mut fresh);
    rdex.push((tree_a, tree_b));
  }
  Net { root, rdex }
}

pub fn book_from_runtime(rt_book: &run::Book) -> Book {
  let mut book = BTreeMap::new();
  for id in 0 .. rt_book.defs.len() {
    let def = &rt_book.defs[id];
    if def.node.len() > 0 {
      let name = val_to_name(id as run::Val);
      let net = net_from_runtime(&run::Net::from_def(def.clone()));
      book.insert(name, net);
    }
  }
  book
}
