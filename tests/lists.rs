use crate::loaders::*;
use hvml::term::Book;
use insta::assert_debug_snapshot;

mod loaders;

fn list_got(index: u32) -> Book {
  let template = load_file("list_put_got.hvm");
  let code = replace_template(template, &[("{fun}", "Got"), ("{args}", &format!("S{}", index))]);
  parse_lang(&code)
}

#[test]
fn test_list_got() {
  let mut rwts = Vec::new();

  for index in [
    0,
    1,
    3,
    7,
    // FIXME: Gpu runtime panics with `CUDA_ERROR_ILLEGAL_ADDRESS` when index is >= 14
    // if the list inside `list_put_got.hvm` is [31..0] instead of [0..31], it fails when <= 17
    #[cfg(not(feature = "cuda"))]
    15,
    // FIXME: Higher numbers than 28 on the `list_put_got.hvm` file are causing a panic `attempt to multiply with overflow` on `hvmc::run::Net::expand`
    // #[cfg(not(feature = "cuda"))]
    // 31,
  ] {
    let mut book = list_got(index);
    let (rnet, _, _) = hvm_lang_normal(&mut book, 2048);
    rwts.push(rnet.rewrites())
  }

  assert_debug_snapshot!(rwts[0], @"306");
  assert_debug_snapshot!(rwts[1], @"341");
  assert_debug_snapshot!(rwts[2], @"411");
  assert_debug_snapshot!(rwts[3], @"551");
  #[cfg(not(feature = "cuda"))]
  assert_debug_snapshot!(rwts[4], @"831");

  // Tests the linearity of the function
  let delta = rwts[1] - rwts[0];
  assert_eq!(rwts[1] + delta * 2, rwts[2]);
  assert_eq!(rwts[2] + delta * 4, rwts[3]);
  #[cfg(not(feature = "cuda"))]
  assert_eq!(rwts[3] + delta * 8, rwts[4]);
}

fn list_put(index: u32, value: u32) -> Book {
  let template = load_file("list_put_got.hvm");
  let code = replace_template(template, &[("{fun}", "Put"), ("{args}", &format!("S{index} S{value}"))]);
  parse_lang(&code)
}

#[test]
fn test_list_put() {
  let mut rwts = Vec::new();

  for (index, value) in [(0, 2), (1, 4), (3, 8), (7, 16), (15, 32)] {
    let mut book = list_put(index, value);
    let (rnet, _, _) = hvm_lang_normal(&mut book, 2048);
    rwts.push(rnet.rewrites())
  }

  assert_debug_snapshot!(rwts[0], @"295");
  assert_debug_snapshot!(rwts[1], @"320");
  assert_debug_snapshot!(rwts[2], @"370");
  assert_debug_snapshot!(rwts[3], @"470");
  assert_debug_snapshot!(rwts[4], @"670");

  //Tests the linearity of the function
  let delta = rwts[1] - rwts[0];
  assert_eq!(rwts[1] + delta * 2, rwts[2]);
  assert_eq!(rwts[2] + delta * 4, rwts[3]);
  assert_eq!(rwts[3] + delta * 8, rwts[4]);
}
