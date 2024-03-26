use std::{
  marker::PhantomData,
  sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
  },
};

use crate::{
  ast::Book,
  dispatch_dyn_net,
  host::{DefRef, Host},
  run::{AsDef, Def, DynNetMut, LabSet, Mode, Net, Port, Tag, Trg},
  util::create_var,
};

/// `@IDENTITY = (x x)`
pub const IDENTITY: *const Def = const { &Def::new(LabSet::from_bits(&[1]), (call_identity, call_identity)) }.upcast();

fn call_identity<M: Mode>(net: &mut Net<M>, port: Port) {
  #[cfg(todo)]
  {
    let (a, b) = net.do_ctr(0, Trg::port(port));
    net.link_trg(a, b);
  }
  todo!()
}

/// The definition of `HVM.log`, parameterized by the readback function.
///
/// `@HVM.log ~ (arg out)` waits for `arg` to be normalized, prints it using the
/// supplied readback function, and then reduces to `arg ~ * & out ~ @IDENTITY`.
///
/// The output can therefore either be erased, or used to sequence other
/// operations after the log.

impl<F: Fn(Tree) + Clone + Send + Sync + 'static> LogDef<F> {
  /// # SAFETY
  /// The caller must ensure that the returned value lives at least as long as
  /// the port where it is used.
  pub unsafe fn new(host: Arc<Mutex<Host>>, f: F) -> DefRef {
    #[cfg(todo)]
    HostedDef::new_hosted(LabSet::ALL, LogDef(host, f));
    todo!()
  }
}

pub struct LogDef<F>(Arc<Mutex<Host>>, F);

#[cfg(todo)]
impl<F: Fn(Tree) + Clone + Send + Sync + 'static> AsHostedDef for LogDef<F> {
  fn call<M: Mode>(def: &Def<Self>, net: &mut Net<M>, port: Port) {
    let (arg, seq) = net.do_ctr(0, Trg::port(port));
    let (wire, port) = net.create_wire();
    if M::LAZY {
      net.link_trg_port(arg, port);
      net.link_trg_port(seq, Port::new_ref(unsafe { &*IDENTITY }));
      net.normal_from(wire.clone());
      def.data.1(def.data.0.lock().unwrap().readback_tree(&wire));
      net.link_wire_port(wire, Port::ERA);
    } else {
      // SAFETY: the function inside `readback` won't live longer than
      // the net, and thus won't live longer than the host, where the
      // `&Def<Self>` points to
      let def: &'static Def<Self> = unsafe { core::mem::transmute(def) };
      let readback = readback(def.data.0.clone(), port, |net, tree| {
        (def.data.1)(tree);
        dispatch_dyn_net!(net => {
          net.link_wire_port(wire, Port::ERA);
          net.link_trg_port(seq, Port::new_ref(unsafe { &*IDENTITY }));
        });
      });
      net.link_trg_port(arg, readback);
    }
  }
}

/// Create a `Host` from a `Book`, including `hvm-core`'s built-in definitions
pub fn create_host(book: &Book) -> Arc<Mutex<Host>> {
  let host = Arc::new(Mutex::new(Host::default()));
  // host.lock().unwrap().insert_def("HVM.log", unsafe {
  //   crate::stdlib::LogDef::new(host.clone(), {
  //     move |tree| {
  //       println!("{}", tree);
  //     }
  //   })
  // });
  host.lock().unwrap().insert_def("HVM.black_box", DefRef::Static(unsafe { &*IDENTITY }));
  host.lock().unwrap().insert_book(&book);
  host
}

#[repr(transparent)]
pub struct BoxDef<T: AsBoxDef>(pub T, PhantomData<()>);

impl<T: AsBoxDef> BoxDef<T> {
  pub fn new_boxed(labs: LabSet, data: T) -> Box<Def<Self>> {
    Box::new(Def::new(labs, BoxDef(data, PhantomData)))
  }
  /// SAFETY: if port is a ref, it must be a valid pointer.
  pub unsafe fn try_downcast_box(port: Port) -> Option<Box<Def<Self>>> {
    if port.is(Tag::Ref) {
      if let Some(port) = unsafe { Def::downcast_ptr::<Self>(port.addr().0 as *const _) } {
        Some(unsafe { Box::from_raw(port as *mut Def<Self>) })
      } else {
        None
      }
    } else {
      None
    }
  }
}

pub trait AsBoxDef: Send + Sync + 'static {
  fn call<M: Mode>(slf: Box<Def<Self>>, net: &mut Net<M>, port: Port);
}

