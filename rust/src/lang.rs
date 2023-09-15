// An interaction combinator language
// ----------------------------------
//
// This file implements a textual syntax to interact with the runtime. It includes a pure AST for
// nets, as well as functions for parsing, stringifying, and converting pure ASTs to runtime nets.
// On the runtime, a net is represented by a list of active trees, plus a root tree. The textual
// syntax reflects this representation. It is specified below:
//
// <net>    ::= <root> <acts>
//   <root> ::= "$" <tree>
//   <acts> ::= "&" <tree> "~" <tree> <net>
// <tree>   ::= <era> | <nod> | <var> | <num>
//   <era>  ::= "*"
//   <nod>  ::= "(" <num_lit> " " <tree> " " <tree> ")"
//   <var>  ::= <str_lit>
//   <num>  ::= <num_lit>
//   <ref>  ::= "@" <str_lit>
//
// For example, below is the church nat 2, encoded as an interaction net:
//
// $ (0 (1 (0 b a) (0 a R)) (0 b R))
//
// The '$' symbol denotes the net's root. A node is denoted as `(LABEL CHILD_1 CHILD_2)`.
// The label 0 is used for CON nodes, while labels >1 are used for DUP nodes. A node has two
// children, representing Port1->Port0 and Port2->Port0 wires. Variables are denoted by
// alphanumeric names, and used to represent auxiliary wires (Port1->Port2 and Port2->Port1).
// Active wires (Port0->Port0) are represented with by '& left_tree ~ right_tree'. For example:
//
// & (0 x x)
// ~ (0 y y)
//
// The net above represents two identity CON nodes connected by their main ports. This net has no
// root, so it will just reduce to nothingness. Numbers are represented by numeric literals.
// References (to closed nets) are denoted by '@name', where the name must have at most 5 letters,
// from the following alphabet: ".0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_".

use crate::core::*;
use std::collections::HashMap;
use std::fmt::Debug;
use std::iter::Peekable;
use std::str::Chars;

// AST
// ---

#[derive(Clone)]
pub enum LTree {
  Era,
  Nod { 
    tag: Tag,
    lft: Box<LTree>,
    rgt: Box<LTree>,
  },
  Var {
    nam: String,
  },
  Ref {
    nam: Val
  },
  NUM {
    val: Val
  },
}

impl Debug for LTree {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Era => write!(f, "Era"),
      Self::Nod { tag, lft, rgt } => f
        .debug_struct("Node")
        .field(
          "tag",
          match *tag {
            CON => &"CON",
            DUP => &"DUP",
            ERA => &"ERA",
            _ => &"???",
          },
        )
        .field("lft", lft)
        .field("rgt", rgt)
        .finish(),
      Self::Var { nam } => write!(f, "\"{}\"", nam),
      Self::Ref { nam } => f.debug_tuple("Ref").field(nam).finish(),
      Self::NUM { val } => f.debug_tuple("NUM").field(val).finish(),
    }
  }
}

type LActs = Vec<(LTree,LTree)>;

#[derive(Debug, Clone)]
pub struct LNet {
  pub root: LTree,
  pub acts: LActs,
}

// Parser
// ------

fn skip_spaces(chars: &mut Peekable<Chars>) {
  while let Some(c) = chars.peek() {
    if *c != ' ' && *c != '\n' {
      break;
    }
    chars.next();
  }
}

fn consume(chars: &mut Peekable<Chars>, text: &str) {
  skip_spaces(chars);
  for c in text.chars() {
    assert_eq!(chars.next().unwrap(), c);
  }
}

pub fn parse_num_lit(chars: &mut Peekable<Chars>) -> u32 {
  let mut num : u32 = 0;
  skip_spaces(chars);
  while let Some(c) = chars.peek() {
    if !c.is_digit(10) {
      break;
    }
    num = num * 10 + c.to_digit(10).unwrap() as u32;
    chars.next();
  }
  num
}

pub fn parse_str_lit(chars: &mut Peekable<Chars>) -> String {
  let mut str = String::new();
  skip_spaces(chars);
  while let Some(c) = chars.peek() {
    if !c.is_alphanumeric() && *c != '_' && *c != '.' {
      break;
    }
    str.push(*c);
    chars.next();
  }
  str
}

pub fn parse_ltree(chars: &mut Peekable<Chars>) -> LTree {
  skip_spaces(chars);
  match chars.peek() {
    Some('*') => {
      chars.next();
      LTree::Era
    },
    Some('(') => {
      chars.next();
      let tag = CON + (parse_num_lit(chars) as u16);
      let lft = Box::new(parse_ltree(chars));
      let rgt = Box::new(parse_ltree(chars));
      consume(chars, ")");
      LTree::Nod { tag, lft, rgt }
    },
    Some('@') => {
      chars.next();
      skip_spaces(chars);
      let name = parse_str_lit(chars);
      LTree::Ref { nam: name_to_u32(&name) }
    },
    Some(c) if c.is_digit(10) => {
      LTree::NUM { val: parse_num_lit(chars) }
    },
    _ => {
      LTree::Var { nam: parse_str_lit(chars) }
    },
  }
}

