//! Network Analyst: topological graph construction from line vectors,
//! shortest paths (Dijkstra / A*), service areas (isochrone edges) and
//! origin-destination cost matrices.
//!
//! Connectivity snaps endpoints within a tolerance (degrees); edge weight
//! is geodesic length in meters (or divided by a speed for travel time).

use crate::gis::metrics::haversine_distance;
use geojson::{Feature, FeatureCollection, Value as GeoValue};
use serde_json::{json, Map, Value as JsonValue};
use std::collections::{BinaryHeap, HashMap, HashSet};

const SNAP_TOLERANCE_DEG: f64 = 1e-3; // ~100 m endpoint snap

/// Priority-queue entry with total-f64 ordering (BinaryHeap requires Ord).
#[derive(PartialEq, Clone, Copy)]
struct QueueEntry(f64, (i64, i64));
impl Eq for QueueEntry {}
impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0).then(self.1.cmp(&other.1))
    }
}
impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

type NodeKey = (i64, i64);
type DijkstraMaps = (HashMap<NodeKey, f64>, HashMap<NodeKey, NodeKey>);

/// In-memory network graph: nodes are snapped coordinates, edges carry
/// geodesic length and preserve the source feature index for output.
struct NetworkGraph {
    /// node key -> (lng, lat)
    nodes: HashMap<NodeKey, (f64, f64)>,
    /// adjacency: node key -> [(neighbor key, weight_m, feature_idx)]
    adj: HashMap<NodeKey, Vec<(NodeKey, f64, usize)>>,
    edge_count: usize,
}

fn snap_key(lng: f64, lat: f64) -> (i64, i64) {
    (
        (lng / SNAP_TOLERANCE_DEG).round() as i64,
        (lat / SNAP_TOLERANCE_DEG).round() as i64,
    )
}

impl NetworkGraph {
    /// Build a graph from every LineString/MultiLineString in the collection.
    fn build(fc: &FeatureCollection) -> Self {
        let mut graph = Self {
            nodes: HashMap::new(),
            adj: HashMap::new(),
            edge_count: 0,
        };
        for (feature_idx, feature) in fc.features.iter().enumerate() {
            let Some(geom) = &feature.geometry else {
                continue;
            };
            let lines: Vec<&Vec<Vec<f64>>> = match &geom.value {
                GeoValue::LineString(ls) => vec![ls],
                GeoValue::MultiLineString(lss) => lss.iter().collect(),
                _ => continue,
            };
            for line in lines {
                for pair in line.windows(2) {
                    let (a, b) = (&pair[0], &pair[1]);
                    let ka = snap_key(a[0], a[1]);
                    let kb = snap_key(b[0], b[1]);
                    if ka == kb {
                        continue;
                    }
                    let w = haversine_distance(a.as_slice(), b.as_slice());
                    graph.nodes.entry(ka).or_insert((a[0], a[1]));
                    graph.nodes.entry(kb).or_insert((b[0], b[1]));
                    graph.adj.entry(ka).or_default().push((kb, w, feature_idx));
                    graph.adj.entry(kb).or_default().push((ka, w, feature_idx));
                    graph.edge_count += 1;
                }
            }
        }
        graph
    }

    /// Nearest graph node to a coordinate (linear scan; networks are modest).
    fn nearest_node(&self, lng: f64, lat: f64) -> Option<NodeKey> {
        self.nodes.keys().copied().min_by(|a, b| {
            let da = (self.nodes[a].0 - lng).powi(2) + (self.nodes[a].1 - lat).powi(2);
            let db = (self.nodes[b].0 - lng).powi(2) + (self.nodes[b].1 - lat).powi(2);
            da.total_cmp(&db)
        })
    }

