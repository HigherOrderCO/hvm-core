//! This file represents the intermediate representation of the compiler as an imperative
//! language, to be translated into the target language.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRepr {
  HvmPtr,
  Ptr,
  USize,
  U8,
  U32,
  Bool,
}

/// Constant values
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Const {
  F(String),
  P1,   // [crate::run::P1]
  P2,   // [crate::run::P2]
  NULL, // [crate::run::NULL]
  ROOT, // [crate::run::ROOT]
  ERAS, // [crate::run::ERAS]
  VR1,  // [crate::run::VR1]
  VR2,  // [crate::run::VR2]
  RD1,  // [crate::run::RD1]
  RD2,  // [crate::run::RD2]
  REF,  // [crate::run::REF]
  ERA,  // [crate::run::ERA]
  NUM,  // [crate::run::NUM]
  OP1,  // [crate::run::OP1]
  OP2,  // [crate::run::OP2]
  MAT,  // [crate::run::MAT]
  CT0,  // [crate::run::CT0]
  CT1,  // [crate::run::CT1]
  CT2,  // [crate::run::CT2]
  CT3,  // [crate::run::CT3]
  CT4,  // [crate::run::CT4]
  CT5,  // [crate::run::CT5]
  USE,  // [crate::run::USE]
  ADD,  // [crate::run::ADD]
  SUB,  // [crate::run::SUB]
  MUL,  // [crate::run::MUL]
  DIV,  // [crate::run::DIV]
  MOD,  // [crate::run::MOD]
  EQ,   // [crate::run::EQ]
  NE,   // [crate::run::NE]
  LT,   // [crate::run::LT]
  GT,   // [crate::run::GT]
  AND,  // [crate::run::AND]
  OR,   // [crate::run::OR]
  XOR,  // [crate::run::XOR]
  NOT,  // [crate::run::NOT]
  RSH,  // [crate::run::RSH]
  LSH,  // [crate::run::LSH]
}

/// Constant values
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prop {
  Anni,
  Oper,
  Eras,
  Comm,
  Var(String),
}

pub struct Constant {
  pub name: String,
  pub value: u32,
}

/// Represents the entire IR.
pub struct Program {
  pub functions: Vec<Function>,
  pub values: Vec<Constant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
  pub name: String,
  pub body: Vec<Stmt>,
}

/// Represents a single statement in the IR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
  Let {
    name: String,
    value: Instr,
  },
  Val {
    name: String,
    type_repr: TypeRepr,
  },
  Assign {
    name: Prop,
    value: Instr,
  },
  Instr(Instr),
  Free(Instr),
  Return(Instr),
  /// self.heap.set(idx, port, value)
  SetHeap {
    idx: Instr,
    port: Instr,
    value: Instr,
  },
  /// self.link(lhs, rhs)
  Link {
    lhs: Instr,
    rhs: Instr,
  },
}

/// Represents a single instruction in the IR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instr {
  True,
  False,
  Int(u32),
  Const(Const),
  Prop(Prop),

  /// SPECIAL TERM: Call
  /// if cond then else els
  If {
    cond: Box<Instr>,
    then: Vec<Stmt>,
    otherwise: Vec<Stmt>,
  },
  /// !ins
  Not {
    ins: Box<Instr>,
  },
  /// lhs op rhs
  Bin {
    op: String,
    lhs: Box<Instr>,
    rhs: Box<Instr>,
  },

  // VALUE FUNCTIONS:
  // These are the functions that are exposed to the user.
  /// ins.val()
  Val {
    ins: Box<Instr>,
  },
  /// ins.tag()
  Tag {
    ins: Box<Instr>,
  },
  /// ins.is_num()
  IsNum {
    ins: Box<Instr>,
  },
  /// ins.is_skp()
  IsSkp {
    ins: Box<Instr>,
  },
  /// Ptr::new(tag, value)
  NewPtr {
    tag: Box<Instr>,
    value: Box<Instr>,
  },

  // FUNCTIONS:
  // These are the functions that are internal to the IR.
  /// self.ops(lhs, op, rhs)
  Op {
    lhs: Box<Instr>,
    rhs: Box<Instr>,
  },
  /// self.alloc(n)
  Alloc {
    size: usize,
  },
  /// self.heap.get(idx, port)
  GetHeap {
    idx: Box<Instr>,
    port: Box<Instr>,
  },
}

impl From<Const> for Instr {
  fn from(value: Const) -> Self {
    Instr::Const(value)
  }
}

impl From<String> for Instr {
  fn from(value: String) -> Self {
    Instr::Prop(Prop::Var(value))
  }
}

impl Instr {
  pub fn is_num(self) -> Instr {
    Instr::IsNum {
      ins: Box::new(self),
    }
  }

  pub fn is_skp(self) -> Instr {
    Instr::IsSkp {
      ins: Box::new(self),
    }
  }

  pub fn val(self) -> Instr {
    Instr::Val {
      ins: Box::new(self),
    }
  }

  pub fn tag(self) -> Instr {
    Instr::Tag {
      ins: Box::new(self),
    }
  }

  pub fn not(self) -> Instr {
    Instr::Not {
      ins: Box::new(self),
    }
  }

  pub fn bin(self, op: &str, rhs: Instr) -> Instr {
    Instr::Bin {
      op: op.to_string(),
      lhs: Box::new(self),
      rhs: Box::new(rhs),
    }
  }

  pub fn eq(self, rhs: Instr) -> Instr {
    self.bin("==", rhs)
  }

  pub fn ne(self, rhs: Instr) -> Instr {
    self.bin("!=", rhs)
  }

  pub fn and(self, rhs: Instr) -> Instr {
    self.bin("&&", rhs)
  }

  pub fn sub(self, rhs: Instr) -> Instr {
    self.bin("-", rhs)
  }

  pub fn add(self, rhs: Instr) -> Instr {
    self.bin("+", rhs)
  }

  pub fn link(self, rhs: Instr) -> Stmt {
    Stmt::Link {
      lhs: self,
      rhs,
    }
  }

  pub fn new_ptr(tag: impl Into<Instr>, value: Instr) -> Instr {
    Instr::NewPtr {
      tag: Box::new(tag.into()),
      value: Box::new(value),
    }
  }
}

impl From<Prop> for Instr {
  fn from(value: Prop) -> Self {
    Instr::Prop(value)
  }
}
