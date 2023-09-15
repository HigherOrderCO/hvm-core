use hvm_core::{LNet, LTree, Tag};
use std::collections::HashMap;

#[derive(Clone, Debug)]
/// Net representation used only as an intermediate for converting to hvm-core format
pub struct INet {
    pub nodes: Vec<NodeVal>,
}

pub type NodeVal = u32;
pub type NodeKind = NodeVal;
pub type Port = NodeVal;
pub type NodeId = NodeVal;
pub type SlotId = NodeVal;

/// The ROOT port is on the deadlocked root node at address 0.
pub const ROOT: Port = 1;
pub const TAG_WIDTH: u32 = 4;
pub const TAG: u32 = NodeVal::BITS - TAG_WIDTH;
pub const ERA: NodeKind = 0 << TAG;
pub const CON: NodeKind = 1 << TAG;
pub const DUP: NodeKind = 2 << TAG;
pub const REF: NodeKind = 3 << TAG;
pub const NUM: NodeKind = 4 << TAG;
pub const NUMOP: NodeKind = 5 << TAG;
pub const LABEL_MASK: NodeKind = (1 << TAG) - 1;
pub const TAG_MASK: NodeKind = !LABEL_MASK;

/// Builds a port (an address / slot pair).
pub fn port(node: NodeId, slot: SlotId) -> Port {
    (node << 2) | slot
}

/// Returns the address of a port (TODO: rename).
pub fn addr(port: Port) -> NodeId {
    port >> 2
}

/// Returns the slot of a port.
pub fn slot(port: Port) -> SlotId {
    port & 3
}

/// Enters a port, returning the port on the other side.
pub fn enter(inet: &INet, port: Port) -> Port {
    inet.nodes[port as usize]
}

/// Kind of the node.
pub fn kind(inet: &INet, node: NodeId) -> NodeKind {
    inet.nodes[port(node, 3) as usize]
}

pub enum CompatError {
    CycleDetected,
}

pub fn compat_net_to_core(inet: &INet) -> Result<LNet, CompatError> {
    let (root_root, acts_roots) = get_tree_roots(inet)?;
    let mut port_to_var_id: HashMap<Port, VarId> = HashMap::new();
    let root = if let Some(root_root) = root_root {
        // If there is a root tree connected to the root node
        compat_tree_to_hvm_tree(inet, root_root, &mut port_to_var_id)
    } else {
        // If the root node points to some aux port (application)
        port_to_var_id.insert(enter(inet, ROOT), 0);
        LTree::Var {
            nam: var_id_to_name(0),
        }
    };
    let mut acts = vec![];
    for [root0, root1] in acts_roots {
        let act0 = compat_tree_to_hvm_tree(inet, root0, &mut port_to_var_id);
        let act1 = compat_tree_to_hvm_tree(inet, root1, &mut port_to_var_id);
        acts.push((act0, act1));
    }
    Ok(LNet { root, acts })
}

type VarId = NodeId;

/// Returns a list of all the tree node roots in the compat inet.
fn get_tree_roots(inet: &INet) -> Result<(Option<NodeId>, Vec<[NodeId; 2]>), CompatError> {
    let mut acts_roots: Vec<[NodeId; 2]> = vec![];
    let mut explored_nodes = vec![false; inet.nodes.len() / 4];
    let mut side_links: Vec<Port> = vec![]; // Links between trees

    // Start by checking the root tree (if any)
    explored_nodes[addr(ROOT) as usize] = true;
    let root_link = enter(inet, ROOT);
    let root_root = if slot(root_link) == 0 {
        // If the root node is connected to a main port, we have a root tree
        let root_node = addr(root_link);
        go_down_tree(inet, root_node, &mut explored_nodes, &mut side_links);
        Some(root_node)
    } else {
        // Otherwise, root node connected to an aux port, no root tree.
        side_links.push(root_link);
        None
    };

    // Check each side-link for a possible new tree pair;
    while let Some(dest_port) = side_links.pop() {
        let dest_node = addr(dest_port);
        // Only go up unmarked trees
        if !explored_nodes[dest_node as usize] {
            let new_roots = go_up_tree(inet, dest_node)?;
            go_down_tree(inet, new_roots[0], &mut explored_nodes, &mut side_links);
            go_down_tree(inet, new_roots[1], &mut explored_nodes, &mut side_links);
            acts_roots.push(new_roots);
        }
    }

    Ok((root_root, acts_roots))
}

