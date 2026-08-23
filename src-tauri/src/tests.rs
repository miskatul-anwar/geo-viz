#[cfg(test)]
mod tests {
    use crate::db::AppDb;
    use crate::gis::{
        bbox::calculate_bounding_boxes,
        buffer::calculate_buffer,
        centroid::calculate_centroids,
        convex_hull::calculate_convex_hull,
        distance_matrix::calculate_nearest_neighbors,
        format_convert::{csv_to_geojson, geojson_to_csv, geojson_to_wkt},
        metrics::calculate_metrics,
        parser::parse_geojson_str,
        random_points::generate_random_points,
        simplify::simplify_geometries,
        spatial_binning::calculate_spatial_binning,
        spatial_query::execute_spatial_query,
    };
    use crate::models::*;
    use geojson::{Feature, FeatureCollection, Geometry, Value};

    fn sample_points_fc() -> FeatureCollection {
        let f1 = Feature {
            bbox: None,
            geometry: Some(Geometry::new(Value::Point(vec![-74.006, 40.7128]))),
            id: None,
            properties: Some({
                let mut m = serde_json::Map::new();
                m.insert("name".to_string(), serde_json::json!("New York"));
                m.insert("pop".to_string(), serde_json::json!(8400000));
                m
            }),
            foreign_members: None,
        };
        let f2 = Feature {
            bbox: None,
            geometry: Some(Geometry::new(Value::Point(vec![0.1278, 51.5074]))),
            id: None,
            properties: Some({
                let mut m = serde_json::Map::new();
                m.insert("name".to_string(), serde_json::json!("London"));
                m.insert("pop".to_string(), serde_json::json!(9000000));
                m
            }),
            foreign_members: None,
        };
        let f3 = Feature {
            bbox: None,
            geometry: Some(Geometry::new(Value::Point(vec![139.6917, 35.6895]))),
            id: None,
            properties: Some({
                let mut m = serde_json::Map::new();
                m.insert("name".to_string(), serde_json::json!("Tokyo"));
                m.insert("pop".to_string(), serde_json::json!(37400000));
                m
            }),
            foreign_members: None,
        };
        FeatureCollection {
            bbox: None,
            features: vec![f1, f2, f3],
            foreign_members: None,
        }
    }

    fn sample_polygon_fc() -> FeatureCollection {
        let poly_coords = vec![vec![
            vec![-10.0, -10.0],
            vec![10.0, -10.0],
            vec![10.0, 10.0],
            vec![-10.0, 10.0],
            vec![-10.0, -10.0],
        ]];
        let f = Feature {
            bbox: None,
            geometry: Some(Geometry::new(Value::Polygon(poly_coords))),
            id: None,
            properties: Some({
                let mut m = serde_json::Map::new();
                m.insert("zone".to_string(), serde_json::json!("Equatorial"));
                m
            }),
            foreign_members: None,
        };
        FeatureCollection {
            bbox: None,
            features: vec![f],
            foreign_members: None,
        }
    }

    // 1. GeoJSON Parser & Boundary Validation
    #[test]
    fn test_parser_valid_and_malformed() {
        // Valid FeatureCollection
        let raw = r#"{"type": "FeatureCollection", "features": [{"type": "Feature", "geometry": {"type": "Point", "coordinates": [10.0, 20.0]}, "properties": {"code": 101}}]}"#;
        let res = parse_geojson_str(raw);
        assert!(res.is_ok());
        let parsed = res.unwrap();
        assert_eq!(parsed.feature_count, 1);
        assert_eq!(parsed.geom_types, vec!["Point"]);

        // Malformed JSON string
        assert!(parse_geojson_str("not json at all").is_err());
        assert!(parse_geojson_str("{").is_err());
        assert!(parse_geojson_str("").is_err());

        // Single Geometry object (auto-wrapped to FeatureCollection)
        let geom_raw = r#"{"type": "Point", "coordinates": [0.0, 0.0]}"#;
        let geom_res = parse_geojson_str(geom_raw);
        assert!(geom_res.is_ok());
        assert_eq!(geom_res.unwrap().feature_count, 1);
    }

