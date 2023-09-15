mod compat;

use compat::CompatError;
use hvm_core::{
    core::Net,
    lang::{lnet_to_net, LNet},
};
use quickcheck::Arbitrary;
use std::ops::Range;

pub type NodeId = usize;
pub type PortId = usize;

#[derive(Clone, Debug)]
struct SimpleNet(Vec<(NodeTag, [Address; 3])>);

#[derive(Clone, Debug, Default, Copy)]
pub struct Address {
    pub node_id: NodeId,
    pub port_id: PortId,
}

#[derive(Clone, Debug, Copy)]
pub enum NodeTag {
    Era,
    Con { tag: u16 },
    Dup { tag: u16 },
}

impl NodeTag {
    fn arity(&self) -> usize {
        match self {
            NodeTag::Era => 1,
            _ => 3,
        }
    }
}

impl Arbitrary for NodeTag {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let era = NodeTag::Era;
        let con = NodeTag::Con {
            tag: u16::arbitrary(g),
        };
        let dup = NodeTag::Dup {
            tag: u16::arbitrary(g),
        };
        *g.choose(&[era, con, dup]).unwrap()
    }
}

impl Arbitrary for SimpleNet {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        fn choose_from_range(g: &mut quickcheck::Gen, range: Range<usize>) -> usize {
            let collected = range.clone().collect::<Vec<_>>();
            let slice = &collected.as_slice();
            let chosen = g.choose(slice);
            *chosen.unwrap()
        }
        let mut nodes: Vec<NodeTag> = Arbitrary::arbitrary(g);
        fn number_of_ports(tags: &[NodeTag]) -> usize {
            tags.iter().map(|x| x.arity()).sum()
        }
        if nodes.is_empty() {
            nodes.push(Arbitrary::arbitrary(g));
            nodes.push(Arbitrary::arbitrary(g));
        }
        if number_of_ports(&nodes) % 2 != 0 {
            nodes.push(Arbitrary::arbitrary(g));
        }
        assert!(number_of_ports(&nodes) % 2 == 0);

        let mut addresses: Vec<[Option<Address>; 3]> = vec![[None; 3]; nodes.len()];

        fn link(addresses: &mut [[Option<Address>; 3]], from: (usize, usize), to: (usize, usize)) {
            assert!(addresses[from.0][from.1].is_none());
            assert!(addresses[to.0][to.1].is_none());
            addresses[from.0][from.1] = Some(Address {
                node_id: to.0,
                port_id: to.1,
            });
            addresses[to.0][to.1] = Some(Address {
                node_id: from.0,
                port_id: from.1,
            });
        }

        fn find_free_port(
            nodes: &Vec<NodeTag>,
            addresses: &[[Option<Address>; 3]],
            g: &mut quickcheck::Gen,
            (curr_idx, curr_port): (usize, usize),
        ) -> (usize, usize) {
            loop {
                let node_id = choose_from_range(g, curr_idx..(nodes.len()));
                let other_tag = nodes[node_id];
                let other_port = choose_from_range(g, 0..other_tag.arity());
                if node_id == curr_idx && other_port == curr_port {
                    continue;
                }
                if addresses[node_id][other_port].is_some() {
                    continue;
                }
                return (node_id, other_port);
            }
        }

        link(&mut addresses, (0, 0), (0, 2));
        let root_other_port = find_free_port(&nodes, &addresses, g, (0, 1));
        link(&mut addresses, (0, 1), root_other_port);

        for (curr_idx, this_tag) in nodes.clone().into_iter().enumerate().skip(1) {
            if this_tag.arity() == 1 {
                link(&mut addresses, (curr_idx, 1), (curr_idx, 2));
            }

            for this_port in 0..this_tag.arity() {
                if addresses[curr_idx][this_port].is_some() {
                    continue;
                }

                let other_port = find_free_port(&nodes, &addresses, g, (curr_idx, this_port));
                link(&mut addresses, (curr_idx, this_port), other_port);
            }
        }

        let addresses = addresses
            .into_iter()
            .map(|x| [x[0].unwrap(), x[1].unwrap(), x[2].unwrap()])
            .collect::<Vec<[Address; 3]>>();

        fn address_eq(a: Address, node_id: usize, port_id: usize) -> bool {
            a.node_id == node_id && a.port_id == port_id
        }
        fn check_well_connected(addresses: &[[Address; 3]]) -> bool {
            addresses
                .iter()
                .enumerate()
                .all(|(idx, [main, aux1, aux2])| {
                    address_eq(addresses[main.node_id][main.port_id], idx, 0)
                        && address_eq(addresses[aux1.node_id][aux1.port_id], idx, 1)
                        && address_eq(addresses[aux2.node_id][aux2.port_id], idx, 2)
                })
        }

        assert!(check_well_connected(&addresses));
        SimpleNet(nodes.into_iter().zip(addresses).collect())
    }
}

impl TryFrom<SimpleNet> for LNet {
    type Error = CompatError;

    fn try_from(SimpleNet(net): SimpleNet) -> Result<Self, Self::Error> {
        use compat::{compat_net_to_core, port, INet, NodeKind, Port, CON, DUP, ERA};
        let mut nodes = Vec::with_capacity(net.len() * 4);
        for (tag, [main, aux1, aux2]) in net {
            fn convert_address(address: Address) -> Port {
                port(address.node_id as u32, address.port_id as u32)
            }
            fn convert_tag(tag: NodeTag) -> NodeKind {
                match tag {
                    NodeTag::Era => ERA,
                    NodeTag::Con { .. } => CON,
                    NodeTag::Dup { .. } => DUP,
                }
            }
            nodes.push(convert_address(main));
            nodes.push(convert_address(aux1));
            nodes.push(convert_address(aux2));
            nodes.push(convert_tag(tag));
        }

        compat_net_to_core(&INet { nodes })
    }
}

// Wrapper for Net so that we can implement Arbitrary for it.
#[derive(Clone, Debug)]
pub struct NetWrapper(pub Net);

impl From<NetWrapper> for Net {
    fn from(NetWrapper(net): NetWrapper) -> Self {
        net
    }
}

impl From<Net> for NetWrapper {
    fn from(net: Net) -> Self {
        NetWrapper(net)
    }
}

impl Arbitrary for NetWrapper {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        loop {
            let simple_net = SimpleNet::arbitrary(g);
            match LNet::try_from(simple_net.clone()) {
                Ok(lnet) => return lnet_to_net(&lnet, 4).into(),
                Err(_) => continue,
            }
        }
    }
}
