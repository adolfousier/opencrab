//! Tests for the `generate_document` XLSX backend (#357).
//!
//! Workbooks are written with `rust_xlsxwriter` and read back with
//! `calamine` (the same crate `parse_document` uses), so the round trip
//! proves the generated file is consumable by our own read side.

use crate::brain::tools::doc_gen::xlsx::{SheetSpec, write_workbook};
use calamine::{DataType, Reader, Xlsx, open_workbook};
use serde_json::json;

fn spec(v: serde_json::Value) -> SheetSpec {
    serde_json::from_value(v).expect("valid sheet spec")
}

#[test]
fn writes_values_and_reads_back_with_calamine() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("report.xlsx");
    let sheets = vec![spec(json!({
        "name": "Sales",
        "rows": [
            ["Item", "Qty", "Price"],
            ["Apples", 10, 2.5],
            ["Pears", 4, 3.0]
        ]
    }))];
    let summary = write_workbook(&path, &sheets).expect("workbook written");
    assert!(summary.contains("1 sheet(s)"));
    assert!(summary.contains("3 row(s)"));

    let mut wb: Xlsx<_> = open_workbook(&path).expect("readable workbook");
    let range = wb.worksheet_range("Sales").expect("sheet exists");
    assert_eq!(range.get_value((0, 0)).unwrap().to_string(), "Item");
    assert_eq!(range.get_value((1, 0)).unwrap().to_string(), "Apples");
    assert_eq!(range.get_value((1, 1)).unwrap().to_string(), "10");
    assert_eq!(range.get_value((2, 2)).unwrap().to_string(), "3");
}

#[test]
fn equals_prefixed_strings_become_live_formulas() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("formulas.xlsx");
    let sheets = vec![spec(json!({
        "name": "Calc",
        "rows": [
            ["Qty", "Price", "Total"],
            [10, 2.5, "=A2*B2"],
            [4, 3.0, "=A3*B3"],
            ["", "", "=SUM(C2:C3)"]
        ]
    }))];
    let summary = write_workbook(&path, &sheets).expect("workbook written");
    assert!(summary.contains("3 formula(s)"));

    let mut wb: Xlsx<_> = open_workbook(&path).expect("readable workbook");
    let formulas = wb.worksheet_formula("Calc").expect("formula sheet");
    let all: Vec<String> = formulas
        .rows()
        .flat_map(|r| r.iter().cloned())
        .filter(|f| !f.is_empty())
        .collect();
    assert!(all.iter().any(|f| f.contains("A2*B2")), "formulas: {all:?}");
    assert!(
        all.iter().any(|f| f.contains("SUM(C2:C3)")),
        "formulas: {all:?}"
    );
}

#[test]
fn multiple_sheets_all_land() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("multi.xlsx");
    let sheets = vec![
        spec(json!({"name": "One", "rows": [["a"]]})),
        spec(json!({"name": "Two", "rows": [["b"]]})),
    ];
    write_workbook(&path, &sheets).expect("workbook written");
    let wb: Xlsx<_> = open_workbook(&path).expect("readable workbook");
    let names = wb.sheet_names();
    assert!(names.contains(&"One".to_string()));
    assert!(names.contains(&"Two".to_string()));
}

#[test]
fn invalid_sheet_names_are_sanitized_not_fatal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sanitized.xlsx");
    let sheets = vec![spec(json!({
        "name": "Q1/Q2: results [draft]? a very long sheet name over the limit",
        "rows": [["ok"]]
    }))];
    write_workbook(&path, &sheets).expect("sanitized name still writes");
    let wb: Xlsx<_> = open_workbook(&path).expect("readable workbook");
    let names = wb.sheet_names();
    assert_eq!(names.len(), 1);
    assert!(names[0].len() <= 31);
    assert!(!names[0].contains('/'));
    assert!(!names[0].contains('['));
}

#[test]
fn null_cells_are_skipped_and_nested_json_is_stringified() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mixed.xlsx");
    let sheets = vec![spec(json!({
        "name": "Mixed",
        "header_bold": false,
        "rows": [[null, true, {"k": 1}]]
    }))];
    write_workbook(&path, &sheets).expect("workbook written");
    let mut wb: Xlsx<_> = open_workbook(&path).expect("readable workbook");
    let range = wb.worksheet_range("Mixed").expect("sheet exists");
    assert!(range.get_value((0, 0)).is_none() || range.get_value((0, 0)).unwrap().is_empty());
    assert_eq!(
        range.get_value((0, 1)).unwrap().to_string().to_lowercase(),
        "true"
    );
    assert!(range.get_value((0, 2)).unwrap().to_string().contains('k'));
}