pub fn parse_lnet(chars: &mut Peekable<Chars>) -> LNet {
  let mut acts = Vec::new();
  let mut root = LTree::Era;
  while let Some(c) = { skip_spaces(chars); chars.peek() } {
    if *c == '$' {
      chars.next();
      root = parse_ltree(chars);
    } else if *c == '&' {
      chars.next();
      let tree1 = parse_ltree(chars);
      consume(chars, "~");
      let tree2 = parse_ltree(chars);
      acts.push((tree1, tree2));
    } else {
      break;
    }
  }
  LNet { root, acts }
}

pub fn do_parse_ltree(code: &str) -> LTree {
  parse_ltree(&mut code.chars().peekable())
}

pub fn do_parse_lnet(code: &str) -> LNet {
  parse_lnet(&mut code.chars().peekable())
}

// Stringifier
// -----------

pub fn show_ltree(tree: &LTree) -> String {
  match tree {
    LTree::Era => {
      "*".to_string()
    },
    LTree::Nod { tag, lft, rgt } => {
      format!("({} {} {})", tag - CON, show_ltree(&*lft), show_ltree(&*rgt))
    },
    LTree::Var { nam } => {
      nam.clone()
    },
    LTree::Ref { nam } => {
      format!("@{}", u32_to_name(*nam))
    },
    LTree::NUM { val } => {
      val.to_string()
    },
  }
}

pub fn show_lnet(lnet: &LNet) -> String {
  let mut result = String::new();
  result.push_str(&format!("$ {}\n", show_ltree(&lnet.root)));
  for (a, b) in &lnet.acts {
    result.push_str(&format!("& {}\n~ {}\n", show_ltree(a), show_ltree(b)));
  }
  return result;
}

pub fn show_net(net: &Net) -> String {
  show_lnet(&readback_lnet(net))
}

// Conversion
// ----------

pub fn num_to_str(num: usize) -> String {
  let mut num = num + 1;
  let mut str = String::new();
  while num > 0 {
    let c = ((num % 26) as u8 + b'a') as char;
    str.push(c);
    num /= 26;
  }
  return str.chars().rev().collect();
}

pub fn tag_to_port(tag: Tag) -> Port {
  match tag {
    VR1 => P1,
    VR2 => P2,
    _   => unreachable!(),
  }
}

pub fn port_to_tag(port: Port) -> Tag {
  match port {
    P1 => VR1,
    P2 => VR2,
    _  => unreachable!(),
  }
}

pub fn lnet_to_net(lnet: &LNet, padding_factor: usize) -> Net {
  let mut vars = HashMap::new();
  let mut net = Net::new(1 << 16);
  net.root = alloc_ltree(&mut net, &lnet.root, &mut vars, Parent::Root);
  for (tree1, tree2) in &lnet.acts {
    let ptr1 = alloc_ltree(&mut net, tree1, &mut vars, Parent::Acts);
    let ptr2 = alloc_ltree(&mut net, tree2, &mut vars, Parent::Acts);
    net.acts.push((ptr1, ptr2));
  }
  net.node = net.node[0 .. net.used * padding_factor].to_vec();
  return net;
}

pub fn name_to_letters(name: &str) -> Vec<u8> {
  let mut letters = Vec::new();
  for c in name.chars() {
    letters.push(match c {
      '.'       => 0,
      '0'..='9' => c as u8 - '0' as u8 + 1,
      'A'..='Z' => c as u8 - 'A' as u8 + 11,
      'a'..='z' => c as u8 - 'a' as u8 + 37,
      '_'       => 63,
      _         => panic!("Invalid character in name"),
    });
  }
  return letters;
}

pub fn letters_to_name(letters: Vec<u8>) -> String {
  let mut name = String::new();
  for letter in letters {
    name.push(match letter {
            0 => '.',
       1..=10 => (letter - 1 + '0' as u8) as char,
      11..=36 => (letter - 11 + 'A' as u8) as char,
      37..=62 => (letter - 37 + 'a' as u8) as char,
           63 => '_',
      _       => panic!("Invalid letter in name"),
    });
  }
  return name;
}

pub fn u32_to_letters(num: u32) -> Vec<u8> {
  let mut letters = Vec::new();
  let mut num = num;
  while num > 0 {
    letters.push((num % 64) as u8);
    num /= 64;
  }
  letters.reverse();
  return letters;
}

pub fn letters_to_u32(letters: Vec<u8>) -> u32 {
  let mut num = 0;
  for letter in letters {
    num = num * 64 + letter as u32;
  }
  return num;
}

pub fn name_to_u32(name: &str) -> u32 {
  letters_to_u32(name_to_letters(name))
}