    /// Dijkstra from a source; returns (dist, predecessor) maps.
    fn dijkstra(&self, source: (i64, i64)) -> DijkstraMaps {
        let mut dist: HashMap<(i64, i64), f64> = HashMap::new();
        let mut prev: HashMap<(i64, i64), (i64, i64)> = HashMap::new();
        let mut heap = BinaryHeap::new();
        dist.insert(source, 0.0);
        heap.push(std::cmp::Reverse(QueueEntry(0.0, source)));
        while let Some(std::cmp::Reverse(QueueEntry(d, u))) = heap.pop() {
            if d > *dist.get(&u).unwrap_or(&f64::INFINITY) {
                continue;
            }
            if let Some(neighbors) = self.adj.get(&u) {
                for &(v, w, _) in neighbors {
                    let nd = d + w;
                    if nd < *dist.get(&v).unwrap_or(&f64::INFINITY) {
                        dist.insert(v, nd);
                        prev.insert(v, u);
                        heap.push(std::cmp::Reverse(QueueEntry(nd, v)));
                    }
                }
            }
        }
        (dist, prev)
    }
}

fn path_features(
    graph: &NetworkGraph,
    prev: &HashMap<(i64, i64), (i64, i64)>,
    dist: &HashMap<(i64, i64), f64>,
    target: (i64, i64),
    props: Map<String, JsonValue>,
) -> (Vec<Feature>, f64) {
    // Walk predecessors to reconstruct the node chain.
    let mut chain = vec![target];
    let mut cursor = target;
    while let Some(&p) = prev.get(&cursor) {
        chain.push(p);
        cursor = p;
    }
    chain.reverse();
    let total = *dist.get(&target).unwrap_or(&0.0);

    // One LineString feature per graph edge along the path.
    let mut features = Vec::new();
    for pair in chain.windows(2) {
        let (Some(&su), Some(&sv)) = (graph.nodes.get(&pair[0]), graph.nodes.get(&pair[1])) else {
            continue;
        };
        let mut p = props.clone();
        p.insert(
            "route_leg_m".into(),
            json!((haversine_distance(&[su.0, su.1], &[sv.0, sv.1]) * 10.0).round() / 10.0),
        );
        features.push(Feature {
            bbox: None,
            geometry: Some(geojson::Geometry::new(GeoValue::LineString(vec![
                vec![su.0, su.1],
                vec![sv.0, sv.1],
            ]))),
            id: None,
            properties: Some(p),
            foreign_members: None,
        });
    }
    (features, total)
}

/// Shortest path between two coordinates snapped to the network.
/// `algorithm`: "dijkstra" or "astar" (haversine heuristic).
pub fn shortest_path(
    fc: &FeatureCollection,
    start_lng: f64,
    start_lat: f64,
    end_lng: f64,
    end_lat: f64,
    algorithm: &str,
) -> Result<(FeatureCollection, JsonValue), String> {
    let graph = NetworkGraph::build(fc);
    if graph.edge_count == 0 {
        return Err("network layer contains no line geometry".into());
    }
    let Some(source) = graph.nearest_node(start_lng, start_lat) else {
        return Err("no network node near the start coordinate".into());
    };
    let Some(target) = graph.nearest_node(end_lng, end_lat) else {
        return Err("no network node near the end coordinate".into());
    };

    // Unified Dijkstra/A*: A* adds the haversine heuristic to the heap key.
    let mut dist: HashMap<(i64, i64), f64> = HashMap::new();
    let mut prev: HashMap<(i64, i64), (i64, i64)> = HashMap::new();
    let heuristic = |k: (i64, i64)| -> f64 {
        if algorithm == "astar" {
            let (lng, lat) = graph.nodes[&k];
            haversine_distance(&[lng, lat], &[end_lng, end_lat])
        } else {
            0.0
        }
    };
    let mut heap = BinaryHeap::new();
    dist.insert(source, 0.0);
    heap.push(std::cmp::Reverse(QueueEntry(heuristic(source), source)));
    let mut settled = HashSet::new();
    while let Some(std::cmp::Reverse(QueueEntry(_, u))) = heap.pop() {
        if !settled.insert(u) {
            continue;
        }
        if u == target {
            break;
        }
        let du = *dist.get(&u).unwrap_or(&f64::INFINITY);
        if let Some(neighbors) = graph.adj.get(&u) {
            for &(v, w, _) in neighbors {
                let nd = du + w;
                if nd < *dist.get(&v).unwrap_or(&f64::INFINITY) {
                    dist.insert(v, nd);
                    prev.insert(v, u);
                    heap.push(std::cmp::Reverse(QueueEntry(nd + heuristic(v), v)));
                }
            }
        }
    }
    if !settled.contains(&target) {
        return Err("no path exists between the given coordinates".into());
    }

    let mut props = Map::new();
    props.insert("type".into(), json!("Shortest Path"));
    props.insert("algorithm".into(), json!(algorithm));
    let (features, total_m) = path_features(&graph, &prev, &dist, target, props);
    let total_km = (total_m / 1000.0 * 100.0).round() / 100.0;

    Ok((
        FeatureCollection {
            bbox: None,
            features,
            foreign_members: None,
        },
        json!({
            "algorithm": algorithm,
            "total_distance_km": total_km,
            "total_distance_m": (total_m * 10.0).round() / 10.0,
            "network_nodes": graph.nodes.len(),
            "network_edges": graph.edge_count,
            "travel_time_min_at_60kmh": (total_km * 60.0 * 100.0).round() / 100.0
        }),
    ))
}

