use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct IniFile {
    pub signature: Option<String>,
    pub output_channels: OutputChannels,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputChannels {
    pub och_block_size: u16,
    pub fields: Vec<OutputChannelField>,
    #[serde(skip)]
    pub by_name: HashMap<String, usize>,
}

impl OutputChannels {
    pub fn index_fields(&mut self) {
        self.by_name.clear();
        for (i, f) in self.fields.iter().enumerate() {
            self.by_name.insert(f.name.clone(), i);
        }
    }

    pub fn field(&self, name: &str) -> Option<&OutputChannelField> {
        self.by_name.get(name).map(|&i| &self.fields[i])
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputChannelField {
    pub name: String,
    pub kind: FieldKind,
}

#[derive(Debug, Clone, Serialize)]
pub enum FieldKind {
    Scalar(ScalarField),
    Bits(BitsField),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ScalarType {
    U08,
    S08,
    U16,
    S16,
    U32,
    S32,
    F32,
}

impl ScalarType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "U08" | "U8" => Some(Self::U08),
            "S08" | "S8" => Some(Self::S08),
            "U16" => Some(Self::U16),
            "S16" => Some(Self::S16),
            "U32" => Some(Self::U32),
            "S32" => Some(Self::S32),
            "F32" => Some(Self::F32),
            _ => None,
        }
    }

    pub fn size_bytes(self) -> usize {
        match self {
            Self::U08 | Self::S08 => 1,
            Self::U16 | Self::S16 => 2,
            Self::U32 | Self::S32 => 4,
            Self::F32 => 4,
        }
    }

    pub fn is_signed(self) -> bool {
        matches!(self, Self::S08 | Self::S16 | Self::S32)
    }

    pub fn is_float(self) -> bool {
        matches!(self, Self::F32)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ScalarField {
    pub ty: ScalarType,
    pub offset: u32,
    pub units: String,
    pub scale: f64,
    pub translate: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BitsField {
    pub ty: ScalarType,
    pub offset: u32,
    pub bit_low: u8,
    pub bit_high: u8,
}