pub fn u32_to_name(num: u32) -> String {
  letters_to_name(u32_to_letters(num))
}

// Injection and Readback
// ----------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Parent {
  Acts,
  Root,
  Node { val: Val, port: Port },
}

pub fn alloc_ltree(net: &mut Net, tree: &LTree, vars: &mut HashMap<String, Parent>, parent: Parent) -> Ptr {
  match tree {
    LTree::Era => {
      Ptr::new(ERA, 0)
    },
    LTree::Nod { tag, lft, rgt } => {
      let val = net.alloc();
      let p1 = alloc_ltree(net, &*lft, vars, Parent::Node { val, port: P1 });
      net.set(val, P1, p1);
      let p2 = alloc_ltree(net, &*rgt, vars, Parent::Node { val, port: P2 });
      net.set(val, P2, p2);
      Ptr::new(*tag, val)
    },
    LTree::Var { nam } => {
      if let Parent::Acts = parent {
        panic!("By definition, can't have variable on active pairs.");
      };
      match vars.get(nam) {
        Some(Parent::Acts) => {
          unreachable!();
        },
        Some(Parent::Root) => {
          match parent {
            Parent::Acts => {
              unreachable!();
            }
            Parent::Node { val, port } => {
              net.root = Ptr::new(port_to_tag(port), val);
            }
            Parent::Root => {
              net.root = Ptr::new(VRR, 0);
            }
          }
          return Ptr::new(VRR, 0);
        },
        Some(Parent::Node { val: other_val, port: other_port }) => {
          //println!("linked {} | set {} {:?} as {} {:?}", nam, other_val, other_port, val, port);
          match parent {
            Parent::Acts => {
              unreachable!();
            }
            Parent::Node { val, port } => {
              net.set(*other_val, *other_port, Ptr::new(port_to_tag(port), val))
            }
            Parent::Root => {
              net.set(*other_val, *other_port, Ptr::new(VRR, 0))
            }
          }
          return Ptr::new(port_to_tag(*other_port), *other_val);
        }
        None => {
          //println!("linkin {} | iam {} {:?}", nam, val, port);
          vars.insert(nam.clone(), parent);
          Ptr::new(NIL, 0)
        }
      }
    },
    LTree::Ref { nam } => {
      Ptr::new(REF, *nam)
    },
    LTree::NUM { val } => {
      Ptr::new(NUM, *val)
    },
  }
}

pub fn do_alloc_ltree(net: &mut Net, tree: &LTree) -> Ptr {
  alloc_ltree(net, tree, &mut HashMap::new(), Parent::Root)
}

pub fn readback_ltree(net: &Net, ptr: Ptr, parent: Parent, vars: &mut HashMap<Parent,String>, fresh: &mut usize) -> LTree {
  match ptr.tag() {
    NIL => {
      LTree::Var { nam: "?".to_string() }
    },
    ERA => {
      LTree::Era
    },
    REF => {
      LTree::Ref { nam: ptr.val() }
    },
    NUM => {
      LTree::NUM { val: ptr.val() }
    },
    VRR | VR1 | VR2 => {
      let key = match ptr.tag() {
        VR1 => Parent::Node { val: ptr.val(), port: P1 },
        VR2 => Parent::Node { val: ptr.val(), port: P2 },
        VRR => Parent::Root,
        _   => unreachable!(),
      };
      if let Some(nam) = vars.get(&key) {
        LTree::Var { nam: nam.clone() }
      } else {
        let nam = num_to_str(*fresh);
        *fresh += 1;
        vars.insert(parent, nam.clone());
        LTree::Var { nam }
      }
    },
    _ => {
      let lft = readback_ltree(net, net.get(ptr.val(), P1), Parent::Node { val: ptr.val(), port: P1 }, vars, fresh);
      let rgt = readback_ltree(net, net.get(ptr.val(), P2), Parent::Node { val: ptr.val(), port: P2 }, vars, fresh);
      LTree::Nod { tag: ptr.tag(), lft: Box::new(lft), rgt: Box::new(rgt) }
    },
  }
}

pub fn readback_lnet(net: &Net) -> LNet {
  let mut vars = HashMap::new();
  let mut fresh = 0;
  let mut acts = Vec::new();
  let root = readback_ltree(net, net.root, Parent::Root, &mut vars, &mut fresh);
  for &(a, b) in &net.acts {
    let tree_a = readback_ltree(net, a, Parent::Acts, &mut vars, &mut fresh);
    let tree_b = readback_ltree(net, b, Parent::Acts, &mut vars, &mut fresh);
    acts.push((tree_a, tree_b));
  }
  LNet { root, acts }
}

// Utils
// -----

pub fn define(book: &mut Book, name: &str, code: &str) -> u32 {
  let id = name_to_u32(name);
  book.def(id, lnet_to_net(&do_parse_lnet(code), 1));
  return id;
}
