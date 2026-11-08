/// Minimal DTB encoder: tests exercise the real fdt parser and boot planner.
pub struct Node {
    name: &'static str,
    properties: Vec<(&'static str, Vec<u8>)>,
    children: Vec<Node>,
}

impl Node {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            properties: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn property(mut self, name: &'static str, value: impl Into<Vec<u8>>) -> Self {
        self.properties.push((name, value.into()));
        self
    }

    pub fn child(mut self, child: Self) -> Self {
        self.children.push(child);
        self
    }

    pub fn compatible(self, compatible: &str) -> Self {
        self.property("compatible", format!("{compatible}\0").into_bytes())
    }

    pub fn reg(self, base: u64, size: u64) -> Self {
        self.property("reg", [base.to_be_bytes(), size.to_be_bytes()].concat())
    }

    pub fn cells(self) -> Self {
        self.property("#address-cells", 2u32.to_be_bytes())
            .property("#size-cells", 2u32.to_be_bytes())
    }

    pub fn encode(self) -> Vec<u8> {
        self.encode_reserved(&[])
    }

    pub fn encode_reserved(self, reservations: &[(u64, u64)]) -> Vec<u8> {
        let mut structure = Vec::new();
        let mut strings = Vec::new();
        self.write(&mut structure, &mut strings);
        structure.extend(9u32.to_be_bytes());
        // Ten-word header, one empty reservation-map entry, structure, strings.
        let structure_offset = 56 + reservations.len() * 16;
        let strings_offset = structure_offset + structure.len();
        let size = strings_offset + strings.len();
        let mut bytes = [
            0xd00dfeed,
            size as u32,
            structure_offset as u32,
            strings_offset as u32,
            40,
            17,
            16,
            0,
            strings.len() as u32,
            structure.len() as u32,
        ]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect::<Vec<_>>();
        for (base, size) in reservations {
            bytes.extend(base.to_be_bytes());
            bytes.extend(size.to_be_bytes());
        }
        bytes.extend([0; 16]);
        bytes.extend(structure);
        bytes.extend(strings);
        bytes
    }

    fn write(self, structure: &mut Vec<u8>, strings: &mut Vec<u8>) {
        structure.extend(1u32.to_be_bytes());
        structure.extend(self.name.as_bytes());
        structure.push(0);
        pad(structure);
        for (name, value) in self.properties {
            structure.extend(3u32.to_be_bytes());
            structure.extend((value.len() as u32).to_be_bytes());
            structure.extend((strings.len() as u32).to_be_bytes());
            strings.extend(name.as_bytes());
            strings.push(0);
            structure.extend(value);
            pad(structure);
        }
        for child in self.children {
            child.write(structure, strings);
        }
        structure.extend(2u32.to_be_bytes());
    }
}

fn pad(bytes: &mut Vec<u8>) {
    bytes.resize(bytes.len().next_multiple_of(4), 0);
}
