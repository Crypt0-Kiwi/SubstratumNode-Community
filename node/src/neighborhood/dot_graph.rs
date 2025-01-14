// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.

use crate::sub_lib::cryptde::PublicKey;
use crate::sub_lib::node_addr::NodeAddr;

pub trait DotRenderable {
    fn render(&self) -> String;
}

pub struct NodeRenderable {
    pub version: Option<u32>,
    pub public_key: PublicKey,
    pub node_addr: Option<NodeAddr>,
    pub known_source: bool,
    pub known_target: bool,
    pub is_present: bool,
}

impl DotRenderable for NodeRenderable {
    fn render(&self) -> String {
        let mut result = String::new();
        result.push_str(&format!("\"{}\"", self.public_key));
        result.push_str(&self.render_label());
        if !self.is_present {
            result.push_str(" [shape=none]")
        } else if self.known_target {
            result.push_str(" [shape=box]")
        }
        if self.known_source {
            result.push_str(" [style=filled]")
        }
        result.push_str(";");
        result
    }
}

impl NodeRenderable {
    fn render_label(&self) -> String {
        let version_string = match self.version {
            Some(v) => format!("v{}\\n", v),
            None => String::new(),
        };
        let public_key_str = format!("{}", self.public_key);
        let public_key_trunc = if public_key_str.len() > 8 {
            &public_key_str[0..8]
        } else {
            &public_key_str
        };
        let node_addr_string = match self.node_addr {
            None => String::new(),
            Some(ref na) => format!("\\n{}", na),
        };

        format!(
            " [label=\"{}{}{}\"]",
            version_string, public_key_trunc, node_addr_string,
        )
    }
}

pub struct EdgeRenderable {
    pub from: PublicKey,
    pub to: PublicKey,
}

impl DotRenderable for EdgeRenderable {
    fn render(&self) -> String {
        let mut result = String::new();
        result.push_str(&format!("\"{}\" -> \"{}\";", self.from, self.to));
        result
    }
}

pub fn render_dot_graph(renderables: Vec<Box<dyn DotRenderable>>) -> String {
    let mut result = String::from("digraph db {");
    for renderable in renderables {
        result.push_str(&format!(" {}", renderable.render()))
    }
    result.push_str(" }");
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_string_contains;

    #[test]
    fn truncation_works_for_long_keys() {
        let public_key = PublicKey::new(&b"ABCDEFGHIJKLMNOPQRSTUVWXYZ"[..]);
        let public_key_64 = format!("{}", public_key);
        let public_key_trunc = String::from(&public_key_64[0..8]);
        let node = NodeRenderable {
            version: Some(1),
            public_key: public_key.clone(),
            node_addr: None,
            known_source: false,
            known_target: false,
            is_present: true,
        };

        let result = render_dot_graph(vec![Box::new(node)]);

        assert_string_contains(
            &result,
            &format!(
                "\"{}\" [label=\"v1\\n{}\"];",
                public_key_64, public_key_trunc
            ),
        );
    }

    #[test]
    fn truncation_works_for_short_keys() {
        let public_key = PublicKey::new(&b"ABC"[..]);
        let public_key_64 = format!("{}", public_key);
        let node = NodeRenderable {
            version: Some(1),
            public_key: public_key.clone(),
            node_addr: None,
            known_source: false,
            known_target: false,
            is_present: true,
        };

        let result = render_dot_graph(vec![Box::new(node)]);

        assert_string_contains(
            &result,
            &format!("\"{}\" [label=\"v1\\n{}\"];", public_key_64, public_key_64),
        );
    }
}