/// Go down a node tree, marking all nodes with the tree_id and storing any side_links found.
fn go_down_tree(
    inet: &INet,
    root: NodeId,
    explored_nodes: &mut [bool],
    side_links: &mut Vec<Port>,
) {
    debug_assert!(!explored_nodes[root as usize], "Explored same tree twice");
    let mut nodes_to_check = vec![root];
    while let Some(node) = nodes_to_check.pop() {
        debug_assert!(!explored_nodes[node as usize]);
        explored_nodes[node as usize] = true;
        for down_slot in [1, 2] {
            let down_port = enter(inet, port(node, down_slot));
            if slot(down_port) == 0 {
                // If this down-link is to a main port, this is a node of the same tree
                nodes_to_check.push(addr(down_port));
            } else {
                // Otherwise it's a side-link
                side_links.push(down_port);
            }
        }
    }
}

/// Goes up a node tree, starting from some given node.
/// Returns the root of this tree and the root of its active pair.
fn go_up_tree(inet: &INet, start_node: NodeId) -> Result<[NodeId; 2], CompatError> {
    let mut crnt_node = start_node;
    let mut explored_nodes = vec![false; inet.nodes.len() / 4];
    loop {
        if !explored_nodes[crnt_node as usize] {
            explored_nodes[crnt_node as usize] = true;
        } else {
            return Err(CompatError::CycleDetected);
        }
        let up_port = enter(inet, port(crnt_node, 0));
        let up_node = addr(up_port);
        if slot(up_port) == 0 {
            return Ok([crnt_node, up_node]);
        } else {
            crnt_node = up_node;
        }
    }
}

fn compat_tree_to_hvm_tree(
    inet: &INet,
    root: NodeId,
    port_to_var_id: &mut HashMap<Port, VarId>,
) -> LTree {
    let kind = kind(inet, root);
    let tag = kind & TAG_MASK;
    let label = kind & LABEL_MASK; // TODO: Check if label too high, do something about it.
    match tag {
        ERA => LTree::Era,
        CON => LTree::Nod {
            tag: hvm_core::CON,
            lft: Box::new(var_or_subtree(inet, port(root, 1), port_to_var_id)),
            rgt: Box::new(var_or_subtree(inet, port(root, 2), port_to_var_id)),
        },
        DUP => LTree::Nod {
            tag: hvm_core::DUP + label as Tag,
            lft: Box::new(var_or_subtree(inet, port(root, 1), port_to_var_id)),
            rgt: Box::new(var_or_subtree(inet, port(root, 2), port_to_var_id)),
        },
        REF => LTree::Ref { nam: label },
        NUM => LTree::NUM { val: label },
        NUMOP => todo!(),
        _ => unreachable!("Invalid tag in compat tree {tag:x}"),
    }
}

fn var_or_subtree(inet: &INet, src_port: Port, port_to_var_id: &mut HashMap<Port, VarId>) -> LTree {
    let dst_port = enter(inet, src_port);
    if slot(dst_port) == 0 {
        // Subtree
        compat_tree_to_hvm_tree(inet, addr(dst_port), port_to_var_id)
    } else {
        // Var
        if let Some(&var_id) = port_to_var_id.get(&src_port) {
            // Previously found var
            LTree::Var {
                nam: var_id_to_name(var_id),
            }
        } else {
            // New var
            let var_id = port_to_var_id.len() as VarId;
            port_to_var_id.insert(dst_port, var_id);
            LTree::Var {
                nam: var_id_to_name(var_id),
            }
        }
    }
}

fn var_id_to_name(mut var_id: VarId) -> String {
    let mut name = String::new();
    loop {
        let c = (var_id % 26) as u8 + b'a';
        name.push(c as char);
        var_id /= 26;
        if var_id == 0 {
            break;
        }
    }
    name
    // format!("x{var_id}")
}
