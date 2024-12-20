use crate::float_range::{RangeF32, flush_f32_to_zero};
use std::fmt::{self, Display};

/// The axis along which a wall projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectionAxis {
    X,
    Z,
}

impl ProjectionAxis {
    /// Determine the projection axis for a wall given its normal vector.
    pub fn of_wall(normal: &[f32; 3]) -> Self {
        if normal[0] < -0.707 || normal[0] > 0.707 {
            Self::X
        } else {
            Self::Z
        }
    }
}

impl Display for ProjectionAxis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectionAxis::X => write!(f, "x"),
            ProjectionAxis::Z => write!(f, "z"),
        }
    }
}

/// The orientation of a wall.
///
/// An x projective surface is positive iff `normal.x > 0`.
/// A z projective surfaces is positive iff `normal.z <= 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Accept r if r >= 0.
    Positive,
    /// Accept r if r <= 0.
    Negative,
}

impl Orientation {
    /// Get the orientation for a wall given its normal vector.
    pub fn of_wall(normal: &[f32; 3]) -> Self {
        match ProjectionAxis::of_wall(normal) {
            ProjectionAxis::X => {
                if normal[0] > 0.0 {
                    Self::Positive
                } else {
                    Self::Negative
                }
            }
            ProjectionAxis::Z => {
                if normal[2] <= 0.0 {
                    Self::Positive
                } else {
                    Self::Negative
                }
            }
        }
    }
}

/// A projected point used for edge calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectedPoint<T> {
    /// The relevant non-y coordinate.
    ///
    /// Equal to x for z projective surfaces, and z for x projective surfaces.
    pub w: T,
    /// The y coordinate.
    pub y: T,
}

impl<T: Clone> ProjectedPoint<T> {
    /// Project the point along the given axis.
    pub fn project(point: [T; 3], axis: ProjectionAxis) -> Self {
        match axis {
            ProjectionAxis::X => Self {
                w: point[2].clone(),
                y: point[1].clone(),
            },
            ProjectionAxis::Z => Self {
                w: point[0].clone(),
                y: point[1].clone(),
            },
        }
    }
}

/// An edge of a wall.
///
/// `vertex1`, `vertex2` should be listed in CCW order (i.e. match the game's order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Edge {
    pub projection_axis: ProjectionAxis,
    pub orientation: Orientation,
    pub vertex1: ProjectedPoint<i16>,
    pub vertex2: ProjectedPoint<i16>,
}

impl Edge {
    pub fn new(vertices: ([i16; 3], [i16; 3]), normal: [f32; 3]) -> Self {
        let projection_axis = ProjectionAxis::of_wall(&normal);
        let orientation = Orientation::of_wall(&normal);
        Self {
            projection_axis,
            orientation,
            vertex1: ProjectedPoint::project(vertices.0, projection_axis),
            vertex2: ProjectedPoint::project(vertices.1, projection_axis),
        }
    }

    pub fn is_vertical(&self) -> bool {
        self.vertex1.w == self.vertex2.w
    }

    pub fn w_range(&self) -> RangeF32 {
        let w1 = self.vertex1.w as f32;
        let w2 = self.vertex2.w as f32;
        RangeF32::inclusive(w1.min(w2), w1.max(w2))
    }

    pub fn y_range(&self) -> RangeF32 {
        let y1 = self.vertex1.y as f32;
        let y2 = self.vertex2.y as f32;
        RangeF32::inclusive(y1.min(y2), y1.max(y2))
    }

    pub fn approx_t(&self, w: f32) -> f32 {
        let w1 = self.vertex1.w as f32;
        let w2 = self.vertex2.w as f32;
        assert_ne!(w1, w2);
        (w - w1) / (w2 - w1)
    }

    pub fn approx_y(&self, w: f32) -> f32 {
        let y1 = self.vertex1.y as f32;
        let y2 = self.vertex2.y as f32;
        y1 + self.approx_t(w) * (y2 - y1)
    }

    pub fn approx_w_f64(&self, y: f64) -> f64 {
        let w1 = self.vertex1.w as f64;
        let w2 = self.vertex2.w as f64;
        let y1 = self.vertex1.y as f64;
        let y2 = self.vertex2.y as f64;
        w1 + (y - y1) / (y2 - y1) * (w2 - w1)
    }

    /// Return true if the projected point lies on the inside of the edge.
    pub fn accepts_projected(&self, point: ProjectedPoint<f32>) -> bool {
        let w = flush_f32_to_zero(point.w);
        let y = flush_f32_to_zero(point.y);

        let w1 = self.vertex1.w as f32;
        let y1 = self.vertex1.y as f32;

        let w2 = self.vertex2.w as f32;
        let y2 = self.vertex2.y as f32;

        let r = flush_f32_to_zero(
            flush_f32_to_zero((y1 - y) * (w2 - w1)) - flush_f32_to_zero((w1 - w) * (y2 - y1)),
        );

        match self.orientation {
            Orientation::Positive => r >= 0.0,
            Orientation::Negative => r <= 0.0,
        }
    }
}
