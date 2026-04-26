use serde::{Deserialize, Serialize};

pub const IR_VERSION: u16 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Document {
    pub ir_version: u16,
    pub metadata: Metadata,
    pub sections: Vec<Section>,
    pub resources: ResourceStore,
    pub warnings: Vec<ConversionWarning>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            ir_version: IR_VERSION,
            metadata: Metadata::default(),
            sections: Vec::new(),
            resources: ResourceStore::default(),
            warnings: Vec::new(),
        }
    }
}

impl Document {
    pub fn from_paragraphs(paragraphs: Vec<String>) -> Self {
        let blocks = paragraphs
            .into_iter()
            .map(|text| {
                Block::Paragraph(Paragraph {
                    role: ParagraphRole::Body,
                    inlines: vec![Inline::Text(TextRun {
                        text,
                        style: TextStyle::default(),
                    })],
                })
            })
            .collect();

        Self {
            ir_version: IR_VERSION,
            metadata: Metadata::default(),
            sections: vec![Section { blocks }],
            resources: ResourceStore::default(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Section {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    Image(Image),
    Unknown(UnknownBlock),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Paragraph {
    pub role: ParagraphRole,
    pub inlines: Vec<Inline>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ParagraphRole {
    #[default]
    Body,
    Heading {
        level: u8,
    },
    Title,
    Caption,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Inline {
    Text(TextRun),
    LineBreak,
    Tab,
    Unknown(UnknownInline),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TextRun {
    pub text: String,
    pub style: TextStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ResourceStore {
    pub entries: Vec<Resource>,
}

impl ResourceStore {
    pub fn get(&self, resource_id: &ResourceId) -> Option<&Resource> {
        self.entries
            .iter()
            .find(|resource| resource.id() == resource_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Resource {
    Image(ImageResource),
    Binary(BinaryResource),
}

impl Resource {
    pub fn id(&self) -> &ResourceId {
        match self {
            Resource::Image(resource) => &resource.id,
            Resource::Binary(resource) => &resource.id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct ResourceId(pub String);

impl ResourceId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ImageResource {
    pub id: ResourceId,
    pub media_type: Option<String>,
    pub extension: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct BinaryResource {
    pub id: ResourceId,
    pub media_type: Option<String>,
    pub extension: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Image {
    pub resource_id: ResourceId,
    pub alt: Option<String>,
    pub caption: Option<String>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Table {
    pub rows: Vec<TableRow>,
    pub style: TableStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableCell {
    pub row_span: u32,
    pub col_span: u32,
    pub blocks: Vec<Block>,
    pub style: TableCellStyle,
}

impl Default for TableCell {
    fn default() -> Self {
        Self {
            row_span: 1,
            col_span: 1,
            blocks: Vec::new(),
            style: TableCellStyle::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TableStyle {}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TableCellStyle {}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UnknownBlock {
    pub kind: String,
    pub fallback_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UnknownInline {
    pub kind: String,
    pub fallback_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ConversionWarning {
    pub code: WarningCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    #[default]
    Unknown,
    UsedHwpxPreviewFallback,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_paragraphs_into_single_section_document() {
        let document = Document::from_paragraphs(vec!["a".to_string(), "b".to_string()]);

        assert_eq!(document.ir_version, IR_VERSION);
        assert_eq!(document.sections.len(), 1);
        assert_eq!(document.sections[0].blocks.len(), 2);
        assert!(document.resources.entries.is_empty());

        match &document.sections[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert_eq!(paragraph.role, ParagraphRole::Body);
                assert_eq!(paragraph.inlines.len(), 1);
                assert_eq!(
                    paragraph.inlines[0],
                    Inline::Text(TextRun {
                        text: "a".to_string(),
                        style: TextStyle::default(),
                    })
                );
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }

        match &document.sections[0].blocks[1] {
            Block::Paragraph(paragraph) => {
                assert_eq!(
                    paragraph.inlines[0],
                    Inline::Text(TextRun {
                        text: "b".to_string(),
                        style: TextStyle::default(),
                    })
                );
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn document_has_default_resource_store() {
        let document = Document::default();

        assert!(document.resources.entries.is_empty());
    }

    #[test]
    fn table_cell_can_hold_nested_blocks() {
        let cell = TableCell {
            blocks: vec![Block::Paragraph(Paragraph {
                role: ParagraphRole::Body,
                inlines: vec![Inline::Text(TextRun {
                    text: "cell paragraph".to_string(),
                    style: TextStyle::default(),
                })],
            })],
            ..Default::default()
        };

        let table = Table {
            rows: vec![TableRow { cells: vec![cell] }],
            style: TableStyle::default(),
        };

        match &table.rows[0].cells[0].blocks[0] {
            Block::Paragraph(paragraph) => {
                assert_eq!(paragraph.inlines.len(), 1);
            }
            other => panic!("expected paragraph block, got {other:?}"),
        }
    }

    #[test]
    fn image_can_reference_image_resource_by_resource_id() {
        let resource_id = ResourceId("image-1".to_string());
        let image = Image {
            resource_id: resource_id.clone(),
            alt: Some("logo".to_string()),
            caption: None,
            width: Some(128.0),
            height: Some(64.0),
        };
        let store = ResourceStore {
            entries: vec![Resource::Image(ImageResource {
                id: resource_id.clone(),
                media_type: Some("image/png".to_string()),
                extension: Some("png".to_string()),
                bytes: vec![137, 80, 78, 71],
            })],
        };

        match store.get(&image.resource_id) {
            Some(Resource::Image(resource)) => {
                assert_eq!(resource.id, resource_id);
                assert_eq!(resource.extension.as_deref(), Some("png"));
            }
            other => panic!("expected image resource, got {other:?}"),
        }
    }
}
