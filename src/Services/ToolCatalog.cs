namespace GeoViz.Services;

/// <summary>Static metadata describing every geoprocessing tool exposed by the backend.</summary>
public static class ToolCatalog
{
    public sealed record ToolInfo(string Key, string Name, string Icon, string Description, string LongDescription);

    public sealed record Category(string Name, ToolInfo[] Tools);

    public static readonly Category[] Categories =
    {
        new("Proximity & Buffer",
        [
            new("buffer", "Buffer Analysis", "circle",
                "Generate geodesic buffer distance zones",
                "Computes spherical geodesic buffer polygons around vector geometries at a given metric radius.")
        ]),
        new("Geometric Envelopes",
        [
            new("convex_hull", "Convex Hull", "hexagon",
                "Minimum enclosing convex polygon",
                "Generates the smallest convex polygon enclosing all geometry vertices in the dataset."),
            new("centroid", "Centroids", "target",
                "Geometric center of mass",
                "Computes the center of gravity and spatial coordinates for polygons, lines, and point clusters."),
            new("bbox", "Bounding Box", "square",
                "Minimum bounding rectangle (MBR)",
                "Calculates the bounding envelope (min/max lon/lat) as an enclosing rectangle polygon.")
        ]),
        new("Generalization & Metrics",
        [
            new("simplify", "Simplify Geometry", "activity",
                "Douglas-Peucker vertex decimation",
                "Reduces vertex density while preserving topological shape using the Douglas-Peucker algorithm."),
            new("metrics", "Spatial Metrics", "bar-chart-2",
                "Spherical area, perimeter & statistics",
                "Calculates geodesic area (m², km², ha), perimeter, and numerical attribute distributions.")
        ]),
        new("Density & Pattern",
        [
            new("spatial_binning", "Hexbin & Grid Density", "grid",
                "Aggregate points into heatmaps",
                "Aggregates point densities across uniform hexagonal or rectangular mesh bins."),
            new("distance_matrix", "Nearest Neighbor", "git-commit",
                "Distance matrix and link vectors",
                "Calculates pairwise distances and connects nearest neighbor points with vector lines."),
            new("spatial_query", "Spatial Filter", "filter",
                "Point-in-polygon & attribute queries",
                "Filters spatial features via polygon containment or attribute comparisons."),
            new("random_points", "Random Sampling", "dices",
                "Generate uniform spatial coordinates",
                "Generates uniform synthetic coordinate points bounded within layer extents.")
        ]),
        new("Overlay & Attribution",
        [
            new("overlay", "Overlay Analysis", "layers",
                "Intersect, difference, symmetric diff or clip",
                "Classic polygon overlay: combines the input layer with a boundary layer using boolean set operations (or clips it to the boundary)."),
            new("dissolve", "Dissolve / Union", "square",
                "Merge polygons into minimal coverage",
                "Dissolves internal boundaries: merges all polygons, optionally grouped by an attribute field."),
            new("spatial_join", "Spatial Join", "table",
                "Attach attributes by location",
                "Joins attributes from a polygon target layer onto source features contained within them."),
            new("join_csv", "CSV Attribute Join", "file-spreadsheet",
                "Join standalone CSV data by key",
                "Attaches columns from a pasted CSV table onto layer features by matching a primary key field."),
            new("topology_check", "Topology Validation", "shield-check",
                "Overlap, dangle & coverage rules",
                "Validates layer integrity: polygon overlaps (with merge/subtract fixes), dangling line endpoints, and point/line coverage by a polygon layer.")
        ]),
        new("Spatial Statistics",
        [
            new("mean_center", "Mean Center", "crosshair",
                "Geographic center of mass",
                "Computes the average center of all feature centroids — the first moment of the distribution."),
            new("median_center", "Median Center", "target",
                "Minimum total travel center",
                "Iteratively finds the point minimizing total distance to all features (geometric median, Weiszfeld)."),
            new("directional_mean", "Directional Trend", "move-up-right",
                "Linear directional mean",
                "Calculates the dominant orientation of line features as a compass bearing."),
            new("morans_i", "Moran's I Autocorrelation", "activity",
                "Clustered, dispersed or random?",
                "Global spatial autocorrelation of a numeric field with z-score and p-value significance testing."),
            new("getis_ord", "Hot Spot Analysis", "flame",
                "Getis-Ord Gi* significance",
                "Assigns z-scores to every feature, flagging statistically significant hot and cold spots (95%/99%)."),
            new("ols_regression", "OLS Regression", "trending-up",
                "Model relationships between fields",
                "Ordinary least squares over up to six explanatory fields with R², adjusted R², AIC and per-feature residuals.")
        ]),
        new("Geostatistics",
        [
            new("idw", "IDW Interpolation", "cloud",
                "Deterministic surface prediction",
                "Inverse Distance Weighting: predicts values on a grid from scattered points, weighted by distance^power."),
            new("kriging", "Ordinary Kriging", "gem",
                "Geostatistical prediction + error",
                "Fits spherical/exponential/gaussian variograms and produces a prediction surface with standard-error estimates.")
        ]),
        new("Network Analysis",
        [
            new("shortest_path", "Shortest Path", "route",
                "Dijkstra / A* routing",
                "Builds a topological graph from line features (endpoint snapping) and finds the optimal route between two coordinates."),
            new("service_area", "Service Area", "circle-dashed",
                "Network isochrone",
                "Returns every network edge reachable within a distance budget from a facility, plus a hull of the covered extent."),
            new("od_matrix", "OD Cost Matrix", "table-2",
                "Origin-destination costs",
                "Network distances from every origin to every destination with summary statistics and unreachable counts.")
        ]),
        new("Raster (Spatial Analyst)",
        [
            new("slope", "Slope", "triangle-right",
                "First derivative of terrain (deg)",
                "Horn's method slope in degrees from a DEM; also the base for aspect and hillshade products."),
            new("hillshade", "Hillshade", "sun",
                "Lambertian terrain shading",
                "3D terrain illumination from a configurable azimuth/altitude light source for cartographic relief."),
            new("raster_calculator", "Raster Calculator", "calculator",
                "Map algebra expressions",
                "Evaluate expressions like 'a * 2 + 1' or 'sqrt(a) / min(a, b)' across one or two rasters cell-by-cell."),
            new("d8_flow", "D8 Flow", "waves",
                "Flow direction & accumulation",
                "Deterministic D8 routing: steepest-descent direction codes and upstream contributing-cell counts."),
            new("zonal_stats", "Zonal Statistics", "chart-column-big",
                "Raster × polygon summaries",
                "Per-polygon min/max/mean/median/std/majority of raster cells — population by district, rainfall by basin."),
            new("viewshed", "Viewshed", "eye",
                "Line-of-sight visibility",
                "Bresenham ray-cast visibility from an observer point over the terrain surface.")
        ])
    };

    public static Dictionary<string, ToolInfo> ById { get; } = Categories
        .SelectMany(c => c.Tools)
        .ToDictionary(t => t.Key, t => t);
}
