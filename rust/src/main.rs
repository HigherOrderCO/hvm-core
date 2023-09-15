#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]
#![allow(non_snake_case)]

mod core;
mod lang;

use crate::core::*;
use crate::lang::*;

fn main() {
  // Initializes the book
  let book = &mut Book::new();

  // Church Nat constructors
  let c_z = define(book, "c_z", "$ (0 * (0 a a))");
  let c_s = define(book, "c_s", "$ (0 (0 s (0 z k)) (0 (1 (0 k r) s) (0 z r)))");

  // Utils
  define(book, "id", "$ (0 x x)");

  // Church Nat constants
  {
    define(book, "c0" , "$ (0 * (0 a a))");
    define(book, "c1" , "$ (0 (0 a R) (0 a R))");
    define(book, "c2" , "$ (0 (1 (0 b a) (0 a R)) (0 b R))");
    define(book, "c3" , "$ (0 (1 (1 (0 c b) (0 b a)) (0 a R)) (0 c R))");
    define(book, "c4" , "$ (0 (1 (1 (1 (0 d c) (0 c b)) (0 b a)) (0 a R)) (0 d R))");
    define(book, "c5" , "$ (0 (1 (1 (1 (1 (0 e d) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 e R))");
    define(book, "c6" , "$ (0 (1 (1 (1 (1 (1 (0 f e) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 f R))");
    define(book, "c7" , "$ (0 (1 (1 (1 (1 (1 (1 (0 g f) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 g R))");
    define(book, "c8" , "$ (0 (1 (1 (1 (1 (1 (1 (1 (0 h g) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 h R))");
    define(book, "c9" , "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (0 i h) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 i R))");
    define(book, "c10", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 j i) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 j R))");
    define(book, "c11", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 k j) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 k R))");
    define(book, "c12", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 l k) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 l R))");
    define(book, "c13", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 m l) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 m R))");
    define(book, "c14", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 n m) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 n R))");
    define(book, "c15", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 o n) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 o R))");
    define(book, "c16", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 p o) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 p R))");
    define(book, "c17", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 q p) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 q R))");
    define(book, "c18", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 r q) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 r R))");
    define(book, "c19", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 s r) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 s R))");
    define(book, "c20", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 t s) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 t R))");
    define(book, "c21", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 u t) (0 t s)) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 u R))");
    define(book, "c22", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 v u) (0 u t)) (0 t s)) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 v R))");
    define(book, "c23", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 w v) (0 v u)) (0 u t)) (0 t s)) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 w R))");
    define(book, "c24", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 x w) (0 w v)) (0 v u)) (0 u t)) (0 t s)) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 x R))");
    define(book, "c25", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 y x) (0 x w)) (0 w v)) (0 v u)) (0 u t)) (0 t s)) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 x R))");
    define(book, "c26", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 z y) (0 y x)) (0 x w)) (0 w v)) (0 v u)) (0 u t)) (0 t s)) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 x R))");
    //define(book, "c26", "$ (0 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (1 (0 z y) (0 y x)) (0 x w)) (0 w v)) (0 v u)) (0 u t)) (0 t s)) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 x R))");

    define(book, "k0" , "$ (0 * (0 a a))");
    define(book, "k1" , "$ (0 (0 a R) (0 a R))");
    define(book, "k2" , "$ (0 (2 (0 b a) (0 a R)) (0 b R))");
    define(book, "k3" , "$ (0 (2 (2 (0 c b) (0 b a)) (0 a R)) (0 c R))");
    define(book, "k4" , "$ (0 (2 (2 (2 (0 d c) (0 c b)) (0 b a)) (0 a R)) (0 d R))");
    define(book, "k5" , "$ (0 (2 (2 (2 (2 (0 e d) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 e R))");
    define(book, "k6" , "$ (0 (2 (2 (2 (2 (2 (0 f e) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 f R))");
    define(book, "k7" , "$ (0 (2 (2 (2 (2 (2 (2 (0 g f) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 g R))");
    define(book, "k8" , "$ (0 (2 (2 (2 (2 (2 (2 (2 (0 h g) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 h R))");
    define(book, "k9" , "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (0 i h) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 i R))");
    define(book, "k10", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 j i) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 j R))");
    define(book, "k11", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 k j) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 k R))");
    define(book, "k12", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 l k) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 l R))");
    define(book, "k13", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 m l) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 m R))");
    define(book, "k14", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 n m) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 n R))");
    define(book, "k15", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 o n) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 o R))");
    define(book, "k16", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 p o) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 p R))");
    define(book, "k17", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 q p) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 q R))");
    define(book, "k18", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 r q) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 r R))");
    define(book, "k19", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 s r) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 s R))");
    define(book, "k20", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 t s) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 t R))");
    define(book, "k21", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 u t) (0 t s)) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 u R))");
    define(book, "k22", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 v u) (0 u t)) (0 t s)) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 v R))");
    define(book, "k23", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 w v) (0 v u)) (0 u t)) (0 t s)) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 w R))");
    define(book, "k24", "$ (0 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (2 (0 x w) (0 w v)) (0 v u)) (0 u t)) (0 t s)) (0 s r)) (0 r q)) (0 q p)) (0 p o)) (0 o n)) (0 n m)) (0 m l)) (0 l k)) (0 k j)) (0 j i)) (0 i h)) (0 h g)) (0 g f)) (0 f e)) (0 e d)) (0 d c)) (0 c b)) (0 b a)) (0 a R)) (0 x R))");
  }

  // Bools
  define(book, "T", "$ (0 t (0 * t))");
  define(book, "F", "$ (0 * (0 f f))");
  define(book, "not", "$ (0 (0 f (0 t r)) (0 t (0 f r)))");

  // Scott Nats
  define(book, "S", "$ (0 a (0 (0 a b) (0 * b)))");
  define(book, "Z", "$ (0 * (0 a a))");

  // Generators for a big binary tree
  // λr. λt. ((t r) r)
  //define(book, "g_s", "$ (0 (2 (0 b c) b) c)");
  define(book, "g_s", "$ (0 (2 r0 r1) (0 (0 r0 (0 r1 r)) r))");
  define(book, "g_z", "$ (0 x x)");

  // BitString constructors
  // O = λxs λo λi λe (o xs)
  // I = λxs λo λi λe (i xs)
  // E =     λo λi λe e
  define(book, "O", "$ (0 xs (0 (0 xs r) (0 * (0 * r))))");
  define(book, "I", "$ (0 xs (0 * (0 (0 xs r) (0 * r))))");
  define(book, "E", "$ (0 * (0 * (0 e e)))");

  // Double
  define(book, "nidS", "
    $ (0 p ret)
    & @S   ~ (0 nidp ret)
    & @nid ~ (0 p nidp)
  ");
  define(book, "nid" , "
    $ (0 (0 @nidS (0 @Z ret)) ret)
  ");

  // Decrements a BitString
  // decO = λp(I (dec p))
  // decI = λp(low p)
  // dec  = λx(((x decO) decI) E)
  define(book, "decO", "
    $ (0 p idecp)
    & @I   ~ (0 decp idecp)
    & @dec ~ (0 p decp)
  ");
  define(book, "decI", "
    $ (0 p lowp)
    & @low ~ (0 p lowp)
  ");
  define(book, "dec" , "
    $ (0 (0 @decO (0 @decI (0 @E ret))) ret)
  ");

  // Auxiliary function
  // lowO = λp(O (O p))
  // lowI = λp(O (I p))
  // low  = λx(((x lowO) lowI) E)
  define(book, "lowO", "
    $ (0 p oop)
    & @O ~ (0 p op)
    & @O ~ (0 op oop)
  ");
  define(book, "lowI", "
    $ (0 p oip)
    & @I ~ (0 p ip)
    & @O ~ (0 ip oip)
  ");
  define(book, "low" , "
    $ (0 (0 @lowO (0 @lowI (0 @E ret))) ret)
  ");

  // Decrements a BitString until it is zero
  // runO = λp(run (dec (O p)))
  // runI = λp(run (dec (I p)))
  // run  = λx(((x runO) runI) E)
  define(book, "runO", "
    $ (0 p ret)
    & @run ~ (0 decop ret)
    & @dec ~ (0 op decop)
    & @O   ~ (0 p op)
  ");
  define(book, "runI", "
    $ (0 p ret)
    & @run ~ (0 decip ret)
    & @dec ~ (0 ip decip)
    & @I   ~ (0 p ip)
  ");
  define(book, "run" , "
    $ (0 (0 @runO (0 @runI (0 @E ret))) ret)
  ");

  // Decrements 2^N BitStrings until they reach zero
  // brnZ = (run (c8 S Z))
  // brnS = λp {(brn p) (brn p)}
  // brn  = λn ((n brnS) brnZ)
  define(book, "brnZ", "
    $ ret
    & @run ~ (0 val ret)
    & @c10 ~ (0 @I (0 @E val))
  ");
  define(book, "brnS", "
    $ (0 (1 p0 p1) (0 r0 r1))
    & @brn ~ (0 p0 r0)
    & @brn ~ (0 p1 r1)
  ");
  define(book, "brn", "
    $ (0 (0 @brnS (0 @brnZ r)) r)
  ");

  // Creates a nat, for testing
  // ex0 = ((n S) Z)
  define(book, "ex0", "
    $ root
    & @c2 ~ (0 @k2 root)
  ");

  // Allocates a big tree
  // ex1 = ((n g_s) g_z)
  define(book, "ex1", "
    $ root
    & @c26 ~ (0 @g_s (0 @g_z root))
  ");

  // This example decreases a binary counter until it reaches 0. It uses recursion, based on
  // supercombinators. This, coupled with the REF-ERA rule, allows recursive calls to be collected
  // before they get the chance of expanding forever. This is key for efficient memory usage. Here,
  // `run` is the looping decrementer, `((n I) E)` is a BitString with `n` bits 1. If this test is
  // a success, our program will never need more than ~256 nodes of space; which is the case, as
  // we allocated only 256 nodes for `net` above.
  // main = (run ((n I) E))
  define(book, "ex2", "
    $ main
    & @run ~ (0 nie main)
    & @c14 ~ (0 @I (0 @E nie))
  "); 

  // Decreases many binary counters until they reach 0
  define(book, "ex3", "
    $ res
    & @brn ~ (0 dep res)
    & @c15 ~ (0 @S (0 @Z dep))
  "); 

  // Initializes the net
  let net = &mut Net::new(1 << 24);
  net.boot(name_to_u32("ex2"));

  // Computes its normal form
  net.expand(book, Ptr::new(VRR,0));
  net.normal(book, None);

  //Shows results and stats
  //println!("[net]\n{}", show_net(&net));
  println!("size: {}", net.node.len());
  println!("used: {}", net.used);
  println!("rwts: {}", net.rwts);

  //println!("net.root = {:08x}", net.root.data);
  //for i in 0 .. net.acts.len() {
    //println!("net.acts[{:04x}] = {:08x} {:08x}", i, net.acts[i].0.data, net.acts[i].1.data);
  //}
  //for i in 0 .. net.node.len() {
    //if net.node[i].ports[0].data != 0 || net.node[i].ports[1].data != 0 {
      //println!("net.node[{:04x}] = {:08x} {:08x}", i, net.node[i].ports[0].data, net.node[i].ports[1].data);
    //}
  //}

  //populate_cuda(&book); // prints CUDA book
}

fn populate_cuda(book: &Book) {
  println!("Term* term;");
  let mut sorted_defs: Vec<(&u32, &Net)> = book.defs.iter().collect();
  sorted_defs.sort_by_key(|&(key, _)| key);
  for (key, def) in sorted_defs {
    println!("  // {}", u32_to_name(*key));
    println!("  book->defs[0x{:08x}]           = (Term*) malloc(sizeof(Term));", key);
    println!("  book->defs[0x{:08x}]->root     = 0x{:08x};", key, def.root.data);
    println!("  book->defs[0x{:08x}]->alen     = {};", key, def.acts.len());
    println!("  book->defs[0x{:08x}]->acts     = (Wire*) malloc({} * sizeof(Wire));", key, def.acts.len());
    for i in 0..def.acts.len() {
        println!("  book->defs[0x{:08x}]->acts[{:2}] = mkwire(0x{:08x},0x{:08x});", key, i, def.acts[i].0.data, def.acts[i].1.data);
    }
    println!("  book->defs[0x{:08x}]->nlen     = {};", key, def.node.len());
    println!("  book->defs[0x{:08x}]->node     = (Node*) malloc({} * sizeof(Node));", key, def.node.len());
    for i in 0..def.node.len() {
      println!("  book->defs[0x{:08x}]->node[{:2}] = (Node) {{0x{:08x},0x{:08x}}};", key, i, def.node[i].ports[P1].data, def.node[i].ports[P2].data);
    }
  }
}
