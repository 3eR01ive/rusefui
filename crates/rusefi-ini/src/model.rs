use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct IniFile {
    pub signature: Option<String>,
    pub blocking_factor: u16,
    /// Размер страницы 0 (основная калибровка), первое значение `pageSize` в INI.
    pub page_size: u32,
    /// `pageReadCommand` для page 0 включает `%2i` (новый формат с номером страницы).
    /// Старый `"R%2o%2c"` (длина 7) — только offset+count, как в Java `BinaryProtocol`.
    pub page_read_has_page_index: bool,
    /// `pageChunkWrite` содержит `%2i` — на проводе `C` с page; иначе `C%2o%2c%v` — только offset+count+data.
    pub page_chunk_write_has_page_index: bool,
    /// INI `interWriteDelay` (ms), типично 10.
    pub inter_write_delay_ms: u16,
    /// INI `pageActivationDelay` (ms) — пауза TS после записи поля перед `Z`, типично 500.
    pub page_activation_delay_ms: u16,
    pub output_channels: OutputChannels,
    /// Поля page 0: скаляры, enum и массивы из секции `[Constants]`.
    pub config_fields: HashMap<String, ConfigFieldKind>,
    /// 2D-таблицы из `[TableEditor]`.
    pub tables: HashMap<String, IniTableDef>,
    /// 1D-кривые из `[CurveEditor]`.
    pub curves: HashMap<String, IniCurveDef>,
    /// Сырые байты из `[ControllerCommands]` (`cmd_enable_self_stim` и т.д.).
    pub ts_commands: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IniTableDef {
    pub id: String,
    pub title: String,
    pub map_id: Option<String>,
    pub x_bins: Option<String>,
    pub y_bins: Option<String>,
    pub z_bins: String,
    pub x_label: Option<String>,
    pub y_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IniCurveDef {
    pub id: String,
    pub title: String,
    pub x_bins: String,
    pub y_bins: String,
    pub x_label: Option<String>,
    pub y_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ArrayShape {
    Vector(usize),
    /// `[cols x rows]` как в INI TunerStudio.
    Matrix { cols: usize, rows: usize },
}

impl ArrayShape {
    pub fn element_count(self) -> usize {
        match self {
            Self::Vector(n) => n,
            Self::Matrix { cols, rows } => cols * rows,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ArrayField {
    pub ty: ScalarType,
    pub offset: u32,
    pub shape: ArrayShape,
    pub units: String,
    pub scale: f64,
    pub translate: f64,
    pub lo: f64,
    pub hi: f64,
    pub digits: u8,
}

#[derive(Debug, Clone, Serialize)]
pub enum ConfigFieldKind {
    Scalar(ScalarField),
    Enum(EnumField),
    Array(ArrayField),
    String(StringField),
}

/// Фиксированная ASCII-строка в page 0 (`string, ASCII, offset, length` в INI).
#[derive(Debug, Clone, Serialize)]
pub struct StringField {
    pub offset: u32,
    pub length: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnumField {
    pub bits: BitsField,
    pub options: Vec<EnumOption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnumOption {
    pub value: u32,
    pub label: String,
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
    Array(ArrayField),
    String(StringField),
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
