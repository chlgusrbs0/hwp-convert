//! Experimental renderer-first snapshot adapter over `rhwp` public renderer queries.
//!
//! This module is intentionally separate from `crate::ir::Document`. It keeps
//! semantic structure unchanged and only captures page-local render output.
//!
//! Unlike `bridge::rhwp`, this path does not fall back to `Preview/PrvText.txt`
//! for HWPX files. Renderer queries need a full `rhwp::DocumentCore` parse and
//! pagination result.
//!
//! TODO: Replace the JSON-string adapter layer when `rhwp` exposes a stable
//! typed render-tree query API to downstream crates.

use std::cmp::Ordering;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::hwpx;

pub fn read_render_snapshot(input_path: &Path) -> Result<RenderSnapshot, Box<dyn Error>> {
    let core = load_document_core(input_path)?;
    RenderSnapshot::from_core(input_path, &core)
}

pub fn render_page_svg(input_path: &Path, page_index: u32) -> Result<String, Box<dyn Error>> {
    let core = load_document_core(input_path)?;
    core.render_page_svg_native(page_index)
        .map_err(|error| render_error("render SVG page", error))
}

fn load_document_core(input_path: &Path) -> Result<rhwp::DocumentCore, Box<dyn Error>> {
    let (_, bytes) = hwpx::read_input_bytes(input_path)?;
    rhwp::DocumentCore::from_bytes(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("rhwp renderer-first snapshot requires a full parseable document: {error}"),
        )
        .into()
    })
}

fn render_error(action: &str, error: rhwp::HwpError) -> Box<dyn Error> {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("failed to {action} via rhwp renderer API: {error}"),
    )
    .into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderSnapshot {
    pub source_path: PathBuf,
    pub pages: Vec<RenderPage>,
}

