use std::io::Write;

use byteorder::{LittleEndian, WriteBytesExt};

use crate::error::{Error, Result};

const HEADER_MAGIC: &[u8; 23] = b"Kaydara FBX Binary  \x00\x1a\x00";
const FBX_VERSION: u32 = 7400;
const FOOTER_MAGIC_TAIL: [u8; 16] = [
    0xF8, 0x5A, 0x8C, 0x6A, 0xDE, 0xF5, 0xD9, 0x7E, 0xEC, 0xE9, 0x0C, 0xE3, 0x75, 0x8F, 0x29, 0x0B,
];
const FOOTER_PRE_KEY: [u8; 16] = [
    0x58, 0xAB, 0xA9, 0xF0, 0x6C, 0xA2, 0xD8, 0x3F, 0x4D, 0x47, 0x49, 0xA3, 0xB4, 0xB2, 0xE7, 0x3D,
];

#[derive(Debug, Clone)]
pub enum FbxProperty {
    Bool(bool),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    String(Vec<u8>),
    Raw(Vec<u8>),
    BoolArray(Vec<bool>),
    I32Array(Vec<i32>),
    I64Array(Vec<i64>),
    F32Array(Vec<f32>),
    F64Array(Vec<f64>),
}

impl FbxProperty {
    pub fn str(s: &str) -> Self {
        FbxProperty::String(s.as_bytes().to_vec())
    }

