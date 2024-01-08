use hvmc::ast::show_net;
use insta::{assert_debug_snapshot, assert_snapshot};
use loaders::*;

mod loaders;

pub fn load_bench(file: &str) -> String {
  let path = format!("{}/benches/programs/{}", env!("CARGO_MANIFEST_DIR"), file);
  std::fs::read_to_string(path).unwrap()
}

// Loads file and generate Book from hvm-core syntax
fn load_core(file: &str) -> hvmc::ast::Book {
  let code = load_bench(file);
  parse_core(&code)
}

// Loads file and generate DefinitionBook from hvm-lang syntax
pub fn load_lang(file: &str) -> hvml::term::Book {
  let code = load_bench(file);
  parse_lang(&code)
}

#[test]
#[ignore] // FIXME: result/file is wrong
#[cfg(not(feature = "cuda"))] // FIXME: hangs indefinitely
fn dec_bits() {
  let book = load_core("binary-counter/dec_bits.hvmc");
  let (rnet, net) = normal(book, 1 << 16);

  assert_snapshot!(show_net(&net), @"(* (* (a a)))");
  assert_debug_snapshot!(rnet.rewrites(), @"180113");
}

#[test]
#[ignore] // FIXME: result/file is wrong
fn dec_bits_tree() {
  let book = load_core("binary-counter/dec_bits_tree.hvmc");
  let (rnet, net) = normal(book, 1 << 13);

  // FIXME: The refs to the rule `@E` are not expanded
  if cfg!(feature = "cuda") {
    assert_snapshot!(show_net(&net), @"((((((@E @E) (@E @E)) ((@E @E) (@E @E))) (((@E @E) (@E @E)) ((@E @E) (@E @E)))) ((((@E @E) (@E @E)) ((@E @E) (@E @E))) (((@E @E) (@E @E)) ((@E @E) (@E @E))))) (((((@E @E) (@E @E)) ((@E @E) (@E @E))) (((@E @E) (@E @E)) ((@E @E) (@E @E)))) ((((@E @E) (@E @E)) ((@E @E) (@E @E))) (((@E @E) (@E @E)) ((@E @E) (@E @E))))))");
    assert_debug_snapshot!(rnet.rewrites(), @"2878529");
  } else {
    assert_snapshot!(show_net(&net), @"(((((((* (* (b b))) (* (* (c c)))) ((* (* (d d))) (* (* (e e))))) (((* (* (f f))) (* (* (g g)))) ((* (* (h h))) (* (* (i i)))))) ((((* (* (j j))) (* (* (k k)))) ((* (* (l l))) (* (* (m m))))) (((* (* (n n))) (* (* (o o)))) ((* (* (p p))) (* (* (q q))))))) (((((* (* (r r))) (* (* (s s)))) ((* (* (t t))) (* (* (u u))))) (((* (* (v v))) (* (* (w w)))) ((* (* (x x))) (* (* (y y)))))) ((((* (* (z z))) (* (* (ba ba)))) ((* (* (bb bb))) (* (* (bc bc))))) (((* (* (bd bd))) (* (* (be be)))) ((* (* (bf bf))) (* (* (bg bg)))))))) ((((((* (* (bh bh))) (* (* (bi bi)))) ((* (* (bj bj))) (* (* (bk bk))))) (((* (* (bl bl))) (* (* (bm bm)))) ((* (* (bn bn))) (* (* (bo bo)))))) ((((* (* (bp bp))) (* (* (bq bq)))) ((* (* (br br))) (* (* (bs bs))))) (((* (* (bt bt))) (* (* (bu bu)))) ((* (* (bv bv))) (* (* (bw bw))))))) (((((* (* (bx bx))) (* (* (by by)))) ((* (* (bz bz))) (* (* (ca ca))))) (((* (* (cb cb))) (* (* (cc cc)))) ((* (* (cd cd))) (* (* (ce ce)))))) ((((* (* (cf cf))) (* (* (cg cg)))) ((* (* (ch ch))) (* (* (ci ci))))) (((* (* (cj cj))) (* (* (ck ck)))) ((* (* (cl cl))) (* (* (cm cm)))))))))");
    assert_debug_snapshot!(rnet.rewrites(), @"2878593");
  }
}

#[test]
#[ignore] // FIXME: hangs indefinitely
#[cfg(not(feature = "cuda"))] // FIXME: gpu runtime panics with `CUDA_ERROR_ILLEGAL_ADDRESS`
fn test_church_exp() {
  let book = load_core("church/church_exp.hvmc");
  let (rnet, net) = normal(book, 1 << 12);

  assert_snapshot!(show_net(&net));
  assert_debug_snapshot!(rnet.rewrites(), @"1943");
}

#[test]
#[ignore] // FIXME: church numbers bigger than C_16 causes stack overflow
fn test_church_mul() {
  let mut book = load_lang("church/church_mul.hvm");
  let (rnet, net, id_map) = hvm_lang_normal(&mut book, 512);
  let (readback, valid_readback) = hvm_lang_readback(&net, &book, id_map);

  assert_debug_snapshot!(valid_readback, @"false"); // invalid because of dup labels 

  // TODO: investigate why this difference exists
  if cfg!(feature = "cuda") {
    assert_snapshot!(show_net(&net), @"({2 ({2 b {3 c {4 d e}}} f) {3 (f g) {4 (g h) {5 i j}}}} (b k))");
    assert_snapshot!(readback, @"λa λb ((* {λc d {λd e {λe (a (a (a {b {c {c c}}}))) {a a}}}}) c)");
    assert_debug_snapshot!(rnet.rewrites(), @"15");
  } else {
    assert_snapshot!(show_net(&net), @"({2 ({2 b {3 c {4 d {5 e f}}}} g) {3 (g h) {4 (h i) {5 (i j) k}}}} (b l))");
    assert_snapshot!(readback, @"λa λb ((* {λc d {λd e {λe f {λf (a (a (a (a {b {c {c {c c}}}})))) a}}}}) c)");
    assert_debug_snapshot!(rnet.rewrites(), @"17");
  }
}

#[test]
#[ignore] // FIXME: hangs indefinitely
fn alloc_big_tree() {
  let book = load_core("tree/alloc_big_tree.hvmc");
  let (rnet, net) = normal(book, 1 << 16);

  assert_snapshot!(show_net(&net)); // file snapshot
  assert_debug_snapshot!(rnet.rewrites(), @"24628");
}

#[test]
fn test_neg_fusion() {
  let mut book = load_lang("fusion/neg_fusion.hvm");
  let (rnet, net, id_map) = hvm_lang_normal(&mut book, 512);
  let (readback, valid_readback) = hvm_lang_readback(&net, &book, id_map);

  assert!(valid_readback);
  assert_snapshot!(show_net(&net), @"(a (* a))");
  assert_snapshot!(readback, @"λa λ* a");

  // TODO: investigate why this difference exists
  if cfg!(feature = "cuda") {
    assert_debug_snapshot!(rnet.rewrites(), @"160");
  } else {
    assert_debug_snapshot!(rnet.rewrites(), @"127");
  }
}
