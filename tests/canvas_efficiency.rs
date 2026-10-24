mod common;
use affine_local_mcp::canvas::CreateCanvas;
use common::{WORKSPACE, db_path, fixture};
use serde_json::json;

#[test]
fn summary_reads_omit_large_brush_payloads_and_creation_uses_one_backup() {
    let (_dir, storage) = fixture();
    let points: Vec<_> = (0..5000).map(|x| [x as f64, (x % 100) as f64]).collect();
    let params: CreateCanvas = serde_json::from_value(json!({
        "workspaceId":WORKSPACE,"documentId":"large-drawing","title":"Large drawing",
        "strokes":[{"id":"ink","points":points,"lineWidth":4}]
    }))
    .unwrap();
    let created = storage.create_canvas(&params).unwrap();
    assert_eq!(created["validation"]["idIntegrityValid"], true);
    assert_eq!(
        std::fs::read_dir(storage.data_dir.join("mcp-backups"))
            .unwrap()
            .count(),
        1
    );
    assert!(db_path(&storage).is_file());
    let full = storage.read_canvas(WORKSPACE, "large-drawing").unwrap();
    let summary = storage
        .read_canvas_with_options(WORKSPACE, "large-drawing", true)
        .unwrap();
    assert_eq!(
        full["elements"][0]["properties"]["points"]
            .as_array()
            .unwrap()
            .len(),
        5000
    );
    assert_eq!(summary["elementCount"], full["elementCount"]);
    assert_eq!(summary["geometryBounds"], full["geometryBounds"]);
    assert_eq!(summary["validation"], full["validation"]);
    assert!(summary.get("elements").is_none() && summary.get("blocks").is_none());
    let full_bytes = serde_json::to_vec(&full).unwrap().len();
    let summary_bytes = serde_json::to_vec(&summary).unwrap().len();
    assert!(
        full_bytes > summary_bytes * 50,
        "Summary did not substantially reduce the payload: {full_bytes} -> {summary_bytes}"
    );
    assert!(
        summary_bytes < 1000,
        "Summary leaked drawing data: {summary_bytes}"
    );
    println!("5000-point response: {full_bytes} bytes full, {summary_bytes} bytes summary");
}