impl<T: AsBoxDef> AsDef for BoxDef<T> {
  unsafe fn call<M: Mode>(slf: *const Def<Self>, net: &mut Net<M>, port: Port) {
    T::call(Box::from_raw(slf as *mut _), net, port)
  }
}

#[repr(transparent)]
pub struct ArcDef<T: AsArcDef>(pub T, PhantomData<()>);

impl<T: AsArcDef> ArcDef<T> {
  pub fn new_arc(labs: LabSet, data: T) -> Arc<Def<Self>> {
    Arc::new(Def::new(labs, ArcDef(data, PhantomData)))
  }
  pub fn new_arc_port(labs: LabSet, data: T) -> Port {
    unsafe { Port::new_ref(Arc::into_raw(Arc::new(Def::new(labs, ArcDef(data, PhantomData)))).as_ref().unwrap()) }
  }
  pub fn to_port(slf: Arc<Def<T>>) -> Port {
    unsafe { Port::new_ref(Arc::into_raw(slf).as_ref().unwrap()) }
  }
  /// SAFETY: if port is a ref, it must be a valid pointer.
  pub unsafe fn try_downcast_arc(port: Port) -> Option<Arc<Def<Self>>> {
    if port.is(Tag::Ref) && port != Port::ERA {
      if let Some(port) = unsafe { Def::downcast_ptr::<Self>(port.addr().0 as *const _) } {
        Some(unsafe { Arc::from_raw(port as *mut Def<Self>) })
      } else {
        None
      }
    } else {
      None
    }
  }
}

pub trait AsArcDef: Send + Sync + 'static {
  fn call<M: Mode>(slf: Arc<Def<Self>>, net: &mut Net<M>, port: Port);
}

impl<T: AsArcDef> AsDef for ArcDef<T> {
  unsafe fn call<M: Mode>(slf: *const Def<Self>, net: &mut Net<M>, port: Port) {
    T::call(Arc::from_raw(slf as *mut _), net, port);
  }
}

#[repr(transparent)]
pub struct HostedDef<T: AsHostedDef>(pub T, PhantomData<()>);

impl<T: AsHostedDef> HostedDef<T> {
  pub unsafe fn new_hosted(labs: LabSet, data: T) -> DefRef {
    DefRef::Owned(Box::new(Def::new(labs, HostedDef(data, PhantomData))))
  }
}

pub trait AsHostedDef: Send + Sync + 'static {
  fn call<M: Mode>(slf: &Def<Self>, net: &mut Net<M>, port: Port);
}

impl<T: AsHostedDef> AsDef for HostedDef<T> {
  unsafe fn call<M: Mode>(slf: *const Def<Self>, net: &mut Net<M>, port: Port) {
    T::call((slf as *const Def<T>).as_ref().unwrap(), net, port)
  }
}

use crate::ast::Tree;

#[derive(Clone)]
pub struct UniqueTreePtr(*mut Tree);
unsafe impl Send for UniqueTreePtr {}
unsafe impl Sync for UniqueTreePtr {}

impl UniqueTreePtr {
  unsafe fn to_box(self) -> Box<Tree> {
    Box::from_raw(self.0)
  }
}

pub struct ReadbackDef<F: FnOnce(DynNetMut) + Send + Sync + 'static> {
  root: Arc<F>,
  host: Arc<Mutex<Host>>,
  var_idx: Arc<AtomicUsize>,
  tree: UniqueTreePtr,
  out: Port,
}

impl<F: FnOnce(DynNetMut) + Send + Sync + 'static> ReadbackDef<F> {
  fn maybe_finish<'a, 'b>(net: DynNetMut<'a, 'b>, root: Arc<F>) {
    let Some(root) = Arc::into_inner(root) else { return };
    (root)(net)
  }
  fn with(&self, tree: *mut Tree, out: Port) -> Port {
    Port::new_ref(Box::leak(BoxDef::new_boxed(LabSet::ALL, Self {
      tree: UniqueTreePtr(tree),
      out,
      root: self.root.clone(),
      var_idx: self.var_idx.clone(),
      host: self.host.clone(),
    })))
  }
}