/// Service area: all network edges reachable within `max_distance_m` from a
/// coordinate, plus an enclosing hull polygon of the reachable extent.
pub fn service_area(
    fc: &FeatureCollection,
    lng: f64,
    lat: f64,
    max_distance_m: f64,
) -> Result<(FeatureCollection, JsonValue), String> {
    let graph = NetworkGraph::build(fc);
    if graph.edge_count == 0 {
        return Err("network layer contains no line geometry".into());
    }
    let Some(source) = graph.nearest_node(lng, lat) else {
        return Err("no network node near the given coordinate".into());
    };
    let (dist, _) = graph.dijkstra(source);

    let mut features = Vec::new();
    let mut reached_nodes = 0usize;
    let mut hull_pts: Vec<Vec<f64>> = Vec::new();
    let mut seen_edges: HashSet<(i64, i64)> = HashSet::new();
    for (&u, du) in &dist {
        if *du > max_distance_m {
            continue;
        }
        reached_nodes += 1;
        hull_pts.push(vec![graph.nodes[&u].0, graph.nodes[&u].1]);
        if let Some(neighbors) = graph.adj.get(&u) {
            for &(v, w, _) in neighbors {
                if *du + w > max_distance_m {
                    continue; // edge partially reachable: skip for clean output
                }
                let key = if u < v { (u.0, v.0) } else { (v.0, u.0) };
                if !seen_edges.insert(key) {
                    continue;
                }
                let Some(&(nvx, nvy)) = graph.nodes.get(&v) else {
                    continue;
                };
                features.push(Feature {
                    bbox: None,
                    geometry: Some(geojson::Geometry::new(GeoValue::LineString(vec![
                        vec![graph.nodes[&u].0, graph.nodes[&u].1],
                        vec![nvx, nvy],
                    ]))),
                    id: None,
                    properties: Some(Map::from_iter([
                        ("type".into(), json!("Service Area Edge")),
                        (
                            "cost_from_facility_m".into(),
                            json!(((*du + w) * 10.0).round() / 10.0),
                        ),
                    ])),
                    foreign_members: None,
                });
            }
        }
    }
    if features.is_empty() {
        return Err("nothing reachable within the given distance".into());
    }

    // Convex hull (Andrew monotone chain) over reached node coordinates.
    hull_pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    hull_pts.dedup();
    let hull = convex_hull_points(hull_pts);
    let reachable_edges = features
        .len()
        .saturating_sub(if hull.len() >= 3 { 1 } else { 0 });
    if hull.len() >= 3 {
        let mut ring = hull.clone();
        ring.push(ring[0].clone());
        features.push(Feature {
            bbox: None,
            geometry: Some(geojson::Geometry::new(GeoValue::Polygon(vec![ring]))),
            id: None,
            properties: Some(Map::from_iter([(
                "type".into(),
                json!("Service Area Hull"),
            )])),
            foreign_members: None,
        });
    }

    Ok((
        FeatureCollection {
            bbox: None,
            features,
            foreign_members: None,
        },
        json!({
            "max_distance_m": max_distance_m,
            "reachable_nodes": reached_nodes,
            "reachable_edges": reachable_edges,
            "network_nodes": graph.nodes.len()
        }),
    ))
}