    /// Build a binary-FBX object name string in the canonical
    /// `name\x00\x01Class` form. Blender's importer
    /// (`io_scene_fbx/import_fbx.py::elem_split_name_class`) does
    /// `elem.props[-2].split(b'\x00\x01')` and raises
    /// `ValueError: not enough values to unpack` if the separator is
    /// missing — which the older `Class::Name` form we emitted does.
    pub fn obj_name(name: &str, class: &str) -> Self {
        let mut v = Vec::with_capacity(name.len() + 2 + class.len());
        v.extend_from_slice(name.as_bytes());
        v.push(0x00);
        v.push(0x01);
        v.extend_from_slice(class.as_bytes());
        FbxProperty::String(v)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FbxNode {
    pub name: String,
    pub properties: Vec<FbxProperty>,
    pub children: Vec<FbxNode>,
}

impl FbxNode {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            properties: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn with_prop(mut self, prop: FbxProperty) -> Self {
        self.properties.push(prop);
        self
    }

    pub fn with_props(mut self, props: Vec<FbxProperty>) -> Self {
        self.properties.extend(props);
        self
    }

    pub fn push_prop(&mut self, prop: FbxProperty) -> &mut Self {
        self.properties.push(prop);
        self
    }

    pub fn push_child(&mut self, child: FbxNode) -> &mut Self {
        self.children.push(child);
        self
    }

    pub fn push_str_prop(&mut self, value: &str) -> &mut Self {
        self.properties.push(FbxProperty::String(value.as_bytes().to_vec()));
        self
    }
}

pub fn serialize_fbx_binary(root_children: &[FbxNode]) -> Result<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
    out.extend_from_slice(HEADER_MAGIC);
    out.write_u32::<LittleEndian>(FBX_VERSION)
        .map_err(|e| Error::GltfWrite(format!("fbx-binary header: {e}")))?;

    for node in root_children {
        write_node(&mut out, node)?;
    }
    write_null_record(&mut out)?;

    write_footer(&mut out)?;
    Ok(out)
}

fn write_node(out: &mut Vec<u8>, node: &FbxNode) -> Result<()> {
    let end_offset_pos = out.len();
    out.write_u32::<LittleEndian>(0)
        .map_err(|e| Error::GltfWrite(format!("fbx-binary node end_offset: {e}")))?;
    out.write_u32::<LittleEndian>(node.properties.len() as u32)
        .map_err(|e| Error::GltfWrite(format!("fbx-binary num props: {e}")))?;
    let prop_list_len_pos = out.len();
    out.write_u32::<LittleEndian>(0)
        .map_err(|e| Error::GltfWrite(format!("fbx-binary prop_list_len: {e}")))?;
    let name_bytes = node.name.as_bytes();
    if name_bytes.len() > 255 {
        return Err(Error::GltfWrite(format!(
            "fbx-binary node name too long: {}",
            node.name
        )));
    }
    out.push(name_bytes.len() as u8);
    out.extend_from_slice(name_bytes);

    let prop_start = out.len();
    for prop in &node.properties {
        write_property(out, prop)?;
    }
    let prop_end = out.len();
    let prop_list_len = (prop_end - prop_start) as u32;

    let has_children = !node.children.is_empty();
    for child in &node.children {
        write_node(out, child)?;
    }
    if has_children {
        write_null_record(out)?;
    }

    let end_offset = out.len() as u32;
    out[end_offset_pos..end_offset_pos + 4]
        .copy_from_slice(&end_offset.to_le_bytes());
    out[prop_list_len_pos..prop_list_len_pos + 4]
        .copy_from_slice(&prop_list_len.to_le_bytes());
    Ok(())
}

fn write_null_record(out: &mut Vec<u8>) -> Result<()> {
    for _ in 0..13 {
        out.push(0);
    }
    Ok(())
}

fn write_property(out: &mut Vec<u8>, prop: &FbxProperty) -> Result<()> {
    match prop {
        FbxProperty::Bool(v) => {
            out.push(b'C');
            out.push(if *v { 1 } else { 0 });
        }
        FbxProperty::I16(v) => {
            out.push(b'Y');
            out.write_i16::<LittleEndian>(*v).map_err(io)?;
        }
        FbxProperty::I32(v) => {
            out.push(b'I');
            out.write_i32::<LittleEndian>(*v).map_err(io)?;
        }
        FbxProperty::I64(v) => {
            out.push(b'L');
            out.write_i64::<LittleEndian>(*v).map_err(io)?;
        }
        FbxProperty::F32(v) => {
            out.push(b'F');
            out.write_f32::<LittleEndian>(*v).map_err(io)?;
        }
        FbxProperty::F64(v) => {
            out.push(b'D');
            out.write_f64::<LittleEndian>(*v).map_err(io)?;
        }
        FbxProperty::String(bytes) => {
            out.push(b'S');
            out.write_u32::<LittleEndian>(bytes.len() as u32).map_err(io)?;
            out.extend_from_slice(bytes);
        }
        FbxProperty::Raw(bytes) => {
            out.push(b'R');
            out.write_u32::<LittleEndian>(bytes.len() as u32).map_err(io)?;
            out.extend_from_slice(bytes);
        }
        FbxProperty::BoolArray(values) => {
            out.push(b'b');
            write_array_header(out, values.len(), values.len())?;
            for v in values {
                out.push(if *v { 1 } else { 0 });
            }
        }
        FbxProperty::I32Array(values) => {
            out.push(b'i');
            write_array_header(out, values.len(), values.len() * 4)?;
            for v in values {
                out.write_i32::<LittleEndian>(*v).map_err(io)?;
            }
        }
        FbxProperty::I64Array(values) => {
            out.push(b'l');
            write_array_header(out, values.len(), values.len() * 8)?;
            for v in values {
                out.write_i64::<LittleEndian>(*v).map_err(io)?;
            }
        }
        FbxProperty::F32Array(values) => {
            out.push(b'f');
            write_array_header(out, values.len(), values.len() * 4)?;
            for v in values {
                out.write_f32::<LittleEndian>(*v).map_err(io)?;
            }
        }
        FbxProperty::F64Array(values) => {
            out.push(b'd');
            write_array_header(out, values.len(), values.len() * 8)?;
            for v in values {
                out.write_f64::<LittleEndian>(*v).map_err(io)?;
            }
        }
    }
    Ok(())
}

fn write_array_header(out: &mut Vec<u8>, count: usize, byte_size: usize) -> Result<()> {
    out.write_u32::<LittleEndian>(count as u32).map_err(io)?;
    out.write_u32::<LittleEndian>(0).map_err(io)?;
    out.write_u32::<LittleEndian>(byte_size as u32).map_err(io)?;
    Ok(())
}

fn io(e: std::io::Error) -> Error {
    Error::GltfWrite(format!("fbx-binary write: {e}"))
}

fn write_footer(out: &mut Vec<u8>) -> Result<()> {
    out.extend_from_slice(&FOOTER_PRE_KEY);
    while out.len() % 16 != 0 {
        out.push(0);
    }
    for _ in 0..4 {
        out.push(0);
    }
    out.write_u32::<LittleEndian>(FBX_VERSION).map_err(io)?;
    for _ in 0..120 {
        out.push(0);
    }
    out.extend_from_slice(&FOOTER_MAGIC_TAIL);
    Ok(())
}

pub fn write_node_to<W: Write>(_w: &mut W, _node: &FbxNode) -> Result<()> {
    unreachable!()
}