    // 2. Buffer Geodesic Analysis
    #[test]
    fn test_buffer_calculations() {
        let fc = sample_points_fc();
        // Positive distance
        let res = calculate_buffer(&fc, 50000.0, 16);
        assert!(res.is_ok());
        let (buf_fc, _metrics) = res.unwrap();
        assert_eq!(buf_fc.features.len(), 3);

        // Edge case: Negative distance on points (must return error)
        let neg_res = calculate_buffer(&fc, -100.0, 8);
        assert!(neg_res.is_err());

        // Empty collection
        let empty_fc = FeatureCollection {
            bbox: None,
            features: vec![],
            foreign_members: None,
        };
        let empty_res = calculate_buffer(&empty_fc, 1000.0, 8);
        assert!(empty_res.is_ok());
    }

    // 3. Convex Hull Engine
    #[test]
    fn test_convex_hull() {
        let fc = sample_points_fc();
        let res_agg = calculate_convex_hull(&fc, false);
        assert!(res_agg.is_ok());
        assert_eq!(res_agg.unwrap().0.features.len(), 1);

        let poly_fc = sample_polygon_fc();
        let res_per = calculate_convex_hull(&poly_fc, true);
        assert!(res_per.is_ok());
        assert_eq!(res_per.unwrap().0.features.len(), 1);
    }

    // 4. Centroid Engine
    #[test]
    fn test_centroids() {
        let poly_fc = sample_polygon_fc();
        let res = calculate_centroids(&poly_fc);
        assert!(res.is_ok());
        let (fc, _metrics) = res.unwrap();
        assert_eq!(fc.features.len(), 1);
        if let Some(Geometry {
            value: Value::Point(coords),
            ..
        }) = &fc.features[0].geometry
        {
            assert!((coords[0] - 0.0).abs() < 0.001);
            assert!((coords[1] - 0.0).abs() < 0.001);
        } else {
            panic!("Expected Point centroid geometry");
        }
    }

    // 5. Bounding Box Engine
    #[test]
    fn test_bounding_boxes() {
        let fc = sample_points_fc();
        let res = calculate_bounding_boxes(&fc, false);
        assert!(res.is_ok());
        let (bbox_fc, _metrics) = res.unwrap();
        assert_eq!(bbox_fc.features.len(), 1);
    }