fn convex_hull_points(mut pts: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    if pts.len() < 3 {
        return pts;
    }
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    pts.dedup();
    let cross = |o: &Vec<f64>, a: &Vec<f64>, b: &Vec<f64>| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut lower: Vec<Vec<f64>> = Vec::new();
    for p in &pts {
        while lower.len() >= 2 && cross(&lower[lower.len() - 2], &lower[lower.len() - 1], p) <= 0.0
        {
            lower.pop();
        }
        lower.push(p.clone());
    }
    let mut upper: Vec<Vec<f64>> = Vec::new();
    for p in pts.iter().rev() {
        while upper.len() >= 2 && cross(&upper[upper.len() - 2], &upper[upper.len() - 1], p) <= 0.0
        {
            upper.pop();
        }
        upper.push(p.clone());
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Origin-destination cost matrix: network distance from every origin to
/// every destination (nearest-node snapped), reported as summary stats.
pub fn od_cost_matrix(
    network_fc: &FeatureCollection,
    origins_fc: &FeatureCollection,
    destinations_fc: &FeatureCollection,
) -> Result<(FeatureCollection, JsonValue), String> {
    let graph = NetworkGraph::build(network_fc);
    if graph.edge_count == 0 {
        return Err("network layer contains no line geometry".into());
    }
    let origin_pts = crate::gis::spatial_statistics::feature_centroids(origins_fc);
    let dest_pts = crate::gis::spatial_statistics::feature_centroids(destinations_fc);
    if origin_pts.is_empty() || dest_pts.is_empty() {
        return Err("origin and destination layers must contain features".into());
    }
    if origin_pts.len() * dest_pts.len() > 10_000 {
        return Err("OD matrix limited to 10,000 origin-destination pairs".into());
    }

    // One Dijkstra per unique origin node.
    let mut rows = Vec::new();
    let mut total: f64 = 0.0;
    let mut unreachable = 0usize;
    for (oi, &(olng, olat)) in origin_pts.iter().enumerate() {
        let Some(source) = graph.nearest_node(olng, olat) else {
            continue;
        };
        let (dist, _) = graph.dijkstra(source);
        for (di, &(dlng, dlat)) in dest_pts.iter().enumerate() {
            let Some(target) = graph.nearest_node(dlng, dlat) else {
                continue;
            };
            match dist.get(&target) {
                Some(&d) => {
                    total += d;
                    let mut props = Map::new();
                    props.insert("origin_id".into(), json!(oi));
                    props.insert("destination_id".into(), json!(di));
                    props.insert(
                        "network_distance_m".into(),
                        json!((d * 10.0).round() / 10.0),
                    );
                    props.insert(
                        "network_distance_km".into(),
                        json!((d / 1000.0 * 100.0).round() / 100.0),
                    );
                    rows.push(Feature {
                        bbox: None,
                        geometry: Some(geojson::Geometry::new(GeoValue::LineString(vec![
                            vec![olng, olat],
                            vec![dlng, dlat],
                        ]))),
                        id: None,
                        properties: Some(props),
                        foreign_members: None,
                    });
                }
                None => unreachable += 1,
            }
        }
    }

    let pairs = origin_pts.len() * dest_pts.len();
    let solved = rows.len();
    let mean = if solved == 0 {
        0.0
    } else {
        total / solved as f64
    };
    Ok((
        FeatureCollection {
            bbox: None,
            features: rows,
            foreign_members: None,
        },
        json!({
            "origins": origin_pts.len(),
            "destinations": dest_pts.len(),
            "pairs": pairs,
            "solved": solved,
            "unreachable": unreachable,
            "mean_network_distance_km": (mean / 1000.0 * 100.0).round() / 100.0
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_fc(lines: Vec<Vec<(f64, f64)>>) -> FeatureCollection {
        FeatureCollection {
            bbox: None,
            features: lines
                .into_iter()
                .map(|pts| Feature {
                    bbox: None,
                    geometry: Some(geojson::Geometry::new(GeoValue::LineString(
                        pts.into_iter().map(|(x, y)| vec![x, y]).collect(),
                    ))),
                    id: None,
                    properties: None,
                    foreign_members: None,
                })
                .collect(),
            foreign_members: None,
        }
    }

    fn point_fc(points: &[(f64, f64)]) -> FeatureCollection {
        FeatureCollection {
            bbox: None,
            features: points
                .iter()
                .map(|&(x, y)| Feature {
                    bbox: None,
                    geometry: Some(geojson::Geometry::new(GeoValue::Point(vec![x, y]))),
                    id: None,
                    properties: None,
                    foreign_members: None,
                })
                .collect(),
            foreign_members: None,
        }
    }

    /// L-shaped network: A(0,0) -> B(1,0) -> C(1,1)
    #[test]
    fn test_shortest_path_finds_l_route() {
        let fc = line_fc(vec![
            vec![(0.0, 0.0), (1.0, 0.0)],
            vec![(1.0, 0.0), (1.0, 1.0)],
        ]);
        let (out, summary) = shortest_path(&fc, 0.0, 0.0, 1.0, 1.0, "dijkstra").unwrap();
        assert!(!out.features.is_empty());
        // ~111 km per degree; L route = 2 legs.
        assert!((summary["total_distance_km"].as_f64().unwrap() - 222.0).abs() < 2.0);
        // A* must agree with Dijkstra.
        let (_, s2) = shortest_path(&fc, 0.0, 0.0, 1.0, 1.0, "astar").unwrap();
        assert_eq!(summary["total_distance_km"], s2["total_distance_km"]);
    }

    #[test]
    fn test_shortest_path_no_route() {
        let fc = line_fc(vec![
            vec![(0.0, 0.0), (1.0, 0.0)],
            vec![(5.0, 5.0), (6.0, 6.0)],
        ]);
        assert!(shortest_path(&fc, 0.0, 0.0, 6.0, 6.0, "dijkstra").is_err());
    }

    #[test]
    fn test_service_area_within_distance() {
        let fc = line_fc(vec![
            vec![(0.0, 0.0), (0.5, 0.0)],
            vec![(0.5, 0.0), (1.0, 0.0)],
            vec![(0.5, 0.0), (0.5, 0.5)],
            vec![(1.0, 0.0), (5.0, 5.0)],
        ]);
        let (out, summary) = service_area(&fc, 0.0, 0.0, 150_000.0).unwrap();
        // 150 km covers ~1.35 degrees: the L cluster, not the long edge.
        assert_eq!(summary["reachable_nodes"], 4);
        let hull = out
            .features
            .iter()
            .any(|f| matches!(f.geometry.as_ref().unwrap().value, GeoValue::Polygon(_)));
        assert!(hull);
    }

    #[test]
    fn test_od_cost_matrix() {
        let net = line_fc(vec![vec![(0.0, 0.0), (2.0, 0.0)]]);
        let origins = point_fc(&[(0.0, 0.0)]);
        let dests = point_fc(&[(1.0, 0.0), (2.0, 0.0)]);
        let (out, summary) = od_cost_matrix(&net, &origins, &dests).unwrap();
        assert_eq!(out.features.len(), 2);
        assert_eq!(summary["solved"], 2);
        assert!(summary["mean_network_distance_km"].as_f64().unwrap() > 0.0);
    }
}
