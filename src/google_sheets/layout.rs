use serde_json::Value;

use super::{ORDER_SHEET_FORMAT_ROW_LIMIT, ORDER_SHEET_HEADERS, ORDER_SHEET_ID};

pub(super) fn sheet_has_header(rows: &[Vec<Value>]) -> bool {
    let Some(row) = rows.first() else {
        return false;
    };
    row.first().and_then(value_text) == Some(ORDER_SHEET_HEADERS[0])
        && row.get(1).and_then(value_text) == Some(ORDER_SHEET_HEADERS[1])
        && row.get(3).and_then(value_text) == Some(ORDER_SHEET_HEADERS[3])
}

pub(super) fn json_insert_header_row() -> Value {
    serde_json::json!({
        "insertDimension": {
            "range": {
                "sheetId": ORDER_SHEET_ID,
                "dimension": "ROWS",
                "startIndex": 0,
                "endIndex": 1
            },
            "inheritFromBefore": false
        }
    })
}

pub(super) fn sheet_format_requests() -> Vec<Value> {
    let full_range = serde_json::json!({
        "sheetId": ORDER_SHEET_ID,
        "startRowIndex": 0,
        "endRowIndex": ORDER_SHEET_FORMAT_ROW_LIMIT,
        "startColumnIndex": 0,
        "endColumnIndex": ORDER_SHEET_HEADERS.len()
    });
    let header_range = serde_json::json!({
        "sheetId": ORDER_SHEET_ID,
        "startRowIndex": 0,
        "endRowIndex": 1,
        "startColumnIndex": 0,
        "endColumnIndex": ORDER_SHEET_HEADERS.len()
    });
    let mut requests = vec![
        serde_json::json!({
            "updateSheetProperties": {
                "properties": {
                    "sheetId": ORDER_SHEET_ID,
                    "gridProperties": {
                        "frozenRowCount": 1
                    }
                },
                "fields": "gridProperties.frozenRowCount"
            }
        }),
        serde_json::json!({
            "repeatCell": {
                "range": full_range.clone(),
                "cell": {
                    "userEnteredFormat": {
                        "backgroundColor": {
                            "red": 0.61,
                            "green": 0.88,
                            "blue": 0.89
                        },
                        "textFormat": {
                            "fontFamily": "Arial",
                            "fontSize": 10,
                            "foregroundColor": {
                                "red": 0.0,
                                "green": 0.0,
                                "blue": 0.0
                            }
                        },
                        "horizontalAlignment": "CENTER",
                        "verticalAlignment": "MIDDLE",
                        "wrapStrategy": "WRAP"
                    }
                },
                "fields": "userEnteredFormat(backgroundColor,textFormat,horizontalAlignment,verticalAlignment,wrapStrategy)"
            }
        }),
        serde_json::json!({
            "repeatCell": {
                "range": header_range,
                "cell": {
                    "userEnteredFormat": {
                        "textFormat": {
                            "bold": true,
                            "fontFamily": "Arial",
                            "fontSize": 10
                        }
                    }
                },
                "fields": "userEnteredFormat.textFormat"
            }
        }),
        serde_json::json!({
            "repeatCell": {
                "range": {
                    "sheetId": ORDER_SHEET_ID,
                    "startRowIndex": 1,
                    "endRowIndex": ORDER_SHEET_FORMAT_ROW_LIMIT,
                    "startColumnIndex": 4,
                    "endColumnIndex": 5
                },
                "cell": {
                    "userEnteredFormat": {
                        "horizontalAlignment": "LEFT",
                        "textFormat": {
                            "bold": true,
                            "italic": false
                        }
                    }
                },
                "fields": "userEnteredFormat(horizontalAlignment,textFormat.bold,textFormat.italic)"
            }
        }),
        serde_json::json!({
            "repeatCell": {
                "range": {
                    "sheetId": ORDER_SHEET_ID,
                    "startRowIndex": 1,
                    "endRowIndex": ORDER_SHEET_FORMAT_ROW_LIMIT,
                    "startColumnIndex": 6,
                    "endColumnIndex": 14
                },
                "cell": {
                    "userEnteredFormat": {
                        "textFormat": {
                            "italic": true,
                            "bold": true
                        }
                    }
                },
                "fields": "userEnteredFormat.textFormat"
            }
        }),
        serde_json::json!({
            "updateBorders": {
                "range": full_range.clone(),
                "top": sheet_border(),
                "bottom": sheet_border(),
                "left": sheet_border(),
                "right": sheet_border(),
                "innerHorizontal": sheet_border(),
                "innerVertical": sheet_border()
            }
        }),
    ];
    for (column, width) in [
        (0, 34),
        (1, 86),
        (2, 86),
        (3, 86),
        (4, 360),
        (5, 84),
        (6, 92),
        (7, 92),
        (8, 92),
        (9, 106),
        (10, 106),
        (11, 106),
        (12, 106),
        (13, 100),
        (14, 90),
        (15, 110),
    ] {
        requests.push(serde_json::json!({
            "updateDimensionProperties": {
                "range": {
                    "sheetId": ORDER_SHEET_ID,
                    "dimension": "COLUMNS",
                    "startIndex": column,
                    "endIndex": column + 1
                },
                "properties": {
                    "pixelSize": width
                },
                "fields": "pixelSize"
            }
        }));
    }
    requests
}

fn sheet_border() -> Value {
    serde_json::json!({
        "style": "SOLID",
        "width": 1,
        "color": {
            "red": 0.0,
            "green": 0.0,
            "blue": 0.0
        }
    })
}

fn value_text(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) => Some(value.as_str()),
        _ => None,
    }
}