impl<F: FnOnce(DynNetMut) + Send + Sync + 'static> AsBoxDef for ReadbackDef<F> {
  fn call<M: Mode>(def: Box<Def<Self>>, net: &mut Net<M>, port: Port) {
    #[cfg(todo)]
    match port.tag() {
      Tag::Red => {
        unreachable!()
      }
      Tag::Ref if port != Port::ERA => {
        if let Some(other) = unsafe { BoxDef::<Self>::try_downcast_box(port.clone()) } {
          let var = def.data.var_idx.fetch_add(1, Ordering::AcqRel);
          let var = Tree::Var { nam: create_var(var) };
          unsafe {
            (*def.data.tree.0) = var.clone();
            (*other.data.0.tree.0) = var;
          }
          net.link_port_port(def.data.out, other.data.0.out);
          Self::maybe_finish(DynNetMut::from(&mut *net), other.data.0.root);
        } else if let Some(back) = def.data.host.lock().unwrap().back.get(&port.addr()) {
          unsafe { *(def.data.tree.0) = Tree::Ref { nam: back.clone() } };
          net.link_port_port(def.data.out, port);
        } else {
          unsafe { *(def.data.tree.0) = Tree::Era };
          net.link_port_port(def.data.out, port);
        }
      }
      Tag::Ref => {
        unsafe { *(def.data.tree.0) = Tree::Era };
        net.link_port_port(def.data.out, port);
      }
      Tag::Num => {
        unsafe { *(def.data.tree.0) = Tree::Num { val: port.num() } };
        net.link_port_port(def.data.out, port)
      }
      Tag::Mat => {
        unsafe {
          *(def.data.tree.0) =
            Tree::Mat { zero: Box::new(Tree::Era), succ: Box::new(Tree::Era), out: Box::new(Tree::Era) }
        };
        let Tree::Mat { zero, succ, out } = (unsafe { &mut *(def.data.tree.0) }) else { unreachable!() };
        let old = port.clone().consume_node();
        let new = net.create_node(Tag::Mat, old.lab);
        let old_sel = old.p1.load_target().consume_node();
        let new_sel = net.create_node(Tag::Ctr, old_sel.lab);
        net.link_port_port(def.data.out.clone(), new.p0);
        net.link_port_port(new.p1, new_sel.p0);
        net.link_wire_port(old.p2, def.data.with(out.as_mut(), new.p2));
        net.link_wire_port(old_sel.p1, def.data.with(zero.as_mut(), new_sel.p1));
        net.link_wire_port(old_sel.p2, def.data.with(succ.as_mut(), new_sel.p2));
      }
      Tag::Var => net.link_port_port(def.data.out, port),
      tag @ (Tag::Op | Tag::Ctr) => {
        let old = port.clone().consume_node();
        let new = net.create_node(tag, old.lab);
        let (lhs, rhs): (*mut Tree, *mut Tree) = match tag {
          Tag::Op => {
            unsafe {
              *(def.data.tree.0) = Tree::Op { op: port.op(), rhs: Box::new(Tree::Era), out: Box::new(Tree::Era) }
            };
            let Tree::Op { rhs, out, .. } = (unsafe { &mut *(def.data.tree.0) }) else { unreachable!() };
            (rhs.as_mut(), out.as_mut())
          }
          Tag::Ctr => {
            unsafe { *(def.data.tree.0) = Tree::Ctr { lab: port.lab(), ports: vec![Tree::Era, Tree::Era] } };
            let Tree::Ctr { ports, .. } = (unsafe { &mut *(def.data.tree.0) }) else { unreachable!() };
            (&mut ports[0], &mut ports[1])
          }
          _ => unreachable!(),
        };
        net.link_port_port(def.data.out.clone(), new.p0);
        net.link_wire_port(old.p1, def.data.with(lhs, new.p1));
        net.link_wire_port(old.p2, def.data.with(rhs, new.p2));
      }
    }
    Self::maybe_finish(DynNetMut::from(net), def.data.root);
  }
}

// public api:
// readback: Wire -> FnOnce(Tree) -> Port
// readback_and_wait: Net -> Wire -> Tree

pub fn readback(host: Arc<Mutex<Host>>, out: Port, f: impl FnOnce(DynNetMut, Tree) + Send + Sync + 'static) -> Port {
  let root = Box::leak(Box::new(Tree::default()));

  let root_ptr = UniqueTreePtr(root);
  let root = UniqueTreePtr(root);

  use crate::run;
  let closure: Box<
    dyn for<'a, 'b> FnOnce(run::DynNetInner<&'a mut run::Net<'b, run::Lazy>, &'a mut run::Net<'b, run::Strict>>)
      + Send
      + Sync
      + 'static,
  > = Box::new(move |net| {
    let root = unsafe { root.to_box() };
    f(net, *root);
  });
  Port::new_ref(Box::leak(BoxDef::new_boxed(LabSet::ALL, ReadbackDef {
    root: Arc::new(closure),
    host,
    tree: root_ptr,
    var_idx: Arc::new(AtomicUsize::from(0)),
    out,
  })))
}
