mod loaders;

#[cfg(not(feature = "cuda"))] // FIXME: Cuda does not support native numbers
mod numeric_tests {
  use crate::loaders::*;
  use hvmc::{
    ast::{show_net, Book},
    run::{self, Lab},
  };
  use insta::{assert_debug_snapshot, assert_snapshot};

  fn op_net(lnum: u32, op: Lab, rnum: u32) -> Book {
    parse_core(&format!("@main = root & #{lnum} ~ <{} #{rnum} root>", hvmc::ast::show_opr(op)))
  }

  #[test]
  fn test_add() {
    let net = op_net(10, run::ADD, 2);
    let (rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#12");
    assert_debug_snapshot!(rnet.rewrites(), @"3");
  }

  #[test]
  fn test_sub() {
    let net = op_net(10, run::SUB, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#8");
  }

  #[test]
  fn test_mul() {
    let net = op_net(10, run::MUL, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#20");
  }

  #[test]
  #[ignore] // FIXME: Parsing of the `/` symbol for numeric operations is broken
  fn test_div() {
    let net = op_net(10, run::DIV, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#5");
  }

  #[test]
  fn test_mod() {
    let net = op_net(10, run::MOD, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#0");
  }

  #[test]
  fn test_eq() {
    let net = op_net(10, run::EQ, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#0");
  }

  #[test]
  fn test_ne() {
    let net = op_net(10, run::NE, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#1");
  }

  #[test]
  fn test_lt() {
    let net = op_net(10, run::LT, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#0");
  }

  #[test]
  fn test_gt() {
    let net = op_net(10, run::GT, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#1");
  }

  #[test]
  fn test_and() {
    let net = op_net(10, run::AND, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#2");
  }

  #[test]
  fn test_or() {
    let net = op_net(10, run::OR, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#10");
  }

  #[test]
  fn test_xor() {
    let net = op_net(10, run::XOR, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#8");
  }

  #[test]
  fn test_not() {
    let net = op_net(0, run::NOT, 256);
    let (rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#1152921504606846975");
    assert_debug_snapshot!(rnet.rewrites(), @"3");
  }

  #[test]
  fn test_lsh() {
    let net = op_net(10, run::LSH, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#40");
  }

  #[test]
  fn test_rsh() {
    let net = op_net(10, run::RSH, 2);
    let (_rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#2");
  }

  #[test]
  #[ignore] // FIXME: Parsing of the `/` symbol for numeric operations is broken
  /// Division by zero always return the value of 0xFFFFFF,
  /// that is read as the unsigned integer `16777215`
  fn test_div_by_0() {
    let net = op_net(9, run::DIV, 0);
    let (rnet, net) = normal(net, 16);
    assert_snapshot!(show_net(&net), @"#16777215");
    assert_debug_snapshot!(rnet.rewrites(), @"5");
  }

  #[test]
  // TODO: we lack a way to check if it's actually doing the chained ops optimization, or if it's doing one op per interaction
  fn test_chained_ops() {
    let mut net = load_lang("chained_ops.hvm");
    let (rnet, net, _id_map) = hvm_lang_normal(&mut net, 256);

    assert_snapshot!(show_net(&net), @"#7184190578800");
    assert_debug_snapshot!(rnet.rewrites(), @"35");
  }
}