    // 6. Douglas-Peucker Simplification
    #[test]
    fn test_simplification() {
        let poly_fc = sample_polygon_fc();
        let res = simplify_geometries(&poly_fc, 0.5);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().0.features.len(), 1);
    }

    // 7. Spatial Metrics Calculation
    #[test]
    fn test_spatial_metrics() {
        let poly_fc = sample_polygon_fc();
        let res = calculate_metrics(&poly_fc);
        assert!(res.is_ok());
        let (out_fc, metrics_json) = res.unwrap();
        assert_eq!(out_fc.features.len(), 1);
        assert!(metrics_json.get("total_area_sqkm").is_some());
    }

    // 8. Spatial Query & Point-In-Polygon Filtering
    #[test]
    fn test_spatial_query() {
        let pt_fc = sample_points_fc();
        let poly_fc = sample_polygon_fc();

        // Spatial intersection filter
        let res = execute_spatial_query(&pt_fc, Some(&poly_fc), "intersects", None, None, None);
        assert!(res.is_ok());

        // Attribute filter (pop > 30000000)
        let attr_res = execute_spatial_query(
            &pt_fc,
            None,
            "intersects",
            Some("pop"),
            Some(">"),
            Some("30000000"),
        );
        assert!(attr_res.is_ok());
        assert_eq!(attr_res.unwrap().0.features.len(), 1); // Tokyo only
    }

    // 9. Hexbin & Square Grid Binning
    #[test]
    fn test_spatial_binning() {
        let fc = sample_points_fc();
        let hex_res = calculate_spatial_binning(&fc, "hexbin", 500.0);
        assert!(hex_res.is_ok());

        let sq_res = calculate_spatial_binning(&fc, "square", 500.0);
        assert!(sq_res.is_ok());
    }

    // 10. Distance Matrix & Nearest Neighbor
    #[test]
    fn test_distance_matrix() {
        let fc = sample_points_fc();
        let res = calculate_nearest_neighbors(&fc, None);
        assert!(res.is_ok());
        let (result_fc, _metrics) = res.unwrap();
        assert!(!result_fc.features.is_empty());
    }

    // 11. Random Point Generation
    #[test]
    fn test_random_points() {
        let fc = sample_points_fc();
        let res = generate_random_points(&fc, 50, false);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().0.features.len(), 50);
    }

    // 12. Lossless Format Converters (GeoJSON <-> CSV <-> WKT: All 6 routes)
    #[test]
    fn test_format_converters() {
        use crate::gis::format_convert::{csv_to_wkt, wkt_to_csv, wkt_to_geojson};

        let raw_geojson = r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[12.5,55.6]},"properties":{"city":"Copenhagen"}}]}"#;
        let parsed = parse_geojson_str(raw_geojson).expect("Parse failed");

        // 1. GeoJSON -> CSV
        let csv_res = geojson_to_csv(&parsed.feature_collection);
        assert!(csv_res.is_ok());
        let csv_text = csv_res.unwrap();
        assert!(csv_text.contains("Copenhagen"));
        assert!(csv_text.contains("12.5"));

        // 2. CSV -> GeoJSON
        let back_geojson = csv_to_geojson(&csv_text);
        assert!(back_geojson.is_ok());
        assert_eq!(back_geojson.unwrap().features.len(), 1);

        // 3. GeoJSON -> WKT
        let wkt_res = geojson_to_wkt(&parsed.feature_collection);
        assert!(wkt_res.is_ok());
        let wkt_text = wkt_res.unwrap();
        assert!(wkt_text.contains("POINT(12.5 55.6)"));

        // 4. CSV -> WKT
        let csv_wkt_res = csv_to_wkt(&csv_text);
        assert!(csv_wkt_res.is_ok());
        assert!(csv_wkt_res.unwrap().contains("POINT(12.5 55.6)"));

        // 5. WKT -> GeoJSON
        let wkt_geo_res = wkt_to_geojson(&wkt_text);
        assert!(wkt_geo_res.is_ok());
        let wkt_fc = wkt_geo_res.unwrap();
        assert_eq!(wkt_fc.features.len(), 1);

        // 6. WKT -> CSV
        let wkt_csv_res = wkt_to_csv(&wkt_text);
        assert!(wkt_csv_res.is_ok());
        assert!(wkt_csv_res.unwrap().contains("12.5"));

        // Polygon & MultiPolygon WKT parsing test
        let poly_wkt = "POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))";
        let poly_geo = wkt_to_geojson(poly_wkt);
        assert!(poly_geo.is_ok());
        assert_eq!(poly_geo.unwrap().features.len(), 1);
    }

    // 13. ESRI Shapefile Binary & ZIP Archive Ingestion
    #[test]
    fn test_shapefile_parsing() {
        use crate::gis::shapefile_reader::parse_shapefile_bytes;

        // 1. Point Shapefile
        let mut shp_buf = Vec::new();
        {
            let mut writer = shapefile::ShapeWriter::new(std::io::Cursor::new(&mut shp_buf));
            writer
                .write_shape(&shapefile::Point::new(12.5, 55.6))
                .expect("Failed to write point shape");
        }

        let res = parse_shapefile_bytes(&shp_buf, Some("Test Point Shapefile".to_string()));
        assert!(res.is_ok(), "Shapefile parser failed: {:?}", res.err());
        let parsed = res.unwrap();
        assert_eq!(parsed.feature_count, 1);
        assert_eq!(parsed.geom_types, vec!["Point".to_string()]);
        assert!(parsed.bbox.is_some());

        // 2. Polygon Shapefile
        let mut poly_buf = Vec::new();
        {
            let mut writer = shapefile::ShapeWriter::new(std::io::Cursor::new(&mut poly_buf));
            let ring = shapefile::PolygonRing::Outer(vec![
                shapefile::Point::new(0.0, 0.0),
                shapefile::Point::new(10.0, 0.0),
                shapefile::Point::new(10.0, 10.0),
                shapefile::Point::new(0.0, 10.0),
                shapefile::Point::new(0.0, 0.0),
            ]);
            let poly_shape = shapefile::Polygon::new(ring);
            writer
                .write_shape(&poly_shape)
                .expect("Failed to write polygon shape");
        }

        let poly_res = parse_shapefile_bytes(&poly_buf, Some("Test Poly Shapefile".to_string()));
        assert!(
            poly_res.is_ok(),
            "Polygon Shapefile parser failed: {:?}",
            poly_res.err()
        );
        let poly_parsed = poly_res.unwrap();
        assert_eq!(poly_parsed.feature_count, 1);
        assert_eq!(poly_parsed.geom_types, vec!["Polygon".to_string()]);

        // 3. ZIP Archive containing Shapefile
        let mut zip_buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("data/test_layer.shp", options).unwrap();
            use std::io::Write;
            zip.write_all(&poly_buf).unwrap();
            zip.finish().unwrap();
        }

        let zip_res = parse_shapefile_bytes(&zip_buf, None);
        assert!(
            zip_res.is_ok(),
            "ZIP Shapefile parser failed: {:?}",
            zip_res.err()
        );
        let zip_parsed = zip_res.unwrap();
        assert_eq!(zip_parsed.feature_count, 1);
        assert_eq!(zip_parsed.geom_types, vec!["Polygon".to_string()]);
    }

    // 13. SQLite SQLx Persistence & WAL Mode Verification
    #[tokio::test]
    async fn test_sqlite_database_lifecycle() {
        let temp_dir = std::env::temp_dir().join(format!("geoviz_test_{}", uuid::Uuid::new_v4()));
        let db = AppDb::init(Some(temp_dir.clone()))
            .await
            .expect("SQLite temp DB init failed");

        // Test stats
        let stats = db.get_stats().await.expect("Failed to get DB stats");
        assert_eq!(stats.dataset_count, 0);

        // Test dataset insert & retrieval
        let dataset = DatasetDetail {
            id: "test-ds-1".to_string(),
            name: "Test Dataset".to_string(),
            format: "geojson".to_string(),
            feature_count: 5,
            geom_types: vec!["Point".to_string()],
            bbox: Some([0.0, 0.0, 10.0, 10.0]),
            properties_schema: vec![],
            geojson: r#"{"type":"FeatureCollection","features":[]}"#.to_string(),
            created_at: "2026-08-14T00:00:00Z".to_string(),
            updated_at: "2026-08-14T00:00:00Z".to_string(),
        };
        db.save_dataset(&dataset)
            .await
            .expect("Save dataset failed");

        let fetched = db
            .get_dataset_detail("test-ds-1")
            .await
            .expect("Get dataset failed");
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().name, "Test Dataset");

        // Test Layer CRUD
        let layer = Layer {
            id: "layer-1".to_string(),
            dataset_id: "test-ds-1".to_string(),
            name: "Test Layer".to_string(),
            is_visible: true,
            opacity: 0.8,
            style: LayerStyle::default(),
            z_index: 1,
            created_at: "2026-08-14T00:00:00Z".to_string(),
        };
        db.save_layer(&layer).await.expect("Save layer failed");
        let layers = db.list_layers().await.expect("List layers failed");
        assert_eq!(layers.len(), 1);

        // Test SQL Console query execution
        let query_res = db
            .execute_sql_query("SELECT id, name FROM datasets WHERE id = 'test-ds-1'")
            .await;
        assert!(query_res.is_ok());
        let qr = query_res.unwrap();
        assert_eq!(qr.row_count, 1);

        // Test SQL Injection / Malicious DDL prevention
        let drop_res = db.execute_sql_query("DROP TABLE datasets").await;
        assert!(
            drop_res.is_err(),
            "DROP TABLE must be prohibited in SQL console"
        );
    }

    fn sample_geojson_text() -> String {
        r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[10.0,20.0]},"properties":{"city":"Alpha","pop":100}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[11.0,21.0]},"properties":{"city":"Beta","pop":200}}
        ]}"#.to_string()
    }

    // 14. Backend-owned import pipeline: parse -> persist dataset -> provision layer
    #[tokio::test]
    async fn test_import_pipeline() {
        let temp_dir = std::env::temp_dir().join(format!("geoviz_test_{}", uuid::Uuid::new_v4()));
        let db = AppDb::init(Some(temp_dir)).await.expect("DB init failed");

        let outcome = crate::services::dataset_service::import_dataset(
            &db,
            Some("Cities".to_string()),
            &sample_geojson_text(),
            "geojson",
        )
        .await
        .expect("import failed");

        assert_eq!(outcome.dataset.name, "Cities");
        assert_eq!(outcome.dataset.feature_count, 2);
        assert_eq!(outcome.layer.dataset_id, outcome.dataset.id);

        let layers = db.list_layers().await.expect("list layers failed");
        assert_eq!(layers.len(), 1);
        assert!(layers[0].is_visible);

        // Empty datasets must be rejected
        let empty = r#"{"type":"FeatureCollection","features":[]}"#;
        assert!(
            crate::services::dataset_service::import_dataset(&db, None, empty, "geojson")
                .await
                .is_err()
        );
    }

    // 15. Unified tool runner: compute, timing, persistence of history
    #[tokio::test]
    async fn test_tool_runner_end_to_end() {
        let temp_dir = std::env::temp_dir().join(format!("geoviz_test_{}", uuid::Uuid::new_v4()));
        let db = AppDb::init(Some(temp_dir)).await.expect("DB init failed");

        let outcome = crate::services::dataset_service::import_dataset(
            &db,
            None,
            &sample_geojson_text(),
            "geojson",
        )
        .await
        .expect("import failed");

        let result = crate::services::tool_service::run_tool(
            &db,
            crate::services::tool_service::ToolKind::Buffer,
            crate::services::tool_service::ToolParams {
                distance_meters: Some(5000.0),
                steps: Some(12),
                ..Default::default()
            },
            Some(outcome.dataset.id.clone()),
            None,
            "tab-1".to_string(),
        )
        .await
        .expect("buffer run failed");

        assert_eq!(result.feature_count, 2);
        assert!(result.execution_time_ms >= 0);
        assert!(result.output_geojson.contains("Polygon"));

        let stats = db.get_stats().await.expect("stats failed");
        assert_eq!(stats.calculation_count, 1, "history entry must be logged");

        // Missing dataset id surfaces a typed error
        let missing = crate::services::tool_service::run_tool(
            &db,
            crate::services::tool_service::ToolKind::Centroid,
            Default::default(),
            Some("nonexistent".to_string()),
            None,
            "tab-1".to_string(),
        )
        .await;
        assert!(missing.is_err());
    }

    // 16. Regression: polygon masks must apply regardless of relation keyword
    #[test]
    fn test_spatial_query_mask_applies_on_intersects_relation() {
        use crate::gis::spatial_query::execute_spatial_query;

        let source = parse_geojson_str(
            r#"{"type":"FeatureCollection","features":[
                {"type":"Feature","geometry":{"type":"Point","coordinates":[0.0,0.0]},"properties":{"name":"inside"}},
                {"type":"Feature","geometry":{"type":"Point","coordinates":[50.0,50.0]},"properties":{"name":"outside"}}
            ]}"#,
        )
        .unwrap()
        .feature_collection;

        let mask = parse_geojson_str(
            r#"{"type":"FeatureCollection","features":[
                {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[-10,-10],[10,-10],[10,10],[-10,10],[-10,-10]]]},"properties":{}}
            ]}"#,
        )
        .unwrap()
        .feature_collection;

        let (out, _) =
            execute_spatial_query(&source, Some(&mask), "intersects", None, None, None).unwrap();
        assert_eq!(
            out.features.len(),
            1,
            "only the contained point may survive"
        );
        assert_eq!(
            out.features[0]
                .properties
                .as_ref()
                .unwrap()
                .get("name")
                .unwrap(),
            "inside"
        );
    }

    fn square_fc(x0: f64, y0: f64, side: f64) -> FeatureCollection {
        parse_geojson_str(&format!(
            r#"{{"type":"FeatureCollection","features":[{{"type":"Feature","geometry":{{"type":"Polygon","coordinates":[[[{x0},{y0}],[{x1},{y0}],[{x1},{y1}],[{x0},{y1}],[{x0},{y0}]]]}},"properties":{{"id":"sq"}}}}]}}"#,
            x1 = x0 + side,
            y1 = y0 + side
        ))
        .unwrap()
        .feature_collection
    }

    // 17. Overlay operations: intersection / difference / clip / dissolve
    #[test]
    fn test_overlay_and_dissolve() {
        use crate::gis::overlay;

        let a = square_fc(0.0, 0.0, 10.0);
        let b = square_fc(5.0, 0.0, 10.0); // overlaps a on [5..10]

        let (inter, _) = overlay::run_overlay(&a, &b, "intersection").unwrap();
        assert_eq!(inter.features.len(), 1);

        let (diff, _) = overlay::run_overlay(&a, &b, "difference").unwrap();
        assert_eq!(diff.features.len(), 1, "a-b must remain non-empty");

        let mask = square_fc(2.0, 2.0, 3.0);
        let (clipped, _) = overlay::run_clip(&a, &mask).unwrap();
        assert_eq!(clipped.features.len(), 1);
        assert_eq!(clipped.features[0].properties.as_ref().unwrap()["id"], "sq");

        let mut two = a.clone();
        let second = square_fc(10.0, 0.0, 10.0); // touches first square edge
        two.features.extend(second.features);
        let (dissolved, _) = overlay::run_dissolve(&two, None).unwrap();
        if let geojson::Value::MultiPolygon(polys) =
            &dissolved.features[0].geometry.as_ref().unwrap().value
        {
            assert_eq!(
                polys.len(),
                1,
                "touching squares must merge into one polygon"
            );
        } else {
            panic!("expected MultiPolygon");
        }
    }

    // 18. Spatial join attaches target attributes by containment
    #[test]
    fn test_spatial_join() {
        use crate::gis::spatial_join::run_spatial_join;

        let source = parse_geojson_str(
            r#"{"type":"FeatureCollection","features":[
                {"type":"Feature","geometry":{"type":"Point","coordinates":[1.0,1.0]},"properties":{"name":"in"}},
                {"type":"Feature","geometry":{"type":"Point","coordinates":[99.0,99.0]},"properties":{"name":"out"}}
            ]}"#,
        )
        .unwrap()
        .feature_collection;
        let target = parse_geojson_str(
            r#"{"type":"FeatureCollection","features":[
                {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[5,0],[5,5],[0,5],[0,0]]]},"properties":{"zone":"A"}}
            ]}"#,
        )
        .unwrap()
        .feature_collection;

        let (joined, summary) = run_spatial_join(&source, &target).unwrap();
        assert_eq!(summary["joined_features"], 1);
        assert_eq!(
            joined.features[0].properties.as_ref().unwrap()["sj_zone"],
            "A"
        );
        assert_eq!(
            joined.features[0].properties.as_ref().unwrap()["sj_join_count"],
            1
        );
        assert_eq!(
            joined.features[1].properties.as_ref().unwrap()["sj_zone"],
            serde_json::json!(null)
        );
    }

    // 19. Classification: equal interval and quantile breaks
    #[test]
    fn test_classification() {
        use crate::gis::classification::{compute_breaks, numeric_values, ClassificationMethod};

        let fc = parse_geojson_str(
            r#"{"type":"FeatureCollection","features":[
                {"type":"Feature","geometry":{"type":"Point","coordinates":[0.0,0.0]},"properties":{"v":10}},
                {"type":"Feature","geometry":{"type":"Point","coordinates":[1.0,0.0]},"properties":{"v":20}},
                {"type":"Feature","geometry":{"type":"Point","coordinates":[2.0,0.0]},"properties":{"v":30}},
                {"type":"Feature","geometry":{"type":"Point","coordinates":[3.0,0.0]},"properties":{"v":40}},
                {"type":"Feature","geometry":{"type":"Point","coordinates":[4.0,0.0]},"properties":{"v":"not-a-number"}}
            ]}"#,
        )
        .unwrap()
        .feature_collection;

        let values = numeric_values(&fc, "v");
        assert_eq!(values.len(), 4, "non-numeric entries must be skipped");

        let breaks = compute_breaks(&values, ClassificationMethod::EqualInterval, 4).unwrap();
        assert_eq!(breaks.len(), 4);
        assert!((breaks[0].min - 10.0).abs() < 1e-9);
        assert!((breaks[3].max - 40.0).abs() < 1e-9);
        assert!(breaks
            .windows(2)
            .all(|w| (w[0].max - w[1].min).abs() < 1e-9));

        let q = compute_breaks(&values, ClassificationMethod::Quantile, 2).unwrap();
        assert_eq!(q.len(), 2);
        assert!(compute_breaks(&[], ClassificationMethod::EqualInterval, 4).is_err());
    }

    // 20. KML ingestion (point + extended data + polygon)
    #[test]
    fn test_kml_import() {
        use crate::gis::kml::parse_kml_str;

        let kml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <kml xmlns="http://www.opengis.net/kml/2.2"><Document>
          <Placemark><name>Site A</name>
            <ExtendedData><Data name="owner"><value>Acme Corp</value></Data></ExtendedData>
            <Point><coordinates>-122.082,37.420,35</coordinates></Point>
          </Placemark>
          <Placemark>
            <Polygon><outerBoundaryIs><LinearRing><coordinates>
              0.0,0.0 1.0,0.0 1.0,1.0 0.0,1.0 0.0,0.0
            </coordinates></LinearRing></outerBoundaryIs></Polygon>
          </Placemark>
        </Document></kml>"#;

        let parsed = parse_kml_str(kml).expect("KML parse failed");
        assert_eq!(parsed.feature_count, 2);
        assert_eq!(parsed.geom_types, vec!["Point", "Polygon"]);
        assert_eq!(
            parsed.feature_collection.features[0]
                .properties
                .as_ref()
                .unwrap()["owner"],
            "Acme Corp"
        );
        assert!(parsed.bbox.is_some());
    }

    // 21. GPX ingestion (waypoints + track segment)
    #[test]
    fn test_gpx_import() {
        use crate::gis::gpx::parse_gpx_str;

        let gpx = r#"<?xml version="1.0"?>
        <gpx version="1.1" creator="test">
          <wpt lat="47.6" lon="-122.3"><name>Start</name><ele>12.5</ele></wpt>
          <trk><name>Morning Run</name>
            <trkseg>
              <trkpt lat="47.6" lon="-122.3"/><trkpt lat="47.61" lon="-122.31"/>
              <trkpt lat="47.62" lon="-122.32"/>
            </trkseg>
          </trk>
        </gpx>"#;

        let parsed = parse_gpx_str(gpx).expect("GPX parse failed");
        assert_eq!(parsed.feature_count, 2);
        assert_eq!(parsed.geom_types, vec!["LineString", "Point"]);
        let track = parsed
            .feature_collection
            .features
            .iter()
            .find(|f| {
                matches!(
                    f.geometry.as_ref().unwrap().value,
                    geojson::Value::LineString(_)
                )
            })
            .unwrap();
        assert_eq!(track.properties.as_ref().unwrap()["gpx_kind"], "track");
    }

    // 22. GeoPackage blob decoding + full file ingestion
    #[tokio::test]
    async fn test_gpkg_import() {
        use crate::gis::gpkg::{decode_gpkg_blob, parse_gpkg_bytes};

        // Unit-test the blob decoder directly first.
        let mut blob = vec![b'G', b'P', 0, 0x01]; // magic, v0, LE header, no envelope
        blob.extend((-1i32).to_le_bytes()); // srs_id
        blob.push(0x01); // WKB little endian
        blob.extend(1u32.to_le_bytes()); // Point
        blob.extend(10.0f64.to_le_bytes());
        blob.extend(20.0f64.to_le_bytes());
        match decode_gpkg_blob(&blob).unwrap() {
            geojson::Value::Point(c) => assert_eq!(c, vec![10.0, 20.0]),
            other => panic!("expected Point, got {other:?}"),
        }

        // Build a minimal valid GeoPackage file.
        let dir = std::env::temp_dir().join(format!("geoviz_gpkg_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.gpkg");
        let conn_str = format!("sqlite://{}?mode=rwc", path.to_string_lossy());

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect(&conn_str)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE gpkg_spatial_ref_sys (srs_name TEXT, srs_id INTEGER, organization TEXT, organization_coordsys_id INTEGER)",
            "CREATE TABLE gpkg_contents (table_name TEXT PRIMARY KEY, data_type TEXT, identifier TEXT, srs_id INTEGER)",
            "CREATE TABLE gpkg_geometry_columns (table_name TEXT, column_name TEXT, geometry_type_name TEXT, srs_id INTEGER)",
            "CREATE TABLE parcels (fid INTEGER PRIMARY KEY, label TEXT, geom BLOB)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO gpkg_contents VALUES ('parcels','features','parcels',-1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO gpkg_geometry_columns VALUES ('parcels','geom','POINT',-1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO parcels VALUES (1,'Plot 1',?)")
            .bind(blob.clone())
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let bytes = std::fs::read(&path).unwrap();
        let parsed = parse_gpkg_bytes(&bytes).await.expect("GPKG parse failed");
        assert_eq!(parsed.feature_count, 1);
        assert_eq!(parsed.geom_types, vec!["Point"]);
        assert_eq!(
            parsed.feature_collection.features[0]
                .properties
                .as_ref()
                .unwrap()["label"],
            "Plot 1"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // 23. Spatial bookmarks persistence
    #[tokio::test]
    async fn test_bookmark_crud() {
        let temp_dir = std::env::temp_dir().join(format!("geoviz_test_{}", uuid::Uuid::new_v4()));
        let db = AppDb::init(Some(temp_dir)).await.expect("DB init failed");

        let bm = MapBookmark {
            id: "bm-1".into(),
            name: "Home".into(),
            center_lat: 41.0,
            center_lng: 29.0,
            zoom: 11.0,
            created_at: "2026-08-24T00:00:00Z".into(),
        };
        db.save_bookmark(&bm).await.unwrap();

        let list = db.list_bookmarks().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Home");
        assert!((list[0].zoom - 11.0).abs() < 1e-9);

        assert!(db.delete_bookmark("bm-1").await.unwrap());
        assert!(db.list_bookmarks().await.unwrap().is_empty());
    }
}
