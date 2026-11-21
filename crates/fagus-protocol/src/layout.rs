//! The layout of a rendered document as JSON, for `--layout-json` and the worker's `layout`
//! (`docs/protocol.md`).

use serde_json::{Map, Value, json};

/// A length in points, to a hundredth of a point.
fn points(value: f32) -> Value {
    let rounded = (f64::from(value) * 100.0).round() / 100.0;
    // (Without a negative zero.)
    json!(rounded + 0.0)
}

fn boxes(boxes: &[fagus::LayoutBox]) -> Value {
    boxes
        .iter()
        .map(|placed| {
            json!({
                "page": placed.page,
                "x": points(placed.x),
                "y": points(placed.y),
                "width": points(placed.width),
                "height": points(placed.height),
            })
        })
        .collect()
}

/// The layout as JSON: its pages and its elements, each with its boxes and, for tables, its
/// rows.
pub fn layout_json(layout: &fagus::Layout) -> Value {
    let pages: Vec<Value> = layout
        .pages
        .iter()
        .map(|page| json!({ "width": points(page.width), "height": points(page.height) }))
        .collect();
    let elements: Vec<Value> = layout
        .elements
        .iter()
        .map(|element| {
            let mut object = Map::new();
            object.insert("id".to_owned(), json!(element.id));
            object.insert("tag".to_owned(), json!(element.tag));
            object.insert("boxes".to_owned(), boxes(&element.boxes));
            if !element.rows.is_empty() {
                let rows: Vec<Value> = element
                    .rows
                    .iter()
                    .map(|row| {
                        let mut object = Map::new();
                        let section = match row.section {
                            fagus::RowSection::Header => "header",
                            fagus::RowSection::Footer => "footer",
                            _ => "body",
                        };
                        object.insert("section".to_owned(), json!(section));
                        object.insert("index".to_owned(), json!(row.index));
                        if let Some(id) = &row.id {
                            object.insert("id".to_owned(), json!(id));
                        }
                        object.insert("boxes".to_owned(), boxes(&row.boxes));
                        Value::Object(object)
                    })
                    .collect();
                object.insert("rows".to_owned(), Value::Array(rows));
            }
            Value::Object(object)
        })
        .collect();
    json!({ "pages": pages, "elements": elements })
}