impl RenderSnapshot {
    fn from_core(source_path: &Path, core: &rhwp::DocumentCore) -> Result<Self, Box<dyn Error>> {
        let mut pages = Vec::new();

        for page_index in 0..core.page_count() {
            let page_info: RhwpPageInfo = serde_json::from_str(
                &core
                    .get_page_info_native(page_index)
                    .map_err(|error| render_error("query page info", error))?,
            )?;
            let text_layout: RhwpTextLayout = serde_json::from_str(
                &core
                    .get_page_text_layout_native(page_index)
                    .map_err(|error| render_error("query page text layout", error))?,
            )?;
            let control_layout: RhwpControlLayout = serde_json::from_str(
                &core
                    .get_page_control_layout_native(page_index)
                    .map_err(|error| render_error("query page control layout", error))?,
            )?;

            pages.push(build_render_page(page_info, text_layout, control_layout));
        }

        Ok(Self {
            source_path: source_path.to_path_buf(),
            pages,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderPage {
    pub page_index: u32,
    pub section_index: usize,
    pub width: f64,
    pub height: f64,
    pub items: Vec<RenderItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RenderItem {
    Text(RenderText),
    Image(RenderImage),
    Box(RenderBox),
}

impl RenderItem {
    fn bounds(&self) -> &RenderBounds {
        match self {
            RenderItem::Text(item) => &item.bounds,
            RenderItem::Image(item) => &item.bounds,
            RenderItem::Box(item) => &item.bounds,
        }
    }

    fn kind_rank(&self) -> u8 {
        match self {
            RenderItem::Text(_) => 0,
            RenderItem::Image(_) => 1,
            RenderItem::Box(_) => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderText {
    pub bounds: RenderBounds,
    pub text: String,
    pub char_offsets_x: Vec<f64>,
    pub doc_ref: Option<RenderDocRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderImage {
    pub bounds: RenderBounds,
    pub doc_ref: Option<RenderDocRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderBox {
    pub kind: RenderBoxKind,
    pub bounds: RenderBounds,
    pub doc_ref: Option<RenderDocRef>,
    pub row_count: Option<u16>,
    pub col_count: Option<u16>,
    pub cells: Vec<RenderTableCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RenderBoxKind {
    Table,
    Equation,
    Group,
    Shape,
    Line,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderTableCell {
    pub bounds: RenderBounds,
    pub row: u16,
    pub col: u16,
    pub row_span: u16,
    pub col_span: u16,
    pub cell_index: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct RenderBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl RenderBounds {
    fn from_xywh(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderDocRef {
    pub section_index: Option<usize>,
    pub paragraph_index: Option<usize>,
    pub char_start: Option<usize>,
    pub control_index: Option<usize>,
    pub cell_index: Option<usize>,
    pub cell_paragraph_index: Option<usize>,
}

fn build_render_page(
    page_info: RhwpPageInfo,
    text_layout: RhwpTextLayout,
    control_layout: RhwpControlLayout,
) -> RenderPage {
    let mut items = Vec::with_capacity(text_layout.runs.len() + control_layout.controls.len());
    items.extend(text_layout.runs.into_iter().map(RenderItem::from));
    items.extend(control_layout.controls.into_iter().map(RenderItem::from));
    items.sort_by(sort_render_items);

    RenderPage {
        page_index: page_info.page_index,
        section_index: page_info.section_index,
        width: page_info.width,
        height: page_info.height,
        items,
    }
}

fn sort_render_items(left: &RenderItem, right: &RenderItem) -> Ordering {
    let left_bounds = left.bounds();
    let right_bounds = right.bounds();

    left_bounds
        .y
        .total_cmp(&right_bounds.y)
        .then_with(|| left_bounds.x.total_cmp(&right_bounds.x))
        .then_with(|| left.kind_rank().cmp(&right.kind_rank()))
}

fn doc_ref(
    section_index: Option<usize>,
    paragraph_index: Option<usize>,
    char_start: Option<usize>,
    control_index: Option<usize>,
    cell_index: Option<usize>,
    cell_paragraph_index: Option<usize>,
) -> Option<RenderDocRef> {
    if section_index.is_none()
        && paragraph_index.is_none()
        && char_start.is_none()
        && control_index.is_none()
        && cell_index.is_none()
        && cell_paragraph_index.is_none()
    {
        return None;
    }

    Some(RenderDocRef {
        section_index,
        paragraph_index,
        char_start,
        control_index,
        cell_index,
        cell_paragraph_index,
    })
}

impl From<RhwpTextRun> for RenderItem {
    fn from(run: RhwpTextRun) -> Self {
        RenderItem::Text(RenderText {
            bounds: RenderBounds::from_xywh(run.x, run.y, run.width, run.height),
            text: run.text,
            char_offsets_x: run.char_x,
            doc_ref: doc_ref(
                run.section_index,
                run.paragraph_index,
                run.char_start,
                run.control_index,
                run.cell_index,
                run.cell_paragraph_index,
            ),
        })
    }
}

impl From<RhwpControl> for RenderItem {
    fn from(control: RhwpControl) -> Self {
        let bounds = RenderBounds::from_xywh(control.x, control.y, control.width, control.height);
        let doc_ref = doc_ref(
            control.section_index,
            control.paragraph_index,
            None,
            control.control_index,
            control.cell_index,
            control.cell_paragraph_index,
        );

        match control.kind.as_str() {
            "image" => RenderItem::Image(RenderImage { bounds, doc_ref }),
            "table" => RenderItem::Box(RenderBox {
                kind: RenderBoxKind::Table,
                bounds,
                doc_ref,
                row_count: control.row_count,
                col_count: control.col_count,
                cells: control
                    .cells
                    .into_iter()
                    .map(RenderTableCell::from)
                    .collect(),
            }),
            "equation" => RenderItem::Box(RenderBox {
                kind: RenderBoxKind::Equation,
                bounds,
                doc_ref,
                row_count: None,
                col_count: None,
                cells: Vec::new(),
            }),
            "group" => RenderItem::Box(RenderBox {
                kind: RenderBoxKind::Group,
                bounds,
                doc_ref,
                row_count: None,
                col_count: None,
                cells: Vec::new(),
            }),
            "shape" => RenderItem::Box(RenderBox {
                kind: RenderBoxKind::Shape,
                bounds,
                doc_ref,
                row_count: None,
                col_count: None,
                cells: Vec::new(),
            }),
            "line" => RenderItem::Box(RenderBox {
                kind: RenderBoxKind::Line,
                bounds,
                doc_ref,
                row_count: None,
                col_count: None,
                cells: Vec::new(),
            }),
            other => RenderItem::Box(RenderBox {
                kind: RenderBoxKind::Other(other.to_string()),
                bounds,
                doc_ref,
                row_count: control.row_count,
                col_count: control.col_count,
                cells: control
                    .cells
                    .into_iter()
                    .map(RenderTableCell::from)
                    .collect(),
            }),
        }
    }
}

impl From<RhwpTableCell> for RenderTableCell {
    fn from(cell: RhwpTableCell) -> Self {
        Self {
            bounds: RenderBounds::from_xywh(cell.x, cell.y, cell.width, cell.height),
            row: cell.row,
            col: cell.col,
            row_span: cell.row_span,
            col_span: cell.col_span,
            cell_index: cell.cell_index,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RhwpPageInfo {
    #[serde(rename = "pageIndex")]
    page_index: u32,
    width: f64,
    height: f64,
    #[serde(rename = "sectionIndex")]
    section_index: usize,
}

#[derive(Debug, Deserialize)]
struct RhwpTextLayout {
    runs: Vec<RhwpTextRun>,
}

#[derive(Debug, Deserialize)]
struct RhwpTextRun {
    text: String,
    x: f64,
    y: f64,
    #[serde(rename = "w")]
    width: f64,
    #[serde(rename = "h")]
    height: f64,
    #[serde(rename = "charX", default)]
    char_x: Vec<f64>,
    #[serde(rename = "secIdx")]
    section_index: Option<usize>,
    #[serde(rename = "paraIdx")]
    paragraph_index: Option<usize>,
    #[serde(rename = "charStart")]
    char_start: Option<usize>,
    #[serde(rename = "controlIdx")]
    control_index: Option<usize>,
    #[serde(rename = "cellIdx")]
    cell_index: Option<usize>,
    #[serde(rename = "cellParaIdx")]
    cell_paragraph_index: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct RhwpControlLayout {
    controls: Vec<RhwpControl>,
}

#[derive(Debug, Deserialize)]
struct RhwpControl {
    #[serde(rename = "type")]
    kind: String,
    x: f64,
    y: f64,
    #[serde(rename = "w")]
    width: f64,
    #[serde(rename = "h")]
    height: f64,
    #[serde(rename = "secIdx")]
    section_index: Option<usize>,
    #[serde(rename = "paraIdx")]
    paragraph_index: Option<usize>,
    #[serde(rename = "controlIdx")]
    control_index: Option<usize>,
    #[serde(rename = "cellIdx")]
    cell_index: Option<usize>,
    #[serde(rename = "cellParaIdx")]
    cell_paragraph_index: Option<usize>,
    #[serde(rename = "rowCount")]
    row_count: Option<u16>,
    #[serde(rename = "colCount")]
    col_count: Option<u16>,
    #[serde(default)]
    cells: Vec<RhwpTableCell>,
}

#[derive(Debug, Deserialize)]
struct RhwpTableCell {
    x: f64,
    y: f64,
    #[serde(rename = "w")]
    width: f64,
    #[serde(rename = "h")]
    height: f64,
    row: u16,
    col: u16,
    #[serde(rename = "rowSpan")]
    row_span: u16,
    #[serde(rename = "colSpan")]
    col_span: u16,
    #[serde(rename = "cellIdx")]
    cell_index: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rhwp::model::bin_data::{
        BinData, BinDataCompression, BinDataContent, BinDataStatus, BinDataType,
    };
    use rhwp::model::control::Control;
    use rhwp::model::document::{DocInfo, Document, Section};
    use rhwp::model::image::{ImageAttr, Picture};
    use rhwp::model::paragraph::Paragraph as RhwpParagraph;
    use rhwp::model::shape::CommonObjAttr;
    use rhwp::model::style::{CharShape, Font, ParaShape};
    use rhwp::model::table::{Cell as RhwpCell, Table as RhwpTable};

    #[test]
    fn build_render_page_merges_text_and_controls() {
        let page = build_render_page(
            RhwpPageInfo {
                page_index: 0,
                width: 640.0,
                height: 480.0,
                section_index: 0,
            },
            RhwpTextLayout {
                runs: vec![RhwpTextRun {
                    text: "snapshot text".to_string(),
                    x: 10.0,
                    y: 20.0,
                    width: 80.0,
                    height: 14.0,
                    char_x: vec![0.0, 12.0],
                    section_index: Some(0),
                    paragraph_index: Some(0),
                    char_start: Some(0),
                    control_index: None,
                    cell_index: None,
                    cell_paragraph_index: None,
                }],
            },
            RhwpControlLayout {
                controls: vec![
                    RhwpControl {
                        kind: "image".to_string(),
                        x: 12.0,
                        y: 40.0,
                        width: 100.0,
                        height: 60.0,
                        section_index: Some(0),
                        paragraph_index: Some(1),
                        control_index: Some(0),
                        cell_index: None,
                        cell_paragraph_index: None,
                        row_count: None,
                        col_count: None,
                        cells: Vec::new(),
                    },
                    RhwpControl {
                        kind: "table".to_string(),
                        x: 14.0,
                        y: 120.0,
                        width: 160.0,
                        height: 90.0,
                        section_index: Some(0),
                        paragraph_index: Some(2),
                        control_index: Some(0),
                        cell_index: None,
                        cell_paragraph_index: None,
                        row_count: Some(1),
                        col_count: Some(1),
                        cells: vec![RhwpTableCell {
                            x: 14.0,
                            y: 120.0,
                            width: 160.0,
                            height: 90.0,
                            row: 0,
                            col: 0,
                            row_span: 1,
                            col_span: 1,
                            cell_index: 0,
                        }],
                    },
                ],
            },
        );

        assert_eq!(page.items.len(), 3);
        assert!(matches!(page.items[0], RenderItem::Text(_)));
        assert!(matches!(page.items[1], RenderItem::Image(_)));

        match &page.items[2] {
            RenderItem::Box(table) => {
                assert_eq!(table.kind, RenderBoxKind::Table);
                assert_eq!(table.row_count, Some(1));
                assert_eq!(table.col_count, Some(1));
                assert_eq!(table.cells.len(), 1);
            }
            other => panic!("expected table box, got {other:?}"),
        }
    }

    #[test]
    fn reads_render_snapshot_from_temp_sample_hwp() -> Result<(), Box<dyn Error>> {
        let temp_dir = unique_temp_dir();
        fs::create_dir_all(&temp_dir)?;

        let sample_path = temp_dir.join("sample.hwp");
        fs::write(&sample_path, rhwp::serialize_document(&sample_document())?)?;

        let svg = render_page_svg(&sample_path, 0)?;
        assert!(svg.contains("<svg"));

        let snapshot = read_render_snapshot(&sample_path)?;
        assert_eq!(
            snapshot
                .source_path
                .file_name()
                .and_then(|name| name.to_str()),
            Some("sample.hwp")
        );
        assert!(!snapshot.pages.is_empty());
        assert!(snapshot.pages.iter().flat_map(|page| &page.items).any(|item| {
            matches!(item, RenderItem::Text(text) if text.text.contains("sample render text"))
        }));
        assert!(
            snapshot
                .pages
                .iter()
                .flat_map(|page| &page.items)
                .any(|item| { matches!(item, RenderItem::Image(_)) })
        );
        assert!(snapshot.pages.iter().flat_map(|page| &page.items).any(|item| {
            matches!(item, RenderItem::Box(render_box) if render_box.kind == RenderBoxKind::Table)
        }));

        fs::remove_file(&sample_path)?;
        fs::remove_dir_all(&temp_dir)?;

        Ok(())
    }

    fn sample_document() -> Document {
        let mut text_paragraph = RhwpParagraph::new_empty();
        text_paragraph.insert_text_at(0, "sample render text");

        let picture = Picture {
            image_attr: ImageAttr {
                bin_data_id: 1,
                ..Default::default()
            },
            common: CommonObjAttr {
                width: 7200,
                height: 3600,
                treat_as_char: true,
                description: "sample image".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let image_paragraph = RhwpParagraph {
            controls: vec![Control::Picture(Box::new(picture))],
            ..RhwpParagraph::new_empty()
        };

        let mut cell_paragraph = RhwpParagraph::new_empty();
        cell_paragraph.insert_text_at(0, "sample cell");

        let table = RhwpTable {
            row_count: 1,
            col_count: 1,
            row_sizes: vec![2400],
            cells: vec![RhwpCell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                width: 4800,
                height: 2400,
                paragraphs: vec![cell_paragraph],
                ..Default::default()
            }],
            cell_grid: vec![Some(0)],
            common: CommonObjAttr {
                width: 4800,
                height: 2400,
                treat_as_char: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let table_paragraph = RhwpParagraph {
            controls: vec![Control::Table(Box::new(table))],
            ..RhwpParagraph::new_empty()
        };

        Document {
            doc_info: DocInfo {
                font_faces: vec![vec![Font {
                    name: "Malgun Gothic".to_string(),
                    ..Default::default()
                }]],
                bin_data_list: vec![BinData {
                    data_type: BinDataType::Embedding,
                    compression: BinDataCompression::Default,
                    status: BinDataStatus::NotAccessed,
                    storage_id: 1,
                    extension: Some("png".to_string()),
                    ..Default::default()
                }],
                char_shapes: vec![CharShape::default()],
                para_shapes: vec![ParaShape::default()],
                ..Default::default()
            },
            sections: vec![Section {
                paragraphs: vec![text_paragraph, image_paragraph, table_paragraph],
                ..Default::default()
            }],
            bin_data_content: vec![BinDataContent {
                id: 1,
                data: tiny_png_bytes(),
                extension: "png".to_string(),
            }],
            ..Default::default()
        }
    }

    fn unique_temp_dir() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after UNIX_EPOCH")
            .as_nanos();
        std::env::temp_dir().join(format!("hwp-convert-render-snapshot-{suffix}"))
    }

    fn tiny_png_bytes() -> Vec<u8> {
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 15, 4,
            0, 9, 251, 3, 253, 160, 90, 53, 209, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ]
    }
}
