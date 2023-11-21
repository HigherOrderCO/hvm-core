//! JIT Runtime functions. These are used combined with the JIT compiler to achieve fast
//! execution of the program. The JIT compiler will generate calls to these functions
//! to perform operations that are not possible to do in the JIT compiler itself.

use crate::run::{self, Port, Ptr, Tag, Val};

type Net = *mut run::Net;

/// [crate::jit::Expr::Val]
pub unsafe extern "C" fn VAL(ptr: Ptr) -> Val {
  ptr.val()
}

/// [crate::jit::Expr::Tag]
pub unsafe extern "C" fn TAG(ptr: Ptr) -> Tag {
  ptr.tag()
}

/// [crate::jit::Expr::IsNum]
pub unsafe extern "C" fn IS_NUM(ptr: Ptr) -> bool {
  ptr.is_num()
}

/// [crate::jit::Expr::IsSkp]
pub unsafe extern "C" fn IS_SKP(ptr: Ptr) -> bool {
  ptr.is_skp()
}

/// [crate::jit::Expr::NewPtr]
pub unsafe extern "C" fn NEW_PTR(tag: Tag, val: Val) -> Ptr {
  Ptr::new(tag, val)
}

/// [crate::jit::Expr::Alloc]
pub unsafe extern "C" fn ALLOC(net: Net, size: usize) -> Val {
  let net = &mut *net;
  net.alloc(size)
}

/// [crate::jit::Expr::Op]
pub unsafe extern "C" fn OP(net: Net, a: Val, b: Val) -> Val {
  let net = &mut *net;
  net.op(a, b)
}

/// [crate::jit::Expr::GetHeap]
pub unsafe extern "C" fn GET_HEAP(net: Net, idx: Val, port: Port) -> Ptr {
  let net = &mut *net;
  let heap = net.heap.get(idx, port);
  heap
}

/// [crate::jit::Instr::SetHeap]
pub unsafe extern "C" fn SET_HEAP(net: Net, idx: Val, port: Port, val: Ptr) {
  let net = &mut *net;
  net.heap.set(idx, port, val);
}
