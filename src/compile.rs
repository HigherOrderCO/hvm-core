use crate::{
  host::Host,
  run::{Instruction, InterpretedDef, LabSet, Port, Tag},
  stdlib::HostedDef,
};
use std::{
  fmt::{self, Write},
  hash::{DefaultHasher, Hasher},
};

/// Compiles a [`Host`] to Rust, returning a file to replace `gen.rs`.
pub fn compile_host(host: &Host) -> String {
  _compile_host(host).unwrap()
}

fn _compile_host(host: &Host) -> Result<String, fmt::Error> {
  let mut code = String::default();

  let defs = host
    .defs
    .iter()
    .filter_map(|(name, def)| Some((name, def.downcast_ref::<HostedDef<InterpretedDef>>()?)))
    .map(|(raw_name, def)| (raw_name, sanitize_name(raw_name), def));

  writeln!(code, "#![allow(non_upper_case_globals, unused_imports)]")?;
  writeln!(code, "use crate::{{host::{{Host, DefRef}}, run::*, ops::Op::*}};")?;
  writeln!(code)?;

  writeln!(code, "pub fn host() -> Host {{")?;
  writeln!(code, "  let mut host = Host::default();")?;
  for (raw_name, name, _) in defs.clone() {
    writeln!(code, r##"  host.insert_def(r#"{raw_name}"#, DefRef::Static(unsafe {{ &*DEF_{name} }}));"##)?;
  }
  writeln!(code, "  host")?;
  writeln!(code, "}}\n")?;

  for (_, name, def) in defs.clone() {
    let labs = compile_lab_set(&def.labs)?;
    writeln!(
      code,
      "pub const DEF_{name}: *const Def = const {{ &Def::new({labs}, (call_{name}, call_{name})) }}.upcast();"
    )?;
  }

  writeln!(code)?;

  for (_, name, def) in defs {
    compile_def(&mut code, host, &name, &def.data.0.instr)?;
  }

  Ok(code)
}

fn compile_def(code: &mut String, host: &Host, name: &str, instr: &[Instruction]) -> fmt::Result {
  writeln!(code, "pub fn call_{name}<M: Mode>(net: &mut Net<M>, to: Port) {{")?;
  writeln!(code, "  let t0 = Trg::port(to);")?;
  for instr in instr {
    write!(code, "  ")?;
    match instr {
      Instruction::Const { trg, port } => {
        writeln!(code, "let {trg} = Trg::port({});", compile_port(host, port))
      }
      Instruction::Link { a, b } => {
        writeln!(code, "net.link_trg({a}, {b});")
      }
      Instruction::LinkConst { trg, port } => {
        writeln!(code, "net.link_trg({trg}, Trg::port({}));", compile_port(host, port))
      }
      Instruction::Ctr2 { lab, trg, lft, rgt } => {
        writeln!(code, "let ({lft}, {rgt}) = net.do_ctr({lab}, {trg});")
      }
      Instruction::Op { op, trg, rhs, out } => {
        writeln!(code, "let ({rhs}, {out}) = net.do_op({op:?}, {trg});")
      }
      Instruction::OpNum { op, trg, rhs, out } => {
        writeln!(code, "let {out} = net.do_op_num({op:?}, {trg}, {rhs});")
      }
      Instruction::Mat { trg, zero, succ, out } => {
        writeln!(code, "let ({zero}, {succ}, {out}) = net.do_mat({trg});")
      }
      Instruction::Wires { av, aw, bv, bw } => {
        writeln!(code, "let ({av}, {aw}, {bv}, {bw}) = net.do_wires();")
      }
      _ => todo!(),
    }?;
  }
  writeln!(code, "}}")?;
  code.write_char('\n')?;

  Ok(())
}

fn compile_port(host: &Host, port: &Port) -> String {
  if port == &Port::ERA {
    "Port::ERA".to_owned()
  } else if port.tag() == Tag::Ref {
    let name = sanitize_name(&host.back[&port.addr()]);
    format!("Port::new_ref(unsafe {{ &*DEF_{name} }})")
  } else if port.tag() == Tag::Num {
    format!("Port::new_num({})", port.num())
  } else {
    unreachable!()
  }
}

/// Adapts `name` to be a valid suffix for a rust identifier, if necessary.
fn sanitize_name(name: &str) -> String {
  if !name.contains('.') && !name.contains('$') {
    name.to_owned()
  } else {
    // Append a hash to the name to avoid clashes between `foo.bar` and `foo_bar`.
    let mut hasher = DefaultHasher::new();
    hasher.write(name.as_bytes());
    let hash = hasher.finish();
    let mut sanitized = name.replace(|c| c == '.' || c == '$', "_");
    sanitized.push_str("__");
    write!(sanitized, "__{:016x}", hash).unwrap();
    sanitized
  }
}

fn compile_lab_set(labs: &LabSet) -> Result<String, fmt::Error> {
  if labs == &LabSet::ALL {
    return Ok("LabSet::ALL".to_owned());
  }
  if labs == &LabSet::NONE {
    return Ok("LabSet::NONE".to_owned());
  }
  let mut str = "LabSet::from_bits(&[".to_owned();
  for (i, word) in labs.bits.iter().enumerate() {
    if i != 0 {
      write!(str, ", ")?;
    }
    write!(str, "0x{:x}", word)?;
  }
  str.push_str("])");
  Ok(str)
}
