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
                "Joins attributes from a polygon target layer onto source features contained within them.")
        ])
    };

    public static Dictionary<string, ToolInfo> ById { get; } = Categories
        .SelectMany(c => c.Tools)
        .ToDictionary(t => t.Key, t => t);
}
